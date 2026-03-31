use std::collections::VecDeque;
use std::fs;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use rumqttc::mqttbytes::v4::SubscribeReasonCode;
use rumqttc::{AsyncClient, Event, Outgoing, Packet, QoS};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::MissedTickBehavior;
use tracing::{debug, error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;

use crate::config::{BootstrapOptions, Config, load_config};
use crate::device::{Identity, Topics};
use crate::integrations::mqtt::{
    DiscoveryPublishArgs, build_lock_mqtt_options, build_mqtt_options,
    is_home_assistant_birth_message, publish_availability, publish_cpu_info_state,
    publish_cpu_state, publish_discovery_if_needed, publish_disk_info_state, publish_disk_state,
    publish_gpu_info_state, publish_gpu_state, publish_host_info_state, publish_memory_info_state,
    publish_memory_state, publish_network_info_state, publish_network_state,
    publish_shutdown_state, publish_uptime_state,
};
use crate::shared::util::slugify;
use crate::system::collector::Collector;
use crate::system::models::{
    CpuInfoState, CpuState, DiskInfoState, DiskState, GpuInfoState, GpuState, HostInfoState,
    MemoryInfoState, MemoryState, NetworkInfoState, NetworkState, ShutdownState, UptimeState,
};
use crate::system::power::shutdown_host;

#[derive(Debug, Default)]
struct DiscoveryState {
    last_payload: Option<Vec<u8>>,
}

struct PublishContext<'a> {
    client: &'a AsyncClient,
    config: &'a Config,
    identity: &'a Identity,
    topics: &'a Topics,
}

struct FullSnapshot {
    cpu_state: CpuState,
    uptime_state: UptimeState,
    shutdown_state: ShutdownState,
    gpu_state: Option<GpuState>,
    memory_state: MemoryState,
    disk_state: DiskState,
    network_state: NetworkState,
}

struct StaticSnapshot {
    host_info: HostInfoState,
    cpu_info: CpuInfoState,
    gpu_info: Option<GpuInfoState>,
    memory_info: MemoryInfoState,
    disk_info: DiskInfoState,
    network_info: NetworkInfoState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownCommandKind {
    Schedule,
    Cancel,
}

#[derive(Debug)]
struct PendingShutdown {
    request_id: u64,
    deadline: Instant,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShutdownElapsed {
    request_id: u64,
    delay_secs: u64,
}

const SUBSCRIPTION_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const NODE_LOCK_SYNC_TIMEOUT: Duration = Duration::from_millis(400);
const NODE_LOCK_CLAIM_WINDOW: Duration = Duration::from_millis(800);
const NODE_LOCK_CONFIRM_TIMEOUT: Duration = Duration::from_millis(800);

#[derive(Debug)]
struct NodeLockGuard {
    client: AsyncClient,
    lock_topic: String,
    offline_payload: Vec<u8>,
    eventloop_task: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct NodeLockPayload {
    node_id: String,
    instance_id: String,
    host_name: String,
    started_at: String,
    status: NodeLockStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum NodeLockStatus {
    Claiming,
    Online,
    Offline,
}

#[derive(Debug, Default)]
struct SubscriptionState {
    ha_status: TopicSubscription,
    shutdown_command: TopicSubscription,
    pending_outgoing: VecDeque<SubscriptionTarget>,
    pending_subacks: Vec<PendingSubscriptionAck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscriptionTarget {
    HomeAssistantStatus,
    ShutdownCommand,
}

#[derive(Debug, Clone, Copy)]
struct PendingSubscriptionAck {
    target: SubscriptionTarget,
    pkid: u16,
}

#[derive(Debug, Default)]
struct TopicSubscription {
    desired: bool,
    request_queued: bool,
    awaiting_suback: bool,
    subscribed: bool,
    last_request_at: Option<Instant>,
}

#[derive(Debug)]
struct PublishedSlot<T> {
    state: Option<T>,
}

#[derive(Debug, Default)]
struct PublishedStates {
    cpu: PublishedSlot<CpuState>,
    uptime: PublishedSlot<UptimeState>,
    shutdown: PublishedSlot<ShutdownState>,
    gpu: PublishedSlot<GpuState>,
    memory: PublishedSlot<MemoryState>,
    disk: PublishedSlot<DiskState>,
    network: PublishedSlot<NetworkState>,
}

impl<T> PublishedSlot<T> {
    fn new() -> Self {
        Self { state: None }
    }

    fn clear(&mut self) {
        self.state = None;
    }

    fn mark_published(&mut self, state: T) {
        self.state = Some(state);
    }
}

impl<T> Default for PublishedSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeLockPayload {
    fn new(identity: &Identity, instance_id: &str, status: NodeLockStatus) -> Self {
        Self {
            node_id: identity.node_id.clone(),
            instance_id: instance_id.to_string(),
            host_name: identity.host_name.clone(),
            started_at: Utc::now().to_rfc3339(),
            status,
        }
    }

    fn is_online_for_other_instance(&self, instance_id: &str) -> bool {
        self.status == NodeLockStatus::Online && self.instance_id != instance_id
    }

    fn is_claiming_for_other_instance(&self, instance_id: &str) -> bool {
        self.status == NodeLockStatus::Claiming && self.instance_id != instance_id
    }
}

impl NodeLockGuard {
    async fn release(self) {
        if let Err(error) = self
            .client
            .publish(
                self.lock_topic.clone(),
                QoS::AtLeastOnce,
                true,
                self.offline_payload.clone(),
            )
            .await
        {
            warn!(%error, topic = %self.lock_topic, "failed to publish offline node lock payload");
        }

        if let Err(error) = self.client.disconnect().await {
            warn!(%error, "failed to disconnect node lock MQTT client");
        }

        self.eventloop_task.abort();
    }

    async fn disconnect(self) {
        if let Err(error) = self.client.disconnect().await {
            warn!(%error, "failed to disconnect node lock MQTT client");
        }

        self.eventloop_task.abort();
    }
}

impl SubscriptionState {
    fn prepare_for_connection(&mut self, enable_shutdown_button: bool) {
        self.reset_runtime();
        self.ha_status.desired = true;
        self.shutdown_command.desired = enable_shutdown_button;
    }

    fn reset_runtime(&mut self) {
        *self = Self::default();
    }

    fn should_request(&self, target: SubscriptionTarget) -> bool {
        let subscription = self.subscription(target);
        subscription.desired
            && !subscription.subscribed
            && !subscription.request_queued
            && !subscription.awaiting_suback
            && subscription
                .last_request_at
                .is_none_or(|instant| instant.elapsed() >= SUBSCRIPTION_RETRY_INTERVAL)
    }

    fn mark_request_queued(&mut self, target: SubscriptionTarget) {
        let subscription = self.subscription_mut(target);
        subscription.request_queued = true;
        subscription.last_request_at = Some(Instant::now());
        self.pending_outgoing.push_back(target);
    }

    fn mark_request_sent(&mut self, pkid: u16) {
        let Some(target) = self.pending_outgoing.pop_front() else {
            return;
        };

        let subscription = self.subscription_mut(target);
        subscription.request_queued = false;
        subscription.awaiting_suback = true;
        self.pending_subacks
            .push(PendingSubscriptionAck { target, pkid });
    }

    fn handle_suback(&mut self, pkid: u16, success: bool) {
        let Some(index) = self
            .pending_subacks
            .iter()
            .position(|pending| pending.pkid == pkid)
        else {
            return;
        };
        let pending = self.pending_subacks.swap_remove(index);
        let subscription = self.subscription_mut(pending.target);
        subscription.awaiting_suback = false;
        subscription.subscribed = success;
        if !success {
            subscription.last_request_at = Some(Instant::now());
        }
    }

    fn subscription(&self, target: SubscriptionTarget) -> &TopicSubscription {
        match target {
            SubscriptionTarget::HomeAssistantStatus => &self.ha_status,
            SubscriptionTarget::ShutdownCommand => &self.shutdown_command,
        }
    }

    fn subscription_mut(&mut self, target: SubscriptionTarget) -> &mut TopicSubscription {
        match target {
            SubscriptionTarget::HomeAssistantStatus => &mut self.ha_status,
            SubscriptionTarget::ShutdownCommand => &mut self.shutdown_command,
        }
    }
}

pub async fn run() -> Result<()> {
    let bootstrap = BootstrapOptions::from_current_process();
    initialize_runtime_with(&bootstrap)?;
    let config = load_config(&bootstrap)?;
    run_with_config(config, async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            error!(%error, "failed to listen for ctrl-c");
        }
    })
    .await
}

pub fn initialize_runtime() {
    let _ = initialize_runtime_with(&BootstrapOptions::from_current_process());
}

pub fn initialize_runtime_with(bootstrap: &BootstrapOptions) -> Result<()> {
    static TRACING_INIT: OnceLock<Result<(), String>> = OnceLock::new();
    match TRACING_INIT.get_or_init(|| init_tracing(bootstrap).map_err(|error| format!("{error:#}")))
    {
        Ok(()) => Ok(()),
        Err(message) => Err(anyhow!(message.clone())),
    }
}

pub async fn run_with_config<F>(config: Config, shutdown_signal: F) -> Result<()>
where
    F: Future<Output = ()>,
{
    let identity = Identity::detect(&config);
    let topics = Topics::from_identity(&config, &identity);
    let (node_lock_guard, mut node_lock_loss_rx) =
        acquire_node_lock(&config, &topics, &identity).await?;

    info!(
        device_name = %identity.device_name,
        node_id = %identity.node_id,
        config_dir = ?config.config_dir,
        log_dir = ?config.log_dir,
        cpu_interval_secs = config.cpu_interval_secs,
        gpu_interval_secs = config.gpu_interval_secs,
        memory_interval_secs = config.memory_interval_secs,
        uptime_interval_secs = config.uptime_interval_secs,
        disk_interval_secs = config.disk_interval_secs,
        network_interval_secs = config.network_interval_secs,
        network_include_interfaces = ?config.network_include_interfaces,
        cpu_change_threshold_pct = config.cpu_change_threshold_pct,
        gpu_usage_change_threshold_pct = config.gpu_usage_change_threshold_pct,
        gpu_memory_change_threshold_mib = config.gpu_memory_change_threshold_mib,
        memory_change_threshold_mib = config.memory_change_threshold_mib,
        disk_change_threshold_mib = config.disk_change_threshold_mib,
        enable_shutdown_button = config.enable_shutdown_button,
        shutdown_delay_secs = config.shutdown_delay_secs,
        shutdown_cancel_payload = config.shutdown_cancel_payload,
        shutdown_dry_run = config.shutdown_dry_run,
        "starting Home Assistant system monitor"
    );

    let mut collector = Collector::new(&identity, &config).await;
    let mut discovery_state = DiscoveryState::default();
    let mut published_states = PublishedStates::default();
    let mut mqtt_options = build_mqtt_options(&config, &identity, &topics);
    mqtt_options.set_keep_alive(Duration::from_secs(30));

    let (client, mut eventloop) = AsyncClient::new(mqtt_options, 256);
    let publish_context = PublishContext {
        client: &client,
        config: &config,
        identity: &identity,
        topics: &topics,
    };

    let mut cpu_interval = tokio::time::interval(Duration::from_secs(config.cpu_interval_secs));
    cpu_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut gpu_interval = tokio::time::interval(Duration::from_secs(config.gpu_interval_secs));
    gpu_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut memory_interval =
        tokio::time::interval(Duration::from_secs(config.memory_interval_secs));
    memory_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut uptime_interval =
        tokio::time::interval(Duration::from_secs(config.uptime_interval_secs));
    uptime_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut shutdown_interval = tokio::time::interval(Duration::from_secs(1));
    shutdown_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut disk_interval = tokio::time::interval(Duration::from_secs(config.disk_interval_secs));
    disk_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut network_interval =
        tokio::time::interval(Duration::from_secs(config.network_interval_secs));
    network_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut connected = false;
    let mut availability_online_pending = false;
    let mut subscription_state = SubscriptionState::default();
    let (shutdown_elapsed_tx, mut shutdown_elapsed_rx) =
        mpsc::unbounded_channel::<ShutdownElapsed>();
    let mut next_shutdown_request_id = 0_u64;
    let mut pending_shutdown: Option<PendingShutdown> = None;
    let mut shutdown_signal = Box::pin(shutdown_signal);

    loop {
        tokio::select! {
            maybe_elapsed = shutdown_elapsed_rx.recv() => {
                let Some(elapsed) = maybe_elapsed else {
                    continue;
                };

                let Some(current) = pending_shutdown.as_ref() else {
                    continue;
                };

                if current.request_id != elapsed.request_id {
                    continue;
                }

                pending_shutdown = None;
                sync_shutdown_state(
                    &client,
                    &topics,
                    &config,
                    pending_shutdown.as_ref(),
                    &mut published_states,
                )
                .await;
                warn!(
                    delay_secs = elapsed.delay_secs,
                    "scheduled shutdown delay elapsed, executing shutdown"
                );
                execute_shutdown_request(config.shutdown_dry_run).await;
            }
            maybe_reason = node_lock_loss_rx.recv() => {
                let reason = maybe_reason.unwrap_or_else(|| {
                    format!("node lock task for node_id `{}` stopped unexpectedly", identity.node_id)
                });
                error!(%reason, node_id = %identity.node_id, "node lock lost, stopping service");
                if cancel_pending_shutdown(&mut pending_shutdown) {
                    info!("cleared pending shutdown because node lock was lost");
                    if connected {
                        sync_shutdown_state(
                            &client,
                            &topics,
                            &config,
                            pending_shutdown.as_ref(),
                            &mut published_states,
                        )
                        .await;
                    }
                }
                if connected {
                    publish_availability(&client, &topics, false).await?;
                }
                client.disconnect().await?;
                node_lock_guard.disconnect().await;
                break;
            }
            _ = &mut shutdown_signal => {
                info!("shutdown signal received");
                if cancel_pending_shutdown(&mut pending_shutdown) {
                    info!("cleared pending shutdown because service is stopping");
                    if connected {
                        sync_shutdown_state(
                            &client,
                            &topics,
                            &config,
                            pending_shutdown.as_ref(),
                            &mut published_states,
                        )
                        .await;
                    }
                }
                if connected {
                    publish_availability(&client, &topics, false).await?;
                }
                client.disconnect().await?;
                node_lock_guard.release().await;
                break;
            }
            event = eventloop.poll() => {
                match event {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        connected = true;
                        availability_online_pending = true;
                        subscription_state.prepare_for_connection(config.enable_shutdown_button);
                        info!("MQTT connected");
                        ensure_runtime_subscriptions(
                            &client,
                            &topics,
                            &mut subscription_state,
                        )
                        .await;

                        let (
                            cpu_state,
                            uptime_state,
                            gpu_state,
                            memory_state,
                            disk_state,
                            network_state,
                        ) = collector.sample_all();
                        let static_snapshot = collect_static_snapshot(&collector);
                        publish_full_snapshot(
                            &publish_context,
                            static_snapshot,
                            FullSnapshot {
                                cpu_state,
                                uptime_state,
                                shutdown_state: current_shutdown_state(pending_shutdown.as_ref()),
                                gpu_state,
                                memory_state,
                                disk_state,
                                network_state,
                            },
                            &mut discovery_state,
                            &mut published_states,
                            &mut availability_online_pending,
                            true,
                        )
                        .await;
                    }
                    Ok(Event::Incoming(Packet::SubAck(suback))) => {
                        let success = suback
                            .return_codes
                            .iter()
                            .all(|code| !matches!(code, SubscribeReasonCode::Failure));
                        if !success {
                            warn!(pkid = suback.pkid, ?suback.return_codes, "subscription rejected by broker");
                        }
                        subscription_state.handle_suback(suback.pkid, success);
                    }
                    Ok(Event::Incoming(Packet::Publish(publish))) => {
                        if config.enable_shutdown_button
                            && publish.topic == topics.shutdown_command
                        {
                            match parse_shutdown_command(
                                &config.shutdown_payload,
                                &config.shutdown_cancel_payload,
                                publish.payload.as_ref(),
                            ) {
                                Some(ShutdownCommandKind::Schedule) => {
                                    if config.shutdown_delay_secs == 0 {
                                        warn!("immediate shutdown command received from MQTT");
                                        execute_shutdown_request(config.shutdown_dry_run).await;
                                    } else {
                                        let replaced = schedule_shutdown(
                                            &mut pending_shutdown,
                                            &mut next_shutdown_request_id,
                                            config.shutdown_delay_secs,
                                            &shutdown_elapsed_tx,
                                        );
                                        warn!(
                                            delay_secs = config.shutdown_delay_secs,
                                            replaced_existing = replaced,
                                            "scheduled shutdown command received from MQTT"
                                        );
                                        sync_shutdown_state(
                                            &client,
                                            &topics,
                                            &config,
                                            pending_shutdown.as_ref(),
                                            &mut published_states,
                                        )
                                        .await;
                                    }

                                    continue;
                                }
                                Some(ShutdownCommandKind::Cancel) => {
                                    if cancel_pending_shutdown(&mut pending_shutdown) {
                                        warn!("pending shutdown canceled from MQTT");
                                    } else {
                                        info!("shutdown cancel requested, but no pending shutdown exists");
                                    }
                                    sync_shutdown_state(
                                        &client,
                                        &topics,
                                        &config,
                                        pending_shutdown.as_ref(),
                                        &mut published_states,
                                    )
                                    .await;

                                    continue;
                                }
                                None => {}
                            }
                        }

                        if !is_home_assistant_birth_message(&topics, &publish) {
                            continue;
                        }

                        info!("Home Assistant birth message received, refreshing discovery");
                        tokio::time::sleep(discovery_birth_delay(&identity.node_id)).await;

                        let (
                            cpu_state,
                            uptime_state,
                            gpu_state,
                            memory_state,
                            disk_state,
                            network_state,
                        ) = collector.sample_all();
                        let static_snapshot = collect_static_snapshot(&collector);
                        publish_full_snapshot(
                            &publish_context,
                            static_snapshot,
                            FullSnapshot {
                                cpu_state,
                                uptime_state,
                                shutdown_state: current_shutdown_state(pending_shutdown.as_ref()),
                                gpu_state,
                                memory_state,
                                disk_state,
                                network_state,
                            },
                            &mut discovery_state,
                            &mut published_states,
                            &mut availability_online_pending,
                            true,
                        )
                        .await;
                    }
                    Ok(Event::Outgoing(Outgoing::Subscribe(pkid))) => {
                        subscription_state.mark_request_sent(pkid);
                    }
                    Ok(Event::Outgoing(Outgoing::Disconnect)) => {
                        connected = false;
                        availability_online_pending = false;
                        subscription_state.reset_runtime();
                        info!("MQTT disconnected");
                    }
                    Ok(other) => {
                        debug!(event = ?other, "ignoring MQTT event");
                    }
                    Err(error) => {
                        connected = false;
                        availability_online_pending = false;
                        subscription_state.reset_runtime();
                        warn!(%error, "MQTT event loop error, waiting before retry");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
            _ = cpu_interval.tick() => {
                if !connected {
                    continue;
                }

                let cpu_state = collector.sample_cpu();
                ensure_runtime_subscriptions(&client, &topics, &mut subscription_state).await;
                let changed = published_states
                    .cpu
                    .state
                    .as_ref()
                    .is_none_or(|previous| {
                        cpu_state.changed_significantly_from(previous, config.cpu_change_threshold_pct)
                    });
                publish_online_availability_if_needed(
                    &client,
                    &topics,
                    &mut availability_online_pending,
                )
                .await;
                if !changed {
                    continue;
                }

                if let Err(error) = publish_cpu_state(&client, &topics, &cpu_state).await {
                    error!(%error, "failed to publish CPU state");
                    published_states.cpu.clear();
                } else {
                    published_states.cpu.mark_published(cpu_state);
                }
            }
            _ = uptime_interval.tick() => {
                if !connected {
                    continue;
                }

                let uptime_state = collector.sample_uptime();
                ensure_runtime_subscriptions(&client, &topics, &mut subscription_state).await;
                let changed = published_states
                    .uptime
                    .state
                    .as_ref()
                    .is_none_or(|previous| uptime_state.changed_from(previous));
                publish_online_availability_if_needed(
                    &client,
                    &topics,
                    &mut availability_online_pending,
                )
                .await;
                if !changed {
                    continue;
                }

                if let Err(error) = publish_uptime_state(&client, &topics, &uptime_state).await {
                    error!(%error, "failed to publish uptime state");
                    published_states.uptime.clear();
                } else {
                    published_states.uptime.mark_published(uptime_state);
                }
            }
            _ = shutdown_interval.tick() => {
                if !connected || !config.enable_shutdown_button || config.shutdown_delay_secs == 0 {
                    continue;
                }

                let shutdown_state = current_shutdown_state(pending_shutdown.as_ref());
                ensure_runtime_subscriptions(&client, &topics, &mut subscription_state).await;
                let changed = published_states
                    .shutdown
                    .state
                    .as_ref()
                    .is_none_or(|previous| shutdown_state != *previous);
                publish_online_availability_if_needed(
                    &client,
                    &topics,
                    &mut availability_online_pending,
                )
                .await;
                if !changed {
                    continue;
                }

                if let Err(error) = publish_shutdown_state(&client, &topics, &shutdown_state).await {
                    error!(%error, "failed to publish shutdown state");
                    published_states.shutdown.clear();
                } else {
                    published_states.shutdown.mark_published(shutdown_state);
                }
            }
            _ = gpu_interval.tick() => {
                if !connected {
                    continue;
                }

                ensure_runtime_subscriptions(&client, &topics, &mut subscription_state).await;
                publish_online_availability_if_needed(
                    &client,
                    &topics,
                    &mut availability_online_pending,
                )
                .await;
                if let Some(gpu_state) = collector.sample_gpu() {
                    let changed = published_states
                        .gpu
                        .state
                        .as_ref()
                        .is_none_or(|previous| {
                            gpu_state.changed_significantly_from(
                                previous,
                                config.gpu_usage_change_threshold_pct,
                                config.gpu_memory_change_threshold_bytes(),
                            )
                        });
                    if !changed {
                        continue;
                    }

                    if let Err(error) = publish_gpu_state(&client, &topics, &gpu_state).await {
                        error!(%error, "failed to publish GPU state");
                        published_states.gpu.clear();
                    } else {
                        published_states.gpu.mark_published(gpu_state);
                    }
                } else {
                    published_states.gpu.clear();
                }
            }
            _ = memory_interval.tick() => {
                if !connected {
                    continue;
                }

                let memory_state = collector.sample_memory();
                ensure_runtime_subscriptions(&client, &topics, &mut subscription_state).await;
                let changed = published_states
                    .memory
                    .state
                    .as_ref()
                    .is_none_or(|previous| {
                        memory_state.changed_significantly_from(
                            previous,
                            config.memory_change_threshold_bytes(),
                        )
                    });
                publish_online_availability_if_needed(
                    &client,
                    &topics,
                    &mut availability_online_pending,
                )
                .await;
                if !changed {
                    continue;
                }

                if let Err(error) = publish_memory_state(&client, &topics, &memory_state).await {
                    error!(%error, "failed to publish memory state");
                    published_states.memory.clear();
                } else {
                    published_states.memory.mark_published(memory_state);
                }
            }
            _ = disk_interval.tick() => {
                if !connected {
                    continue;
                }

                let disk_state = collector.sample_disks();
                ensure_runtime_subscriptions(&client, &topics, &mut subscription_state).await;
                let changed = published_states
                    .disk
                    .state
                    .as_ref()
                    .is_none_or(|previous| {
                        disk_state.changed_significantly_from(
                            previous,
                            config.disk_change_threshold_bytes(),
                        )
                    });

                publish_online_availability_if_needed(
                    &client,
                    &topics,
                    &mut availability_online_pending,
                )
                .await;

                if !changed {
                    continue;
                }

                if let Err(error) = publish_disk_state(&client, &topics, &disk_state).await {
                    error!(%error, "failed to publish disk state");
                    published_states.disk.clear();
                } else {
                    published_states.disk.mark_published(disk_state);
                }
            }
            _ = network_interval.tick() => {
                if !connected {
                    continue;
                }

                let network_state = collector.sample_network();
                ensure_runtime_subscriptions(&client, &topics, &mut subscription_state).await;
                let changed = published_states
                    .network
                    .state
                    .as_ref()
                    .is_none_or(|previous| network_state.changed_from(previous));

                publish_online_availability_if_needed(
                    &client,
                    &topics,
                    &mut availability_online_pending,
                )
                .await;

                if !changed {
                    continue;
                }

                if let Err(error) = publish_network_state(&client, &topics, &network_state).await {
                    error!(%error, "failed to publish network state");
                    published_states.network.clear();
                } else {
                    published_states.network.mark_published(network_state);
                }
            }
        }
    }

    Ok(())
}

async fn acquire_node_lock(
    config: &Config,
    topics: &Topics,
    identity: &Identity,
) -> Result<(NodeLockGuard, UnboundedReceiver<String>)> {
    let instance_id = build_lock_instance_id(identity);
    let claiming_payload = NodeLockPayload::new(identity, &instance_id, NodeLockStatus::Claiming);
    let online_payload = NodeLockPayload::new(identity, &instance_id, NodeLockStatus::Online);
    let offline_payload = NodeLockPayload::new(identity, &instance_id, NodeLockStatus::Offline);
    let offline_payload_bytes = serde_json::to_vec(&offline_payload)
        .context("failed to serialize offline node lock payload")?;

    let mut mqtt_options = build_lock_mqtt_options(
        config,
        format!("ha-system-ronitor-lock-{instance_id}"),
        offline_payload_bytes.clone(),
        topics,
    );
    mqtt_options.set_keep_alive(Duration::from_secs(10));

    let (client, mut eventloop) = AsyncClient::new(mqtt_options, 32);
    client
        .subscribe(topics.node_lock.clone(), QoS::AtLeastOnce)
        .await
        .with_context(|| format!("failed to subscribe node lock topic `{}`", topics.node_lock))?;

    if let Some(existing_lock) =
        wait_for_latest_lock_payload(&mut eventloop, &topics.node_lock, NODE_LOCK_SYNC_TIMEOUT)
            .await?
        && existing_lock.is_online_for_other_instance(&instance_id)
    {
        return Err(anyhow!(
            "node_id `{}` is already locked by instance `{}` on host `{}`",
            identity.node_id,
            existing_lock.instance_id,
            existing_lock.host_name
        ));
    }

    let claiming_payload_bytes = serde_json::to_vec(&claiming_payload)
        .context("failed to serialize claiming node lock payload")?;
    client
        .publish(
            topics.node_lock.clone(),
            QoS::AtLeastOnce,
            true,
            claiming_payload_bytes,
        )
        .await
        .with_context(|| {
            format!(
                "failed to publish claiming lock payload to `{}`",
                topics.node_lock
            )
        })?;

    let mut saw_self_claim = false;
    let mut has_higher_priority_foreign_claim = false;
    let claim_deadline = Instant::now() + NODE_LOCK_CLAIM_WINDOW;
    while let Some(lock_payload) =
        next_lock_payload_until(&mut eventloop, &topics.node_lock, claim_deadline).await?
    {
        if lock_payload.instance_id == instance_id
            && lock_payload.status == NodeLockStatus::Claiming
        {
            saw_self_claim = true;
            continue;
        }

        if lock_payload.is_online_for_other_instance(&instance_id) {
            client.disconnect().await.ok();
            return Err(anyhow!(
                "node_id `{}` became locked by instance `{}` while claiming",
                identity.node_id,
                lock_payload.instance_id
            ));
        }

        if lock_payload.is_claiming_for_other_instance(&instance_id)
            && lock_payload.instance_id < instance_id
        {
            has_higher_priority_foreign_claim = true;
        }
    }

    if !saw_self_claim {
        publish_node_lock_payload(&client, topics, &offline_payload).await?;
        client.disconnect().await.ok();
        return Err(anyhow!(
            "failed to confirm node lock claim for node_id `{}`",
            identity.node_id
        ));
    }

    if has_higher_priority_foreign_claim {
        client.disconnect().await.ok();
        return Err(anyhow!(
            "node_id `{}` claim lost to another concurrently starting instance",
            identity.node_id
        ));
    }

    publish_node_lock_payload(&client, topics, &online_payload).await?;

    let mut saw_self_online = false;
    let confirm_deadline = Instant::now() + NODE_LOCK_CONFIRM_TIMEOUT;
    while let Some(lock_payload) =
        next_lock_payload_until(&mut eventloop, &topics.node_lock, confirm_deadline).await?
    {
        if lock_payload.instance_id == instance_id && lock_payload.status == NodeLockStatus::Online
        {
            saw_self_online = true;
            continue;
        }

        if lock_payload.is_online_for_other_instance(&instance_id) {
            client.disconnect().await.ok();
            return Err(anyhow!(
                "node_id `{}` lock was overwritten by instance `{}` during confirmation",
                identity.node_id,
                lock_payload.instance_id
            ));
        }
    }

    if !saw_self_online {
        publish_node_lock_payload(&client, topics, &offline_payload).await?;
        client.disconnect().await.ok();
        return Err(anyhow!(
            "failed to confirm online node lock for node_id `{}`",
            identity.node_id
        ));
    }

    let (loss_tx, loss_rx) = mpsc::unbounded_channel();
    let lock_topic = topics.node_lock.clone();
    let instance_id_for_task = instance_id.clone();
    let eventloop_task = tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::Publish(publish))) if publish.topic == lock_topic => {
                    match parse_node_lock_payload(&publish.payload) {
                        Ok(lock_payload)
                            if lock_payload.is_online_for_other_instance(&instance_id_for_task) =>
                        {
                            let _ = loss_tx.send(format!(
                                "node lock `{}` was overwritten by instance `{}` on host `{}`",
                                lock_topic, lock_payload.instance_id, lock_payload.host_name
                            ));
                            break;
                        }
                        Ok(_) => {}
                        Err(error) => {
                            let _ = loss_tx.send(format!(
                                "failed to parse node lock payload on `{}`: {error:#}",
                                lock_topic
                            ));
                            break;
                        }
                    }
                }
                Ok(Event::Outgoing(Outgoing::Disconnect)) => break,
                Ok(_) => {}
                Err(error) => {
                    let _ = loss_tx.send(format!(
                        "node lock MQTT connection failed for topic `{}`: {error:#}",
                        lock_topic
                    ));
                    break;
                }
            }
        }
    });

    info!(
        node_id = %identity.node_id,
        instance_id = %instance_id,
        topic = %topics.node_lock,
        "acquired node lock"
    );

    Ok((
        NodeLockGuard {
            client,
            lock_topic: topics.node_lock.clone(),
            offline_payload: offline_payload_bytes,
            eventloop_task,
        },
        loss_rx,
    ))
}

async fn publish_node_lock_payload(
    client: &AsyncClient,
    topics: &Topics,
    payload: &NodeLockPayload,
) -> Result<()> {
    let payload = serde_json::to_vec(payload).context("failed to serialize node lock payload")?;
    client
        .publish(topics.node_lock.clone(), QoS::AtLeastOnce, true, payload)
        .await
        .with_context(|| {
            format!(
                "failed to publish node lock payload to `{}`",
                topics.node_lock
            )
        })?;
    Ok(())
}

async fn wait_for_latest_lock_payload(
    eventloop: &mut rumqttc::EventLoop,
    lock_topic: &str,
    duration: Duration,
) -> Result<Option<NodeLockPayload>> {
    let deadline = Instant::now() + duration;
    let mut latest = None;

    while let Some(lock_payload) = next_lock_payload_until(eventloop, lock_topic, deadline).await? {
        latest = Some(lock_payload);
    }

    Ok(latest)
}

async fn next_lock_payload_until(
    eventloop: &mut rumqttc::EventLoop,
    lock_topic: &str,
    deadline: Instant,
) -> Result<Option<NodeLockPayload>> {
    loop {
        let Some(timeout) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(None);
        };

        match tokio::time::timeout(timeout, eventloop.poll()).await {
            Ok(Ok(Event::Incoming(Packet::Publish(publish)))) if publish.topic == lock_topic => {
                return parse_node_lock_payload(&publish.payload).map(Some);
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                return Err(error).context("failed while polling node lock MQTT events");
            }
            Err(_) => return Ok(None),
        }
    }
}

fn parse_node_lock_payload(payload: &[u8]) -> Result<NodeLockPayload> {
    serde_json::from_slice(payload).context("invalid node lock payload")
}

fn build_lock_instance_id(identity: &Identity) -> String {
    let started_at_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let host_slug = match slugify(&identity.host_name) {
        slug if slug.is_empty() => "host".to_string(),
        slug => slug,
    };

    format!(
        "{started_at_nanos:032x}-{:08x}-{host_slug}",
        std::process::id()
    )
}

async fn execute_shutdown_request(dry_run: bool) {
    match tokio::task::spawn_blocking(move || shutdown_host(dry_run)).await {
        Ok(Ok(())) => {
            if dry_run {
                warn!("shutdown dry-run enabled; host shutdown skipped");
            } else {
                warn!("shutdown command executed");
            }
        }
        Ok(Err(error)) => error!(%error, "failed to execute shutdown command"),
        Err(error) => error!(%error, "shutdown task failed"),
    }
}

fn parse_shutdown_command(
    shutdown_payload: &str,
    cancel_payload: &str,
    payload: &[u8],
) -> Option<ShutdownCommandKind> {
    if payload == shutdown_payload.as_bytes() {
        Some(ShutdownCommandKind::Schedule)
    } else if payload == cancel_payload.as_bytes() {
        Some(ShutdownCommandKind::Cancel)
    } else {
        None
    }
}

fn schedule_shutdown(
    pending_shutdown: &mut Option<PendingShutdown>,
    next_request_id: &mut u64,
    delay_secs: u64,
    elapsed_tx: &UnboundedSender<ShutdownElapsed>,
) -> bool {
    let replaced = cancel_pending_shutdown(pending_shutdown);
    *next_request_id = next_request_id.wrapping_add(1);
    let request_id = *next_request_id;
    let deadline = Instant::now() + Duration::from_secs(delay_secs);
    let elapsed_tx = elapsed_tx.clone();
    let task = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
        let _ = elapsed_tx.send(ShutdownElapsed {
            request_id,
            delay_secs,
        });
    });

    *pending_shutdown = Some(PendingShutdown {
        request_id,
        deadline,
        task,
    });
    replaced
}

fn cancel_pending_shutdown(pending_shutdown: &mut Option<PendingShutdown>) -> bool {
    let Some(pending_shutdown) = pending_shutdown.take() else {
        return false;
    };

    pending_shutdown.task.abort();
    true
}

fn current_shutdown_state(pending_shutdown: Option<&PendingShutdown>) -> ShutdownState {
    ShutdownState {
        shutdown_remaining_secs: pending_shutdown.map(remaining_shutdown_secs).unwrap_or(0),
    }
}

async fn sync_shutdown_state(
    client: &AsyncClient,
    topics: &Topics,
    config: &Config,
    pending_shutdown: Option<&PendingShutdown>,
    published_states: &mut PublishedStates,
) {
    if !config.enable_shutdown_button || config.shutdown_delay_secs == 0 {
        published_states.shutdown.clear();
        return;
    }

    let shutdown_state = current_shutdown_state(pending_shutdown);
    if let Err(error) = publish_shutdown_state(client, topics, &shutdown_state).await {
        error!(%error, "failed to publish shutdown state");
        published_states.shutdown.clear();
    } else {
        published_states.shutdown.mark_published(shutdown_state);
    }
}

fn remaining_shutdown_secs(pending_shutdown: &PendingShutdown) -> u64 {
    let remaining = pending_shutdown
        .deadline
        .saturating_duration_since(Instant::now());
    let secs = remaining.as_secs();
    if remaining.subsec_nanos() == 0 {
        secs
    } else {
        secs.saturating_add(1)
    }
}

fn init_tracing(bootstrap: &BootstrapOptions) -> Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,rumqttc=warn".into());

    if let Some(log_dir) = bootstrap.log_dir.as_ref() {
        fs::create_dir_all(log_dir)
            .with_context(|| format!("creating log directory `{}`", log_dir.display()))?;
        let file_appender = tracing_appender::rolling::daily(log_dir, "ha-system-ronitor.log");
        let (writer, guard) = tracing_appender::non_blocking(file_appender);
        static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
        let _ = LOG_GUARD.set(guard);

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_ansi(false)
            .compact()
            .with_writer(writer)
            .try_init()
            .ok();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .compact()
            .try_init()
            .ok();
    }

    Ok(())
}

fn discovery_birth_delay(node_id: &str) -> Duration {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    node_id.hash(&mut hasher);
    let jitter_ms = 250 + (hasher.finish() % 1_251);
    Duration::from_millis(jitter_ms)
}

async fn ensure_runtime_subscriptions(
    client: &AsyncClient,
    topics: &Topics,
    subscription_state: &mut SubscriptionState,
) {
    if subscription_state.should_request(SubscriptionTarget::HomeAssistantStatus) {
        if let Err(error) = client
            .subscribe(topics.ha_status.clone(), QoS::AtLeastOnce)
            .await
        {
            error!(
                %error,
                topic = %topics.ha_status,
                "failed to subscribe Home Assistant status topic"
            );
        } else {
            subscription_state.mark_request_queued(SubscriptionTarget::HomeAssistantStatus);
        }
    }

    if subscription_state.should_request(SubscriptionTarget::ShutdownCommand) {
        if let Err(error) = client
            .subscribe(topics.shutdown_command.clone(), QoS::AtLeastOnce)
            .await
        {
            error!(
                %error,
                topic = %topics.shutdown_command,
                "failed to subscribe shutdown command topic"
            );
        } else {
            subscription_state.mark_request_queued(SubscriptionTarget::ShutdownCommand);
        }
    }
}

async fn publish_online_availability_if_needed(
    client: &AsyncClient,
    topics: &Topics,
    availability_online_pending: &mut bool,
) {
    if !*availability_online_pending {
        return;
    }

    if let Err(error) = publish_availability(client, topics, true).await {
        error!(%error, "failed to publish online availability");
    } else {
        *availability_online_pending = false;
    }
}

async fn publish_full_snapshot(
    context: &PublishContext<'_>,
    static_snapshot: StaticSnapshot,
    snapshot: FullSnapshot,
    discovery_state: &mut DiscoveryState,
    published_states: &mut PublishedStates,
    availability_online_pending: &mut bool,
    force_discovery: bool,
) {
    let FullSnapshot {
        cpu_state,
        uptime_state,
        shutdown_state,
        gpu_state,
        memory_state,
        disk_state,
        network_state,
    } = snapshot;

    publish_online_availability_if_needed(
        context.client,
        context.topics,
        availability_online_pending,
    )
    .await;
    if let Err(error) = publish_discovery_if_needed(
        context.client,
        DiscoveryPublishArgs {
            config: context.config,
            identity: context.identity,
            topics: context.topics,
            gpu_info: static_snapshot.gpu_info.as_ref(),
            disk_info: &static_snapshot.disk_info,
            network_info: &static_snapshot.network_info,
        },
        &mut discovery_state.last_payload,
        force_discovery,
    )
    .await
    {
        error!(%error, "failed to publish discovery payload");
        discovery_state.last_payload = None;
    }
    publish_static_info(context.client, context.topics, &static_snapshot).await;

    if let Err(error) = publish_cpu_state(context.client, context.topics, &cpu_state).await {
        error!(%error, "failed to publish CPU state");
        published_states.cpu.clear();
    } else {
        published_states.cpu.mark_published(cpu_state);
    }

    if let Err(error) = publish_uptime_state(context.client, context.topics, &uptime_state).await {
        error!(%error, "failed to publish uptime state");
        published_states.uptime.clear();
    } else {
        published_states.uptime.mark_published(uptime_state);
    }

    if context.config.enable_shutdown_button && context.config.shutdown_delay_secs > 0 {
        if let Err(error) =
            publish_shutdown_state(context.client, context.topics, &shutdown_state).await
        {
            error!(%error, "failed to publish shutdown state");
            published_states.shutdown.clear();
        } else {
            published_states.shutdown.mark_published(shutdown_state);
        }
    } else {
        published_states.shutdown.clear();
    }

    if let Some(gpu_state) = gpu_state {
        if let Err(error) = publish_gpu_state(context.client, context.topics, &gpu_state).await {
            error!(%error, "failed to publish GPU state");
            published_states.gpu.clear();
        } else {
            published_states.gpu.mark_published(gpu_state);
        }
    } else {
        published_states.gpu.clear();
    }

    if let Err(error) = publish_memory_state(context.client, context.topics, &memory_state).await {
        error!(%error, "failed to publish memory state");
        published_states.memory.clear();
    } else {
        published_states.memory.mark_published(memory_state);
    }

    if let Err(error) = publish_disk_state(context.client, context.topics, &disk_state).await {
        error!(%error, "failed to publish disk state");
        published_states.disk.clear();
    } else {
        published_states.disk.mark_published(disk_state);
    }

    if let Err(error) = publish_network_state(context.client, context.topics, &network_state).await
    {
        error!(%error, "failed to publish network state");
        published_states.network.clear();
    } else {
        published_states.network.mark_published(network_state);
    }
}

async fn publish_static_info(client: &AsyncClient, topics: &Topics, snapshot: &StaticSnapshot) {
    if let Err(error) = publish_host_info_state(client, topics, &snapshot.host_info).await {
        error!(%error, "failed to publish host info state");
    }

    if let Err(error) = publish_cpu_info_state(client, topics, &snapshot.cpu_info).await {
        error!(%error, "failed to publish CPU info state");
    }

    if let Some(gpu_info) = snapshot.gpu_info.as_ref()
        && let Err(error) = publish_gpu_info_state(client, topics, gpu_info).await
    {
        error!(%error, "failed to publish GPU info state");
    }

    if let Err(error) = publish_memory_info_state(client, topics, &snapshot.memory_info).await {
        error!(%error, "failed to publish memory info state");
    }

    if let Err(error) = publish_disk_info_state(client, topics, &snapshot.disk_info).await {
        error!(%error, "failed to publish disk info state");
    }

    if let Err(error) = publish_network_info_state(client, topics, &snapshot.network_info).await {
        error!(%error, "failed to publish network info state");
    }
}

fn collect_static_snapshot(collector: &Collector) -> StaticSnapshot {
    StaticSnapshot {
        host_info: collector.host_info(),
        cpu_info: collector.cpu_info(),
        gpu_info: collector.gpu_info(),
        memory_info: collector.memory_info(),
        disk_info: collector.disk_info(),
        network_info: collector.network_info(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SUBSCRIPTION_RETRY_INTERVAL, ShutdownCommandKind, SubscriptionState, SubscriptionTarget,
        parse_shutdown_command,
    };
    use std::time::Instant;

    #[test]
    fn subscription_state_tracks_successful_suback() {
        let mut state = SubscriptionState::default();
        state.prepare_for_connection(false);

        assert!(state.should_request(SubscriptionTarget::HomeAssistantStatus));

        state.mark_request_queued(SubscriptionTarget::HomeAssistantStatus);
        state.mark_request_sent(7);
        state.handle_suback(7, true);

        assert!(!state.should_request(SubscriptionTarget::HomeAssistantStatus));
        assert!(state.ha_status.subscribed);
    }

    #[test]
    fn subscription_state_retries_after_failed_suback_backoff() {
        let mut state = SubscriptionState::default();
        state.prepare_for_connection(false);

        state.mark_request_queued(SubscriptionTarget::HomeAssistantStatus);
        state.mark_request_sent(9);
        state.handle_suback(9, false);

        assert!(!state.should_request(SubscriptionTarget::HomeAssistantStatus));

        state.ha_status.last_request_at = Some(Instant::now() - SUBSCRIPTION_RETRY_INTERVAL);
        assert!(state.should_request(SubscriptionTarget::HomeAssistantStatus));
    }

    #[test]
    fn shutdown_command_parser_distinguishes_schedule_and_cancel() {
        assert_eq!(
            parse_shutdown_command("shutdown", "cancel", b"shutdown"),
            Some(ShutdownCommandKind::Schedule)
        );
        assert_eq!(
            parse_shutdown_command("shutdown", "cancel", b"cancel"),
            Some(ShutdownCommandKind::Cancel)
        );
        assert_eq!(parse_shutdown_command("shutdown", "cancel", b"noop"), None);
    }
}

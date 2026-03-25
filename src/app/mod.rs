use std::collections::VecDeque;
use std::fs;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use rumqttc::mqttbytes::v4::SubscribeReasonCode;
use rumqttc::{AsyncClient, Event, Outgoing, Packet, QoS};
use tokio::time::MissedTickBehavior;
use tracing::{debug, error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;

use crate::config::{BootstrapOptions, Config, load_config};
use crate::device::{Identity, Topics};
use crate::integrations::mqtt::{
    DiscoveryPublishArgs, build_mqtt_options, is_home_assistant_birth_message,
    publish_availability, publish_cpu_state, publish_discovery_if_needed, publish_disk_state,
    publish_gpu_state, publish_memory_state, publish_uptime_state,
};
use crate::system::collector::Collector;
use crate::system::models::{CpuState, DiskState, GpuState, MemoryState, UptimeState};
use crate::system::power::shutdown_host;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoveryLayout {
    has_gpu: bool,
    has_gpu_temperature: bool,
    has_gpu_memory: bool,
    disks: Vec<(String, String)>,
}

#[derive(Debug, Default)]
struct DiscoveryState {
    last_payload: Option<Vec<u8>>,
    last_layout: Option<DiscoveryLayout>,
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
    gpu_state: Option<GpuState>,
    memory_state: MemoryState,
    disk_state: DiskState,
}

const SUBSCRIPTION_RETRY_INTERVAL: Duration = Duration::from_secs(5);

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
    gpu: PublishedSlot<GpuState>,
    memory: PublishedSlot<MemoryState>,
    disk: PublishedSlot<DiskState>,
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

impl DiscoveryLayout {
    fn from_states(gpu_state: Option<&GpuState>, disk_state: &DiskState) -> Self {
        Self {
            has_gpu: gpu_state.is_some(),
            has_gpu_temperature: gpu_state.is_some_and(|state| state.gpu_temperature.is_some()),
            has_gpu_memory: gpu_state.is_some_and(|state| state.gpu_memory_total > 0),
            disks: disk_state
                .disks
                .iter()
                .map(|(disk_id, disk)| (disk_id.clone(), disk.mount_point.clone()))
                .collect(),
        }
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
    let topics = Topics::from_config(&config, &identity.node_id);

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
        cpu_change_threshold_pct = config.cpu_change_threshold_pct,
        gpu_usage_change_threshold_pct = config.gpu_usage_change_threshold_pct,
        gpu_memory_change_threshold_mib = config.gpu_memory_change_threshold_mib,
        memory_change_threshold_mib = config.memory_change_threshold_mib,
        disk_change_threshold_mib = config.disk_change_threshold_mib,
        enable_shutdown_button = config.enable_shutdown_button,
        shutdown_dry_run = config.shutdown_dry_run,
        "starting Home Assistant system monitor"
    );

    let mut collector = Collector::new(&identity).await;
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

    let mut disk_interval = tokio::time::interval(Duration::from_secs(config.disk_interval_secs));
    disk_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut connected = false;
    let mut availability_online_pending = false;
    let mut subscription_state = SubscriptionState::default();
    let mut shutdown_signal = Box::pin(shutdown_signal);

    loop {
        tokio::select! {
            _ = &mut shutdown_signal => {
                info!("shutdown signal received");
                if connected {
                    publish_availability(&client, &topics, false).await?;
                }
                client.disconnect().await?;
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

                        let (cpu_state, uptime_state, gpu_state, memory_state, disk_state) =
                            collector.sample_all();
                        publish_full_snapshot(
                            &publish_context,
                            FullSnapshot {
                                cpu_state,
                                uptime_state,
                                gpu_state,
                                memory_state,
                                disk_state,
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
                            && publish.payload.as_ref() == config.shutdown_payload.as_bytes()
                        {
                            warn!("shutdown command received from MQTT");

                            match tokio::task::spawn_blocking({
                                let dry_run = config.shutdown_dry_run;
                                move || shutdown_host(dry_run)
                            }).await {
                                Ok(Ok(())) => {
                                    if config.shutdown_dry_run {
                                        warn!("shutdown dry-run enabled; host shutdown skipped");
                                    } else {
                                        warn!("shutdown command executed");
                                    }
                                }
                                Ok(Err(error)) => error!(%error, "failed to execute shutdown command"),
                                Err(error) => error!(%error, "shutdown task failed"),
                            }

                            continue;
                        }

                        if !is_home_assistant_birth_message(&topics, &publish) {
                            continue;
                        }

                        info!("Home Assistant birth message received, refreshing discovery");
                        tokio::time::sleep(discovery_birth_delay(&identity.node_id)).await;

                        let (cpu_state, uptime_state, gpu_state, memory_state, disk_state) =
                            collector.sample_all();
                        publish_full_snapshot(
                            &publish_context,
                            FullSnapshot {
                                cpu_state,
                                uptime_state,
                                gpu_state,
                                memory_state,
                                disk_state,
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
                    if let Some(disk_state) = published_states.disk.state.as_ref() {
                        refresh_discovery_layout(
                            &publish_context,
                            Some(&gpu_state),
                            disk_state,
                            &mut discovery_state,
                            false,
                        )
                        .await;
                    }

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
                    if let Some(disk_state) = published_states.disk.state.as_ref() {
                        refresh_discovery_layout(
                            &publish_context,
                            None,
                            disk_state,
                            &mut discovery_state,
                            false,
                        )
                        .await;
                    }
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
                let gpu_state_for_discovery = published_states.gpu.state.as_ref();
                let next_layout =
                    DiscoveryLayout::from_states(gpu_state_for_discovery, &disk_state);
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
                if discovery_state.last_layout.as_ref() != Some(&next_layout) {
                    refresh_discovery_layout(
                        &publish_context,
                        gpu_state_for_discovery,
                        &disk_state,
                        &mut discovery_state,
                        false,
                    )
                    .await;
                }

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
        }
    }

    Ok(())
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

async fn refresh_discovery_layout(
    context: &PublishContext<'_>,
    gpu_state: Option<&GpuState>,
    disk_state: &DiskState,
    discovery_state: &mut DiscoveryState,
    force: bool,
) {
    let next_layout = DiscoveryLayout::from_states(gpu_state, disk_state);
    if !force && discovery_state.last_layout.as_ref() == Some(&next_layout) {
        return;
    }

    if let Err(error) = publish_discovery_if_needed(
        context.client,
        DiscoveryPublishArgs {
            config: context.config,
            identity: context.identity,
            topics: context.topics,
            gpu_state,
            disk_state,
        },
        &mut discovery_state.last_payload,
        force,
    )
    .await
    {
        error!(%error, "failed to publish discovery payload");
        discovery_state.last_payload = None;
        discovery_state.last_layout = None;
    } else {
        discovery_state.last_layout = Some(next_layout);
    }
}

async fn publish_full_snapshot(
    context: &PublishContext<'_>,
    snapshot: FullSnapshot,
    discovery_state: &mut DiscoveryState,
    published_states: &mut PublishedStates,
    availability_online_pending: &mut bool,
    force_discovery: bool,
) {
    let FullSnapshot {
        cpu_state,
        uptime_state,
        gpu_state,
        memory_state,
        disk_state,
    } = snapshot;

    publish_online_availability_if_needed(
        context.client,
        context.topics,
        availability_online_pending,
    )
    .await;
    refresh_discovery_layout(
        context,
        gpu_state.as_ref(),
        &disk_state,
        discovery_state,
        force_discovery,
    )
    .await;

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
}

#[cfg(test)]
mod tests {
    use super::{SUBSCRIPTION_RETRY_INTERVAL, SubscriptionState, SubscriptionTarget};
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
}

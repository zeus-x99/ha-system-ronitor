use std::collections::VecDeque;
use std::fs;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use rumqttc::{AsyncClient, Event, Outgoing, Packet, QoS};
use tokio::time::MissedTickBehavior;
use tracing::{debug, error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;

use crate::config::{BootstrapOptions, Config, load_config_file_from};
use crate::device::{Identity, Topics};
use crate::integrations::mqtt::{
    DiscoveryPublishArgs, build_mqtt_options, is_home_assistant_birth_message,
    publish_availability, publish_cpu_state, publish_discovery_if_needed, publish_disk_state,
    publish_gpu_state, publish_memory_state,
};
use crate::system::collector::Collector;
use crate::system::models::{CpuState, DiskState, GpuState, MemoryState};
use crate::system::power::shutdown_host;

#[derive(Debug)]
struct CpuSmoother {
    samples: VecDeque<f32>,
    sum: f32,
    window: usize,
}

#[derive(Debug)]
struct PublishedSlot<T> {
    state: Option<T>,
    last_sent_at: Option<Instant>,
}

#[derive(Debug, Default)]
struct PublishedStates {
    cpu: PublishedSlot<CpuState>,
    gpu: PublishedSlot<GpuState>,
    memory: PublishedSlot<MemoryState>,
    disk: PublishedSlot<DiskState>,
}

impl CpuSmoother {
    fn new(window: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(window),
            sum: 0.0,
            window,
        }
    }

    fn smooth(&mut self, raw: CpuState) -> CpuState {
        self.samples.push_back(raw.cpu_usage);
        self.sum += raw.cpu_usage;

        while self.samples.len() > self.window {
            if let Some(removed) = self.samples.pop_front() {
                self.sum -= removed;
            }
        }

        let smoothed = if self.samples.is_empty() {
            raw.cpu_usage
        } else {
            self.sum / self.samples.len() as f32
        };

        CpuState {
            timestamp: raw.timestamp,
            cpu_usage: smoothed,
            cpu_package_temp: raw.cpu_package_temp,
            cpu_model: raw.cpu_model,
            os_version: raw.os_version,
            uptime: raw.uptime,
        }
    }
}

impl<T> PublishedSlot<T> {
    fn new() -> Self {
        Self {
            state: None,
            last_sent_at: None,
        }
    }

    fn should_force_publish(&self, max_silence: Duration) -> bool {
        self.last_sent_at
            .is_none_or(|instant| instant.elapsed() >= max_silence)
    }

    fn mark_published(&mut self, state: T) {
        self.state = Some(state);
        self.last_sent_at = Some(Instant::now());
    }
}

impl<T> Default for PublishedSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn run() -> Result<()> {
    let bootstrap = BootstrapOptions::from_current_process();
    initialize_runtime_with(&bootstrap)?;
    let config = Config::parse();
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
    let config_directories = bootstrap.config_directories();
    load_config_file_from(&config_directories)?;

    static TRACING_INIT: OnceLock<Result<(), String>> = OnceLock::new();
    match TRACING_INIT.get_or_init(|| init_tracing(bootstrap).map_err(|error| format!("{error:#}")))
    {
        Ok(()) => Ok(()),
        Err(message) => Err(anyhow!(message.clone())),
    }
}

pub fn parse_config_from<I, T>(args: I) -> Result<Config>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Config::try_parse_from(args).map_err(Into::into)
}

pub async fn run_with_config<F>(config: Config, shutdown_signal: F) -> Result<()>
where
    F: Future<Output = ()>,
{
    let identity = Identity::detect(&config);
    let topics = Topics::from_config(&config, &identity.node_id);
    let cpu_smoothing_window = config.cpu_smoothing_window.max(1);

    info!(
        device_name = %identity.device_name,
        node_id = %identity.node_id,
        config_dir = ?config.config_dir,
        log_dir = ?config.log_dir,
        cpu_interval_secs = config.cpu_interval_secs,
        gpu_interval_secs = config.gpu_interval_secs,
        memory_interval_secs = config.memory_interval_secs,
        disk_interval_secs = config.disk_interval_secs,
        cpu_change_threshold_pct = config.cpu_change_threshold_pct,
        gpu_usage_change_threshold_pct = config.gpu_usage_change_threshold_pct,
        gpu_memory_change_threshold_mib = config.gpu_memory_change_threshold_mib,
        memory_change_threshold_mib = config.memory_change_threshold_mib,
        disk_change_threshold_mib = config.disk_change_threshold_mib,
        cpu_smoothing_window = cpu_smoothing_window,
        cpu_max_silence_secs = config.cpu_max_silence_secs,
        gpu_max_silence_secs = config.gpu_max_silence_secs,
        memory_max_silence_secs = config.memory_max_silence_secs,
        disk_max_silence_secs = config.disk_max_silence_secs,
        enable_shutdown_button = config.enable_shutdown_button,
        shutdown_dry_run = config.shutdown_dry_run,
        "starting Home Assistant system monitor"
    );

    let cpu_max_silence = Duration::from_secs(config.cpu_max_silence_secs);
    let gpu_max_silence = Duration::from_secs(config.gpu_max_silence_secs);
    let memory_max_silence = Duration::from_secs(config.memory_max_silence_secs);
    let disk_max_silence = Duration::from_secs(config.disk_max_silence_secs);

    let mut collector = Collector::new(&identity).await;
    let mut cpu_smoother = CpuSmoother::new(cpu_smoothing_window);
    let mut last_discovery_payload = None;
    let mut published_states = PublishedStates::default();
    let mut mqtt_options = build_mqtt_options(&config, &identity, &topics);
    mqtt_options.set_keep_alive(Duration::from_secs(30));

    let (client, mut eventloop) = AsyncClient::new(mqtt_options, 256);

    let mut cpu_interval = tokio::time::interval(Duration::from_secs(config.cpu_interval_secs));
    cpu_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut gpu_interval = tokio::time::interval(Duration::from_secs(config.gpu_interval_secs));
    gpu_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut memory_interval =
        tokio::time::interval(Duration::from_secs(config.memory_interval_secs));
    memory_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut disk_interval = tokio::time::interval(Duration::from_secs(config.disk_interval_secs));
    disk_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut connected = false;
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
                        info!("MQTT connected");
                        client.subscribe(topics.ha_status.clone(), QoS::AtLeastOnce).await?;
                        if config.enable_shutdown_button {
                            client.subscribe(topics.shutdown_command.clone(), QoS::AtLeastOnce).await?;
                        }
                        publish_availability(&client, &topics, true).await?;

                        let (raw_cpu_state, gpu_state, memory_state, disk_state) = collector.sample_all()?;
                        let cpu_state = cpu_smoother.smooth(raw_cpu_state);

                        publish_discovery_if_needed(
                            &client,
                            DiscoveryPublishArgs {
                                config: &config,
                                identity: &identity,
                                topics: &topics,
                                gpu_state: gpu_state.as_ref(),
                                disk_state: &disk_state,
                            },
                            &mut last_discovery_payload,
                            true,
                        ).await?;
                        publish_cpu_state(&client, &topics, &cpu_state).await?;
                        if let Some(gpu_state) = &gpu_state {
                            publish_gpu_state(&client, &topics, gpu_state).await?;
                        }
                        publish_memory_state(&client, &topics, &memory_state).await?;
                        publish_disk_state(&client, &topics, &disk_state).await?;

                        published_states.cpu.mark_published(cpu_state);
                        if let Some(gpu_state) = gpu_state {
                            published_states.gpu.mark_published(gpu_state);
                        }
                        published_states.memory.mark_published(memory_state);
                        published_states.disk.mark_published(disk_state);
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

                        let (raw_cpu_state, gpu_state, memory_state, disk_state) = collector.sample_all()?;
                        let cpu_state = cpu_smoother.smooth(raw_cpu_state);

                        publish_discovery_if_needed(
                            &client,
                            DiscoveryPublishArgs {
                                config: &config,
                                identity: &identity,
                                topics: &topics,
                                gpu_state: gpu_state.as_ref(),
                                disk_state: &disk_state,
                            },
                            &mut last_discovery_payload,
                            true,
                        ).await?;
                        publish_cpu_state(&client, &topics, &cpu_state).await?;
                        if let Some(gpu_state) = &gpu_state {
                            publish_gpu_state(&client, &topics, gpu_state).await?;
                        }
                        publish_memory_state(&client, &topics, &memory_state).await?;
                        publish_disk_state(&client, &topics, &disk_state).await?;

                        published_states.cpu.mark_published(cpu_state);
                        if let Some(gpu_state) = gpu_state {
                            published_states.gpu.mark_published(gpu_state);
                        }
                        published_states.memory.mark_published(memory_state);
                        published_states.disk.mark_published(disk_state);
                    }
                    Ok(Event::Outgoing(Outgoing::Disconnect)) => {
                        connected = false;
                        info!("MQTT disconnected");
                    }
                    Ok(other) => {
                        debug!(event = ?other, "ignoring MQTT event");
                    }
                    Err(error) => {
                        connected = false;
                        warn!(%error, "MQTT event loop error, waiting before retry");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
            _ = cpu_interval.tick() => {
                if !connected {
                    continue;
                }

                let cpu_state = cpu_smoother.smooth(collector.sample_cpu());
                let changed = published_states
                    .cpu
                    .state
                    .as_ref()
                    .is_none_or(|previous| {
                        cpu_state.changed_significantly_from(previous, config.cpu_change_threshold_pct)
                    });
                let force_publish = published_states.cpu.should_force_publish(cpu_max_silence);

                if !(changed || force_publish) {
                    continue;
                }

                if let Err(error) = publish_cpu_state(&client, &topics, &cpu_state).await {
                    error!(%error, "failed to publish CPU state");
                } else {
                    published_states.cpu.mark_published(cpu_state);
                }
            }
            _ = gpu_interval.tick() => {
                if !connected {
                    continue;
                }

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
                    let force_publish = published_states.gpu.should_force_publish(gpu_max_silence);

                    if !(changed || force_publish) {
                        continue;
                    }

                    if let Err(error) = publish_gpu_state(&client, &topics, &gpu_state).await {
                        error!(%error, "failed to publish GPU state");
                    } else {
                        published_states.gpu.mark_published(gpu_state);
                    }
                }
            }
            _ = memory_interval.tick() => {
                if !connected {
                    continue;
                }

                let memory_state = collector.sample_memory();
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
                let force_publish = published_states.memory.should_force_publish(memory_max_silence);

                if !(changed || force_publish) {
                    continue;
                }

                if let Err(error) = publish_memory_state(&client, &topics, &memory_state).await {
                    error!(%error, "failed to publish memory state");
                } else {
                    published_states.memory.mark_published(memory_state);
                }
            }
            _ = disk_interval.tick() => {
                if !connected {
                    continue;
                }

                let disk_state = collector.sample_disks();
                let gpu_state_for_discovery =
                    collector.sample_gpu().or_else(|| published_states.gpu.state.clone());
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
                let force_publish = published_states.disk.should_force_publish(disk_max_silence);

                if let Err(error) = publish_discovery_if_needed(
                    &client,
                    DiscoveryPublishArgs {
                        config: &config,
                        identity: &identity,
                        topics: &topics,
                        gpu_state: gpu_state_for_discovery.as_ref(),
                        disk_state: &disk_state,
                    },
                    &mut last_discovery_payload,
                    false,
                ).await {
                    error!(%error, "failed to publish discovery payload");
                    continue;
                }

                if !(changed || force_publish) {
                    continue;
                }

                if let Err(error) = publish_disk_state(&client, &topics, &disk_state).await {
                    error!(%error, "failed to publish disk state");
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

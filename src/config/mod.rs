mod bootstrap;
mod file;
mod paths;

use std::path::PathBuf;

use anyhow::{Result, anyhow};

pub use bootstrap::BootstrapOptions;
pub use file::{
    CONFIG_EXAMPLE_FILE_NAME, CONFIG_FILE_NAME, CpuThresholdConfig, DeviceConfig, FileConfig,
    GpuThresholdConfig, HomeAssistantConfig, MetricSamplingConfig, MetricThresholdConfig,
    MqttConfig, NetworkConfig, SamplingConfig, ShutdownConfig, ThresholdsConfig,
    load_config_file_from, seed_config_toml,
};
pub use paths::{candidate_config_directories, candidate_config_directories_with};

#[derive(Debug, Clone)]
pub struct Config {
    pub config_dir: Option<PathBuf>,
    pub log_dir: Option<PathBuf>,
    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub mqtt_username: Option<String>,
    pub mqtt_password: Option<String>,
    pub discovery_prefix: String,
    pub home_assistant_status_topic: String,
    pub topic_prefix: String,
    pub node_id: Option<String>,
    pub device_name: Option<String>,
    pub network_include_interfaces: Vec<String>,
    pub enable_shutdown_button: bool,
    pub shutdown_payload: String,
    pub shutdown_dry_run: bool,
    pub cpu_interval_secs: u64,
    pub gpu_interval_secs: u64,
    pub memory_interval_secs: u64,
    pub uptime_interval_secs: u64,
    pub disk_interval_secs: u64,
    pub network_interval_secs: u64,
    pub cpu_change_threshold_pct: f32,
    pub gpu_usage_change_threshold_pct: f32,
    pub gpu_memory_change_threshold_mib: u64,
    pub memory_change_threshold_mib: u64,
    pub disk_change_threshold_mib: u64,
}

pub fn load_config(bootstrap: &BootstrapOptions) -> Result<Config> {
    let config_directories = bootstrap.config_directories();
    let file_config = load_config_file_from(&config_directories)?.ok_or_else(|| {
        let searched = config_directories
            .iter()
            .map(|path| format!("`{}`", path.join(CONFIG_FILE_NAME).display()))
            .collect::<Vec<_>>()
            .join(", ");
        anyhow!(
            "missing required configuration file `{}`; searched: {}",
            CONFIG_FILE_NAME,
            searched
        )
    })?;

    Config::from_file(bootstrap, file_config)
}

impl Config {
    fn from_file(bootstrap: &BootstrapOptions, file_config: FileConfig) -> Result<Self> {
        let FileConfig {
            mqtt,
            home_assistant,
            device,
            network,
            sampling,
            thresholds,
            shutdown,
        } = file_config;

        Ok(Self {
            config_dir: bootstrap.config_dir.clone(),
            log_dir: bootstrap.log_dir.clone(),
            mqtt_host: required_value(mqtt.host, "mqtt.host")?,
            mqtt_port: value_or_default(mqtt.port, 1883),
            mqtt_username: mqtt.username,
            mqtt_password: mqtt.password,
            discovery_prefix: value_or_default(
                home_assistant.discovery_prefix,
                "homeassistant".to_string(),
            ),
            home_assistant_status_topic: value_or_default(
                home_assistant.status_topic,
                "homeassistant/status".to_string(),
            ),
            topic_prefix: value_or_default(
                home_assistant.topic_prefix,
                "monitor/system".to_string(),
            ),
            node_id: device.node_id,
            device_name: device.name,
            network_include_interfaces: network
                .include_interfaces
                .into_iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect(),
            enable_shutdown_button: value_or_default(shutdown.enable_button, false),
            shutdown_payload: value_or_default(shutdown.payload, "shutdown".to_string()),
            shutdown_dry_run: value_or_default(shutdown.dry_run, false),
            cpu_interval_secs: value_or_default(sampling.cpu.interval_secs, 1),
            gpu_interval_secs: value_or_default(sampling.gpu.interval_secs, 1),
            memory_interval_secs: value_or_default(sampling.memory.interval_secs, 5),
            uptime_interval_secs: value_or_default(sampling.uptime.interval_secs, 300),
            disk_interval_secs: value_or_default(sampling.disk.interval_secs, 30),
            network_interval_secs: value_or_default(sampling.network.interval_secs, 1),
            cpu_change_threshold_pct: value_or_default(thresholds.cpu.usage_pct, 1.0),
            gpu_usage_change_threshold_pct: value_or_default(thresholds.gpu.usage_pct, 1.0),
            gpu_memory_change_threshold_mib: value_or_default(thresholds.gpu.memory_change_mib, 8),
            memory_change_threshold_mib: value_or_default(thresholds.memory.change_mib, 8),
            disk_change_threshold_mib: value_or_default(thresholds.disk.change_mib, 32),
        })
    }

    pub fn gpu_memory_change_threshold_bytes(&self) -> u64 {
        self.gpu_memory_change_threshold_mib * 1024 * 1024
    }

    pub fn memory_change_threshold_bytes(&self) -> u64 {
        self.memory_change_threshold_mib * 1024 * 1024
    }

    pub fn disk_change_threshold_bytes(&self) -> u64 {
        self.disk_change_threshold_mib * 1024 * 1024
    }
}

fn required_value<T>(value: Option<T>, key: &str) -> Result<T> {
    value.ok_or_else(|| {
        anyhow!("missing required configuration value `{key}` in `{CONFIG_FILE_NAME}`")
    })
}

fn value_or_default<T>(value: Option<T>, default: T) -> T {
    value.unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_config_values_are_used_and_defaults_are_preserved() {
        let file_config = FileConfig {
            mqtt: MqttConfig {
                host: Some("10.0.0.10".to_string()),
                port: Some(2883),
                username: Some("user".to_string()),
                password: Some("pass".to_string()),
            },
            home_assistant: HomeAssistantConfig {
                discovery_prefix: Some("ha".to_string()),
                status_topic: Some("ha/status".to_string()),
                topic_prefix: Some("custom/monitor".to_string()),
            },
            device: DeviceConfig {
                node_id: Some("node-a".to_string()),
                name: Some("Node A".to_string()),
            },
            network: NetworkConfig {
                include_interfaces: vec!["Ethernet".to_string(), "Wi-Fi".to_string()],
            },
            sampling: SamplingConfig {
                cpu: MetricSamplingConfig {
                    interval_secs: Some(2),
                },
                gpu: MetricSamplingConfig {
                    interval_secs: Some(3),
                },
                memory: MetricSamplingConfig {
                    interval_secs: Some(7),
                },
                uptime: MetricSamplingConfig {
                    interval_secs: Some(600),
                },
                disk: MetricSamplingConfig {
                    interval_secs: Some(45),
                },
                network: MetricSamplingConfig {
                    interval_secs: Some(1),
                },
            },
            thresholds: ThresholdsConfig {
                cpu: CpuThresholdConfig {
                    usage_pct: Some(2.5),
                },
                gpu: GpuThresholdConfig {
                    usage_pct: Some(3.5),
                    memory_change_mib: Some(16),
                },
                memory: MetricThresholdConfig {
                    change_mib: Some(12),
                },
                disk: MetricThresholdConfig {
                    change_mib: Some(64),
                },
            },
            shutdown: ShutdownConfig {
                enable_button: Some(true),
                payload: Some("poweroff".to_string()),
                dry_run: Some(true),
            },
        };

        let config = Config::from_file(
            &BootstrapOptions {
                config_dir: Some(PathBuf::from("C:/cfg")),
                log_dir: Some(PathBuf::from("C:/logs")),
            },
            file_config,
        )
        .expect("config should load");

        assert_eq!(config.config_dir, Some(PathBuf::from("C:/cfg")));
        assert_eq!(config.log_dir, Some(PathBuf::from("C:/logs")));
        assert_eq!(config.mqtt_host, "10.0.0.10");
        assert_eq!(config.mqtt_port, 2883);
        assert_eq!(config.discovery_prefix, "ha");
        assert_eq!(config.topic_prefix, "custom/monitor");
        assert_eq!(config.node_id.as_deref(), Some("node-a"));
        assert_eq!(config.network_include_interfaces, vec!["Ethernet", "Wi-Fi"]);
        assert_eq!(config.cpu_interval_secs, 2);
        assert_eq!(config.gpu_interval_secs, 3);
        assert_eq!(config.memory_interval_secs, 7);
        assert_eq!(config.uptime_interval_secs, 600);
        assert_eq!(config.disk_interval_secs, 45);
        assert_eq!(config.network_interval_secs, 1);
        assert_eq!(config.cpu_change_threshold_pct, 2.5);
        assert_eq!(config.gpu_usage_change_threshold_pct, 3.5);
        assert_eq!(config.gpu_memory_change_threshold_mib, 16);
        assert_eq!(config.memory_change_threshold_mib, 12);
        assert_eq!(config.disk_change_threshold_mib, 64);
        assert!(config.enable_shutdown_button);
        assert_eq!(config.shutdown_payload, "poweroff");
        assert!(config.shutdown_dry_run);
    }

    #[test]
    fn optional_values_fall_back_to_defaults() {
        let config = Config::from_file(
            &BootstrapOptions::default(),
            FileConfig {
                mqtt: MqttConfig {
                    host: Some("10.0.0.10".to_string()),
                    ..MqttConfig::default()
                },
                ..FileConfig::default()
            },
        )
        .expect("config should load");

        assert_eq!(config.mqtt_port, 1883);
        assert_eq!(config.discovery_prefix, "homeassistant");
        assert_eq!(config.home_assistant_status_topic, "homeassistant/status");
        assert_eq!(config.topic_prefix, "monitor/system");
        assert_eq!(config.cpu_interval_secs, 1);
        assert_eq!(config.gpu_interval_secs, 1);
        assert_eq!(config.memory_interval_secs, 5);
        assert_eq!(config.uptime_interval_secs, 300);
        assert_eq!(config.disk_interval_secs, 30);
        assert_eq!(config.network_interval_secs, 1);
        assert!(config.network_include_interfaces.is_empty());
        assert_eq!(config.cpu_change_threshold_pct, 1.0);
        assert_eq!(config.gpu_usage_change_threshold_pct, 1.0);
        assert_eq!(config.gpu_memory_change_threshold_mib, 8);
        assert_eq!(config.memory_change_threshold_mib, 8);
        assert_eq!(config.disk_change_threshold_mib, 32);
        assert!(!config.enable_shutdown_button);
        assert_eq!(config.shutdown_payload, "shutdown");
        assert!(!config.shutdown_dry_run);
    }

    #[test]
    fn missing_mqtt_host_is_rejected() {
        let error =
            Config::from_file(&BootstrapOptions::default(), FileConfig::default()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("missing required configuration value `mqtt.host`")
        );
    }
}

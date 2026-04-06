mod bootstrap;
mod file;
mod paths;

use std::path::PathBuf;

use anyhow::{Result, anyhow};

pub use bootstrap::BootstrapOptions;
pub use file::{
    CONFIG_EXAMPLE_FILE_NAME, CONFIG_FILE_NAME, CpuConfig, DeviceConfig, DiskConfig, FileConfig,
    GpuConfig, HomeAssistantConfig, HostConfig, LighthouseConfig, MemoryConfig, MqttConfig,
    NetworkConfig, ShutdownConfig, UptimeConfig, load_config_file_from, seed_config_toml,
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
    pub host_metrics_enabled: bool,
    pub cpu_metrics_enabled: bool,
    pub gpu_metrics_enabled: bool,
    pub memory_metrics_enabled: bool,
    pub uptime_metrics_enabled: bool,
    pub disk_metrics_enabled: bool,
    pub network_metrics_enabled: bool,
    pub lighthouse_enabled: bool,
    pub lighthouse_secret_id: Option<String>,
    pub lighthouse_secret_key: Option<String>,
    pub lighthouse_session_token: Option<String>,
    pub lighthouse_endpoint: String,
    pub lighthouse_region: Option<String>,
    pub lighthouse_instance_id: Option<String>,
    pub network_include_interfaces: Vec<String>,
    pub disk_include_paths: Vec<String>,
    pub enable_shutdown_button: bool,
    pub shutdown_payload: String,
    pub shutdown_cancel_payload: String,
    pub shutdown_delay_secs: u64,
    pub shutdown_dry_run: bool,
    pub cpu_interval_secs: u64,
    pub gpu_interval_secs: u64,
    pub lighthouse_interval_secs: u64,
    pub memory_interval_secs: u64,
    pub uptime_interval_secs: u64,
    pub disk_interval_secs: u64,
    pub network_interval_secs: u64,
    pub cpu_change_threshold_pct: f32,
    pub gpu_usage_change_threshold_pct: f32,
    pub gpu_memory_change_threshold_mib: u64,
    pub memory_change_threshold_mib: u64,
    pub disk_change_threshold_mib: u64,
    pub network_rate_change_threshold_bytes_per_sec: u64,
    pub network_total_change_threshold_bytes: u64,
}

const DEFAULT_LIGHTHOUSE_ENDPOINT: &str = "lighthouse.tencentcloudapi.com";
const DEFAULT_LIGHTHOUSE_INTERVAL_SECS: u64 = 300;
const DEFAULT_NETWORK_RATE_CHANGE_THRESHOLD_BYTES_PER_SEC: u64 = 10 * 1024;
const DEFAULT_NETWORK_TOTAL_CHANGE_THRESHOLD_BYTES: u64 = 10 * 1024;

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
            host,
            cpu,
            gpu,
            memory,
            uptime,
            disk,
            network,
            lighthouse,
            shutdown,
        } = file_config;

        let shutdown_payload = value_or_default(shutdown.payload, "shutdown".to_string())
            .trim()
            .to_string();
        let shutdown_cancel_payload =
            value_or_default(shutdown.cancel_payload, "cancel".to_string())
                .trim()
                .to_string();

        if shutdown_payload.is_empty() {
            return Err(anyhow!(
                "configuration value `shutdown.payload` must not be empty"
            ));
        }

        if shutdown_cancel_payload.is_empty() {
            return Err(anyhow!(
                "configuration value `shutdown.cancel_payload` must not be empty"
            ));
        }

        if shutdown_payload == shutdown_cancel_payload {
            return Err(anyhow!(
                "configuration values `shutdown.payload` and `shutdown.cancel_payload` must be different"
            ));
        }

        let lighthouse_enabled = value_or_default(lighthouse.enabled, false);
        let lighthouse_secret_id = trim_optional(lighthouse.secret_id);
        let lighthouse_secret_key = trim_optional(lighthouse.secret_key);
        let lighthouse_session_token = trim_optional(lighthouse.session_token);
        let lighthouse_endpoint = value_or_default(
            trim_optional(lighthouse.endpoint),
            DEFAULT_LIGHTHOUSE_ENDPOINT.to_string(),
        );
        let lighthouse_region = trim_optional(lighthouse.region);
        let lighthouse_instance_id = trim_optional(lighthouse.instance_id);
        let disk_include_paths = disk
            .include_paths
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();

        if lighthouse_enabled {
            required_trimmed_value(lighthouse_secret_id.as_ref(), "lighthouse.secret_id")?;
            required_trimmed_value(lighthouse_secret_key.as_ref(), "lighthouse.secret_key")?;
            required_trimmed_value(lighthouse_region.as_ref(), "lighthouse.region")?;
            required_trimmed_value(lighthouse_instance_id.as_ref(), "lighthouse.instance_id")?;
        }

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
            host_metrics_enabled: value_or_default(host.enabled, true),
            cpu_metrics_enabled: value_or_default(cpu.enabled, true),
            gpu_metrics_enabled: value_or_default(gpu.enabled, true),
            memory_metrics_enabled: value_or_default(memory.enabled, true),
            uptime_metrics_enabled: value_or_default(uptime.enabled, true),
            disk_metrics_enabled: value_or_default(disk.enabled, true)
                && !disk_include_paths.is_empty(),
            network_metrics_enabled: value_or_default(network.enabled, true),
            lighthouse_enabled,
            lighthouse_secret_id,
            lighthouse_secret_key,
            lighthouse_session_token,
            lighthouse_endpoint,
            lighthouse_region,
            lighthouse_instance_id,
            network_include_interfaces: network
                .include_interfaces
                .into_iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect(),
            disk_include_paths,
            enable_shutdown_button: value_or_default(shutdown.enable_button, false),
            shutdown_payload,
            shutdown_cancel_payload,
            shutdown_delay_secs: value_or_default(shutdown.delay_secs, 30),
            shutdown_dry_run: value_or_default(shutdown.dry_run, false),
            cpu_interval_secs: value_or_default(cpu.sampling_interval_secs, 1),
            gpu_interval_secs: value_or_default(gpu.sampling_interval_secs, 1),
            lighthouse_interval_secs: value_or_default(
                lighthouse.sampling_interval_secs,
                DEFAULT_LIGHTHOUSE_INTERVAL_SECS,
            ),
            memory_interval_secs: value_or_default(memory.sampling_interval_secs, 5),
            uptime_interval_secs: value_or_default(uptime.sampling_interval_secs, 300),
            disk_interval_secs: value_or_default(disk.sampling_interval_secs, 30),
            network_interval_secs: value_or_default(network.sampling_interval_secs, 1),
            cpu_change_threshold_pct: value_or_default(cpu.usage_threshold_pct, 1.0),
            gpu_usage_change_threshold_pct: value_or_default(gpu.usage_threshold_pct, 1.0),
            gpu_memory_change_threshold_mib: value_or_default(gpu.memory_change_threshold_mib, 8),
            memory_change_threshold_mib: value_or_default(memory.change_threshold_mib, 8),
            disk_change_threshold_mib: value_or_default(disk.change_threshold_mib, 32),
            network_rate_change_threshold_bytes_per_sec: value_or_default(
                network.rate_change_threshold_bps,
                DEFAULT_NETWORK_RATE_CHANGE_THRESHOLD_BYTES_PER_SEC,
            ),
            network_total_change_threshold_bytes: value_or_default(
                network.total_change_threshold_bytes,
                DEFAULT_NETWORK_TOTAL_CHANGE_THRESHOLD_BYTES,
            ),
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

    pub fn network_rate_change_threshold_bytes_per_sec_f64(&self) -> f64 {
        self.network_rate_change_threshold_bytes_per_sec as f64
    }
}

fn required_value<T>(value: Option<T>, key: &str) -> Result<T> {
    value.ok_or_else(|| {
        anyhow!("missing required configuration value `{key}` in `{CONFIG_FILE_NAME}`")
    })
}

fn required_trimmed_value(value: Option<&String>, key: &str) -> Result<()> {
    match value {
        Some(value) if !value.trim().is_empty() => Ok(()),
        _ => Err(anyhow!(
            "missing required configuration value `{key}` in `{CONFIG_FILE_NAME}`"
        )),
    }
}

fn trim_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
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
            host: HostConfig {
                enabled: Some(true),
            },
            cpu: CpuConfig {
                enabled: Some(true),
                sampling_interval_secs: Some(2),
                usage_threshold_pct: Some(2.5),
            },
            gpu: GpuConfig {
                enabled: Some(true),
                sampling_interval_secs: Some(3),
                usage_threshold_pct: Some(3.5),
                memory_change_threshold_mib: Some(16),
            },
            memory: MemoryConfig {
                enabled: Some(true),
                sampling_interval_secs: Some(7),
                change_threshold_mib: Some(12),
            },
            uptime: UptimeConfig {
                enabled: Some(true),
                sampling_interval_secs: Some(600),
            },
            disk: DiskConfig {
                enabled: Some(true),
                sampling_interval_secs: Some(45),
                change_threshold_mib: Some(64),
                include_paths: vec!["/".to_string(), "/mnt/data".to_string()],
            },
            network: NetworkConfig {
                enabled: Some(true),
                sampling_interval_secs: Some(1),
                include_interfaces: vec!["Ethernet".to_string(), "Wi-Fi".to_string()],
                rate_change_threshold_bps: Some(32 * 1024),
                total_change_threshold_bytes: Some(64 * 1024),
            },
            lighthouse: LighthouseConfig {
                enabled: Some(true),
                sampling_interval_secs: Some(600),
                secret_id: Some("secret-id".to_string()),
                secret_key: Some("secret-key".to_string()),
                session_token: Some("session-token".to_string()),
                endpoint: Some("lighthouse.tencentcloudapi.com".to_string()),
                region: Some("ap-chengdu".to_string()),
                instance_id: Some("lhins-example".to_string()),
            },
            shutdown: ShutdownConfig {
                enable_button: Some(true),
                payload: Some("poweroff".to_string()),
                cancel_payload: Some("cancel".to_string()),
                delay_secs: Some(90),
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
        assert!(config.host_metrics_enabled);
        assert!(config.cpu_metrics_enabled);
        assert!(config.gpu_metrics_enabled);
        assert!(config.memory_metrics_enabled);
        assert!(config.uptime_metrics_enabled);
        assert!(config.disk_metrics_enabled);
        assert!(config.network_metrics_enabled);
        assert!(config.lighthouse_enabled);
        assert_eq!(config.lighthouse_secret_id.as_deref(), Some("secret-id"));
        assert_eq!(config.lighthouse_secret_key.as_deref(), Some("secret-key"));
        assert_eq!(
            config.lighthouse_session_token.as_deref(),
            Some("session-token")
        );
        assert_eq!(config.lighthouse_endpoint, "lighthouse.tencentcloudapi.com");
        assert_eq!(config.lighthouse_region.as_deref(), Some("ap-chengdu"));
        assert_eq!(
            config.lighthouse_instance_id.as_deref(),
            Some("lhins-example")
        );
        assert_eq!(config.network_include_interfaces, vec!["Ethernet", "Wi-Fi"]);
        assert_eq!(config.disk_include_paths, vec!["/", "/mnt/data"]);
        assert_eq!(config.cpu_interval_secs, 2);
        assert_eq!(config.gpu_interval_secs, 3);
        assert_eq!(config.lighthouse_interval_secs, 600);
        assert_eq!(config.memory_interval_secs, 7);
        assert_eq!(config.uptime_interval_secs, 600);
        assert_eq!(config.disk_interval_secs, 45);
        assert_eq!(config.network_interval_secs, 1);
        assert_eq!(config.cpu_change_threshold_pct, 2.5);
        assert_eq!(config.gpu_usage_change_threshold_pct, 3.5);
        assert_eq!(config.gpu_memory_change_threshold_mib, 16);
        assert_eq!(config.memory_change_threshold_mib, 12);
        assert_eq!(config.disk_change_threshold_mib, 64);
        assert_eq!(
            config.network_rate_change_threshold_bytes_per_sec,
            32 * 1024
        );
        assert_eq!(config.network_total_change_threshold_bytes, 64 * 1024);
        assert!(config.enable_shutdown_button);
        assert_eq!(config.shutdown_payload, "poweroff");
        assert_eq!(config.shutdown_cancel_payload, "cancel");
        assert_eq!(config.shutdown_delay_secs, 90);
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
        assert!(config.host_metrics_enabled);
        assert!(config.cpu_metrics_enabled);
        assert!(config.gpu_metrics_enabled);
        assert!(config.memory_metrics_enabled);
        assert!(config.uptime_metrics_enabled);
        assert!(!config.disk_metrics_enabled);
        assert!(config.network_metrics_enabled);
        assert_eq!(config.cpu_interval_secs, 1);
        assert_eq!(config.gpu_interval_secs, 1);
        assert_eq!(
            config.lighthouse_interval_secs,
            DEFAULT_LIGHTHOUSE_INTERVAL_SECS
        );
        assert_eq!(config.memory_interval_secs, 5);
        assert_eq!(config.uptime_interval_secs, 300);
        assert_eq!(config.disk_interval_secs, 30);
        assert_eq!(config.network_interval_secs, 1);
        assert!(config.network_include_interfaces.is_empty());
        assert!(config.disk_include_paths.is_empty());
        assert_eq!(config.cpu_change_threshold_pct, 1.0);
        assert_eq!(config.gpu_usage_change_threshold_pct, 1.0);
        assert_eq!(config.gpu_memory_change_threshold_mib, 8);
        assert_eq!(config.memory_change_threshold_mib, 8);
        assert_eq!(config.disk_change_threshold_mib, 32);
        assert_eq!(
            config.network_rate_change_threshold_bytes_per_sec,
            DEFAULT_NETWORK_RATE_CHANGE_THRESHOLD_BYTES_PER_SEC
        );
        assert_eq!(
            config.network_total_change_threshold_bytes,
            DEFAULT_NETWORK_TOTAL_CHANGE_THRESHOLD_BYTES
        );
        assert!(!config.lighthouse_enabled);
        assert_eq!(config.lighthouse_endpoint, DEFAULT_LIGHTHOUSE_ENDPOINT);
        assert!(config.lighthouse_secret_id.is_none());
        assert!(config.lighthouse_secret_key.is_none());
        assert!(config.lighthouse_region.is_none());
        assert!(config.lighthouse_instance_id.is_none());
        assert!(!config.enable_shutdown_button);
        assert_eq!(config.shutdown_payload, "shutdown");
        assert_eq!(config.shutdown_cancel_payload, "cancel");
        assert_eq!(config.shutdown_delay_secs, 30);
        assert!(!config.shutdown_dry_run);
    }

    #[test]
    fn duplicate_shutdown_payloads_are_rejected() {
        let error = Config::from_file(
            &BootstrapOptions::default(),
            FileConfig {
                mqtt: MqttConfig {
                    host: Some("10.0.0.10".to_string()),
                    ..MqttConfig::default()
                },
                shutdown: ShutdownConfig {
                    payload: Some("shutdown".to_string()),
                    cancel_payload: Some("shutdown".to_string()),
                    ..ShutdownConfig::default()
                },
                ..FileConfig::default()
            },
        )
        .expect_err("duplicate shutdown payloads should be rejected");

        assert!(
            error
                .to_string()
                .contains("`shutdown.payload` and `shutdown.cancel_payload` must be different")
        );
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

    #[test]
    fn enabled_lighthouse_requires_credentials_and_instance_details() {
        let error = Config::from_file(
            &BootstrapOptions::default(),
            FileConfig {
                mqtt: MqttConfig {
                    host: Some("10.0.0.10".to_string()),
                    ..MqttConfig::default()
                },
                lighthouse: LighthouseConfig {
                    enabled: Some(true),
                    region: Some("ap-chengdu".to_string()),
                    ..LighthouseConfig::default()
                },
                ..FileConfig::default()
            },
        )
        .expect_err("missing lighthouse credentials should be rejected");

        assert!(
            error
                .to_string()
                .contains("missing required configuration value `lighthouse.secret_id`")
        );
    }

    #[test]
    fn metric_publish_switches_can_disable_individual_metric_groups() {
        let config = Config::from_file(
            &BootstrapOptions::default(),
            FileConfig {
                mqtt: MqttConfig {
                    host: Some("10.0.0.10".to_string()),
                    ..MqttConfig::default()
                },
                host: HostConfig {
                    enabled: Some(false),
                },
                cpu: CpuConfig {
                    enabled: Some(false),
                    ..CpuConfig::default()
                },
                gpu: GpuConfig {
                    enabled: Some(true),
                    ..GpuConfig::default()
                },
                memory: MemoryConfig {
                    enabled: Some(false),
                    ..MemoryConfig::default()
                },
                uptime: UptimeConfig {
                    enabled: Some(true),
                    ..UptimeConfig::default()
                },
                disk: DiskConfig {
                    enabled: Some(false),
                    ..DiskConfig::default()
                },
                network: NetworkConfig {
                    enabled: Some(true),
                    ..NetworkConfig::default()
                },
                ..FileConfig::default()
            },
        )
        .expect("config should load");

        assert!(!config.host_metrics_enabled);
        assert!(!config.cpu_metrics_enabled);
        assert!(config.gpu_metrics_enabled);
        assert!(!config.memory_metrics_enabled);
        assert!(config.uptime_metrics_enabled);
        assert!(!config.disk_metrics_enabled);
        assert!(config.network_metrics_enabled);
    }

    #[test]
    fn disk_metrics_require_include_paths() {
        let config = Config::from_file(
            &BootstrapOptions::default(),
            FileConfig {
                mqtt: MqttConfig {
                    host: Some("10.0.0.10".to_string()),
                    ..MqttConfig::default()
                },
                disk: DiskConfig {
                    enabled: Some(true),
                    ..DiskConfig::default()
                },
                ..FileConfig::default()
            },
        )
        .expect("config should load");

        assert!(!config.disk_metrics_enabled);
        assert!(config.disk_include_paths.is_empty());
    }
}

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const CONFIG_EXAMPLE_FILE_NAME: &str = "config.example.toml";
const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../../config.example.toml");

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(default, skip_serializing_if = "MqttConfig::is_empty")]
    pub mqtt: MqttConfig,
    #[serde(default, skip_serializing_if = "HomeAssistantConfig::is_empty")]
    pub home_assistant: HomeAssistantConfig,
    #[serde(default, skip_serializing_if = "DeviceConfig::is_empty")]
    pub device: DeviceConfig,
    #[serde(default, skip_serializing_if = "SamplingConfig::is_empty")]
    pub sampling: SamplingConfig,
    #[serde(default, skip_serializing_if = "ThresholdsConfig::is_empty")]
    pub thresholds: ThresholdsConfig,
    #[serde(default, skip_serializing_if = "ShutdownConfig::is_empty")]
    pub shutdown: ShutdownConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MqttConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HomeAssistantConfig {
    pub discovery_prefix: Option<String>,
    pub status_topic: Option<String>,
    pub topic_prefix: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub node_id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SamplingConfig {
    #[serde(default, skip_serializing_if = "CpuSamplingConfig::is_empty")]
    pub cpu: CpuSamplingConfig,
    #[serde(default, skip_serializing_if = "MetricSamplingConfig::is_empty")]
    pub gpu: MetricSamplingConfig,
    #[serde(default, skip_serializing_if = "MetricSamplingConfig::is_empty")]
    pub memory: MetricSamplingConfig,
    #[serde(default, skip_serializing_if = "MetricSamplingConfig::is_empty")]
    pub disk: MetricSamplingConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CpuSamplingConfig {
    pub interval_secs: Option<u64>,
    pub smoothing_window: Option<usize>,
    pub max_silence_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricSamplingConfig {
    pub interval_secs: Option<u64>,
    pub max_silence_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThresholdsConfig {
    #[serde(default, skip_serializing_if = "CpuThresholdConfig::is_empty")]
    pub cpu: CpuThresholdConfig,
    #[serde(default, skip_serializing_if = "GpuThresholdConfig::is_empty")]
    pub gpu: GpuThresholdConfig,
    #[serde(default, skip_serializing_if = "MetricThresholdConfig::is_empty")]
    pub memory: MetricThresholdConfig,
    #[serde(default, skip_serializing_if = "MetricThresholdConfig::is_empty")]
    pub disk: MetricThresholdConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CpuThresholdConfig {
    pub usage_pct: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuThresholdConfig {
    pub usage_pct: Option<f32>,
    pub memory_change_mib: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricThresholdConfig {
    pub change_mib: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShutdownConfig {
    pub enable_button: Option<bool>,
    pub payload: Option<String>,
    pub dry_run: Option<bool>,
}

impl FileConfig {
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("reading config file `{}`", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("parsing TOML config `{}`", path.display()))
    }

    pub fn apply_env_defaults(&self) {
        self.mqtt.set_defaults();
        self.home_assistant.set_defaults();
        self.device.set_defaults();
        self.sampling.set_defaults();
        self.thresholds.set_defaults();
        self.shutdown.set_defaults();
    }
}

pub fn load_config_file_from(directories: &[PathBuf]) -> Result<Option<PathBuf>> {
    for directory in directories {
        let path = directory.join(CONFIG_FILE_NAME);
        if path.is_file() {
            let config = FileConfig::load_from_path(&path)?;
            config.apply_env_defaults();
            return Ok(Some(path));
        }
    }

    Ok(None)
}

pub fn seed_config_toml(
    config_dir: &Path,
    source_directories: &[PathBuf],
) -> Result<Option<PathBuf>> {
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    if config_path.is_file() {
        return Ok(Some(config_path));
    }

    if let Some(source_toml) = find_file(source_directories, CONFIG_FILE_NAME) {
        copy_file_if_needed(&source_toml, &config_path).with_context(|| {
            format!(
                "copying config file from `{}` to `{}`",
                source_toml.display(),
                config_path.display()
            )
        })?;
        return Ok(Some(config_path));
    }

    if let Some(example_toml) = find_file(source_directories, CONFIG_EXAMPLE_FILE_NAME) {
        copy_file_if_needed(&example_toml, &config_path).with_context(|| {
            format!(
                "copying config template from `{}` to `{}`",
                example_toml.display(),
                config_path.display()
            )
        })?;
        return Ok(Some(config_path));
    }

    fs::write(&config_path, DEFAULT_CONFIG_TEMPLATE).with_context(|| {
        format!(
            "writing default config template to `{}`",
            config_path.display()
        )
    })?;
    Ok(Some(config_path))
}

fn find_file(directories: &[PathBuf], file_name: &str) -> Option<PathBuf> {
    directories
        .iter()
        .map(|directory| directory.join(file_name))
        .find(|path| path.is_file())
}

fn copy_file_if_needed(source: &Path, destination: &Path) -> Result<()> {
    if same_path(source, destination) {
        return Ok(());
    }

    match fs::metadata(destination) {
        Ok(metadata) if metadata.is_file() && files_match(source, destination)? => Ok(()),
        Ok(_) | Err(_) => {
            fs::copy(source, destination).with_context(|| {
                format!(
                    "copying file from `{}` to `{}`",
                    source.display(),
                    destination.display()
                )
            })?;
            Ok(())
        }
    }
}

fn files_match(source: &Path, destination: &Path) -> Result<bool> {
    let source_metadata = fs::metadata(source)
        .with_context(|| format!("reading metadata for `{}`", source.display()))?;
    let destination_metadata = fs::metadata(destination)
        .with_context(|| format!("reading metadata for `{}`", destination.display()))?;

    if source_metadata.len() != destination_metadata.len() {
        return Ok(false);
    }

    Ok(
        fs::read(source).with_context(|| format!("reading `{}`", source.display()))?
            == fs::read(destination)
                .with_context(|| format!("reading `{}`", destination.display()))?,
    )
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn set_env_if_absent(key: &str, value: impl ToString) {
    if env::var_os(key).is_some() {
        return;
    }

    unsafe {
        env::set_var(key, value.to_string());
    }
}

fn set_optional<T>(key: &str, value: Option<T>)
where
    T: ToString + Copy,
{
    if let Some(value) = value {
        set_env_if_absent(key, value);
    }
}

impl MqttConfig {
    fn is_empty(&self) -> bool {
        self.host.is_none()
            && self.port.is_none()
            && self.username.is_none()
            && self.password.is_none()
    }

    fn set_defaults(&self) {
        if let Some(value) = &self.host {
            set_env_if_absent("HA_MONITOR_MQTT_HOST", value);
        }
        if let Some(value) = self.port {
            set_env_if_absent("HA_MONITOR_MQTT_PORT", value);
        }
        if let Some(value) = &self.username {
            set_env_if_absent("HA_MONITOR_MQTT_USERNAME", value);
        }
        if let Some(value) = &self.password {
            set_env_if_absent("HA_MONITOR_MQTT_PASSWORD", value);
        }
    }
}

impl HomeAssistantConfig {
    fn is_empty(&self) -> bool {
        self.discovery_prefix.is_none()
            && self.status_topic.is_none()
            && self.topic_prefix.is_none()
    }

    fn set_defaults(&self) {
        if let Some(value) = &self.discovery_prefix {
            set_env_if_absent("HA_MONITOR_DISCOVERY_PREFIX", value);
        }
        if let Some(value) = &self.status_topic {
            set_env_if_absent("HA_MONITOR_HOME_ASSISTANT_STATUS_TOPIC", value);
        }
        if let Some(value) = &self.topic_prefix {
            set_env_if_absent("HA_MONITOR_TOPIC_PREFIX", value);
        }
    }
}

impl DeviceConfig {
    fn is_empty(&self) -> bool {
        self.node_id.is_none() && self.name.is_none()
    }

    fn set_defaults(&self) {
        if let Some(value) = &self.node_id {
            set_env_if_absent("HA_MONITOR_NODE_ID", value);
        }
        if let Some(value) = &self.name {
            set_env_if_absent("HA_MONITOR_DEVICE_NAME", value);
        }
    }
}

impl SamplingConfig {
    fn is_empty(&self) -> bool {
        self.cpu.is_empty() && self.gpu.is_empty() && self.memory.is_empty() && self.disk.is_empty()
    }

    fn set_defaults(&self) {
        set_optional("HA_MONITOR_CPU_INTERVAL_SECS", self.cpu.interval_secs);
        set_optional("HA_MONITOR_GPU_INTERVAL_SECS", self.gpu.interval_secs);
        set_optional("HA_MONITOR_MEMORY_INTERVAL_SECS", self.memory.interval_secs);
        set_optional("HA_MONITOR_DISK_INTERVAL_SECS", self.disk.interval_secs);
        set_optional("HA_MONITOR_CPU_SMOOTHING_WINDOW", self.cpu.smoothing_window);
        set_optional("HA_MONITOR_CPU_MAX_SILENCE_SECS", self.cpu.max_silence_secs);
        set_optional("HA_MONITOR_GPU_MAX_SILENCE_SECS", self.gpu.max_silence_secs);
        set_optional(
            "HA_MONITOR_MEMORY_MAX_SILENCE_SECS",
            self.memory.max_silence_secs,
        );
        set_optional(
            "HA_MONITOR_DISK_MAX_SILENCE_SECS",
            self.disk.max_silence_secs,
        );
    }
}

impl CpuSamplingConfig {
    fn is_empty(&self) -> bool {
        self.interval_secs.is_none()
            && self.smoothing_window.is_none()
            && self.max_silence_secs.is_none()
    }
}

impl MetricSamplingConfig {
    fn is_empty(&self) -> bool {
        self.interval_secs.is_none() && self.max_silence_secs.is_none()
    }
}

impl ThresholdsConfig {
    fn is_empty(&self) -> bool {
        self.cpu.is_empty() && self.gpu.is_empty() && self.memory.is_empty() && self.disk.is_empty()
    }

    fn set_defaults(&self) {
        set_optional("HA_MONITOR_CPU_CHANGE_THRESHOLD_PCT", self.cpu.usage_pct);
        set_optional(
            "HA_MONITOR_GPU_USAGE_CHANGE_THRESHOLD_PCT",
            self.gpu.usage_pct,
        );
        set_optional(
            "HA_MONITOR_GPU_MEMORY_CHANGE_THRESHOLD_MIB",
            self.gpu.memory_change_mib,
        );
        set_optional(
            "HA_MONITOR_MEMORY_CHANGE_THRESHOLD_MIB",
            self.memory.change_mib,
        );
        set_optional("HA_MONITOR_DISK_CHANGE_THRESHOLD_MIB", self.disk.change_mib);
    }
}

impl CpuThresholdConfig {
    fn is_empty(&self) -> bool {
        self.usage_pct.is_none()
    }
}

impl GpuThresholdConfig {
    fn is_empty(&self) -> bool {
        self.usage_pct.is_none() && self.memory_change_mib.is_none()
    }
}

impl MetricThresholdConfig {
    fn is_empty(&self) -> bool {
        self.change_mib.is_none()
    }
}

impl ShutdownConfig {
    fn is_empty(&self) -> bool {
        self.enable_button.is_none() && self.payload.is_none() && self.dry_run.is_none()
    }

    fn set_defaults(&self) {
        set_optional("HA_MONITOR_ENABLE_SHUTDOWN_BUTTON", self.enable_button);
        if let Some(value) = &self.payload {
            set_env_if_absent("HA_MONITOR_SHUTDOWN_PAYLOAD", value);
        }
        set_optional("HA_MONITOR_SHUTDOWN_DRY_RUN", self.dry_run);
    }
}

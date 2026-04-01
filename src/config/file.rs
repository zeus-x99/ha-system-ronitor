use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::shared::util::{files_match, same_path};

pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const CONFIG_EXAMPLE_FILE_NAME: &str = "config.example.toml";
const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../../config.example.toml");

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    #[serde(default, skip_serializing_if = "MqttConfig::is_empty")]
    pub mqtt: MqttConfig,
    #[serde(default, skip_serializing_if = "HomeAssistantConfig::is_empty")]
    pub home_assistant: HomeAssistantConfig,
    #[serde(default, skip_serializing_if = "DeviceConfig::is_empty")]
    pub device: DeviceConfig,
    #[serde(default, skip_serializing_if = "NetworkConfig::is_empty")]
    pub network: NetworkConfig,
    #[serde(default, skip_serializing_if = "SamplingConfig::is_empty")]
    pub sampling: SamplingConfig,
    #[serde(default, skip_serializing_if = "ThresholdsConfig::is_empty")]
    pub thresholds: ThresholdsConfig,
    #[serde(default, skip_serializing_if = "ShutdownConfig::is_empty")]
    pub shutdown: ShutdownConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MqttConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeAssistantConfig {
    pub discovery_prefix: Option<String>,
    pub status_topic: Option<String>,
    pub topic_prefix: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceConfig {
    pub node_id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include_interfaces: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamplingConfig {
    #[serde(default, skip_serializing_if = "MetricSamplingConfig::is_empty")]
    pub cpu: MetricSamplingConfig,
    #[serde(default, skip_serializing_if = "MetricSamplingConfig::is_empty")]
    pub gpu: MetricSamplingConfig,
    #[serde(default, skip_serializing_if = "MetricSamplingConfig::is_empty")]
    pub memory: MetricSamplingConfig,
    #[serde(default, skip_serializing_if = "MetricSamplingConfig::is_empty")]
    pub uptime: MetricSamplingConfig,
    #[serde(default, skip_serializing_if = "MetricSamplingConfig::is_empty")]
    pub disk: MetricSamplingConfig,
    #[serde(default, skip_serializing_if = "MetricSamplingConfig::is_empty")]
    pub network: MetricSamplingConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricSamplingConfig {
    pub interval_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThresholdsConfig {
    #[serde(default, skip_serializing_if = "CpuThresholdConfig::is_empty")]
    pub cpu: CpuThresholdConfig,
    #[serde(default, skip_serializing_if = "GpuThresholdConfig::is_empty")]
    pub gpu: GpuThresholdConfig,
    #[serde(default, skip_serializing_if = "MetricThresholdConfig::is_empty")]
    pub memory: MetricThresholdConfig,
    #[serde(default, skip_serializing_if = "MetricThresholdConfig::is_empty")]
    pub disk: MetricThresholdConfig,
    #[serde(default, skip_serializing_if = "NetworkThresholdConfig::is_empty")]
    pub network: NetworkThresholdConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuThresholdConfig {
    pub usage_pct: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuThresholdConfig {
    pub usage_pct: Option<f32>,
    pub memory_change_mib: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricThresholdConfig {
    pub change_mib: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkThresholdConfig {
    pub rate_change_bps: Option<u64>,
    pub total_change_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShutdownConfig {
    pub enable_button: Option<bool>,
    pub payload: Option<String>,
    pub cancel_payload: Option<String>,
    pub delay_secs: Option<u64>,
    pub dry_run: Option<bool>,
}

impl FileConfig {
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("reading config file `{}`", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("parsing TOML config `{}`", path.display()))
    }
}

pub fn load_config_file_from(directories: &[PathBuf]) -> Result<Option<FileConfig>> {
    for directory in directories {
        let path = directory.join(CONFIG_FILE_NAME);
        if path.is_file() {
            return Ok(Some(FileConfig::load_from_path(&path)?));
        }
    }

    Ok(None)
}

pub fn seed_config_toml(config_dir: &Path, source_directories: &[PathBuf]) -> Result<PathBuf> {
    let config_path = config_dir.join(CONFIG_FILE_NAME);
    if config_path.is_file() {
        return Ok(config_path);
    }

    if let Some(source_toml) = find_file(source_directories, CONFIG_FILE_NAME) {
        copy_file_if_needed(&source_toml, &config_path).with_context(|| {
            format!(
                "copying config file from `{}` to `{}`",
                source_toml.display(),
                config_path.display()
            )
        })?;
        return Ok(config_path);
    }

    if let Some(example_toml) = find_file(source_directories, CONFIG_EXAMPLE_FILE_NAME) {
        copy_file_if_needed(&example_toml, &config_path).with_context(|| {
            format!(
                "copying config template from `{}` to `{}`",
                example_toml.display(),
                config_path.display()
            )
        })?;
        return Ok(config_path);
    }

    fs::write(&config_path, DEFAULT_CONFIG_TEMPLATE).with_context(|| {
        format!(
            "writing default config template to `{}`",
            config_path.display()
        )
    })?;
    Ok(config_path)
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

impl MqttConfig {
    fn is_empty(&self) -> bool {
        self.host.is_none()
            && self.port.is_none()
            && self.username.is_none()
            && self.password.is_none()
    }
}

impl HomeAssistantConfig {
    fn is_empty(&self) -> bool {
        self.discovery_prefix.is_none()
            && self.status_topic.is_none()
            && self.topic_prefix.is_none()
    }
}

impl DeviceConfig {
    fn is_empty(&self) -> bool {
        self.node_id.is_none() && self.name.is_none()
    }
}

impl NetworkConfig {
    fn is_empty(&self) -> bool {
        self.include_interfaces.is_empty()
    }
}

impl SamplingConfig {
    fn is_empty(&self) -> bool {
        self.cpu.is_empty()
            && self.gpu.is_empty()
            && self.memory.is_empty()
            && self.uptime.is_empty()
            && self.disk.is_empty()
            && self.network.is_empty()
    }
}

impl MetricSamplingConfig {
    fn is_empty(&self) -> bool {
        self.interval_secs.is_none()
    }
}

impl ThresholdsConfig {
    fn is_empty(&self) -> bool {
        self.cpu.is_empty()
            && self.gpu.is_empty()
            && self.memory.is_empty()
            && self.disk.is_empty()
            && self.network.is_empty()
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

impl NetworkThresholdConfig {
    fn is_empty(&self) -> bool {
        self.rate_change_bps.is_none() && self.total_change_bytes.is_none()
    }
}

impl ShutdownConfig {
    fn is_empty(&self) -> bool {
        self.enable_button.is_none()
            && self.payload.is_none()
            && self.cancel_payload.is_none()
            && self.delay_secs.is_none()
            && self.dry_run.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::FileConfig;

    #[test]
    fn rejects_unknown_fields() {
        let error = toml::from_str::<FileConfig>(
            r#"
            [mqtt]
            host = "10.0.0.1"
            typo_field = true
            "#,
        )
        .expect_err("unknown fields should be rejected");

        assert!(error.to_string().contains("unknown field"));
    }
}

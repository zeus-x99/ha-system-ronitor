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
    #[serde(default, skip_serializing_if = "HostConfig::is_empty")]
    pub host: HostConfig,
    #[serde(default, skip_serializing_if = "CpuConfig::is_empty")]
    pub cpu: CpuConfig,
    #[serde(default, skip_serializing_if = "GpuConfig::is_empty")]
    pub gpu: GpuConfig,
    #[serde(default, skip_serializing_if = "MemoryConfig::is_empty")]
    pub memory: MemoryConfig,
    #[serde(default, skip_serializing_if = "UptimeConfig::is_empty")]
    pub uptime: UptimeConfig,
    #[serde(default, skip_serializing_if = "DiskConfig::is_empty")]
    pub disk: DiskConfig,
    #[serde(default, skip_serializing_if = "NetworkConfig::is_empty")]
    pub network: NetworkConfig,
    #[serde(default, skip_serializing_if = "LighthouseConfig::is_empty")]
    pub lighthouse: LighthouseConfig,
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
pub struct HostConfig {
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuConfig {
    pub enabled: Option<bool>,
    pub sampling_interval_secs: Option<u64>,
    pub usage_threshold_pct: Option<f32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuConfig {
    pub enabled: Option<bool>,
    pub sampling_interval_secs: Option<u64>,
    pub usage_threshold_pct: Option<f32>,
    pub memory_change_threshold_mib: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryConfig {
    pub enabled: Option<bool>,
    pub sampling_interval_secs: Option<u64>,
    pub change_threshold_mib: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UptimeConfig {
    pub enabled: Option<bool>,
    pub sampling_interval_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiskConfig {
    pub enabled: Option<bool>,
    pub sampling_interval_secs: Option<u64>,
    pub change_threshold_mib: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfig {
    pub enabled: Option<bool>,
    pub sampling_interval_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include_interfaces: Vec<String>,
    pub rate_change_threshold_bps: Option<u64>,
    pub total_change_threshold_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LighthouseConfig {
    pub enabled: Option<bool>,
    pub sampling_interval_secs: Option<u64>,
    pub secret_id: Option<String>,
    pub secret_key: Option<String>,
    pub session_token: Option<String>,
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub instance_id: Option<String>,
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

impl HostConfig {
    fn is_empty(&self) -> bool {
        self.enabled.is_none()
    }
}

impl CpuConfig {
    fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.sampling_interval_secs.is_none()
            && self.usage_threshold_pct.is_none()
    }
}

impl GpuConfig {
    fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.sampling_interval_secs.is_none()
            && self.usage_threshold_pct.is_none()
            && self.memory_change_threshold_mib.is_none()
    }
}

impl MemoryConfig {
    fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.sampling_interval_secs.is_none()
            && self.change_threshold_mib.is_none()
    }
}

impl UptimeConfig {
    fn is_empty(&self) -> bool {
        self.enabled.is_none() && self.sampling_interval_secs.is_none()
    }
}

impl DiskConfig {
    fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.sampling_interval_secs.is_none()
            && self.change_threshold_mib.is_none()
            && self.include_paths.is_empty()
    }
}

impl NetworkConfig {
    fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.sampling_interval_secs.is_none()
            && self.include_interfaces.is_empty()
            && self.rate_change_threshold_bps.is_none()
            && self.total_change_threshold_bytes.is_none()
    }
}

impl LighthouseConfig {
    fn is_empty(&self) -> bool {
        self.enabled.is_none()
            && self.sampling_interval_secs.is_none()
            && self.secret_id.is_none()
            && self.secret_key.is_none()
            && self.session_token.is_none()
            && self.endpoint.is_none()
            && self.region.is_none()
            && self.instance_id.is_none()
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

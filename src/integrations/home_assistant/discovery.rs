use std::collections::BTreeMap;

use serde::Serialize;

use crate::config::Config;
use crate::device::{Identity, Topics};
use crate::system::models::{DiskInfoState, GpuInfoState, NetworkInfoState};

const CELSIUS_UNIT: &str = "\u{00B0}C";

#[derive(Debug, Clone)]
pub struct DeviceDiscoveryMessage {
    pub topic: String,
    pub payload: DeviceDiscoveryPayload,
}

impl DeviceDiscoveryMessage {
    pub fn component_count(&self) -> usize {
        self.payload.components.len()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceDiscoveryPayload {
    #[serde(rename = "dev")]
    device: DeviceInfo,
    #[serde(rename = "o")]
    origin: OriginInfo,
    availability: Vec<Availability>,
    qos: u8,
    #[serde(rename = "cmps")]
    components: BTreeMap<String, Component>,
}

#[derive(Debug, Clone, Serialize)]
struct DeviceInfo {
    #[serde(rename = "ids")]
    identifiers: Vec<String>,
    name: String,
    #[serde(rename = "mf")]
    manufacturer: String,
    #[serde(rename = "mdl")]
    model: String,
    #[serde(rename = "sw")]
    sw_version: String,
}

#[derive(Debug, Clone, Serialize)]
struct OriginInfo {
    name: String,
    sw: String,
    url: String,
}

#[derive(Debug, Clone, Serialize)]
struct Availability {
    topic: String,
    payload_available: String,
    payload_not_available: String,
}

#[derive(Debug, Clone, Serialize)]
struct Component {
    #[serde(rename = "p")]
    platform: &'static str,
    unique_id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_entity_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_press: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_class: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit_of_measurement: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_class: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggested_display_precision: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    entity_category: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<&'static str>,
}

impl Component {
    fn sensor(
        identity: &Identity,
        component_id: &str,
        name: impl Into<String>,
        state_topic: String,
        value_template: impl Into<String>,
    ) -> Self {
        Self {
            platform: "sensor",
            unique_id: format!("{}-{}", identity.device_id, component_id),
            name: name.into(),
            default_entity_id: Some(default_entity_id(identity, "sensor", component_id)),
            value_template: Some(value_template.into()),
            state_topic: Some(state_topic),
            command_topic: None,
            payload_press: None,
            device_class: None,
            unit_of_measurement: None,
            state_class: None,
            suggested_display_precision: None,
            entity_category: None,
            icon: None,
        }
    }

    fn button(
        identity: &Identity,
        component_id: &str,
        name: impl Into<String>,
        command_topic: String,
        payload_press: String,
    ) -> Self {
        Self {
            platform: "button",
            unique_id: format!("{}-{}", identity.device_id, component_id),
            name: name.into(),
            default_entity_id: Some(default_entity_id(identity, "button", component_id)),
            value_template: None,
            state_topic: None,
            command_topic: Some(command_topic),
            payload_press: Some(payload_press),
            device_class: None,
            unit_of_measurement: None,
            state_class: None,
            suggested_display_precision: None,
            entity_category: None,
            icon: None,
        }
    }

    fn with_device_class(mut self, device_class: &'static str) -> Self {
        self.device_class = Some(device_class);
        self
    }

    fn with_unit(mut self, unit_of_measurement: &'static str) -> Self {
        self.unit_of_measurement = Some(unit_of_measurement);
        self
    }

    fn with_state_class(mut self, state_class: &'static str) -> Self {
        self.state_class = Some(state_class);
        self
    }

    fn with_precision(mut self, suggested_display_precision: u8) -> Self {
        self.suggested_display_precision = Some(suggested_display_precision);
        self
    }

    fn with_entity_category(mut self, entity_category: &'static str) -> Self {
        self.entity_category = Some(entity_category);
        self
    }

    fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }
}

fn default_entity_id(identity: &Identity, domain: &str, component_id: &str) -> String {
    format!(
        "{domain}.{}_{}",
        identity.entity_id_prefix,
        component_id.to_ascii_lowercase()
    )
}

pub fn build_device_discovery_message(
    config: &Config,
    identity: &Identity,
    topics: &Topics,
    gpu_info: Option<&GpuInfoState>,
    disks: Option<&DiskInfoState>,
    network_info: Option<&NetworkInfoState>,
) -> DeviceDiscoveryMessage {
    let components = build_components(config, identity, topics, gpu_info, disks, network_info);

    let payload = DeviceDiscoveryPayload {
        device: DeviceInfo {
            identifiers: vec![identity.device_id.clone()],
            name: identity.device_name.clone(),
            manufacturer: "Rust".to_string(),
            model: format!("{} {}", identity.os_name, identity.os_version),
            sw_version: env!("CARGO_PKG_VERSION").to_string(),
        },
        origin: OriginInfo {
            name: env!("CARGO_PKG_NAME").to_string(),
            sw: env!("CARGO_PKG_VERSION").to_string(),
            url: "https://www.home-assistant.io/integrations/mqtt".to_string(),
        },
        availability: vec![Availability {
            topic: topics.availability.clone(),
            payload_available: "online".to_string(),
            payload_not_available: "offline".to_string(),
        }],
        qos: 1,
        components,
    };

    DeviceDiscoveryMessage {
        topic: topics.device_discovery.clone(),
        payload,
    }
}

fn build_components(
    config: &Config,
    identity: &Identity,
    topics: &Topics,
    gpu_info: Option<&GpuInfoState>,
    disks: Option<&DiskInfoState>,
    network_info: Option<&NetworkInfoState>,
) -> BTreeMap<String, Component> {
    let mut components = BTreeMap::new();

    if config.cpu_metrics_enabled {
        components.insert(
            "cpu_usage".to_string(),
            Component::sensor(
                identity,
                "cpu_usage",
                "CPU Usage",
                topics.cpu_state.clone(),
                "{{ value_json.cpu_usage }}",
            )
            .with_unit("%")
            .with_state_class("measurement")
            .with_precision(1)
            .with_icon("mdi:chip"),
        );

        components.insert(
            "cpu_package_temp".to_string(),
            Component::sensor(
                identity,
                "cpu_package_temp",
                "CPU Package Temperature",
                topics.cpu_state.clone(),
                "{{ value_json.cpu_package_temp | default(none) }}",
            )
            .with_unit(CELSIUS_UNIT)
            .with_device_class("temperature")
            .with_state_class("measurement")
            .with_precision(1)
            .with_icon("mdi:thermometer"),
        );

        components.insert(
            "cpu_model".to_string(),
            Component::sensor(
                identity,
                "cpu_model",
                "CPU Model",
                topics.cpu_info_state.clone(),
                "{{ value_json.cpu_model }}",
            )
            .with_icon("mdi:chip"),
        );
    }

    if config.host_metrics_enabled {
        components.insert(
            "os_version".to_string(),
            Component::sensor(
                identity,
                "os_version",
                "OS Version",
                topics.host_info_state.clone(),
                "{{ value_json.os_version }}",
            )
            .with_icon("mdi:information-outline"),
        );
    }

    if config.uptime_metrics_enabled {
        components.insert(
            "uptime".to_string(),
            Component::sensor(
                identity,
                "uptime",
                "Uptime",
                topics.uptime_state.clone(),
                "{{ value_json.uptime }}",
            )
            .with_unit("s")
            .with_device_class("duration")
            .with_state_class("measurement")
            .with_icon("mdi:timer-outline"),
        );
    }

    if config.enable_shutdown_button && config.shutdown_delay_secs > 0 {
        components.insert(
            "shutdown_remaining_secs".to_string(),
            Component::sensor(
                identity,
                "shutdown_remaining_secs",
                "Shutdown Remaining",
                topics.shutdown_state.clone(),
                "{{ value_json.shutdown_remaining_secs }}",
            )
            .with_unit("s")
            .with_device_class("duration")
            .with_state_class("measurement")
            .with_icon("mdi:timer-sand"),
        );
    }

    if config.gpu_metrics_enabled
        && let Some(gpu_info) = gpu_info
    {
        components.insert(
            "gpu_name".to_string(),
            Component::sensor(
                identity,
                "gpu_name",
                "GPU Name",
                topics.gpu_info_state.clone(),
                "{{ value_json.gpu_name }}",
            )
            .with_icon("mdi:expansion-card"),
        );

        components.insert(
            "gpu_usage".to_string(),
            Component::sensor(
                identity,
                "gpu_usage",
                "GPU Usage",
                topics.gpu_state.clone(),
                "{{ value_json.gpu_usage }}",
            )
            .with_unit("%")
            .with_state_class("measurement")
            .with_precision(1)
            .with_icon("mdi:expansion-card"),
        );

        components.insert(
            "gpu_temperature".to_string(),
            Component::sensor(
                identity,
                "gpu_temperature",
                "GPU Temperature",
                topics.gpu_state.clone(),
                "{{ value_json.gpu_temperature | default(none) }}",
            )
            .with_unit(CELSIUS_UNIT)
            .with_device_class("temperature")
            .with_state_class("measurement")
            .with_precision(1)
            .with_icon("mdi:thermometer"),
        );

        components.insert(
            "gpu_memory_available".to_string(),
            Component::sensor(
                identity,
                "gpu_memory_available",
                "GPU Memory Available",
                topics.gpu_state.clone(),
                "{{ value_json.gpu_memory_available }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("measurement")
            .with_icon("mdi:memory"),
        );

        components.insert(
            "gpu_memory_used".to_string(),
            Component::sensor(
                identity,
                "gpu_memory_used",
                "GPU Memory Used",
                topics.gpu_state.clone(),
                "{{ value_json.gpu_memory_used }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("measurement")
            .with_icon("mdi:memory"),
        );

        components.insert(
            "gpu_memory_total".to_string(),
            Component::sensor(
                identity,
                "gpu_memory_total",
                "GPU Memory Total",
                topics.gpu_info_state.clone(),
                "{{ value_json.gpu_memory_total }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("measurement")
            .with_icon("mdi:memory"),
        );

        components.insert(
            "gpu_memory_usage".to_string(),
            Component::sensor(
                identity,
                "gpu_memory_usage",
                "GPU Memory Usage",
                topics.gpu_state.clone(),
                "{{ value_json.gpu_memory_usage }}",
            )
            .with_unit("%")
            .with_state_class("measurement")
            .with_precision(1)
            .with_icon("mdi:memory"),
        );

        if gpu_info.gpu_memory_total == 0 {
            components.remove("gpu_memory_total");
            components.remove("gpu_memory_available");
            components.remove("gpu_memory_used");
            components.remove("gpu_memory_usage");
        }
    }

    if config.lighthouse_enabled {
        components.insert(
            "lighthouse_instance_id".to_string(),
            Component::sensor(
                identity,
                "lighthouse_instance_id",
                "Tencent Cloud Lighthouse Instance ID",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_instance_id | default(none) }}",
            )
            .with_entity_category("diagnostic")
            .with_icon("mdi:cloud-outline"),
        );

        components.insert(
            "lighthouse_package_id".to_string(),
            Component::sensor(
                identity,
                "lighthouse_package_id",
                "Tencent Cloud Lighthouse Package ID",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_package_id | default(none) }}",
            )
            .with_entity_category("diagnostic")
            .with_icon("mdi:identifier"),
        );

        components.insert(
            "lighthouse_used".to_string(),
            Component::sensor(
                identity,
                "lighthouse_used",
                "Tencent Cloud Traffic Used",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_used | default(none) }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("measurement")
            .with_icon("mdi:download-network"),
        );

        components.insert(
            "lighthouse_total".to_string(),
            Component::sensor(
                identity,
                "lighthouse_total",
                "Tencent Cloud Traffic Total",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_total | default(none) }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("measurement")
            .with_icon("mdi:database"),
        );

        components.insert(
            "lighthouse_remaining".to_string(),
            Component::sensor(
                identity,
                "lighthouse_remaining",
                "Tencent Cloud Traffic Remaining",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_remaining | default(none) }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("measurement")
            .with_icon("mdi:gauge"),
        );

        components.insert(
            "lighthouse_overflow".to_string(),
            Component::sensor(
                identity,
                "lighthouse_overflow",
                "Tencent Cloud Traffic Overflow",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_overflow | default(none) }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("measurement")
            .with_icon("mdi:alert-circle-outline"),
        );

        components.insert(
            "lighthouse_usage".to_string(),
            Component::sensor(
                identity,
                "lighthouse_usage",
                "Tencent Cloud Traffic Usage",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_usage | default(none) }}",
            )
            .with_unit("%")
            .with_state_class("measurement")
            .with_precision(2)
            .with_icon("mdi:percent"),
        );

        components.insert(
            "lighthouse_status".to_string(),
            Component::sensor(
                identity,
                "lighthouse_status",
                "Tencent Cloud Lighthouse Status",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_status | default(none) }}",
            )
            .with_entity_category("diagnostic")
            .with_icon("mdi:check-network-outline"),
        );

        components.insert(
            "lighthouse_cycle_start".to_string(),
            Component::sensor(
                identity,
                "lighthouse_cycle_start",
                "Tencent Cloud Lighthouse Cycle Start",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_cycle_start | default(none) }}",
            )
            .with_device_class("timestamp")
            .with_entity_category("diagnostic")
            .with_icon("mdi:calendar-start"),
        );

        components.insert(
            "lighthouse_cycle_end".to_string(),
            Component::sensor(
                identity,
                "lighthouse_cycle_end",
                "Tencent Cloud Lighthouse Cycle End",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_cycle_end | default(none) }}",
            )
            .with_device_class("timestamp")
            .with_entity_category("diagnostic")
            .with_icon("mdi:calendar-end"),
        );

        components.insert(
            "lighthouse_deadline".to_string(),
            Component::sensor(
                identity,
                "lighthouse_deadline",
                "Tencent Cloud Lighthouse Deadline",
                topics.lighthouse_state.clone(),
                "{{ value_json.lighthouse_deadline | default(none) }}",
            )
            .with_device_class("timestamp")
            .with_entity_category("diagnostic")
            .with_icon("mdi:calendar-clock"),
        );
    }

    if config.memory_metrics_enabled {
        components.insert(
            "memory_used".to_string(),
            Component::sensor(
                identity,
                "memory_used",
                "Memory Used",
                topics.memory_state.clone(),
                "{{ value_json.memory_used }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("measurement")
            .with_icon("mdi:memory"),
        );

        components.insert(
            "memory_total".to_string(),
            Component::sensor(
                identity,
                "memory_total",
                "Memory Total",
                topics.memory_info_state.clone(),
                "{{ value_json.memory_total }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("measurement")
            .with_icon("mdi:memory"),
        );

        components.insert(
            "memory_usage".to_string(),
            Component::sensor(
                identity,
                "memory_usage",
                "Memory Usage",
                topics.memory_state.clone(),
                "{{ value_json.memory_usage }}",
            )
            .with_unit("%")
            .with_state_class("measurement")
            .with_precision(1)
            .with_icon("mdi:memory"),
        );
    }

    if config.network_metrics_enabled {
        components.insert(
            "network_download_rate".to_string(),
            Component::sensor(
                identity,
                "network_download_rate",
                "Network Download Rate",
                topics.network_state.clone(),
                "{{ value_json.network_download_rate }}",
            )
            .with_unit("B/s")
            .with_device_class("data_rate")
            .with_state_class("measurement")
            .with_precision(1)
            .with_icon("mdi:download-network"),
        );

        components.insert(
            "network_upload_rate".to_string(),
            Component::sensor(
                identity,
                "network_upload_rate",
                "Network Upload Rate",
                topics.network_state.clone(),
                "{{ value_json.network_upload_rate }}",
            )
            .with_unit("B/s")
            .with_device_class("data_rate")
            .with_state_class("measurement")
            .with_precision(1)
            .with_icon("mdi:upload-network"),
        );

        components.insert(
            "network_total_download".to_string(),
            Component::sensor(
                identity,
                "network_total_download",
                "Network Total Download",
                topics.network_state.clone(),
                "{{ value_json.network_total_download }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("total_increasing")
            .with_icon("mdi:download"),
        );

        components.insert(
            "network_total_upload".to_string(),
            Component::sensor(
                identity,
                "network_total_upload",
                "Network Total Upload",
                topics.network_state.clone(),
                "{{ value_json.network_total_upload }}",
            )
            .with_unit("B")
            .with_device_class("data_size")
            .with_state_class("total_increasing")
            .with_icon("mdi:upload"),
        );
    }

    if config.enable_shutdown_button {
        let shutdown_button = if config.shutdown_delay_secs > 0 {
            Component::button(
                identity,
                "shutdown_host",
                "Schedule Shutdown",
                topics.shutdown_command.clone(),
                config.shutdown_payload.clone(),
            )
            .with_entity_category("config")
            .with_icon("mdi:power-sleep")
        } else {
            Component::button(
                identity,
                "shutdown_host",
                "Shut Down Host",
                topics.shutdown_command.clone(),
                config.shutdown_payload.clone(),
            )
            .with_entity_category("config")
            .with_icon("mdi:power")
        };

        components.insert("shutdown_host".to_string(), shutdown_button);

        if config.shutdown_delay_secs > 0 {
            components.insert(
                "cancel_shutdown".to_string(),
                Component::button(
                    identity,
                    "cancel_shutdown",
                    "Cancel Pending Shutdown",
                    topics.shutdown_command.clone(),
                    config.shutdown_cancel_payload.clone(),
                )
                .with_entity_category("config")
                .with_icon("mdi:cancel"),
            );
        }
    }

    if config.network_metrics_enabled
        && let Some(network_info) = network_info
    {
        for (interface_id, interface) in &network_info.interfaces {
            components.insert(
                format!("network_{}_download_rate", interface_id),
                Component::sensor(
                    identity,
                    &format!("network_{}_download_rate", interface_id),
                    format!("Network {} Download Rate", interface.name),
                    topics.network_state.clone(),
                    format!(
                        "{{{{ value_json.interfaces.{}.download_rate | default(none) }}}}",
                        interface_id
                    ),
                )
                .with_unit("B/s")
                .with_device_class("data_rate")
                .with_state_class("measurement")
                .with_precision(1)
                .with_icon("mdi:download-network"),
            );

            components.insert(
                format!("network_{}_upload_rate", interface_id),
                Component::sensor(
                    identity,
                    &format!("network_{}_upload_rate", interface_id),
                    format!("Network {} Upload Rate", interface.name),
                    topics.network_state.clone(),
                    format!(
                        "{{{{ value_json.interfaces.{}.upload_rate | default(none) }}}}",
                        interface_id
                    ),
                )
                .with_unit("B/s")
                .with_device_class("data_rate")
                .with_state_class("measurement")
                .with_precision(1)
                .with_icon("mdi:upload-network"),
            );

            components.insert(
                format!("network_{}_total_download", interface_id),
                Component::sensor(
                    identity,
                    &format!("network_{}_total_download", interface_id),
                    format!("Network {} Total Download", interface.name),
                    topics.network_state.clone(),
                    format!(
                        "{{{{ value_json.interfaces.{}.total_download | default(none) }}}}",
                        interface_id
                    ),
                )
                .with_unit("B")
                .with_device_class("data_size")
                .with_state_class("total_increasing")
                .with_icon("mdi:download"),
            );

            components.insert(
                format!("network_{}_total_upload", interface_id),
                Component::sensor(
                    identity,
                    &format!("network_{}_total_upload", interface_id),
                    format!("Network {} Total Upload", interface.name),
                    topics.network_state.clone(),
                    format!(
                        "{{{{ value_json.interfaces.{}.total_upload | default(none) }}}}",
                        interface_id
                    ),
                )
                .with_unit("B")
                .with_device_class("data_size")
                .with_state_class("total_increasing")
                .with_icon("mdi:upload"),
            );
        }
    }

    if config.disk_metrics_enabled
        && let Some(disks) = disks
    {
        for (disk_id, disk) in &disks.disks {
            components.insert(
                format!("disk_{}_used", disk_id),
                Component::sensor(
                    identity,
                    &format!("disk_{}_used", disk_id),
                    format!("Disk {} Used", disk.path),
                    topics.disk_state.clone(),
                    format!(
                        "{{{{ value_json.disks.{}.used | default(none) }}}}",
                        disk_id
                    ),
                )
                .with_unit("B")
                .with_device_class("data_size")
                .with_state_class("measurement")
                .with_icon("mdi:harddisk"),
            );

            components.insert(
                format!("disk_{}_available", disk_id),
                Component::sensor(
                    identity,
                    &format!("disk_{}_available", disk_id),
                    format!("Disk {} Available", disk.path),
                    topics.disk_state.clone(),
                    format!(
                        "{{{{ value_json.disks.{}.available | default(none) }}}}",
                        disk_id
                    ),
                )
                .with_unit("B")
                .with_device_class("data_size")
                .with_state_class("measurement")
                .with_icon("mdi:harddisk"),
            );

            components.insert(
                format!("disk_{}_total", disk_id),
                Component::sensor(
                    identity,
                    &format!("disk_{}_total", disk_id),
                    format!("Disk {} Total", disk.path),
                    topics.disk_info_state.clone(),
                    format!(
                        "{{{{ value_json.disks.{}.total | default(none) }}}}",
                        disk_id
                    ),
                )
                .with_unit("B")
                .with_device_class("data_size")
                .with_state_class("measurement")
                .with_icon("mdi:harddisk"),
            );

            components.insert(
                format!("disk_{}_usage", disk_id),
                Component::sensor(
                    identity,
                    &format!("disk_{}_usage", disk_id),
                    format!("Disk {} Usage", disk.path),
                    topics.disk_state.clone(),
                    format!(
                        "{{{{ value_json.disks.{}.usage | default(none) }}}}",
                        disk_id
                    ),
                )
                .with_unit("%")
                .with_state_class("measurement")
                .with_precision(1)
                .with_icon("mdi:harddisk"),
            );
        }
    }

    components
}

#[cfg(test)]
mod tests {
    use super::build_device_discovery_message;
    use crate::config::Config;
    use crate::device::{Identity, Topics};
    use crate::system::models::{
        DiskInfoState, GpuInfoState, NetworkInfoState, NetworkInterfaceInfoPayload,
    };
    use std::collections::BTreeMap;

    fn test_config() -> Config {
        Config {
            config_dir: None,
            log_dir: None,
            mqtt_host: "127.0.0.1".to_string(),
            mqtt_port: 1883,
            mqtt_username: None,
            mqtt_password: None,
            discovery_prefix: "homeassistant".to_string(),
            home_assistant_status_topic: "homeassistant/status".to_string(),
            topic_prefix: "monitor/system".to_string(),
            node_id: Some("test-node".to_string()),
            device_name: Some("Test Node".to_string()),
            host_metrics_enabled: true,
            cpu_metrics_enabled: true,
            gpu_metrics_enabled: true,
            memory_metrics_enabled: true,
            uptime_metrics_enabled: true,
            disk_metrics_enabled: true,
            network_metrics_enabled: true,
            lighthouse_enabled: false,
            lighthouse_secret_id: None,
            lighthouse_secret_key: None,
            lighthouse_session_token: None,
            lighthouse_endpoint: "lighthouse.tencentcloudapi.com".to_string(),
            lighthouse_region: None,
            lighthouse_instance_id: None,
            network_include_interfaces: vec!["Ethernet".to_string()],
            disk_include_paths: vec!["/srv/data".to_string()],
            enable_shutdown_button: false,
            shutdown_payload: "shutdown".to_string(),
            shutdown_cancel_payload: "cancel".to_string(),
            shutdown_delay_secs: 30,
            shutdown_dry_run: false,
            cpu_interval_secs: 1,
            gpu_interval_secs: 1,
            lighthouse_interval_secs: 300,
            memory_interval_secs: 5,
            uptime_interval_secs: 300,
            disk_interval_secs: 30,
            network_interval_secs: 1,
            cpu_change_threshold_pct: 1.0,
            gpu_usage_change_threshold_pct: 1.0,
            gpu_memory_change_threshold_mib: 8,
            memory_change_threshold_mib: 8,
            disk_change_threshold_mib: 32,
            network_rate_change_threshold_bytes_per_sec: 10 * 1024,
            network_total_change_threshold_bytes: 10 * 1024,
        }
    }

    fn test_identity() -> Identity {
        Identity {
            node_id: "test-node".to_string(),
            device_id: "ha-system-ronitor-test-node".to_string(),
            discovery_object_id: "ha_system_ronitor_test_node".to_string(),
            entity_id_prefix: "ha_system_ronitor_test_node".to_string(),
            device_name: "Test Node".to_string(),
            host_name: "test-host".to_string(),
            os_name: "TestOS".to_string(),
            os_version: "1.0".to_string(),
        }
    }

    fn test_disk_info() -> DiskInfoState {
        let mut disks = BTreeMap::new();
        disks.insert(
            "c".to_string(),
            crate::system::models::DiskInfoPayload {
                name: "Disk".to_string(),
                path: "/srv/data".to_string(),
                mount_point: "/".to_string(),
                file_system: "ntfs".to_string(),
                total: 100,
            },
        );
        DiskInfoState { disks }
    }

    fn test_network_info() -> NetworkInfoState {
        let mut interfaces = BTreeMap::new();
        interfaces.insert(
            "ethernet".to_string(),
            NetworkInterfaceInfoPayload {
                name: "Ethernet".to_string(),
            },
        );
        NetworkInfoState { interfaces }
    }

    #[test]
    fn discovery_omits_disabled_metric_groups() {
        let mut config = test_config();
        config.cpu_metrics_enabled = false;
        config.memory_metrics_enabled = false;
        config.disk_metrics_enabled = false;
        config.network_metrics_enabled = false;

        let identity = test_identity();
        let topics = Topics::from_identity(&config, &identity);
        let gpu_info = GpuInfoState {
            gpu_name: "GPU".to_string(),
            gpu_memory_total: 1024,
        };

        let message = build_device_discovery_message(
            &config,
            &identity,
            &topics,
            Some(&gpu_info),
            Some(&test_disk_info()),
            Some(&test_network_info()),
        );

        assert!(!message.payload.components.contains_key("cpu_usage"));
        assert!(!message.payload.components.contains_key("memory_used"));
        assert!(
            !message
                .payload
                .components
                .contains_key("network_download_rate")
        );
        assert!(!message.payload.components.contains_key("disk_c_used"));
        assert!(message.payload.components.contains_key("gpu_usage"));
        assert!(message.payload.components.contains_key("uptime"));
    }
}

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
    disks: &DiskInfoState,
    network_info: &NetworkInfoState,
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
    disks: &DiskInfoState,
    network_info: &NetworkInfoState,
) -> BTreeMap<String, Component> {
    let mut components = BTreeMap::new();

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

    if let Some(gpu_info) = gpu_info {
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

    for (disk_id, disk) in &disks.disks {
        components.insert(
            format!("disk_{}_used", disk_id),
            Component::sensor(
                identity,
                &format!("disk_{}_used", disk_id),
                format!("Disk {} Used", disk.mount_point),
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
                format!("Disk {} Available", disk.mount_point),
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
                format!("Disk {} Total", disk.mount_point),
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
                format!("Disk {} Usage", disk.mount_point),
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

    components
}

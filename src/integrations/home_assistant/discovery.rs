use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::json;

use crate::config::Config;
use crate::device::{Identity, Topics};
use crate::system::models::{DiskState, GpuState};

const CELSIUS_UNIT: &str = "\u{00B0}C";
const LEGACY_SENSOR_COMPONENT_IDS: &[&str] = &[
    "cpu_usage",
    "cpu_package_temp",
    "cpu_model",
    "os_version",
    "cpu_cores",
    "cpu_threads",
    "uptime",
    "process_count",
    "gpu_name",
    "gpu_usage",
    "gpu_temperature",
    "gpu_memory_available",
    "gpu_memory_used",
    "gpu_memory_total",
    "gpu_memory_usage",
    "memory_used",
    "memory_total",
    "memory_usage",
    "swap_used",
    "swap_total",
    "swap_usage",
];
const LEGACY_BUTTON_COMPONENT_IDS: &[&str] = &["shutdown_host"];
const REMOVED_DEVICE_SENSOR_COMPONENT_IDS: &[&str] = &[
    "cpu_cores",
    "cpu_threads",
    "process_count",
    "swap_used",
    "swap_total",
    "swap_usage",
    "gpu_memory_free",
];

#[derive(Debug, Clone)]
pub struct DeviceDiscoveryMessage {
    pub topic: String,
    pub payload: DeviceDiscoveryPayload,
    pub legacy_topics: BTreeMap<String, String>,
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
    identifiers: Vec<String>,
    name: String,
    manufacturer: String,
    model: String,
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

pub fn build_device_discovery_message(
    config: &Config,
    identity: &Identity,
    topics: &Topics,
    gpu_state: Option<&GpuState>,
    disks: &DiskState,
) -> DeviceDiscoveryMessage {
    let components = build_components(config, identity, topics, gpu_state, disks);
    let legacy_topics = build_legacy_topics(topics, disks);

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
        legacy_topics,
    }
}

pub fn build_removed_device_components_cleanup_payload(
    config: &Config,
    identity: &Identity,
    topics: &Topics,
    gpu_state: Option<&GpuState>,
    disks: &DiskState,
) -> Option<Vec<u8>> {
    if REMOVED_DEVICE_SENSOR_COMPONENT_IDS.is_empty() {
        return None;
    }

    let message = build_device_discovery_message(config, identity, topics, gpu_state, disks);
    let mut payload = serde_json::to_value(&message.payload).ok()?;
    let components = payload.get_mut("cmps")?.as_object_mut()?;

    for component_id in REMOVED_DEVICE_SENSOR_COMPONENT_IDS {
        components.insert((*component_id).to_string(), json!({ "p": "sensor" }));
    }

    serde_json::to_vec(&payload).ok()
}

fn build_legacy_topics(topics: &Topics, disks: &DiskState) -> BTreeMap<String, String> {
    let mut legacy_topics = BTreeMap::new();

    for component_id in LEGACY_SENSOR_COMPONENT_IDS {
        legacy_topics.insert(
            (*component_id).to_string(),
            topics.legacy_component_discovery("sensor", component_id),
        );
    }

    for component_id in LEGACY_BUTTON_COMPONENT_IDS {
        legacy_topics.insert(
            (*component_id).to_string(),
            topics.legacy_component_discovery("button", component_id),
        );
    }

    for disk_id in disks.disks.keys() {
        for suffix in ["used", "available", "total", "usage"] {
            let component_id = format!("disk_{disk_id}_{suffix}");
            legacy_topics.insert(
                component_id.clone(),
                topics.legacy_component_discovery("sensor", &component_id),
            );
        }
    }

    legacy_topics
}

fn build_components(
    config: &Config,
    identity: &Identity,
    topics: &Topics,
    gpu_state: Option<&GpuState>,
    disks: &DiskState,
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
            topics.cpu_state.clone(),
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
            topics.cpu_state.clone(),
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
            topics.cpu_state.clone(),
            "{{ value_json.uptime }}",
        )
        .with_unit("s")
        .with_device_class("duration")
        .with_state_class("measurement")
        .with_icon("mdi:timer-outline"),
    );

    if let Some(gpu_state) = gpu_state {
        components.insert(
            "gpu_name".to_string(),
            Component::sensor(
                identity,
                "gpu_name",
                "GPU Name",
                topics.gpu_state.clone(),
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

        if gpu_state.gpu_temperature.is_some() {
            components.insert(
                "gpu_temperature".to_string(),
                Component::sensor(
                    identity,
                    "gpu_temperature",
                    "GPU Temperature",
                    topics.gpu_state.clone(),
                    "{{ value_json.gpu_temperature }}",
                )
                .with_unit(CELSIUS_UNIT)
                .with_device_class("temperature")
                .with_state_class("measurement")
                .with_precision(1)
                .with_icon("mdi:thermometer"),
            );
        }

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
                topics.gpu_state.clone(),
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

        if gpu_state.gpu_memory_total == 0 {
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
            topics.memory_state.clone(),
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

    if config.enable_shutdown_button {
        components.insert(
            "shutdown_host".to_string(),
            Component::button(
                identity,
                "shutdown_host",
                "Shut Down Host",
                topics.shutdown_command.clone(),
                config.shutdown_payload.clone(),
            )
            .with_entity_category("config")
            .with_icon("mdi:power"),
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
                format!("{{{{ value_json.disks.{}.used }}}}", disk_id),
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
                format!("{{{{ value_json.disks.{}.available }}}}", disk_id),
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
                topics.disk_state.clone(),
                format!("{{{{ value_json.disks.{}.total }}}}", disk_id),
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
                format!("{{{{ value_json.disks.{}.usage }}}}", disk_id),
            )
            .with_unit("%")
            .with_state_class("measurement")
            .with_precision(1)
            .with_icon("mdi:harddisk"),
        );
    }

    components
}

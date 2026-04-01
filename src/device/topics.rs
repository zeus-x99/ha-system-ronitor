use crate::config::Config;
use crate::device::Identity;

#[derive(Debug, Clone)]
pub struct Topics {
    pub device_discovery: String,
    pub host_info_state: String,
    pub cpu_state: String,
    pub cpu_info_state: String,
    pub uptime_state: String,
    pub shutdown_state: String,
    pub gpu_state: String,
    pub gpu_info_state: String,
    pub memory_state: String,
    pub memory_info_state: String,
    pub disk_state: String,
    pub disk_info_state: String,
    pub network_state: String,
    pub network_info_state: String,
    pub node_lock: String,
    pub shutdown_command: String,
    pub availability: String,
    pub ha_status: String,
}

impl Topics {
    pub fn from_identity(config: &Config, identity: &Identity) -> Self {
        let device_discovery = format!(
            "{}/device/{}/config",
            config.discovery_prefix, identity.discovery_object_id
        );

        Self {
            device_discovery,
            host_info_state: format!("{}/{}/host/info", config.topic_prefix, identity.node_id),
            cpu_state: format!("{}/{}/cpu/state", config.topic_prefix, identity.node_id),
            cpu_info_state: format!("{}/{}/cpu/info", config.topic_prefix, identity.node_id),
            uptime_state: format!("{}/{}/uptime/state", config.topic_prefix, identity.node_id),
            shutdown_state: format!(
                "{}/{}/shutdown/state",
                config.topic_prefix, identity.node_id
            ),
            gpu_state: format!("{}/{}/gpu/state", config.topic_prefix, identity.node_id),
            gpu_info_state: format!("{}/{}/gpu/info", config.topic_prefix, identity.node_id),
            memory_state: format!("{}/{}/memory/state", config.topic_prefix, identity.node_id),
            memory_info_state: format!("{}/{}/memory/info", config.topic_prefix, identity.node_id),
            disk_state: format!("{}/{}/disk/state", config.topic_prefix, identity.node_id),
            disk_info_state: format!("{}/{}/disk/info", config.topic_prefix, identity.node_id),
            network_state: format!("{}/{}/network/state", config.topic_prefix, identity.node_id),
            network_info_state: format!(
                "{}/{}/network/info",
                config.topic_prefix, identity.node_id
            ),
            node_lock: format!("{}/{}/meta/lock", config.topic_prefix, identity.node_id),
            shutdown_command: format!(
                "{}/{}/command/shutdown",
                config.topic_prefix, identity.node_id
            ),
            availability: format!("{}/{}/availability", config.topic_prefix, identity.node_id),
            ha_status: config.home_assistant_status_topic.clone(),
        }
    }
}

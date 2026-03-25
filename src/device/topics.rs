use crate::config::Config;

#[derive(Debug, Clone)]
pub struct Topics {
    pub device_discovery: String,
    pub cpu_state: String,
    pub uptime_state: String,
    pub gpu_state: String,
    pub memory_state: String,
    pub disk_state: String,
    pub shutdown_command: String,
    pub availability: String,
    pub ha_status: String,
}

impl Topics {
    pub fn from_config(config: &Config, node_id: &str) -> Self {
        Self {
            device_discovery: format!("{}/device/{}/config", config.discovery_prefix, node_id),
            cpu_state: format!("{}/{}/cpu/state", config.topic_prefix, node_id),
            uptime_state: format!("{}/{}/uptime/state", config.topic_prefix, node_id),
            gpu_state: format!("{}/{}/gpu/state", config.topic_prefix, node_id),
            memory_state: format!("{}/{}/memory/state", config.topic_prefix, node_id),
            disk_state: format!("{}/{}/disk/state", config.topic_prefix, node_id),
            shutdown_command: format!("{}/{}/command/shutdown", config.topic_prefix, node_id),
            availability: format!("{}/{}/availability", config.topic_prefix, node_id),
            ha_status: config.home_assistant_status_topic.clone(),
        }
    }
}

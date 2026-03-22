use sysinfo::System;

use crate::config::Config;
use crate::shared::util::slugify;

#[derive(Debug, Clone)]
pub struct Identity {
    pub node_id: String,
    pub device_id: String,
    pub device_name: String,
    pub host_name: String,
    pub os_name: String,
    pub os_version: String,
}

impl Identity {
    pub fn detect(config: &Config) -> Self {
        let host_name = System::host_name().unwrap_or_else(|| "unknown-host".to_string());
        let node_id = config
            .node_id
            .clone()
            .unwrap_or_else(|| slugify(&host_name));
        let os_name = System::name().unwrap_or_else(|| "Unknown OS".to_string());
        let os_version = System::long_os_version()
            .or_else(System::os_version)
            .unwrap_or_else(|| "unknown".to_string());
        let device_name = config
            .device_name
            .clone()
            .unwrap_or_else(|| format!("{host_name} System Monitor"));

        Self {
            node_id: node_id.clone(),
            device_id: format!("ha-system-ronitor-{node_id}"),
            device_name,
            host_name,
            os_name,
            os_version,
        }
    }
}

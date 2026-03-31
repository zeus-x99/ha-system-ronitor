use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HostInfoState {
    pub os_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CpuState {
    pub timestamp: String,
    pub cpu_usage: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_package_temp: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CpuInfoState {
    pub cpu_model: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UptimeState {
    pub timestamp: String,
    pub uptime: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuState {
    pub timestamp: String,
    pub gpu_usage: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_temperature: Option<f32>,
    pub gpu_memory_available: u64,
    pub gpu_memory_used: u64,
    pub gpu_memory_usage: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuInfoState {
    pub gpu_name: String,
    pub gpu_memory_total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryInfoState {
    pub memory_total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryState {
    pub timestamp: String,
    pub memory_used: u64,
    pub memory_usage: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkInfoState {
    pub interfaces: BTreeMap<String, NetworkInterfaceInfoPayload>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkInterfaceInfoPayload {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkState {
    pub timestamp: String,
    pub network_download_rate: f64,
    pub network_upload_rate: f64,
    pub network_total_download: u64,
    pub network_total_upload: u64,
    pub interfaces: BTreeMap<String, NetworkInterfaceStatePayload>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkInterfaceStatePayload {
    pub download_rate: f64,
    pub upload_rate: f64,
    pub total_download: u64,
    pub total_upload: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskInfoState {
    pub disks: BTreeMap<String, DiskInfoPayload>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskInfoPayload {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskState {
    pub timestamp: String,
    pub disks: BTreeMap<String, DiskStatePayload>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskStatePayload {
    pub available: u64,
    pub used: u64,
    pub usage: f64,
}

impl CpuState {
    pub fn changed_significantly_from(&self, previous: &Self, threshold_pct: f32) -> bool {
        if (self.cpu_usage - previous.cpu_usage).abs() >= threshold_pct {
            return true;
        }

        if option_abs_diff_f32(self.cpu_package_temp, previous.cpu_package_temp)
            >= Some(CPU_TEMPERATURE_CHANGE_THRESHOLD_C)
        {
            return true;
        }

        false
    }
}

impl UptimeState {
    pub fn changed_from(&self, previous: &Self) -> bool {
        self.uptime != previous.uptime
    }
}

impl MemoryState {
    pub fn changed_significantly_from(&self, previous: &Self, threshold_bytes: u64) -> bool {
        abs_diff_u64(self.memory_used, previous.memory_used) >= threshold_bytes
    }
}

impl NetworkState {
    pub fn changed_from(&self, previous: &Self) -> bool {
        if self.network_total_download != previous.network_total_download
            || self.network_total_upload != previous.network_total_upload
            || abs_diff_f64(self.network_download_rate, previous.network_download_rate)
                >= NETWORK_RATE_CHANGE_THRESHOLD_BYTES_PER_SEC
            || abs_diff_f64(self.network_upload_rate, previous.network_upload_rate)
                >= NETWORK_RATE_CHANGE_THRESHOLD_BYTES_PER_SEC
            || self.interfaces.len() != previous.interfaces.len()
        {
            return true;
        }

        for (interface_id, interface) in &self.interfaces {
            let Some(previous_interface) = previous.interfaces.get(interface_id) else {
                return true;
            };

            if interface.total_download != previous_interface.total_download
                || interface.total_upload != previous_interface.total_upload
                || abs_diff_f64(interface.download_rate, previous_interface.download_rate)
                    >= NETWORK_RATE_CHANGE_THRESHOLD_BYTES_PER_SEC
                || abs_diff_f64(interface.upload_rate, previous_interface.upload_rate)
                    >= NETWORK_RATE_CHANGE_THRESHOLD_BYTES_PER_SEC
            {
                return true;
            }
        }

        false
    }
}

impl GpuState {
    pub fn changed_significantly_from(
        &self,
        previous: &Self,
        usage_threshold_pct: f32,
        memory_threshold_bytes: u64,
    ) -> bool {
        option_abs_diff_f32(self.gpu_temperature, previous.gpu_temperature)
            >= Some(GPU_TEMPERATURE_CHANGE_THRESHOLD_C)
            || (self.gpu_usage - previous.gpu_usage).abs() >= usage_threshold_pct
            || abs_diff_u64(self.gpu_memory_used, previous.gpu_memory_used)
                >= memory_threshold_bytes
    }
}

impl DiskState {
    pub fn changed_significantly_from(&self, previous: &Self, threshold_bytes: u64) -> bool {
        if self.disks.len() != previous.disks.len() {
            return true;
        }

        for (disk_id, disk) in &self.disks {
            let Some(previous_disk) = previous.disks.get(disk_id) else {
                return true;
            };

            if abs_diff_u64(disk.used, previous_disk.used) >= threshold_bytes
                || abs_diff_u64(disk.available, previous_disk.available) >= threshold_bytes
            {
                return true;
            }
        }

        false
    }
}

fn abs_diff_u64(left: u64, right: u64) -> u64 {
    left.abs_diff(right)
}

fn abs_diff_f64(left: f64, right: f64) -> f64 {
    (left - right).abs()
}

fn option_abs_diff_f32(left: Option<f32>, right: Option<f32>) -> Option<f32> {
    match (left, right) {
        (Some(left), Some(right)) => Some((left - right).abs()),
        (None, None) => None,
        _ => Some(f32::MAX),
    }
}

const CPU_TEMPERATURE_CHANGE_THRESHOLD_C: f32 = 1.0;
const GPU_TEMPERATURE_CHANGE_THRESHOLD_C: f32 = 1.0;
const NETWORK_RATE_CHANGE_THRESHOLD_BYTES_PER_SEC: f64 = 1.0;

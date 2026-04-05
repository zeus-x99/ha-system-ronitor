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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ShutdownState {
    pub shutdown_remaining_secs: u64,
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
pub struct LighthouseState {
    pub timestamp: String,
    pub lighthouse_instance_id: String,
    pub lighthouse_package_id: String,
    pub lighthouse_used: u64,
    pub lighthouse_total: u64,
    pub lighthouse_remaining: u64,
    pub lighthouse_overflow: u64,
    pub lighthouse_usage: f64,
    pub lighthouse_status: String,
    pub lighthouse_cycle_start: String,
    pub lighthouse_cycle_end: String,
    pub lighthouse_deadline: String,
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
    pub fn changed_from(
        &self,
        previous: &Self,
        rate_threshold_bytes_per_sec: f64,
        total_threshold_bytes: u64,
    ) -> bool {
        if abs_diff_u64(self.network_total_download, previous.network_total_download)
            >= total_threshold_bytes
            || abs_diff_u64(self.network_total_upload, previous.network_total_upload)
                >= total_threshold_bytes
            || abs_diff_f64(self.network_download_rate, previous.network_download_rate)
                >= rate_threshold_bytes_per_sec
            || abs_diff_f64(self.network_upload_rate, previous.network_upload_rate)
                >= rate_threshold_bytes_per_sec
            || self.interfaces.len() != previous.interfaces.len()
        {
            return true;
        }

        for (interface_id, interface) in &self.interfaces {
            let Some(previous_interface) = previous.interfaces.get(interface_id) else {
                return true;
            };

            if abs_diff_u64(interface.total_download, previous_interface.total_download)
                >= total_threshold_bytes
                || abs_diff_u64(interface.total_upload, previous_interface.total_upload)
                    >= total_threshold_bytes
                || abs_diff_f64(interface.download_rate, previous_interface.download_rate)
                    >= rate_threshold_bytes_per_sec
                || abs_diff_f64(interface.upload_rate, previous_interface.upload_rate)
                    >= rate_threshold_bytes_per_sec
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

impl LighthouseState {
    pub fn changed_from(&self, previous: &Self) -> bool {
        self.lighthouse_instance_id != previous.lighthouse_instance_id
            || self.lighthouse_package_id != previous.lighthouse_package_id
            || self.lighthouse_used != previous.lighthouse_used
            || self.lighthouse_total != previous.lighthouse_total
            || self.lighthouse_remaining != previous.lighthouse_remaining
            || self.lighthouse_overflow != previous.lighthouse_overflow
            || self.lighthouse_status != previous.lighthouse_status
            || self.lighthouse_cycle_start != previous.lighthouse_cycle_start
            || self.lighthouse_cycle_end != previous.lighthouse_cycle_end
            || self.lighthouse_deadline != previous.lighthouse_deadline
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

#[cfg(test)]
mod tests {
    use super::LighthouseState;

    fn sample_lighthouse_state() -> LighthouseState {
        LighthouseState {
            timestamp: "2026-04-05T00:00:00Z".to_string(),
            lighthouse_instance_id: "lhins-example".to_string(),
            lighthouse_package_id: "lhtfp-example".to_string(),
            lighthouse_used: 1,
            lighthouse_total: 10,
            lighthouse_remaining: 9,
            lighthouse_overflow: 0,
            lighthouse_usage: 10.0,
            lighthouse_status: "NETWORK_NORMAL".to_string(),
            lighthouse_cycle_start: "2026-04-01T00:00:00Z".to_string(),
            lighthouse_cycle_end: "2026-05-01T00:00:00Z".to_string(),
            lighthouse_deadline: "2030-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn lighthouse_change_detection_ignores_timestamp_only_updates() {
        let previous = sample_lighthouse_state();
        let mut next = sample_lighthouse_state();
        next.timestamp = "2026-04-05T00:05:00Z".to_string();

        assert!(!next.changed_from(&previous));
    }

    #[test]
    fn lighthouse_change_detection_tracks_usage_changes() {
        let previous = sample_lighthouse_state();
        let mut next = sample_lighthouse_state();
        next.lighthouse_used = 2;
        next.lighthouse_remaining = 8;
        next.lighthouse_usage = 20.0;

        assert!(next.changed_from(&previous));
    }
}

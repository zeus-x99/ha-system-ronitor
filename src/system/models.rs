use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CpuState {
    pub timestamp: String,
    pub cpu_usage: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_package_temp: Option<f32>,
    pub cpu_model: String,
    pub os_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UptimeState {
    pub timestamp: String,
    pub uptime: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuState {
    pub timestamp: String,
    pub gpu_name: String,
    pub gpu_usage: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu_temperature: Option<f32>,
    pub gpu_memory_available: u64,
    pub gpu_memory_used: u64,
    pub gpu_memory_total: u64,
    pub gpu_memory_usage: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryState {
    pub timestamp: String,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_usage: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskState {
    pub timestamp: String,
    pub disks: BTreeMap<String, DiskPayload>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskPayload {
    pub name: String,
    pub mount_point: String,
    pub file_system: String,
    pub total: u64,
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

        if self.cpu_model != previous.cpu_model || self.os_version != previous.os_version {
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
        self.memory_total != previous.memory_total
            || abs_diff_u64(self.memory_used, previous.memory_used) >= threshold_bytes
    }
}

impl GpuState {
    pub fn changed_significantly_from(
        &self,
        previous: &Self,
        usage_threshold_pct: f32,
        memory_threshold_bytes: u64,
    ) -> bool {
        self.gpu_name != previous.gpu_name
            || option_abs_diff_f32(self.gpu_temperature, previous.gpu_temperature)
                >= Some(GPU_TEMPERATURE_CHANGE_THRESHOLD_C)
            || self.gpu_memory_total != previous.gpu_memory_total
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

            if disk.total != previous_disk.total
                || disk.mount_point != previous_disk.mount_point
                || disk.file_system != previous_disk.file_system
                || abs_diff_u64(disk.used, previous_disk.used) >= threshold_bytes
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

fn option_abs_diff_f32(left: Option<f32>, right: Option<f32>) -> Option<f32> {
    match (left, right) {
        (Some(left), Some(right)) => Some((left - right).abs()),
        (None, None) => None,
        _ => Some(f32::MAX),
    }
}

const CPU_TEMPERATURE_CHANGE_THRESHOLD_C: f32 = 1.0;
const GPU_TEMPERATURE_CHANGE_THRESHOLD_C: f32 = 1.0;

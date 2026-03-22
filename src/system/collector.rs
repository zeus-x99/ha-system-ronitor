use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;
use sysinfo::{Components, CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

use crate::device::Identity;
use crate::shared::util::disk_component_id;
use crate::system::gpu::GpuReader;
use crate::system::models::{CpuState, DiskPayload, DiskState, GpuState, MemoryState};
use crate::system::runtime::{CpuTemperatureReader, detect_gpu_temp_from_components};

const RUNTIME_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub struct Collector {
    system: System,
    cpu_usage_system: System,
    disks: Disks,
    components: Components,
    cpu_temperature_reader: CpuTemperatureReader,
    gpu_reader: GpuReader,
    cpu_model: String,
    os_version: String,
    cpu_package_temp: Option<f32>,
    gpu_temperature: Option<f32>,
    last_runtime_refresh_at: Option<Instant>,
}

impl Collector {
    pub async fn new(identity: &Identity) -> Self {
        let mut cpu_usage_system = System::new_with_specifics(
            RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing().with_cpu_usage()),
        );
        cpu_usage_system.refresh_cpu_usage();
        tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;

        let mut system = System::new_with_specifics(
            RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()),
        );
        system.refresh_memory();

        let cpu_model = cpu_usage_system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().trim().to_string())
            .filter(|brand| !brand.is_empty())
            .unwrap_or_else(|| "Unknown CPU".to_string());
        let disks = Disks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();
        let mut cpu_temperature_reader = CpuTemperatureReader::new();
        let gpu_reader = GpuReader::new(Some(&cpu_model));
        let cpu_package_temp = cpu_temperature_reader.read(&components);
        let gpu_temperature = detect_gpu_temp_from_components(&components);

        Self {
            system,
            cpu_usage_system,
            disks,
            components,
            cpu_temperature_reader,
            gpu_reader,
            cpu_model,
            os_version: identity.os_version.clone(),
            cpu_package_temp,
            gpu_temperature,
            last_runtime_refresh_at: Some(Instant::now()),
        }
    }

    pub fn sample_cpu(&mut self) -> CpuState {
        self.cpu_usage_system.refresh_cpu_usage();
        self.refresh_runtime_snapshot_if_needed(false);

        let timestamp = Utc::now().to_rfc3339();
        let cpu_usage = self.cpu_usage_system.global_cpu_usage();

        CpuState {
            timestamp,
            cpu_usage,
            cpu_package_temp: self.cpu_package_temp,
            cpu_model: self.cpu_model.clone(),
            os_version: self.os_version.clone(),
            uptime: System::uptime(),
        }
    }

    pub fn sample_memory(&mut self) -> MemoryState {
        self.system.refresh_memory();

        let memory_total = self.system.total_memory();
        let memory_used = self.system.used_memory();
        MemoryState {
            timestamp: Utc::now().to_rfc3339(),
            memory_total,
            memory_used,
            memory_usage: percent(memory_used, memory_total),
        }
    }

    pub fn sample_gpu(&mut self) -> Option<GpuState> {
        self.refresh_runtime_snapshot_if_needed(false);

        let mut gpu_state = self.gpu_reader.read()?;
        gpu_state.gpu_temperature = gpu_state.gpu_temperature.or(self.gpu_temperature);
        Some(gpu_state)
    }

    pub fn sample_disks(&mut self) -> DiskState {
        self.disks.refresh(true);

        let disks = self
            .disks
            .list()
            .iter()
            .filter_map(|disk| {
                let mount_point = disk.mount_point().display().to_string();
                let disk_id = disk_component_id(&mount_point, &disk.name().to_string_lossy());
                let total = disk.total_space();
                if total == 0 {
                    return None;
                }
                let available = disk.available_space();
                let used = total.saturating_sub(available);
                let usage = percent(used, total);
                let file_system = disk.file_system().to_string_lossy().into_owned();

                Some((
                    disk_id,
                    DiskPayload {
                        name: disk.name().to_string_lossy().into_owned(),
                        mount_point,
                        file_system,
                        total,
                        available,
                        used,
                        usage,
                    },
                ))
            })
            .collect::<BTreeMap<_, _>>();

        DiskState {
            timestamp: Utc::now().to_rfc3339(),
            disks,
        }
    }

    pub fn sample_all(&mut self) -> Result<(CpuState, Option<GpuState>, MemoryState, DiskState)> {
        Ok((
            self.sample_cpu(),
            self.sample_gpu(),
            self.sample_memory(),
            self.sample_disks(),
        ))
    }

    fn refresh_runtime_snapshot_if_needed(&mut self, force: bool) {
        let should_refresh = force
            || self
                .last_runtime_refresh_at
                .is_none_or(|instant| instant.elapsed() >= RUNTIME_REFRESH_INTERVAL);

        if !should_refresh {
            return;
        }

        self.components.refresh(false);
        self.cpu_package_temp = self.cpu_temperature_reader.read(&self.components);
        self.gpu_temperature = detect_gpu_temp_from_components(&self.components);

        self.last_runtime_refresh_at = Some(Instant::now());
    }
}

fn percent(value: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (value as f64 / total as f64) * 100.0
    }
}

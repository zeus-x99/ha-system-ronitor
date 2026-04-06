use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;
use sha2::{Digest, Sha256};
use sysinfo::{Components, CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

use crate::config::Config;
use crate::device::Identity;
use crate::shared::util::{disk_component_id, slugify};
use crate::system::gpu::{GpuReader, GpuReading};
use crate::system::lighthouse::LighthouseReader;
use crate::system::models::{
    CpuInfoState, CpuState, DiskInfoPayload, DiskInfoState, DiskState, DiskStatePayload,
    GpuInfoState, GpuState, HostInfoState, LighthouseState, MemoryInfoState, MemoryState,
    NetworkInfoState, NetworkState, UptimeState,
};
use crate::system::network::NetworkReader;
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
    lighthouse_reader: Option<LighthouseReader>,
    network_reader: NetworkReader,
    disk_include_paths: Vec<String>,
    host_info: HostInfoState,
    cpu_info: CpuInfoState,
    memory_info: MemoryInfoState,
    gpu_info: Option<GpuInfoState>,
    disk_info: DiskInfoState,
    network_info: NetworkInfoState,
    cpu_package_temp: Option<f32>,
    gpu_temperature: Option<f32>,
    last_runtime_refresh_at: Option<Instant>,
}

impl Collector {
    pub async fn new(identity: &Identity, config: &Config) -> Result<Self> {
        let mut cpu_usage_system = System::new_with_specifics(
            RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing().with_cpu_usage()),
        );
        cpu_usage_system.refresh_cpu_usage();
        tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;

        let mut system = System::new_with_specifics(
            RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()),
        );
        system.refresh_memory();

        let disks = Disks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();
        let mut cpu_temperature_reader = CpuTemperatureReader::new();
        let mut gpu_reader = GpuReader::new();
        let lighthouse_reader = LighthouseReader::new(config)?;
        let network_reader = NetworkReader::new(&config.network_include_interfaces);

        let cpu_model = cpu_usage_system
            .cpus()
            .first()
            .map(|cpu| cpu.brand().trim().to_string())
            .filter(|brand| !brand.is_empty())
            .unwrap_or_else(|| "Unknown CPU".to_string());

        let host_info = HostInfoState {
            os_version: identity.os_version.clone(),
        };
        let cpu_info = CpuInfoState { cpu_model };
        let memory_info = MemoryInfoState {
            memory_total: system.total_memory(),
        };
        let disk_info = build_disk_info(&disks, &config.disk_include_paths);
        let network_info = network_reader.info_state();
        let cpu_package_temp = cpu_temperature_reader.read(&components);
        let gpu_temperature = detect_gpu_temp_from_components(&components);
        let gpu_info = gpu_reader
            .read()
            .map(|reading| gpu_info_from_reading(&reading));

        Ok(Self {
            system,
            cpu_usage_system,
            disks,
            components,
            cpu_temperature_reader,
            gpu_reader,
            lighthouse_reader,
            network_reader,
            disk_include_paths: config.disk_include_paths.clone(),
            host_info,
            cpu_info,
            memory_info,
            gpu_info,
            disk_info,
            network_info,
            cpu_package_temp,
            gpu_temperature,
            last_runtime_refresh_at: Some(Instant::now()),
        })
    }

    pub fn host_info(&self) -> HostInfoState {
        self.host_info.clone()
    }

    pub fn cpu_info(&self) -> CpuInfoState {
        self.cpu_info.clone()
    }

    pub fn memory_info(&self) -> MemoryInfoState {
        self.memory_info.clone()
    }

    pub fn gpu_info(&self) -> Option<GpuInfoState> {
        self.gpu_info.clone()
    }

    pub fn disk_info(&self) -> DiskInfoState {
        self.disk_info.clone()
    }

    pub fn network_info(&self) -> NetworkInfoState {
        self.network_info.clone()
    }

    pub fn sample_cpu(&mut self) -> CpuState {
        self.cpu_usage_system.refresh_cpu_usage();
        self.refresh_runtime_snapshot_if_needed(false);

        CpuState {
            timestamp: chrono::Utc::now().to_rfc3339(),
            cpu_usage: self.cpu_usage_system.global_cpu_usage(),
            cpu_package_temp: self.cpu_package_temp,
        }
    }

    pub fn sample_uptime(&self) -> UptimeState {
        UptimeState {
            timestamp: chrono::Utc::now().to_rfc3339(),
            uptime: System::uptime(),
        }
    }

    pub fn sample_memory(&mut self) -> MemoryState {
        self.system.refresh_memory();

        let memory_total = self.system.total_memory();
        let memory_used = self.system.used_memory();
        MemoryState {
            timestamp: chrono::Utc::now().to_rfc3339(),
            memory_used,
            memory_usage: percent(memory_used, memory_total),
        }
    }

    pub fn sample_gpu(&mut self) -> Option<GpuState> {
        self.refresh_runtime_snapshot_if_needed(false);

        let mut reading = self.gpu_reader.read()?;
        reading.gpu_temperature = reading.gpu_temperature.or(self.gpu_temperature);
        self.update_gpu_info(&reading);

        Some(GpuState {
            timestamp: reading.timestamp,
            gpu_usage: reading.gpu_usage,
            gpu_temperature: reading.gpu_temperature,
            gpu_memory_available: reading.gpu_memory_available,
            gpu_memory_used: reading.gpu_memory_used,
            gpu_memory_usage: reading.gpu_memory_usage,
        })
    }

    pub fn sample_disks(&mut self) -> DiskState {
        self.disks.refresh(true);

        build_disk_state(&self.disks, &self.disk_include_paths)
    }

    pub fn sample_network(&mut self) -> NetworkState {
        self.network_reader.read()
    }

    pub async fn sample_lighthouse(&self) -> Result<Option<LighthouseState>> {
        match &self.lighthouse_reader {
            Some(reader) => reader.read().await,
            None => Ok(None),
        }
    }

    pub fn sample_all(
        &mut self,
    ) -> (
        CpuState,
        UptimeState,
        Option<GpuState>,
        MemoryState,
        DiskState,
        NetworkState,
    ) {
        (
            self.sample_cpu(),
            self.sample_uptime(),
            self.sample_gpu(),
            self.sample_memory(),
            self.sample_disks(),
            self.sample_network(),
        )
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

    fn update_gpu_info(&mut self, reading: &GpuReading) {
        let next = gpu_info_from_reading(reading);
        let should_replace = self.gpu_info.as_ref().is_none_or(|current| {
            current.gpu_name != next.gpu_name || current.gpu_memory_total != next.gpu_memory_total
        });

        if should_replace {
            self.gpu_info = Some(next);
        }
    }
}

fn build_disk_info(disks: &Disks, include_paths: &[String]) -> DiskInfoState {
    let disks = collect_disk_entries(disks, include_paths)
        .into_iter()
        .map(|entry| {
            (
                entry.disk_id,
                DiskInfoPayload {
                    name: entry.name,
                    path: entry.path,
                    mount_point: entry.mount_point,
                    file_system: entry.file_system,
                    total: entry.total,
                },
            )
        })
        .collect();

    DiskInfoState { disks }
}

fn build_disk_state(disks: &Disks, include_paths: &[String]) -> DiskState {
    let disks = collect_disk_entries(disks, include_paths)
        .into_iter()
        .map(|entry| {
            (
                entry.disk_id,
                DiskStatePayload {
                    available: entry.available,
                    used: entry.used,
                    usage: entry.usage,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    DiskState {
        timestamp: chrono::Utc::now().to_rfc3339(),
        disks,
    }
}

#[derive(Debug)]
struct SelectedDiskEntry {
    disk_id: String,
    name: String,
    path: String,
    mount_point: String,
    file_system: String,
    total: u64,
    available: u64,
    used: u64,
    usage: f64,
}

fn collect_disk_entries(disks: &Disks, include_paths: &[String]) -> Vec<SelectedDiskEntry> {
    if include_paths.is_empty() {
        return Vec::new();
    }

    include_paths
        .iter()
        .filter_map(|path| select_disk_entry(disks, path))
        .collect()
}

fn select_disk_entry(disks: &Disks, path: &str) -> Option<SelectedDiskEntry> {
    let requested_path = Path::new(path);
    let disk = disks
        .list()
        .iter()
        .filter(|disk| requested_path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().components().count())?;

    let total = disk.total_space();
    if total == 0 {
        return None;
    }

    let mount_point = disk.mount_point().display().to_string();
    let name = disk.name().to_string_lossy().into_owned();
    let available = disk.available_space();
    let used = total.saturating_sub(available);
    let disk_id = disk_path_component_id(path, &mount_point, &name);

    Some(SelectedDiskEntry {
        disk_id,
        name,
        path: path.to_string(),
        mount_point,
        file_system: disk.file_system().to_string_lossy().into_owned(),
        total,
        available,
        used,
        usage: percent(used, total),
    })
}

fn disk_path_component_id(path: &str, mount_point: &str, fallback_name: &str) -> String {
    let normalized_path = normalize_disk_path(path);
    let slug = slugify(&normalized_path);
    if slug.is_empty() {
        stable_disk_path_id(&normalized_path, mount_point, fallback_name)
    } else {
        slug
    }
}

fn normalize_disk_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut normalized = std::path::PathBuf::new();
    for component in Path::new(trimmed).components() {
        normalized.push(component.as_os_str());
    }

    let normalized = normalized.display().to_string();
    if normalized.is_empty() {
        trimmed.to_string()
    } else {
        normalized
    }
}

fn stable_disk_path_id(path: &str, mount_point: &str, fallback_name: &str) -> String {
    let digest = stable_disk_path_hash(path);
    if path == "/" || path == "\\" {
        format!("path_root_{digest}")
    } else {
        let fallback = disk_component_id(mount_point, fallback_name);
        format!("path_{fallback}_{digest}")
    }
}

fn stable_disk_path_hash(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    let digest = hex::encode(hasher.finalize());
    digest[..12].to_string()
}

fn gpu_info_from_reading(reading: &GpuReading) -> GpuInfoState {
    GpuInfoState {
        gpu_name: reading.gpu_name.clone(),
        gpu_memory_total: reading.gpu_memory_total,
    }
}

fn percent(value: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (value as f64 / total as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::disk_path_component_id;

    #[test]
    fn ascii_disk_paths_keep_human_readable_ids() {
        assert_eq!(
            disk_path_component_id("/mnt/data", "/", "rootfs"),
            "mnt_data"
        );
    }

    #[test]
    fn root_disk_path_gets_stable_non_colliding_id() {
        let root_id = disk_path_component_id("/", "/", "rootfs");
        let child_id = disk_path_component_id("/root", "/", "rootfs");

        assert!(root_id.starts_with("path_root_"));
        assert_ne!(root_id, child_id);
    }

    #[test]
    fn non_ascii_disk_paths_get_distinct_fallback_ids() {
        let first = disk_path_component_id("/下载", "/", "rootfs");
        let second = disk_path_component_id("/数据", "/", "rootfs");
        let repeated = disk_path_component_id("/下载/", "/", "rootfs");

        assert!(first.starts_with("path_"));
        assert!(second.starts_with("path_"));
        assert_eq!(first, repeated);
        assert_ne!(first, second);
    }
}

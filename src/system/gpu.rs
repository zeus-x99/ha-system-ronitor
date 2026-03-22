#[cfg(any(target_os = "windows", target_os = "linux"))]
mod nvml_native {
    use chrono::Utc;
    use nvml_wrapper::{Nvml, enum_wrappers::device::TemperatureSensor};
    use tracing::debug;

    use crate::system::models::GpuState;

    #[derive(Debug)]
    pub struct NvidiaGpuReader {
        nvml: Nvml,
        device_index: u32,
        device_name: String,
    }

    impl NvidiaGpuReader {
        pub fn new() -> Option<Self> {
            let nvml = match Nvml::init() {
                Ok(nvml) => nvml,
                Err(error) => {
                    debug!(%error, "NVML init failed");
                    return None;
                }
            };
            let device_index = select_nvml_device_index(&nvml)?;
            let device = nvml.device_by_index(device_index).ok()?;
            let device_name = device.name().ok()?;

            Some(Self {
                nvml,
                device_index,
                device_name,
            })
        }

        pub fn read(&self) -> Option<GpuState> {
            let device = self.nvml.device_by_index(self.device_index).ok()?;
            let memory = device.memory_info().ok()?;
            let utilization = device.utilization_rates().ok()?;
            let temperature = device.temperature(TemperatureSensor::Gpu).ok();
            let memory_used = memory.used.min(memory.total);
            let memory_available = memory.total.saturating_sub(memory_used);

            Some(GpuState {
                timestamp: Utc::now().to_rfc3339(),
                gpu_name: self.device_name.clone(),
                gpu_usage: utilization.gpu as f32,
                gpu_temperature: temperature.map(|value| value as f32),
                gpu_memory_available: memory_available,
                gpu_memory_used: memory_used,
                gpu_memory_total: memory.total,
                gpu_memory_usage: percent(memory_used, memory.total),
            })
        }
    }

    fn select_nvml_device_index(nvml: &Nvml) -> Option<u32> {
        let device_count = nvml.device_count().ok()?;
        let mut best = None::<(u32, u64)>;

        for device_index in 0..device_count {
            let device = match nvml.device_by_index(device_index) {
                Ok(device) => device,
                Err(_) => continue,
            };
            let memory_total = match device.memory_info() {
                Ok(memory) => memory.total,
                Err(_) => continue,
            };

            if memory_total == 0 {
                continue;
            }

            let should_replace = best
                .as_ref()
                .is_none_or(|(_, best_memory_total)| memory_total > *best_memory_total);
            if should_replace {
                best = Some((device_index, memory_total));
            }
        }

        best.map(|(device_index, _)| device_index)
    }

    fn percent(value: u64, total: u64) -> f64 {
        if total == 0 {
            0.0
        } else {
            (value as f64 / total as f64) * 100.0
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
use nvml_native::NvidiaGpuReader;

#[cfg(target_os = "windows")]
#[derive(Debug)]
pub struct GpuReader {
    nvml_reader: Option<NvidiaGpuReader>,
}

#[cfg(target_os = "windows")]
impl Default for GpuReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "windows")]
impl GpuReader {
    pub fn new() -> Self {
        Self {
            nvml_reader: NvidiaGpuReader::new(),
        }
    }

    pub fn read(&mut self) -> Option<crate::system::models::GpuState> {
        self.nvml_reader.as_ref().and_then(NvidiaGpuReader::read)
    }
}

#[cfg(target_os = "linux")]
mod linux_native {
    use std::fs;
    use std::path::{Path, PathBuf};

    use chrono::Utc;

    use crate::system::gpu::NvidiaGpuReader;
    use crate::system::models::GpuState;

    const DRM_CLASS_DISPLAY_VGA: u64 = 0x030000;
    const DRM_CLASS_DISPLAY_3D: u64 = 0x030200;

    #[derive(Debug)]
    pub struct GpuReader {
        backend: Option<GpuBackend>,
    }

    #[derive(Debug)]
    enum GpuBackend {
        Nvidia(NvidiaGpuReader),
        Sysfs(LinuxSysfsGpuReader),
    }

    #[derive(Debug)]
    struct LinuxSysfsGpuReader {
        gpu_name: String,
        usage_source: Option<UsageSource>,
        memory_source: Option<MemorySource>,
    }

    #[derive(Debug)]
    enum UsageSource {
        BusyPercent(PathBuf),
        IntelFrequency {
            current_path: PathBuf,
            max_path: PathBuf,
        },
    }

    #[derive(Debug)]
    struct MemorySource {
        total_path: PathBuf,
        used_path: PathBuf,
    }

    impl Default for GpuReader {
        fn default() -> Self {
            Self::new()
        }
    }

    impl GpuReader {
        pub fn new() -> Self {
            let backend = NvidiaGpuReader::new()
                .map(GpuBackend::Nvidia)
                .or_else(|| LinuxSysfsGpuReader::new().map(GpuBackend::Sysfs));

            Self { backend }
        }

        pub fn read(&mut self) -> Option<GpuState> {
            self.backend.as_ref().and_then(GpuBackend::read)
        }
    }

    impl GpuBackend {
        fn read(&self) -> Option<GpuState> {
            match self {
                Self::Nvidia(reader) => reader.read(),
                Self::Sysfs(reader) => reader.read(),
            }
        }
    }

    impl LinuxSysfsGpuReader {
        fn new() -> Option<Self> {
            let mut candidates = fs::read_dir("/sys/class/drm")
                .ok()?
                .filter_map(Result::ok)
                .filter_map(|entry| Self::from_card_path(entry.path()))
                .collect::<Vec<_>>();

            candidates.sort_by_key(Self::score);
            candidates.pop()
        }

        fn from_card_path(card_path: PathBuf) -> Option<Self> {
            let card_name = card_path.file_name()?.to_str()?;
            if !card_name.starts_with("card")
                || !card_name
                    .strip_prefix("card")
                    .is_some_and(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
            {
                return None;
            }

            let device_path = card_path.join("device");
            let class_code = read_hex_u64(device_path.join("class"))?;
            if class_code != DRM_CLASS_DISPLAY_VGA && class_code != DRM_CLASS_DISPLAY_3D {
                return None;
            }

            let driver_name = driver_name(&device_path)?;
            let vendor_id = read_hex_u64(device_path.join("vendor"))?;
            let gpu_name = gpu_name(vendor_id, &driver_name);
            let usage_source = usage_source(&card_path, &device_path, &driver_name);
            let memory_source = memory_source(&device_path);

            if usage_source.is_none() && memory_source.is_none() {
                return None;
            }

            Some(Self {
                gpu_name,
                usage_source,
                memory_source,
            })
        }

        fn score(&self) -> (u8, u8, u8) {
            let vendor_score = if self.gpu_name.starts_with("NVIDIA") {
                4
            } else if self.gpu_name.starts_with("AMD") {
                3
            } else if self.gpu_name.starts_with("Intel") {
                2
            } else {
                1
            };

            (
                vendor_score,
                u8::from(self.usage_source.is_some()),
                u8::from(self.memory_source.is_some()),
            )
        }

        fn read(&self) -> Option<GpuState> {
            let gpu_usage = self
                .usage_source
                .as_ref()
                .and_then(UsageSource::read)
                .unwrap_or(0.0);

            let (gpu_memory_total, gpu_memory_used) = self
                .memory_source
                .as_ref()
                .and_then(MemorySource::read)
                .unwrap_or((0, 0));
            let gpu_memory_used = gpu_memory_used.min(gpu_memory_total);

            Some(GpuState {
                timestamp: Utc::now().to_rfc3339(),
                gpu_name: self.gpu_name.clone(),
                gpu_usage,
                gpu_temperature: None,
                gpu_memory_available: gpu_memory_total.saturating_sub(gpu_memory_used),
                gpu_memory_used,
                gpu_memory_total,
                gpu_memory_usage: percent(gpu_memory_used, gpu_memory_total),
            })
        }
    }

    impl UsageSource {
        fn read(&self) -> Option<f32> {
            match self {
                Self::BusyPercent(path) => read_trimmed(path)?.parse::<f32>().ok(),
                Self::IntelFrequency {
                    current_path,
                    max_path,
                } => {
                    let current = read_trimmed(current_path)?.parse::<f32>().ok()?;
                    let max = read_trimmed(max_path)?.parse::<f32>().ok()?;
                    if max <= 0.0 {
                        return Some(0.0);
                    }

                    Some(((current / max) * 100.0).clamp(0.0, 100.0))
                }
            }
        }
    }

    impl MemorySource {
        fn read(&self) -> Option<(u64, u64)> {
            let total = read_trimmed(&self.total_path)?.parse::<u64>().ok()?;
            let used = read_trimmed(&self.used_path)?.parse::<u64>().ok()?;
            Some((total, used))
        }
    }

    fn usage_source(
        card_path: &Path,
        device_path: &Path,
        driver_name: &str,
    ) -> Option<UsageSource> {
        let busy_percent = device_path.join("gpu_busy_percent");
        if busy_percent.exists() {
            return Some(UsageSource::BusyPercent(busy_percent));
        }

        if matches!(driver_name, "i915" | "xe") {
            for current_name in ["gt_act_freq_mhz", "gt_cur_freq_mhz"] {
                let current_path = card_path.join(current_name);
                let max_path = card_path.join("gt_max_freq_mhz");
                if current_path.exists() && max_path.exists() {
                    return Some(UsageSource::IntelFrequency {
                        current_path,
                        max_path,
                    });
                }
            }
        }

        None
    }

    fn memory_source(device_path: &Path) -> Option<MemorySource> {
        let total_path = device_path.join("mem_info_vram_total");
        let used_path = device_path.join("mem_info_vram_used");
        if total_path.exists() && used_path.exists() {
            return Some(MemorySource {
                total_path,
                used_path,
            });
        }

        None
    }

    fn gpu_name(vendor_id: u64, driver_name: &str) -> String {
        let vendor = match vendor_id {
            0x10de => "NVIDIA",
            0x1002 => "AMD",
            0x1022 => "AMD",
            0x8086 => "Intel",
            _ => "Linux",
        };

        format!("{vendor} GPU ({driver_name})")
    }

    fn driver_name(device_path: &Path) -> Option<String> {
        let driver_path = fs::canonicalize(device_path.join("driver")).ok()?;
        let name = driver_path.file_name()?.to_str()?;
        Some(name.to_string())
    }

    fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
        let value = fs::read_to_string(path).ok()?;
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }

    fn read_hex_u64(path: impl AsRef<Path>) -> Option<u64> {
        let value = read_trimmed(path)?;
        u64::from_str_radix(value.trim_start_matches("0x"), 16).ok()
    }

    fn percent(value: u64, total: u64) -> f64 {
        if total == 0 {
            0.0
        } else {
            (value as f64 / total as f64) * 100.0
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux_native::GpuReader;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
#[derive(Debug, Default, Clone)]
pub struct GpuReader;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl GpuReader {
    pub fn new() -> Self {
        Self
    }

    pub fn read(&mut self) -> Option<crate::system::models::GpuState> {
        None
    }
}

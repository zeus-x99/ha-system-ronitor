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
    use std::mem::size_of;
    use std::os::fd::RawFd;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use chrono::Utc;
    use tracing::debug;

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
        I915Pmu(I915PmuUsageReader),
        IntelFrequency {
            current_path: PathBuf,
            max_path: PathBuf,
        },
    }

    #[derive(Debug)]
    struct I915PmuUsageReader {
        counters: Vec<PmuCounter>,
        last_sample: Option<PmuSnapshot>,
    }

    #[derive(Debug)]
    struct PmuCounter {
        name: String,
        fd: RawFd,
    }

    #[derive(Debug)]
    struct PmuSnapshot {
        captured_at: Instant,
        values: Vec<u64>,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct PerfEventAttr {
        type_: u32,
        size: u32,
        config: u64,
        sample_period_or_freq: u64,
        sample_type: u64,
        read_format: u64,
        flags: u64,
        wakeup_events_or_watermark: u32,
        bp_type: u32,
        bp_addr_or_config1: u64,
        bp_len_or_config2: u64,
        branch_sample_type: u64,
        sample_regs_user: u64,
        sample_stack_user: u32,
        clockid: i32,
        sample_regs_intr: u64,
        aux_watermark: u32,
        sample_max_stack: u16,
        reserved_2: u16,
        aux_sample_size: u32,
        reserved_3: u32,
        sig_data: u64,
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
            self.backend.as_mut().and_then(GpuBackend::read)
        }
    }

    impl GpuBackend {
        fn read(&mut self) -> Option<GpuState> {
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

        fn read(&mut self) -> Option<GpuState> {
            let gpu_usage = self
                .usage_source
                .as_mut()
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
        fn read(&mut self) -> Option<f32> {
            match self {
                Self::BusyPercent(path) => read_trimmed(path)?.parse::<f32>().ok(),
                Self::I915Pmu(reader) => reader.read(),
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

    impl I915PmuUsageReader {
        fn new() -> Option<Self> {
            let pmu_root = Path::new("/sys/devices/i915");
            if !pmu_root.exists() {
                return None;
            }

            let perf_type = read_trimmed(pmu_root.join("type"))?.parse::<u32>().ok()?;
            let cpu = parse_first_cpu(&read_trimmed(pmu_root.join("cpumask"))?)?;
            let counters = busy_event_names(pmu_root.join("events"))
                .into_iter()
                .filter_map(|name| {
                    let config = read_event_config(pmu_root, &name)?;
                    open_counter(perf_type, cpu, &name, config)
                        .map_err(|error| {
                            debug!(event = %name, %error, "i915 PMU counter init failed");
                            error
                        })
                        .ok()
                })
                .collect::<Vec<_>>();

            if counters.is_empty() {
                return None;
            }

            Some(Self {
                counters,
                last_sample: None,
            })
        }

        fn read(&mut self) -> Option<f32> {
            let snapshot = self.snapshot()?;
            let previous = self.last_sample.replace(snapshot)?;
            let current = self.last_sample.as_ref()?;
            let elapsed_ns = current
                .captured_at
                .duration_since(previous.captured_at)
                .as_nanos() as f64;
            if elapsed_ns <= 0.0 {
                return Some(0.0);
            }

            let busy_ns = previous
                .values
                .iter()
                .zip(&current.values)
                .map(|(before, after)| after.saturating_sub(*before))
                .sum::<u64>() as f64;

            Some(((busy_ns / elapsed_ns) * 100.0).clamp(0.0, 100.0) as f32)
        }

        fn snapshot(&self) -> Option<PmuSnapshot> {
            let mut values = Vec::with_capacity(self.counters.len());
            for counter in &self.counters {
                let value = read_counter(counter)
                    .map_err(|error| {
                        debug!(event = %counter.name, %error, "i915 PMU read failed");
                        error
                    })
                    .ok()?;
                values.push(value);
            }

            Some(PmuSnapshot {
                captured_at: Instant::now(),
                values,
            })
        }
    }

    impl Drop for I915PmuUsageReader {
        fn drop(&mut self) {
            for counter in &self.counters {
                unsafe {
                    libc::close(counter.fd);
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
            if let Some(reader) = I915PmuUsageReader::new() {
                return Some(UsageSource::I915Pmu(reader));
            }

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

    fn busy_event_names(events_dir: PathBuf) -> Vec<String> {
        let mut names = fs::read_dir(events_dir)
            .ok()
            .into_iter()
            .flat_map(|entries| entries.filter_map(Result::ok))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.ends_with("-busy") && !name.ends_with(".unit"))
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn read_event_config(pmu_root: &Path, name: &str) -> Option<u64> {
        let raw = read_trimmed(pmu_root.join("events").join(name))?;
        let config = raw
            .strip_prefix("config=0x")
            .or_else(|| raw.strip_prefix("config="))?;
        u64::from_str_radix(config.trim_start_matches("0x"), 16).ok()
    }

    fn parse_first_cpu(mask: &str) -> Option<i32> {
        let token = mask.split(',').next()?.split('-').next()?;
        token.parse::<i32>().ok()
    }

    fn open_counter(
        perf_type: u32,
        cpu: i32,
        name: &str,
        config: u64,
    ) -> Result<PmuCounter, std::io::Error> {
        let attr = PerfEventAttr {
            type_: perf_type,
            size: size_of::<PerfEventAttr>() as u32,
            config,
            sample_period_or_freq: 0,
            sample_type: 0,
            read_format: 0,
            flags: 0,
            wakeup_events_or_watermark: 0,
            bp_type: 0,
            bp_addr_or_config1: 0,
            bp_len_or_config2: 0,
            branch_sample_type: 0,
            sample_regs_user: 0,
            sample_stack_user: 0,
            clockid: 0,
            sample_regs_intr: 0,
            aux_watermark: 0,
            sample_max_stack: 0,
            reserved_2: 0,
            aux_sample_size: 0,
            reserved_3: 0,
            sig_data: 0,
        };

        let fd = unsafe {
            libc::syscall(
                libc::SYS_perf_event_open,
                &attr as *const PerfEventAttr,
                -1,
                cpu,
                -1,
                0,
            ) as RawFd
        };

        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }

        Ok(PmuCounter {
            name: name.to_string(),
            fd,
        })
    }

    fn read_counter(counter: &PmuCounter) -> Result<u64, std::io::Error> {
        let mut value = 0_u64;
        let read_size = unsafe {
            libc::read(
                counter.fd,
                &mut value as *mut u64 as *mut libc::c_void,
                size_of::<u64>(),
            )
        };

        if read_size != size_of::<u64>() as isize {
            return Err(std::io::Error::last_os_error());
        }

        Ok(value)
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
        use super::{busy_event_names, parse_first_cpu};
        use std::fs;
        use std::path::PathBuf;
        use std::time::{SystemTime, UNIX_EPOCH};

        #[test]
        fn parses_first_cpu_from_cpumask() {
            assert_eq!(parse_first_cpu("0"), Some(0));
            assert_eq!(parse_first_cpu("2-5"), Some(2));
            assert_eq!(parse_first_cpu("4,8-11"), Some(4));
        }

        #[test]
        fn discovers_only_busy_events() {
            let mut dir = std::env::temp_dir();
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            dir.push(format!("ha-system-ronitor-busy-events-{unique}"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("rcs0-busy"), "config=0x0\n").unwrap();
            fs::write(dir.join("vcs0-busy.unit"), "ns\n").unwrap();
            fs::write(dir.join("rc6-residency"), "config=0x1\n").unwrap();
            fs::write(dir.join("interrupts"), "config=0x2\n").unwrap();

            let names = busy_event_names(PathBuf::from(&dir));
            assert_eq!(names, vec!["rcs0-busy".to_string()]);

            fs::remove_dir_all(dir).unwrap();
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

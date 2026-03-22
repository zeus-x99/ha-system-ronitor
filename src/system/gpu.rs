#[cfg(target_os = "windows")]
mod windows_native {
    use chrono::Utc;
    use nvml_wrapper::{Nvml, enum_wrappers::device::TemperatureSensor};
    use tracing::debug;

    use crate::system::models::GpuState;

    #[derive(Debug)]
    pub struct GpuReader {
        nvml_reader: Option<NvidiaGpuReader>,
    }

    #[derive(Debug)]
    struct NvidiaGpuReader {
        nvml: Nvml,
        device_index: u32,
        device_name: String,
    }

    impl Default for GpuReader {
        fn default() -> Self {
            Self::new()
        }
    }

    impl GpuReader {
        pub fn new() -> Self {
            Self {
                nvml_reader: NvidiaGpuReader::new(),
            }
        }

        pub fn read(&mut self) -> Option<GpuState> {
            self.nvml_reader.as_ref().and_then(NvidiaGpuReader::read)
        }
    }

    impl NvidiaGpuReader {
        fn new() -> Option<Self> {
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

        fn read(&self) -> Option<GpuState> {
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

#[cfg(target_os = "windows")]
pub use windows_native::GpuReader;

#[cfg(not(target_os = "windows"))]
#[derive(Debug, Default, Clone)]
pub struct GpuReader;

#[cfg(not(target_os = "windows"))]
impl GpuReader {
    pub fn new() -> Self {
        Self
    }

    pub fn read(&mut self) -> Option<crate::system::models::GpuState> {
        None
    }
}

#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

use sysinfo::{Component, Components};

#[cfg(target_os = "windows")]
use crate::system::pawnio::PawnIoCpuTemperatureReader;

#[cfg(target_os = "windows")]
const PAWNIO_RETRY_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct CpuTemperatureReader {
    #[cfg(target_os = "windows")]
    pawnio_reader: Option<PawnIoCpuTemperatureReader>,
    #[cfg(target_os = "windows")]
    last_pawnio_probe_at: Option<Instant>,
}

impl Default for CpuTemperatureReader {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuTemperatureReader {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "windows")]
            pawnio_reader: PawnIoCpuTemperatureReader::new(),
            #[cfg(target_os = "windows")]
            last_pawnio_probe_at: Some(Instant::now()),
        }
    }

    pub fn read(&mut self, components: &Components) -> Option<f32> {
        self.read_platform_specific_temperature(components)
    }

    #[cfg(not(target_os = "windows"))]
    fn read_platform_specific_temperature(&mut self, components: &Components) -> Option<f32> {
        detect_cpu_package_temp_from_components(components)
    }

    #[cfg(target_os = "windows")]
    fn read_platform_specific_temperature(&mut self, _components: &Components) -> Option<f32> {
        self.ensure_pawnio_reader();
        let mut recreate_reader = false;
        let temperature = if let Some(reader) = self.pawnio_reader.as_mut() {
            let value = reader.read();
            recreate_reader = value.is_none() && reader.should_recreate_after_failure();
            value
        } else {
            None
        };

        if recreate_reader {
            self.pawnio_reader = None;
            self.last_pawnio_probe_at = None;
        }

        if temperature.is_none() {
            self.ensure_pawnio_reader();
        }

        temperature
    }

    #[cfg(target_os = "windows")]
    fn ensure_pawnio_reader(&mut self) {
        if self.pawnio_reader.is_some() {
            return;
        }

        let should_probe = self
            .last_pawnio_probe_at
            .is_none_or(|instant| instant.elapsed() >= PAWNIO_RETRY_INTERVAL);
        if !should_probe {
            return;
        }

        self.last_pawnio_probe_at = Some(Instant::now());
        self.pawnio_reader = PawnIoCpuTemperatureReader::new();
    }
}

pub fn detect_cpu_package_temp_from_components(components: &Components) -> Option<f32> {
    let mut package_matches = Vec::new();
    let mut cpu_matches = Vec::new();

    for component in components.list() {
        let Some(temperature) = component.temperature() else {
            continue;
        };

        push_temperature_match(
            component_search_text(component),
            temperature,
            &mut package_matches,
            &mut cpu_matches,
        );
    }

    average_temperature(&package_matches).or_else(|| average_temperature(&cpu_matches))
}

pub fn detect_gpu_temp_from_components(components: &Components) -> Option<f32> {
    let mut preferred_matches = Vec::new();
    let mut gpu_matches = Vec::new();

    for component in components.list() {
        let Some(temperature) = component.temperature() else {
            continue;
        };

        let label = component_search_text(component);
        if label.is_empty() {
            continue;
        }

        if is_preferred_gpu_temperature_label(&label) {
            preferred_matches.push(temperature);
            continue;
        }

        if is_gpu_temperature_label(&label) {
            gpu_matches.push(temperature);
        }
    }

    average_temperature(&preferred_matches).or_else(|| average_temperature(&gpu_matches))
}

fn push_temperature_match(
    label: String,
    temperature: f32,
    package_matches: &mut Vec<f32>,
    cpu_matches: &mut Vec<f32>,
) {
    if label.is_empty() {
        return;
    }

    if is_package_temperature_label(&label) {
        package_matches.push(temperature);
        return;
    }

    if is_cpu_temperature_label(&label) {
        cpu_matches.push(temperature);
    }
}

fn component_search_text(component: &Component) -> String {
    let mut parts = vec![normalize_label(component.label())];

    if let Some(id) = component.id() {
        let id = normalize_label(id);
        if !id.is_empty() {
            parts.push(id);
        }
    }

    parts.join(" ").trim().to_string()
}

fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_ascii_lowercase()
}

fn is_cpu_temperature_label(label: &str) -> bool {
    [
        "cpu",
        "core",
        "tdie",
        "tctl",
        "k10temp",
        "zenpower",
        "processor",
        "ccd",
        "soc",
        "die",
    ]
    .iter()
    .any(|keyword| label.contains(keyword))
}

fn is_package_temperature_label(label: &str) -> bool {
    if label.contains("ccd") || label.contains("core #") {
        return false;
    }

    label.contains("package")
        || label.contains("tctl")
        || matches!(
            label,
            "cpu" | "tdie" | "core" | "core (tdie)" | "core (tctl/tdie)"
        )
}

fn is_gpu_temperature_label(label: &str) -> bool {
    [
        "gpu",
        "graphics",
        "amdgpu",
        "radeon",
        "nvidia",
        "geforce",
        "rtx",
        "gtx",
        "arc",
        "intel gpu",
        "vga",
        "tg0p",
    ]
    .iter()
    .any(|keyword| label.contains(keyword))
}

fn is_preferred_gpu_temperature_label(label: &str) -> bool {
    [
        "gpu package",
        "graphics package",
        "gpu edge",
        "amdgpu edge",
        "nvidia gpu",
        "geforce gpu",
        "tg0p",
    ]
    .iter()
    .any(|keyword| label.contains(keyword))
}

fn average_temperature(values: &[f32]) -> Option<f32> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().copied().sum::<f32>() / values.len() as f32)
    }
}

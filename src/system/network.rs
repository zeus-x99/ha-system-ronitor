use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use chrono::Utc;
use sysinfo::{NetworkData, Networks};

use crate::shared::util::slugify;
use crate::system::models::{
    NetworkInfoState, NetworkInterfaceInfoPayload, NetworkInterfaceStatePayload, NetworkState,
};

#[derive(Debug)]
pub struct NetworkReader {
    networks: Networks,
    include_interfaces: Vec<String>,
    last_refresh_at: Option<Instant>,
}

impl NetworkReader {
    pub fn new(include_interfaces: &[String]) -> Self {
        Self {
            networks: Networks::new_with_refreshed_list(),
            include_interfaces: sanitize_include_interfaces(include_interfaces),
            last_refresh_at: None,
        }
    }

    pub fn read(&mut self) -> NetworkState {
        self.networks.refresh(true);

        let now = Instant::now();
        let elapsed_secs = self
            .last_refresh_at
            .replace(now)
            .map(|previous| now.duration_since(previous).as_secs_f64());

        let mut network_download_rate = 0.0_f64;
        let mut network_upload_rate = 0.0_f64;

        let interfaces = if self.include_interfaces.is_empty() {
            self.collect_all_interfaces(
                elapsed_secs,
                &mut network_download_rate,
                &mut network_upload_rate,
            )
        } else {
            self.collect_included_interfaces(
                elapsed_secs,
                &mut network_download_rate,
                &mut network_upload_rate,
            )
        };

        NetworkState {
            timestamp: Utc::now().to_rfc3339(),
            network_download_rate,
            network_upload_rate,
            interfaces,
        }
    }

    pub fn info_state(&self) -> NetworkInfoState {
        let interfaces = if self.include_interfaces.is_empty() {
            self.collect_all_interface_info()
        } else {
            self.collect_included_interface_info()
        };

        NetworkInfoState { interfaces }
    }

    fn collect_all_interfaces(
        &self,
        elapsed_secs: Option<f64>,
        total_download_rate: &mut f64,
        total_upload_rate: &mut f64,
    ) -> BTreeMap<String, NetworkInterfaceStatePayload> {
        let mut interfaces = BTreeMap::new();
        let mut used_ids = HashSet::new();

        for (interface_name, network) in self.networks.list() {
            if is_loopback_interface(interface_name) {
                continue;
            }

            let interface_id = unique_interface_id(interface_name, &mut used_ids);
            let payload = build_interface_state_payload(network, elapsed_secs);
            accumulate_totals(&payload, total_download_rate, total_upload_rate);
            interfaces.insert(interface_id, payload);
        }

        interfaces
    }

    fn collect_included_interfaces(
        &self,
        elapsed_secs: Option<f64>,
        total_download_rate: &mut f64,
        total_upload_rate: &mut f64,
    ) -> BTreeMap<String, NetworkInterfaceStatePayload> {
        let actual_by_name = self
            .networks
            .list()
            .iter()
            .filter(|(interface_name, _)| !is_loopback_interface(interface_name))
            .map(|(interface_name, network)| (normalize_interface_name(interface_name), network))
            .collect::<HashMap<_, _>>();

        let mut interfaces = BTreeMap::new();
        let mut used_ids = HashSet::new();

        for interface_name in &self.include_interfaces {
            let interface_id = unique_interface_id(interface_name, &mut used_ids);
            let payload = match actual_by_name.get(&normalize_interface_name(interface_name)) {
                Some(network) => {
                    let payload = build_interface_state_payload(network, elapsed_secs);
                    accumulate_totals(&payload, total_download_rate, total_upload_rate);
                    payload
                }
                None => NetworkInterfaceStatePayload {
                    download_rate: 0.0,
                    upload_rate: 0.0,
                },
            };

            interfaces.insert(interface_id, payload);
        }

        interfaces
    }

    fn collect_all_interface_info(&self) -> BTreeMap<String, NetworkInterfaceInfoPayload> {
        let mut interfaces = BTreeMap::new();
        let mut used_ids = HashSet::new();

        for interface_name in self.networks.list().keys() {
            if is_loopback_interface(interface_name) {
                continue;
            }

            let interface_id = unique_interface_id(interface_name, &mut used_ids);
            interfaces.insert(
                interface_id,
                NetworkInterfaceInfoPayload {
                    name: interface_name.to_string(),
                },
            );
        }

        interfaces
    }

    fn collect_included_interface_info(&self) -> BTreeMap<String, NetworkInterfaceInfoPayload> {
        let mut interfaces = BTreeMap::new();
        let mut used_ids = HashSet::new();

        for interface_name in &self.include_interfaces {
            let interface_id = unique_interface_id(interface_name, &mut used_ids);
            interfaces.insert(
                interface_id,
                NetworkInterfaceInfoPayload {
                    name: interface_name.clone(),
                },
            );
        }

        interfaces
    }
}

fn sanitize_include_interfaces(include_interfaces: &[String]) -> Vec<String> {
    let mut sanitized = Vec::new();
    let mut seen = HashSet::new();

    for interface_name in include_interfaces {
        let trimmed = interface_name.trim();
        if trimmed.is_empty() {
            continue;
        }

        let normalized = normalize_interface_name(trimmed);
        if seen.insert(normalized) {
            sanitized.push(trimmed.to_string());
        }
    }

    sanitized
}

fn build_interface_state_payload(
    network: &NetworkData,
    elapsed_secs: Option<f64>,
) -> NetworkInterfaceStatePayload {
    NetworkInterfaceStatePayload {
        download_rate: bytes_per_second(network.received(), elapsed_secs),
        upload_rate: bytes_per_second(network.transmitted(), elapsed_secs),
    }
}

fn accumulate_totals(
    payload: &NetworkInterfaceStatePayload,
    total_download_rate: &mut f64,
    total_upload_rate: &mut f64,
) {
    *total_download_rate += payload.download_rate;
    *total_upload_rate += payload.upload_rate;
}

fn bytes_per_second(bytes_since_last_refresh: u64, elapsed_secs: Option<f64>) -> f64 {
    match elapsed_secs {
        Some(seconds) if seconds > 0.0 => bytes_since_last_refresh as f64 / seconds,
        _ => 0.0,
    }
}

fn unique_interface_id(interface_name: &str, used_ids: &mut HashSet<String>) -> String {
    let base = match slugify(interface_name) {
        slug if slug.is_empty() => "network".to_string(),
        slug => slug,
    };

    let mut candidate = base.clone();
    let mut index = 2_u32;
    while !used_ids.insert(candidate.clone()) {
        candidate = format!("{base}_{index}");
        index += 1;
    }

    candidate
}

fn normalize_interface_name(interface_name: &str) -> String {
    interface_name.trim().to_ascii_lowercase()
}

fn is_loopback_interface(interface_name: &str) -> bool {
    matches!(
        normalize_interface_name(interface_name).as_str(),
        "lo" | "loopback" | "loopback pseudo-interface 1"
    ) || normalize_interface_name(interface_name).contains("loopback")
}

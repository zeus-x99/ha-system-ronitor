use anyhow::{Context, Result};
use rumqttc::{AsyncClient, LastWill, MqttOptions, Publish, QoS};
use serde::Serialize;
use tracing::info;

use crate::config::Config;
use crate::device::{Identity, Topics};
use crate::integrations::home_assistant::discovery::build_device_discovery_message;
use crate::system::models::{
    CpuInfoState, CpuState, DiskInfoState, DiskState, GpuInfoState, GpuState, HostInfoState,
    LighthouseState, MemoryInfoState, MemoryState, NetworkInfoState, NetworkState, ShutdownState,
    UptimeState,
};

const MQTT_MAX_PACKET_SIZE: usize = 64 * 1024;

pub struct DiscoveryPublishArgs<'a> {
    pub config: &'a Config,
    pub identity: &'a Identity,
    pub topics: &'a Topics,
    pub gpu_info: Option<&'a GpuInfoState>,
    pub disk_info: Option<&'a DiskInfoState>,
    pub network_info: Option<&'a NetworkInfoState>,
}

pub fn build_mqtt_options(config: &Config, identity: &Identity, topics: &Topics) -> MqttOptions {
    let client_id = format!("ha-system-ronitor-{}", identity.node_id);
    build_mqtt_options_with_client_id(
        config,
        client_id,
        Some(LastWill::new(
            topics.availability.clone(),
            "offline",
            QoS::AtLeastOnce,
            true,
        )),
    )
}

pub fn build_lock_mqtt_options(
    config: &Config,
    client_id: String,
    lock_will_payload: Vec<u8>,
    topics: &Topics,
) -> MqttOptions {
    build_mqtt_options_with_client_id(
        config,
        client_id,
        Some(LastWill::new(
            topics.node_lock.clone(),
            lock_will_payload,
            QoS::AtLeastOnce,
            true,
        )),
    )
}

fn build_mqtt_options_with_client_id(
    config: &Config,
    client_id: String,
    last_will: Option<LastWill>,
) -> MqttOptions {
    let mut mqtt_options = MqttOptions::new(client_id, &config.mqtt_host, config.mqtt_port);
    mqtt_options.set_max_packet_size(MQTT_MAX_PACKET_SIZE, MQTT_MAX_PACKET_SIZE);

    if let Some(username) = &config.mqtt_username {
        mqtt_options.set_credentials(
            username.clone(),
            config.mqtt_password.clone().unwrap_or_default(),
        );
    }

    if let Some(last_will) = last_will {
        mqtt_options.set_last_will(last_will);
    }

    mqtt_options
}

pub fn is_home_assistant_birth_message(topics: &Topics, publish: &Publish) -> bool {
    publish.topic == topics.ha_status && publish.payload.as_ref() == b"online"
}

pub async fn publish_discovery_if_needed(
    client: &AsyncClient,
    args: DiscoveryPublishArgs<'_>,
    last_payload: &mut Option<Vec<u8>>,
    force: bool,
) -> Result<()> {
    let message = build_device_discovery_message(
        args.config,
        args.identity,
        args.topics,
        args.gpu_info,
        args.disk_info,
        args.network_info,
    );
    let device_payload = serde_json::to_vec(&message.payload)
        .context("failed to serialize device discovery payload")?;

    if !force
        && last_payload
            .as_deref()
            .is_some_and(|previous| previous == device_payload.as_slice())
    {
        return Ok(());
    }

    client
        .publish(
            message.topic.clone(),
            QoS::AtLeastOnce,
            true,
            device_payload.clone(),
        )
        .await
        .context("failed to publish device discovery payload")?;

    *last_payload = Some(device_payload);
    info!(
        topic = %message.topic,
        component_count = message.component_count(),
        "published Home Assistant device discovery payload"
    );

    Ok(())
}

pub async fn publish_cpu_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &CpuState,
) -> Result<()> {
    publish_json(client, &topics.cpu_state, state, false, "CPU state payload").await
}

pub async fn publish_host_info_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &HostInfoState,
) -> Result<()> {
    publish_json(
        client,
        &topics.host_info_state,
        state,
        true,
        "host info payload",
    )
    .await
}

pub async fn publish_cpu_info_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &CpuInfoState,
) -> Result<()> {
    publish_json(
        client,
        &topics.cpu_info_state,
        state,
        true,
        "CPU info payload",
    )
    .await
}

pub async fn publish_uptime_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &UptimeState,
) -> Result<()> {
    publish_json(
        client,
        &topics.uptime_state,
        state,
        false,
        "uptime state payload",
    )
    .await
}

pub async fn publish_shutdown_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &ShutdownState,
) -> Result<()> {
    publish_json(
        client,
        &topics.shutdown_state,
        state,
        true,
        "shutdown state payload",
    )
    .await
}

pub async fn publish_gpu_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &GpuState,
) -> Result<()> {
    publish_json(client, &topics.gpu_state, state, false, "GPU state payload").await
}

pub async fn publish_gpu_info_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &GpuInfoState,
) -> Result<()> {
    publish_json(
        client,
        &topics.gpu_info_state,
        state,
        true,
        "GPU info payload",
    )
    .await
}

pub async fn publish_memory_info_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &MemoryInfoState,
) -> Result<()> {
    publish_json(
        client,
        &topics.memory_info_state,
        state,
        true,
        "memory info payload",
    )
    .await
}

pub async fn publish_lighthouse_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &LighthouseState,
) -> Result<()> {
    publish_json(
        client,
        &topics.lighthouse_state,
        state,
        true,
        "lighthouse state payload",
    )
    .await
}

pub async fn publish_memory_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &MemoryState,
) -> Result<()> {
    publish_json(
        client,
        &topics.memory_state,
        state,
        false,
        "memory state payload",
    )
    .await
}

pub async fn publish_disk_info_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &DiskInfoState,
) -> Result<()> {
    publish_json(
        client,
        &topics.disk_info_state,
        state,
        true,
        "disk info payload",
    )
    .await
}

pub async fn publish_disk_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &DiskState,
) -> Result<()> {
    publish_json(
        client,
        &topics.disk_state,
        state,
        false,
        "disk state payload",
    )
    .await
}

pub async fn publish_network_info_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &NetworkInfoState,
) -> Result<()> {
    publish_json(
        client,
        &topics.network_info_state,
        state,
        true,
        "network info payload",
    )
    .await
}

pub async fn publish_network_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &NetworkState,
) -> Result<()> {
    publish_json(
        client,
        &topics.network_state,
        state,
        false,
        "network state payload",
    )
    .await
}

pub async fn publish_availability(
    client: &AsyncClient,
    topics: &Topics,
    online: bool,
) -> Result<()> {
    let payload = if online { "online" } else { "offline" };
    client
        .publish(
            topics.availability.clone(),
            QoS::AtLeastOnce,
            true,
            payload.as_bytes(),
        )
        .await
        .context("failed to publish availability payload")?;

    Ok(())
}

async fn publish_json<T: Serialize>(
    client: &AsyncClient,
    topic: &str,
    state: &T,
    retain: bool,
    payload_name: &str,
) -> Result<()> {
    let payload =
        serde_json::to_vec(state).with_context(|| format!("failed to serialize {payload_name}"))?;
    client
        .publish(topic.to_string(), QoS::AtLeastOnce, retain, payload)
        .await
        .with_context(|| format!("failed to publish {payload_name}"))?;

    Ok(())
}

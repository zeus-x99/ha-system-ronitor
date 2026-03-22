use anyhow::{Context, Result};
use rumqttc::{AsyncClient, LastWill, MqttOptions, Publish, QoS};
use tracing::info;

use crate::config::Config;
use crate::device::{Identity, Topics};
use crate::integrations::home_assistant::discovery::{
    build_device_discovery_message, build_removed_device_components_cleanup_payload,
};
use crate::system::models::{CpuState, DiskState, GpuState, MemoryState};

pub struct DiscoveryPublishArgs<'a> {
    pub config: &'a Config,
    pub identity: &'a Identity,
    pub topics: &'a Topics,
    pub gpu_state: Option<&'a GpuState>,
    pub disk_state: &'a DiskState,
}

pub fn build_mqtt_options(config: &Config, identity: &Identity, topics: &Topics) -> MqttOptions {
    let client_id = format!("ha-system-monitor-{}", identity.node_id);
    let mut mqtt_options = MqttOptions::new(client_id, &config.mqtt_host, config.mqtt_port);

    if let Some(username) = &config.mqtt_username {
        mqtt_options.set_credentials(
            username.clone(),
            config.mqtt_password.clone().unwrap_or_default(),
        );
    }

    mqtt_options.set_last_will(LastWill::new(
        topics.availability.clone(),
        "offline",
        QoS::AtLeastOnce,
        true,
    ));

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
        args.gpu_state,
        args.disk_state,
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

    if let Some(cleanup_payload) = build_removed_device_components_cleanup_payload(
        args.config,
        args.identity,
        args.topics,
        args.gpu_state,
        args.disk_state,
    ) {
        client
            .publish(
                message.topic.clone(),
                QoS::AtLeastOnce,
                true,
                cleanup_payload,
            )
            .await
            .context("failed to publish device discovery cleanup payload")?;
    }

    for (component_id, topic) in &message.legacy_topics {
        client
            .publish(topic.clone(), QoS::AtLeastOnce, true, Vec::<u8>::new())
            .await
            .with_context(|| format!("failed to clear legacy discovery topic: {component_id}"))?;
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
        component_count = message.component_count(),
        legacy_topic_count = message.legacy_topics.len(),
        "published Home Assistant device discovery payload"
    );

    Ok(())
}

pub async fn publish_cpu_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &CpuState,
) -> Result<()> {
    let payload = serde_json::to_vec(state).context("failed to serialize CPU state payload")?;
    client
        .publish(topics.cpu_state.clone(), QoS::AtLeastOnce, false, payload)
        .await
        .context("failed to publish CPU state payload")?;

    Ok(())
}

pub async fn publish_gpu_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &GpuState,
) -> Result<()> {
    let payload = serde_json::to_vec(state).context("failed to serialize GPU state payload")?;
    client
        .publish(topics.gpu_state.clone(), QoS::AtLeastOnce, false, payload)
        .await
        .context("failed to publish GPU state payload")?;

    Ok(())
}

pub async fn publish_memory_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &MemoryState,
) -> Result<()> {
    let payload = serde_json::to_vec(state).context("failed to serialize memory state payload")?;
    client
        .publish(
            topics.memory_state.clone(),
            QoS::AtLeastOnce,
            false,
            payload,
        )
        .await
        .context("failed to publish memory state payload")?;

    Ok(())
}

pub async fn publish_disk_state(
    client: &AsyncClient,
    topics: &Topics,
    state: &DiskState,
) -> Result<()> {
    let payload = serde_json::to_vec(state).context("failed to serialize disk state payload")?;
    client
        .publish(topics.disk_state.clone(), QoS::AtLeastOnce, false, payload)
        .await
        .context("failed to publish disk state payload")?;

    Ok(())
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

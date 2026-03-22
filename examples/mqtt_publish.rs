use std::time::Duration;

use anyhow::{Context, Result};
use ha_system_ronitor::config::{candidate_config_directories, load_config_file_from};
use rumqttc::{AsyncClient, MqttOptions, QoS};

fn env_required(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("missing environment variable {name}"))
}

fn load_runtime_defaults() -> Result<()> {
    load_config_file_from(&candidate_config_directories())?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    load_runtime_defaults()?;

    let host = env_required("HA_MONITOR_MQTT_HOST")?;
    let port = std::env::var("HA_MONITOR_MQTT_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(1883);
    let username = env_required("HA_MONITOR_MQTT_USERNAME")?;
    let password = env_required("HA_MONITOR_MQTT_PASSWORD")?;

    let mut args = std::env::args().skip(1);
    let topic = args
        .next()
        .context("expected topic as the first argument")?;
    let payload = match args
        .next()
        .context("expected payload as the second argument")?
        .as_str()
    {
        "--empty-payload" => String::new(),
        value => value.to_string(),
    };
    let retain = args.any(|arg| arg == "--retain");

    let mut options = MqttOptions::new("ha-system-ronitor-publish", host, port);
    options.set_credentials(username, password);
    options.set_keep_alive(Duration::from_secs(10));

    let (client, mut eventloop) = AsyncClient::new(options, 10);
    let _eventloop = tokio::spawn(async move {
        loop {
            let _ = eventloop.poll().await;
        }
    });

    client
        .publish(topic, QoS::AtLeastOnce, retain, payload)
        .await
        .context("failed to publish MQTT message")?;

    tokio::time::sleep(Duration::from_millis(500)).await;
    Ok(())
}

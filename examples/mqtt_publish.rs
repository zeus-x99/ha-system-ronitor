use std::time::Duration;

use anyhow::{Context, Result};
use ha_system_ronitor::config::{BootstrapOptions, load_config};
use rumqttc::{AsyncClient, MqttOptions, QoS};

#[tokio::main]
async fn main() -> Result<()> {
    let config = load_config(&BootstrapOptions::from_current_process())?;

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

    let mut options = MqttOptions::new(
        "ha-system-ronitor-publish",
        config.mqtt_host.clone(),
        config.mqtt_port,
    );
    if let Some(username) = config.mqtt_username {
        options.set_credentials(username, config.mqtt_password.unwrap_or_default());
    }
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

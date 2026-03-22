use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ha_system_ronitor::config::{candidate_config_directories, load_config_file_from};
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, Packet, QoS};

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

    let mut options = MqttOptions::new("ha-system-ronitor-peek", host, port);
    options.set_credentials(username, password);
    options.set_keep_alive(Duration::from_secs(10));

    let (client, mut eventloop) = AsyncClient::new(options, 10);
    client
        .subscribe("homeassistant/#", QoS::AtLeastOnce)
        .await?;
    client
        .subscribe("monitor/system/#", QoS::AtLeastOnce)
        .await?;

    let deadline = Instant::now() + Duration::from_secs(5);

    while Instant::now() < deadline {
        match eventloop.poll().await? {
            Event::Incoming(Packet::Publish(message)) => {
                println!("TOPIC={}", message.topic);
                println!("RETAIN={}", message.retain);
                println!("QOS={:?}", message.qos);
                println!(
                    "PAYLOAD={}",
                    String::from_utf8_lossy(message.payload.as_ref())
                );
                println!("---");
            }
            Event::Incoming(Incoming::ConnAck(_)) => {}
            Event::Outgoing(_) | Event::Incoming(_) => {}
        }
    }

    Ok(())
}

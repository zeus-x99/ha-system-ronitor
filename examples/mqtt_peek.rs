use std::time::{Duration, Instant};

use anyhow::Result;
use ha_system_ronitor::config::{BootstrapOptions, load_config};
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, Packet, QoS};

#[tokio::main]
async fn main() -> Result<()> {
    let config = load_config(&BootstrapOptions::from_current_process())?;

    let mut options = MqttOptions::new(
        "ha-system-ronitor-peek",
        config.mqtt_host.clone(),
        config.mqtt_port,
    );
    if let Some(username) = config.mqtt_username {
        options.set_credentials(username, config.mqtt_password.unwrap_or_default());
    }
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

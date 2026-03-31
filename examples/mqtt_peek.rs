use std::time::{Duration, Instant};

use anyhow::Result;
use ha_system_ronitor::config::{BootstrapOptions, load_config};
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, Packet, QoS};

#[tokio::main]
async fn main() -> Result<()> {
    let config = load_config(&BootstrapOptions::from_current_process())?;
    let topics = user_args();

    let mut options = MqttOptions::new(
        "ha-system-ronitor-peek",
        config.mqtt_host.clone(),
        config.mqtt_port,
    );
    if let Some(username) = config.mqtt_username {
        options.set_credentials(username, config.mqtt_password.unwrap_or_default());
    }
    options.set_keep_alive(Duration::from_secs(10));
    options.set_max_packet_size(1024 * 1024, 1024 * 1024);

    let (client, mut eventloop) = AsyncClient::new(options, 10);
    if topics.is_empty() {
        client
            .subscribe("homeassistant/#", QoS::AtLeastOnce)
            .await?;
        client
            .subscribe("monitor/system/#", QoS::AtLeastOnce)
            .await?;
    } else {
        for topic in topics {
            client.subscribe(topic, QoS::AtLeastOnce).await?;
        }
    }

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

fn user_args() -> Vec<String> {
    let mut args = Vec::new();
    let mut skip_next = false;
    let mut passthrough = false;

    for arg in std::env::args().skip(1) {
        if passthrough {
            args.push(arg);
            continue;
        }

        if skip_next {
            skip_next = false;
            continue;
        }

        match arg.as_str() {
            "--" => {
                passthrough = true;
            }
            "--config-dir" | "--log-dir" => {
                skip_next = true;
            }
            _ => args.push(arg),
        }
    }

    args
}

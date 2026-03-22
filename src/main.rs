use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    ha_system_monitor::app::run().await
}

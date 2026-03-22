use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    ha_system_ronitor::app::run().await
}

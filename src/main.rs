use anyhow::Result;

fn main() -> Result<()> {
    #[cfg(windows)]
    {
        ha_system_ronitor::windows_service_host::run()
    }

    #[cfg(not(windows))]
    {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        runtime.block_on(ha_system_ronitor::app::run())
    }
}

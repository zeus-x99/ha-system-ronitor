use std::process::Command;

#[cfg(any(target_os = "linux", target_os = "macos"))]
use anyhow::anyhow;
use anyhow::{Context, Result, bail};

pub fn shutdown_host(dry_run: bool) -> Result<()> {
    if dry_run {
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        run_command("shutdown", &["/s", "/t", "0"])
    }

    #[cfg(target_os = "linux")]
    {
        run_first_available(&[
            ("systemctl", &["poweroff"][..]),
            ("shutdown", &["-h", "now"][..]),
        ])
    }

    #[cfg(target_os = "macos")]
    {
        run_first_available(&[
            (
                "osascript",
                &["-e", "tell app \"System Events\" to shut down"][..],
            ),
            ("shutdown", &["-h", "now"][..]),
        ])
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        bail!("shutdown is not implemented for this operating system");
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_first_available(candidates: &[(&str, &[&str])]) -> Result<()> {
    let mut last_error = None;

    for (program, args) in candidates {
        match run_command(program, args) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("no shutdown command candidates available")))
}

fn run_command(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to start shutdown command `{program}`"))?;

    if status.success() {
        Ok(())
    } else {
        bail!("shutdown command `{program}` exited with status {status}")
    }
}

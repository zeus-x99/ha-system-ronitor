#[cfg(target_os = "windows")]
mod app {
    use std::env;
    use std::path::{Path, PathBuf};
    use std::thread;
    use std::time::Duration;

    use anyhow::{Result, bail};
    use clap::Parser;
    use ha_system_monitor::system::pawnio::PawnIoCpuTemperatureReader;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    const AMDFAMILY17_MODULE_NAME: &str = "AMDFamily17.bin";
    const PAWNIO_DLL_NAME: &str = "PawnIOLib.dll";

    #[derive(Debug, Parser)]
    #[command(about = "Probe CPU package temperature through PawnIO")]
    struct Args {
        #[arg(long, default_value_t = 1)]
        count: u64,

        #[arg(
            long,
            default_value_t = 1000,
            value_parser = clap::value_parser!(u64).range(1..)
        )]
        interval_ms: u64,
    }

    pub fn run() -> Result<()> {
        let args = Args::parse();

        println!("windows_pawnio_temp_probe");
        println!("module_path: {}", display_path(resolve_module_path()));
        println!("dll_path: {}", display_path(resolve_dll_path()));
        println!("is_elevated: {}", is_running_as_administrator()?);
        println!(
            "count={} interval_ms={} (set --count 0 for continuous watch)",
            args.count, args.interval_ms
        );
        println!(
            "note: PawnIO CPU temperature reads on Windows usually require an elevated administrator terminal"
        );

        if !is_running_as_administrator()? {
            bail!(
                "this terminal is not elevated; please reopen PowerShell or cmd as Administrator and run the example again"
            );
        }

        let Some(mut reader) = PawnIoCpuTemperatureReader::new() else {
            bail!(
                "failed to initialize PawnIO temperature reader; check that PawnIO is installed, the process is elevated, and the CPU/module path is supported"
            );
        };

        let interval = Duration::from_millis(args.interval_ms);
        let mut sample_index = 0u64;

        loop {
            sample_index += 1;

            let Some(temperature_celsius) = reader.read() else {
                bail!(
                    "PawnIO initialized, but no cpu_package_temp value was returned; try running this example as Administrator"
                );
            };

            println!(
                "sample={:03} cpu_package_temp={:.1} C",
                sample_index, temperature_celsius
            );

            if args.count != 0 && sample_index >= args.count {
                break;
            }

            thread::sleep(interval);
        }

        Ok(())
    }

    fn display_path(path: Option<PathBuf>) -> String {
        path.map(|value| value.display().to_string())
            .unwrap_or_else(|| "<not found>".to_string())
    }

    fn is_running_as_administrator() -> Result<bool> {
        let mut token = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }?;

        let result = (|| {
            let mut elevation = TOKEN_ELEVATION::default();
            let mut returned_length = 0u32;
            unsafe {
                GetTokenInformation(
                    token,
                    TokenElevation,
                    Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
                    std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                    &mut returned_length,
                )
            }?;

            Ok(elevation.TokenIsElevated != 0)
        })();

        unsafe {
            let _ = CloseHandle(token);
        }

        result
    }

    fn resolve_module_path() -> Option<PathBuf> {
        let bundled_path =
            bundled_runtime_path(["pawnio", "windows", "modules", AMDFAMILY17_MODULE_NAME]);
        if bundled_path.is_some() {
            return bundled_path;
        }

        let source_tree_path =
            source_tree_vendor_path(["pawnio", "windows", "modules", AMDFAMILY17_MODULE_NAME]);
        source_tree_path.is_file().then_some(source_tree_path)
    }

    fn resolve_dll_path() -> Option<PathBuf> {
        let bundled_path = bundled_runtime_path(["pawnio", "windows", PAWNIO_DLL_NAME]);
        if bundled_path.is_some() {
            return bundled_path;
        }

        let source_tree_path = source_tree_vendor_path(["pawnio", "windows", PAWNIO_DLL_NAME]);
        if source_tree_path.is_file() {
            return Some(source_tree_path);
        }

        env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .map(|base| base.join("PawnIO").join(PAWNIO_DLL_NAME))
            .filter(|path| path.is_file())
    }

    fn bundled_runtime_path<const N: usize>(segments: [&str; N]) -> Option<PathBuf> {
        let current_exe = env::current_exe().ok()?;
        let current_dir = current_exe.parent()?;

        for ancestor in current_dir.ancestors().take(4) {
            let candidate = join_segments(ancestor, segments);
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        None
    }

    fn source_tree_vendor_path<const N: usize>(segments: [&str; N]) -> PathBuf {
        join_segments(Path::new(env!("CARGO_MANIFEST_DIR")), ["vendor"])
            .join(join_segments(Path::new(""), segments))
    }

    fn join_segments<const N: usize>(base: &Path, segments: [&str; N]) -> PathBuf {
        let mut path = PathBuf::from(base);
        for segment in segments {
            if !segment.is_empty() {
                path.push(segment);
            }
        }
        path
    }
}

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    app::run()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("this example only runs on Windows");
}

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use tokio::runtime::{Builder, Runtime};
use tracing::error;
use windows_service::Error as WindowsServiceError;
use windows_service::define_windows_service;
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{
    self, ServiceControlHandlerResult, ServiceStatusHandle,
};
use windows_service::service_dispatcher;
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

use crate::app::initialize_runtime_with;
use crate::config::{
    BootstrapOptions, CONFIG_EXAMPLE_FILE_NAME, CONFIG_FILE_NAME,
    candidate_config_directories_with, seed_config_toml,
};

const SERVICE_NAME: &str = "ha-system-ronitor";
const SERVICE_DISPLAY_NAME: &str = "HA System Ronitor";
const SERVICE_DESCRIPTION: &str = "Publishes system metrics to Home Assistant over MQTT.";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const SERVICE_FAILURE_EXIT_CODE: u32 = 1;
const SERVICE_STATE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_SERVICE_RECOVERY_DELAY: Duration = Duration::from_secs(5);
const PROGRAM_FILES_FALLBACK: &str = r"C:\Program Files";
const PROGRAM_DATA_FALLBACK: &str = r"C:\ProgramData";
const ERROR_FAILED_SERVICE_CONTROLLER_CONNECT: i32 = 1063;
const ERROR_SERVICE_DOES_NOT_EXIST: i32 = 1060;
const ERROR_SERVICE_EXISTS: i32 = 1073;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServiceStartMode {
    DelayedAuto,
    Auto,
    Manual,
}

#[derive(Debug)]
struct InstallOptions {
    binary_dir: PathBuf,
    config_dir: PathBuf,
    log_dir: PathBuf,
    seed_config: bool,
    start_mode: ServiceStartMode,
    failure_restart_delay: Duration,
    use_current_executable: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            binary_dir: default_binary_dir(),
            config_dir: default_config_dir(),
            log_dir: default_log_dir(),
            seed_config: true,
            start_mode: ServiceStartMode::DelayedAuto,
            failure_restart_delay: DEFAULT_SERVICE_RECOVERY_DELAY,
            use_current_executable: false,
        }
    }
}

#[derive(Debug)]
struct ServicePaths {
    executable_path: PathBuf,
    config_dir: PathBuf,
    log_dir: PathBuf,
}

enum ServiceCommand {
    Install(InstallOptions),
    Start,
    Stop,
    Restart,
    Status,
    Uninstall,
    Help,
}

pub fn run() -> Result<()> {
    let args: Vec<OsString> = env::args_os().collect();

    if handle_service_cli(&args)? {
        return Ok(());
    }

    match service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
        Ok(()) => Ok(()),
        Err(error) if is_service_dispatcher_connect_error(&error) => run_console(),
        Err(error) => Err(anyhow!(error).context("failed to start Windows service dispatcher")),
    }
}

define_windows_service!(ffi_service_main, service_main);

pub fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = run_service() {
        error!(%error, "Windows service terminated with an error");
    }
}

fn run_console() -> Result<()> {
    build_runtime()?.block_on(crate::app::run())
}

fn run_service() -> Result<()> {
    let bootstrap = BootstrapOptions::from_current_process();
    initialize_runtime_with(&bootstrap)?;

    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();
    let event_handler = move |control| match control {
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        ServiceControl::Stop | ServiceControl::Shutdown => {
            let _ = shutdown_tx.send(());
            ServiceControlHandlerResult::NoError
        }
        _ => ServiceControlHandlerResult::NotImplemented,
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)
        .context("registering Windows service control handler")?;

    set_service_state(
        &status_handle,
        ServiceState::StartPending,
        ServiceControlAccept::empty(),
        ServiceExitCode::NO_ERROR,
    )?;

    let service_result = (|| -> Result<()> {
        let config = crate::app::parse_config_from(env::args_os())
            .context("parsing configuration for Windows service")?;
        let runtime = build_runtime()?;

        set_service_state(
            &status_handle,
            ServiceState::Running,
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            ServiceExitCode::NO_ERROR,
        )?;

        runtime.block_on(crate::app::run_with_config(config, async move {
            let _ = tokio::task::spawn_blocking(move || {
                let _ = shutdown_rx.recv();
            })
            .await;
        }))
    })();

    let _ = set_service_state(
        &status_handle,
        ServiceState::StopPending,
        ServiceControlAccept::empty(),
        ServiceExitCode::NO_ERROR,
    );

    match service_result {
        Ok(()) => {
            set_service_state(
                &status_handle,
                ServiceState::Stopped,
                ServiceControlAccept::empty(),
                ServiceExitCode::NO_ERROR,
            )?;
            Ok(())
        }
        Err(error) => {
            let _ = set_service_state(
                &status_handle,
                ServiceState::Stopped,
                ServiceControlAccept::empty(),
                ServiceExitCode::ServiceSpecific(SERVICE_FAILURE_EXIT_CODE),
            );
            Err(error)
        }
    }
}

fn handle_service_cli(args: &[OsString]) -> Result<bool> {
    let Some(command) = parse_service_command(args)? else {
        return Ok(false);
    };

    match command {
        ServiceCommand::Install(options) => install_service(options)?,
        ServiceCommand::Start => start_service()?,
        ServiceCommand::Stop => stop_service()?,
        ServiceCommand::Restart => {
            stop_service()?;
            start_service()?;
        }
        ServiceCommand::Status => print_service_status()?,
        ServiceCommand::Uninstall => uninstall_service()?,
        ServiceCommand::Help => print_service_usage(),
    }

    Ok(true)
}

fn parse_service_command(args: &[OsString]) -> Result<Option<ServiceCommand>> {
    if !matches!(args.get(1).and_then(os_string_to_str), Some("service")) {
        return Ok(None);
    }

    let command = match args.get(2).and_then(os_string_to_str) {
        Some("install")
            if args[3..]
                .iter()
                .any(|arg| matches!(os_string_to_str(arg), Some("--help") | Some("-h"))) =>
        {
            ServiceCommand::Help
        }
        Some("install") => ServiceCommand::Install(parse_install_options(&args[3..])?),
        Some("start") => ServiceCommand::Start,
        Some("stop") => ServiceCommand::Stop,
        Some("restart") => ServiceCommand::Restart,
        Some("status") => ServiceCommand::Status,
        Some("uninstall") => ServiceCommand::Uninstall,
        Some("help") | Some("--help") | Some("-h") | None => ServiceCommand::Help,
        Some(other) => bail!(
            "unknown service command `{other}`; expected: install, start, stop, restart, status, uninstall"
        ),
    };

    Ok(Some(command))
}

fn parse_install_options(args: &[OsString]) -> Result<InstallOptions> {
    let mut options = InstallOptions::default();
    let mut index = 0;

    while index < args.len() {
        let Some(arg) = os_string_to_str(&args[index]) else {
            bail!("service install received a non-utf8 argument");
        };

        match arg {
            "--install-dir" | "--binary-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("missing value for `{arg}`"))?;
                options.binary_dir = PathBuf::from(value);
                options.use_current_executable = false;
            }
            "--config-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("missing value for `--config-dir`"))?;
                options.config_dir = PathBuf::from(value);
            }
            "--log-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("missing value for `--log-dir`"))?;
                options.log_dir = PathBuf::from(value);
            }
            "--seed-config" => {
                options.seed_config = true;
            }
            "--no-seed-config" => {
                options.seed_config = false;
            }
            "--in-place" => {
                options.use_current_executable = true;
            }
            "--start-mode" => {
                index += 1;
                let value = args
                    .get(index)
                    .and_then(os_string_to_str)
                    .ok_or_else(|| anyhow!("missing value for `--start-mode`"))?;
                options.start_mode = match value {
                    "delayed-auto" => ServiceStartMode::DelayedAuto,
                    "auto" => ServiceStartMode::Auto,
                    "manual" => ServiceStartMode::Manual,
                    _ => bail!(
                        "invalid `--start-mode` value `{value}`; expected delayed-auto, auto, or manual"
                    ),
                };
            }
            "--failure-restart-secs" => {
                index += 1;
                let value = args
                    .get(index)
                    .and_then(os_string_to_str)
                    .ok_or_else(|| anyhow!("missing value for `--failure-restart-secs`"))?;
                let seconds = value
                    .parse::<u64>()
                    .with_context(|| format!("invalid `--failure-restart-secs` value `{value}`"))?;
                options.failure_restart_delay = Duration::from_secs(seconds);
            }
            "--defaults" => {
                options = InstallOptions::default();
            }
            other => bail!(
                "unknown `service install` option `{other}`; supported: --binary-dir, --install-dir, --config-dir, --log-dir, --seed-config, --no-seed-config, --in-place, --start-mode, --failure-restart-secs, --defaults"
            ),
        }

        index += 1;
    }

    Ok(options)
}

fn install_service(options: InstallOptions) -> Result<()> {
    let service_paths = prepare_service_layout(&options)?;
    let service_manager =
        open_service_manager(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
    let service_info = desired_service_info(&service_paths, options.start_mode);
    let service_access = ServiceAccess::QUERY_STATUS
        | ServiceAccess::QUERY_CONFIG
        | ServiceAccess::CHANGE_CONFIG
        | ServiceAccess::START
        | ServiceAccess::STOP
        | ServiceAccess::DELETE;

    let service = match service_manager.create_service(&service_info, service_access) {
        Ok(service) => {
            println!(
                "Installed Windows service `{}` at `{}`.",
                SERVICE_NAME,
                service_paths.executable_path.display()
            );
            service
        }
        Err(error) if raw_os_error(&error) == Some(ERROR_SERVICE_EXISTS) => {
            let service = service_manager
                .open_service(SERVICE_NAME, service_access)
                .context("opening existing Windows service")?;
            service
                .change_config(&service_info)
                .context("updating existing Windows service configuration")?;
            println!(
                "Updated Windows service `{}` to `{}`.",
                SERVICE_NAME,
                service_paths.executable_path.display()
            );
            service
        }
        Err(error) => return Err(anyhow!(error).context("creating Windows service")),
    };

    configure_service_defaults(
        &service,
        options.start_mode == ServiceStartMode::DelayedAuto,
        options.failure_restart_delay,
    )?;

    println!("Startup type: {}.", format_start_mode(options.start_mode));
    println!(
        "Failure action: restart after {} seconds.",
        options.failure_restart_delay.as_secs()
    );
    println!(
        "Binary directory: {}.",
        service_paths
            .executable_path
            .parent()
            .map_or_else(|| "<unknown>".into(), |path| path.display().to_string())
    );
    println!("Config directory: {}.", service_paths.config_dir.display());
    println!("Log directory: {}.", service_paths.log_dir.display());
    println!(
        "Run `{} service start` to launch it now.",
        current_exe_display()?
    );

    Ok(())
}

fn start_service() -> Result<()> {
    let service = open_installed_service(ServiceAccess::QUERY_STATUS | ServiceAccess::START)?;
    let status = service.query_status().context("querying service status")?;

    match status.current_state {
        ServiceState::Running => {
            println!("Windows service `{SERVICE_NAME}` is already running.");
            return Ok(());
        }
        ServiceState::StartPending => {
            wait_for_state(&service, ServiceState::Running, SERVICE_STATE_TIMEOUT)?;
            println!("Windows service `{SERVICE_NAME}` is running.");
            return Ok(());
        }
        ServiceState::StopPending => {
            wait_for_state(&service, ServiceState::Stopped, SERVICE_STATE_TIMEOUT)?;
        }
        _ => {}
    }

    service
        .start::<OsString>(&[])
        .context("starting Windows service")?;
    wait_for_state(&service, ServiceState::Running, SERVICE_STATE_TIMEOUT)?;
    println!("Windows service `{SERVICE_NAME}` started.");

    Ok(())
}

fn stop_service() -> Result<()> {
    let service = open_installed_service(ServiceAccess::QUERY_STATUS | ServiceAccess::STOP)?;
    let status = service.query_status().context("querying service status")?;

    match status.current_state {
        ServiceState::Stopped => {
            println!("Windows service `{SERVICE_NAME}` is already stopped.");
            return Ok(());
        }
        ServiceState::StopPending => {
            wait_for_state(&service, ServiceState::Stopped, SERVICE_STATE_TIMEOUT)?;
            println!("Windows service `{SERVICE_NAME}` stopped.");
            return Ok(());
        }
        _ => {}
    }

    service.stop().context("stopping Windows service")?;
    wait_for_state(&service, ServiceState::Stopped, SERVICE_STATE_TIMEOUT)?;
    println!("Windows service `{SERVICE_NAME}` stopped.");

    Ok(())
}

fn uninstall_service() -> Result<()> {
    let service = open_installed_service(
        ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
    )?;
    let status = service.query_status().context("querying service status")?;

    if status.current_state != ServiceState::Stopped {
        let _ = service.stop();
        wait_for_state(&service, ServiceState::Stopped, SERVICE_STATE_TIMEOUT)?;
    }

    service.delete().context("deleting Windows service")?;
    println!("Windows service `{SERVICE_NAME}` has been removed.");

    Ok(())
}

fn print_service_status() -> Result<()> {
    let service =
        open_installed_service(ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG)?;
    let status = service.query_status().context("querying service status")?;
    let config = service
        .query_config()
        .context("querying service configuration")?;

    println!("name: {SERVICE_NAME}");
    println!("display_name: {SERVICE_DISPLAY_NAME}");
    println!("state: {}", format_service_state(status.current_state));
    println!("start_type: {:?}", config.start_type);
    println!("binary: {}", config.executable_path.display());
    if let Some(account_name) = config.account_name {
        println!("account: {}", account_name.to_string_lossy());
    }

    Ok(())
}

fn prepare_service_layout(options: &InstallOptions) -> Result<ServicePaths> {
    let current_exe = env::current_exe().context("locating current executable")?;

    fs::create_dir_all(&options.config_dir).with_context(|| {
        format!(
            "creating config directory `{}`",
            options.config_dir.display()
        )
    })?;
    fs::create_dir_all(&options.log_dir)
        .with_context(|| format!("creating log directory `{}`", options.log_dir.display()))?;

    if options.seed_config {
        seed_config_files(&options.config_dir)?;
    }

    let executable_path = if options.use_current_executable {
        current_exe
    } else {
        fs::create_dir_all(&options.binary_dir).with_context(|| {
            format!(
                "creating binary directory `{}`",
                options.binary_dir.display()
            )
        })?;

        let executable_name = current_exe
            .file_name()
            .ok_or_else(|| anyhow!("current executable has no file name"))?;
        let target_exe = options.binary_dir.join(executable_name);

        copy_file_if_needed(&current_exe, &target_exe)
            .with_context(|| format!("copying executable to `{}`", target_exe.display()))?;

        let current_exe_dir = current_exe
            .parent()
            .ok_or_else(|| anyhow!("current executable has no parent directory"))?;
        let pawnio_dir = current_exe_dir.join("pawnio");
        if pawnio_dir.exists() {
            copy_dir_recursive(&pawnio_dir, &options.binary_dir.join("pawnio")).with_context(
                || {
                    format!(
                        "copying PawnIO runtime bundle to `{}`",
                        options.binary_dir.join("pawnio").display()
                    )
                },
            )?;
        }

        let config_example = current_exe_dir.join(CONFIG_EXAMPLE_FILE_NAME);
        if config_example.is_file() {
            copy_file_if_needed(
                &config_example,
                &options.binary_dir.join(CONFIG_EXAMPLE_FILE_NAME),
            )
            .with_context(|| {
                format!(
                    "copying `{}` to `{}`",
                    CONFIG_EXAMPLE_FILE_NAME,
                    options.binary_dir.join(CONFIG_EXAMPLE_FILE_NAME).display()
                )
            })?;
        }

        remove_legacy_env_files(&options.binary_dir)?;

        target_exe
    };

    Ok(ServicePaths {
        executable_path,
        config_dir: options.config_dir.clone(),
        log_dir: options.log_dir.clone(),
    })
}

fn remove_legacy_env_files(directory: &Path) -> Result<()> {
    for file_name in [".env", ".env.local"] {
        let path = directory.join(file_name);
        if path.is_file() {
            fs::remove_file(&path).with_context(|| {
                format!("removing legacy configuration file `{}`", path.display())
            })?;
        }
    }

    Ok(())
}

fn seed_config_files(config_dir: &Path) -> Result<()> {
    let source_directories = candidate_config_directories_with([config_dir.to_path_buf()]);
    let generated = seed_config_toml(config_dir, &source_directories)?;

    if generated.is_some() {
        remove_legacy_env_files(config_dir)?;
    }

    Ok(())
}

fn configure_service_defaults(
    service: &windows_service::service::Service,
    delayed_auto_start: bool,
    failure_restart_delay: Duration,
) -> Result<()> {
    service
        .set_description(SERVICE_DESCRIPTION)
        .context("setting service description")?;
    service
        .set_delayed_auto_start(delayed_auto_start)
        .context("configuring delayed auto start")?;
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(24 * 60 * 60)),
            reboot_msg: None,
            command: None,
            actions: Some(vec![
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: failure_restart_delay,
                },
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: failure_restart_delay,
                },
                ServiceAction {
                    action_type: ServiceActionType::Restart,
                    delay: failure_restart_delay,
                },
            ]),
        })
        .context("configuring service recovery actions")?;
    service
        .set_failure_actions_on_non_crash_failures(true)
        .context("enabling service recovery on non-crash failures")?;

    Ok(())
}

fn desired_service_info(service_paths: &ServicePaths, start_mode: ServiceStartMode) -> ServiceInfo {
    ServiceInfo {
        name: SERVICE_NAME.into(),
        display_name: SERVICE_DISPLAY_NAME.into(),
        service_type: SERVICE_TYPE,
        start_type: match start_mode {
            ServiceStartMode::DelayedAuto | ServiceStartMode::Auto => ServiceStartType::AutoStart,
            ServiceStartMode::Manual => ServiceStartType::OnDemand,
        },
        error_control: ServiceErrorControl::Normal,
        executable_path: service_paths.executable_path.clone(),
        launch_arguments: vec![
            OsString::from("--config-dir"),
            service_paths.config_dir.as_os_str().to_owned(),
            OsString::from("--log-dir"),
            service_paths.log_dir.as_os_str().to_owned(),
        ],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    }
}

fn open_service_manager(access: ServiceManagerAccess) -> Result<ServiceManager> {
    ServiceManager::local_computer(None::<&str>, access).context("opening Windows service manager")
}

fn open_installed_service(access: ServiceAccess) -> Result<windows_service::service::Service> {
    let manager = open_service_manager(ServiceManagerAccess::CONNECT)?;
    match manager.open_service(SERVICE_NAME, access) {
        Ok(service) => Ok(service),
        Err(error) if raw_os_error(&error) == Some(ERROR_SERVICE_DOES_NOT_EXIST) => {
            bail!(
                "Windows service `{}` is not installed yet; run `{} service install` first",
                SERVICE_NAME,
                current_exe_display()?
            )
        }
        Err(error) => Err(anyhow!(error).context("opening Windows service")),
    }
}

fn wait_for_state(
    service: &windows_service::service::Service,
    desired_state: ServiceState,
    timeout: Duration,
) -> Result<()> {
    let started_at = Instant::now();

    loop {
        let status = service.query_status().context("querying service state")?;
        if status.current_state == desired_state {
            return Ok(());
        }

        if started_at.elapsed() >= timeout {
            bail!(
                "timed out waiting for Windows service `{}` to reach `{}`",
                SERVICE_NAME,
                format_service_state(desired_state)
            );
        }

        thread::sleep(Duration::from_millis(500));
    }
}

fn set_service_state(
    status_handle: &ServiceStatusHandle,
    current_state: ServiceState,
    controls_accepted: ServiceControlAccept,
    exit_code: ServiceExitCode,
) -> windows_service::Result<()> {
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state,
        controls_accepted,
        exit_code,
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    })
}

fn build_runtime() -> Result<Runtime> {
    Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building Tokio runtime")
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let destination_path = destination.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &destination_path)?;
        } else if file_type.is_file() {
            copy_file_if_needed(&entry.path(), &destination_path)?;
        }
    }

    Ok(())
}

fn copy_file_if_needed(source: &Path, destination: &Path) -> io::Result<()> {
    if same_path(source, destination) || files_match(source, destination)? {
        return Ok(());
    }

    fs::copy(source, destination)?;
    Ok(())
}

fn files_match(source: &Path, destination: &Path) -> io::Result<bool> {
    let source_metadata = fs::metadata(source)?;
    let destination_metadata = match fs::metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };

    if source_metadata.len() != destination_metadata.len() {
        return Ok(false);
    }

    Ok(fs::read(source)? == fs::read(destination)?)
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn raw_os_error(error: &WindowsServiceError) -> Option<i32> {
    match error {
        WindowsServiceError::Winapi(io_error) => io_error.raw_os_error(),
        _ => None,
    }
}

fn is_service_dispatcher_connect_error(error: &WindowsServiceError) -> bool {
    raw_os_error(error) == Some(ERROR_FAILED_SERVICE_CONTROLLER_CONNECT)
}

fn os_string_to_str(value: &OsString) -> Option<&str> {
    value.to_str()
}

fn current_exe_display() -> Result<String> {
    Ok(env::current_exe()
        .context("locating current executable")?
        .display()
        .to_string())
}

fn default_binary_dir() -> PathBuf {
    env::var_os("ProgramFiles")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(PROGRAM_FILES_FALLBACK))
        .join(SERVICE_NAME)
}

fn default_data_root() -> PathBuf {
    env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(PROGRAM_DATA_FALLBACK))
        .join(SERVICE_NAME)
}

fn default_config_dir() -> PathBuf {
    default_data_root().join("config")
}

fn default_log_dir() -> PathBuf {
    default_data_root().join("logs")
}

fn print_service_usage() {
    println!("Usage:");
    println!("  ha-system-ronitor.exe service install [options]");
    println!("  ha-system-ronitor.exe service start");
    println!("  ha-system-ronitor.exe service stop");
    println!("  ha-system-ronitor.exe service restart");
    println!("  ha-system-ronitor.exe service status");
    println!("  ha-system-ronitor.exe service uninstall");
    println!();
    print_install_usage();
}

fn print_install_usage() {
    println!("Install options:");
    println!("  --install-dir <PATH>           Alias of --binary-dir");
    println!("  --binary-dir <PATH>            Program files directory for the service executable");
    println!("  --config-dir <PATH>            Machine-wide configuration directory");
    println!("  --log-dir <PATH>               Machine-wide log directory");
    println!(
        "  --seed-config                  Copy existing `{}` or create it from `{}`",
        CONFIG_FILE_NAME, CONFIG_EXAMPLE_FILE_NAME
    );
    println!(
        "  --no-seed-config               Skip automatic `{}` generation during install",
        CONFIG_FILE_NAME
    );
    println!("  --in-place                     Keep using the current executable path");
    println!("  --start-mode <MODE>            delayed-auto | auto | manual");
    println!("  --failure-restart-secs <N>     Restart delay after failure in seconds");
    println!(
        "  --defaults                     Reset to Program Files + ProgramData best-practice layout"
    );
}

fn format_service_state(state: ServiceState) -> &'static str {
    match state {
        ServiceState::Stopped => "stopped",
        ServiceState::StartPending => "start_pending",
        ServiceState::StopPending => "stop_pending",
        ServiceState::Running => "running",
        ServiceState::ContinuePending => "continue_pending",
        ServiceState::PausePending => "pause_pending",
        ServiceState::Paused => "paused",
    }
}

fn format_start_mode(mode: ServiceStartMode) -> &'static str {
    match mode {
        ServiceStartMode::DelayedAuto => "automatic (delayed)",
        ServiceStartMode::Auto => "automatic",
        ServiceStartMode::Manual => "manual",
    }
}

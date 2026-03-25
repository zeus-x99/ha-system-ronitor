#[cfg(target_os = "windows")]
mod windows_backend {
    use std::env;
    use std::ffi::CString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::OnceLock;
    use std::thread;
    use std::time::Duration;

    use libloading::Library;
    use tracing::{debug, info, warn};
    use windows::Win32::Foundation::{
        CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
    use windows::core::{HRESULT, PCWSTR};

    const ACCESS_PCI_MUTEX_NAME: &str = "Global\\Access_PCI";
    const AMDFAMILY17_MODULE_NAME: &str = "AMDFamily17.bin";
    const PAWNIO_DLL_NAME: &str = "PawnIOLib.dll";
    const PAWNIO_INSTALLER_NAME: &str = "PawnIO_setup.exe";
    const PAWNIO_TIMEOUT: Duration = Duration::from_millis(250);
    const PAWNIO_INSTALL_RETRY_DELAY: Duration = Duration::from_millis(500);
    const PAWNIO_INSTALL_RETRY_COUNT: usize = 10;

    const ZEN_REPORTED_TEMP_CTRL_BASE: u32 = 0x0005_9800;
    const ZEN_CUR_TEMP_SHIFT: u32 = 21;
    const ZEN_CUR_TEMP_RANGE_SEL_MASK: u32 = 1 << 19;
    const ZEN_CUR_TEMP_TJ_SEL_MASK: u32 = 0b11 << 16;

    static PAWNIO_AUTO_INSTALL_ATTEMPTED: OnceLock<bool> = OnceLock::new();

    type PawnioOpen = unsafe extern "system" fn(*mut HANDLE) -> HRESULT;
    type PawnioLoad = unsafe extern "system" fn(HANDLE, *const u8, usize) -> HRESULT;
    type PawnioExecute = unsafe extern "system" fn(
        HANDLE,
        *const i8,
        *const u64,
        usize,
        *mut u64,
        usize,
        *mut usize,
    ) -> HRESULT;
    type PawnioClose = unsafe extern "system" fn(HANDLE) -> HRESULT;

    pub struct PawnIoCpuTemperatureReader {
        executor: PawnIoExecutor,
        access_pci_mutex: Option<HANDLE>,
        ioctl_read_smn_name: CString,
        recreate_after_failure: bool,
    }

    impl std::fmt::Debug for PawnIoCpuTemperatureReader {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("PawnIoCpuTemperatureReader")
                .field("access_pci_mutex", &self.access_pci_mutex.is_some())
                .finish()
        }
    }

    impl PawnIoCpuTemperatureReader {
        pub fn new() -> Option<Self> {
            let module_path = resolve_module_path()?;
            let dll_path = resolve_existing_dll_path()
                .or_else(ensure_pawnio_installed_and_resolve_dll_path)?;

            let library = unsafe { Library::new(&dll_path) }.ok().or_else(|| {
                debug!("failed to load PawnIO library from {}", dll_path.display());
                None
            })?;

            let open = load_symbol::<PawnioOpen>(&library, b"pawnio_open\0")?;
            let load = load_symbol::<PawnioLoad>(&library, b"pawnio_load\0")?;
            let execute = load_symbol::<PawnioExecute>(&library, b"pawnio_execute\0")?;
            let close = load_symbol::<PawnioClose>(&library, b"pawnio_close\0")?;
            let module_blob = fs::read(&module_path).ok().or_else(|| {
                debug!(
                    "failed to read PawnIO module from {}",
                    module_path.display()
                );
                None
            })?;

            let mut handle = HANDLE::default();
            let open_result = unsafe { open(&mut handle) };
            if !open_result.is_ok() {
                log_pawnio_error("open", &dll_path, open_result);
                return None;
            }

            let load_result = unsafe { load(handle, module_blob.as_ptr(), module_blob.len()) };
            if !load_result.is_ok() {
                log_pawnio_error("load", &module_path, load_result);
                unsafe {
                    let _ = close(handle);
                }
                return None;
            }

            Some(Self {
                executor: PawnIoExecutor {
                    _library: library,
                    handle,
                    execute,
                    close,
                },
                access_pci_mutex: open_access_pci_mutex(),
                ioctl_read_smn_name: CString::new("ioctl_read_smn").ok()?,
                recreate_after_failure: false,
            })
        }

        pub fn read(&mut self) -> Option<f32> {
            self.recreate_after_failure = false;

            match self.read_temperature_register_raw() {
                Ok(register_value) => decode_zen_temperature(register_value)
                    .map(|value| value as f32 / 1000.0)
                    .or_else(|| {
                        self.recreate_after_failure = true;
                        None
                    }),
                Err(ReadFailureKind::Transient) => None,
                Err(ReadFailureKind::Fatal) => {
                    self.recreate_after_failure = true;
                    None
                }
            }
        }

        pub fn should_recreate_after_failure(&self) -> bool {
            self.recreate_after_failure
        }

        fn read_temperature_register_raw(&mut self) -> Result<u32, ReadFailureKind> {
            self.read_smn(ZEN_REPORTED_TEMP_CTRL_BASE)
        }

        fn read_smn(&mut self, address: u32) -> Result<u32, ReadFailureKind> {
            let _guard = AccessPciGuard::acquire(self.access_pci_mutex, PAWNIO_TIMEOUT)?;
            self.execute_scalar(&self.ioctl_read_smn_name, &[address as u64])
                .map(|value| value as u32)
        }

        fn execute_scalar(
            &self,
            function_name: &CString,
            input: &[u64],
        ) -> Result<u64, ReadFailureKind> {
            let mut output = [0u64; 1];
            let mut return_size = 0usize;

            let execute_result = unsafe {
                (self.executor.execute)(
                    self.executor.handle,
                    function_name.as_ptr(),
                    input.as_ptr(),
                    input.len(),
                    output.as_mut_ptr(),
                    output.len(),
                    &mut return_size,
                )
            };

            if !execute_result.is_ok() {
                debug!(
                    operation = function_name.to_string_lossy().into_owned(),
                    hresult = format_hresult(execute_result),
                    "PawnIO execute failed"
                );
                return Err(ReadFailureKind::Fatal);
            }

            if return_size == 0 {
                debug!(
                    operation = function_name.to_string_lossy().into_owned(),
                    "PawnIO execute returned no data"
                );
                return Err(ReadFailureKind::Fatal);
            }

            Ok(output[0])
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ReadFailureKind {
        Transient,
        Fatal,
    }

    impl Drop for PawnIoCpuTemperatureReader {
        fn drop(&mut self) {
            if let Some(handle) = self.access_pci_mutex.take() {
                unsafe {
                    let _ = CloseHandle(handle);
                }
            }
        }
    }

    struct PawnIoExecutor {
        _library: Library,
        handle: HANDLE,
        execute: PawnioExecute,
        close: PawnioClose,
    }

    impl Drop for PawnIoExecutor {
        fn drop(&mut self) {
            unsafe {
                let _ = (self.close)(self.handle);
            }
        }
    }

    struct AccessPciGuard {
        handle: HANDLE,
    }

    impl AccessPciGuard {
        fn acquire(
            handle: Option<HANDLE>,
            timeout: Duration,
        ) -> Result<Option<Self>, ReadFailureKind> {
            let Some(handle) = handle else {
                return Ok(None);
            };

            let wait_ms = timeout.as_millis().min(u32::MAX as u128) as u32;
            let wait_result = unsafe { WaitForSingleObject(handle, wait_ms) };

            if wait_result == WAIT_OBJECT_0 || wait_result == WAIT_ABANDONED {
                return Ok(Some(Self { handle }));
            }

            if wait_result == WAIT_TIMEOUT {
                debug!("timed out waiting for Global\\Access_PCI");
                Err(ReadFailureKind::Transient)
            } else {
                debug!(
                    wait_result = wait_result.0,
                    "failed waiting for Global\\Access_PCI"
                );
                Err(ReadFailureKind::Fatal)
            }
        }
    }

    impl Drop for AccessPciGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = ReleaseMutex(self.handle);
            }
        }
    }

    fn load_symbol<T: Copy>(library: &Library, symbol_name: &[u8]) -> Option<T> {
        unsafe { library.get::<T>(symbol_name).ok().map(|symbol| *symbol) }.or_else(|| {
            let name = String::from_utf8_lossy(symbol_name)
                .trim_end_matches('\0')
                .to_string();
            debug!(%name, "failed to resolve PawnIO symbol");
            None
        })
    }

    fn resolve_module_path() -> Option<PathBuf> {
        let bundled_path =
            bundled_runtime_path(["pawnio", "windows", "modules", AMDFAMILY17_MODULE_NAME]);
        if let Some(path) = bundled_path {
            return Some(path);
        }

        let source_tree_path =
            source_tree_vendor_path(["pawnio", "windows", "modules", AMDFAMILY17_MODULE_NAME]);
        if source_tree_path.is_file() {
            return Some(source_tree_path);
        }

        debug!("PawnIO module {} was not found", AMDFAMILY17_MODULE_NAME);
        None
    }

    fn resolve_existing_dll_path() -> Option<PathBuf> {
        if let Some(path) = bundled_runtime_path(["pawnio", "windows", PAWNIO_DLL_NAME]) {
            return Some(path);
        }

        let source_tree_path = source_tree_vendor_path(["pawnio", "windows", PAWNIO_DLL_NAME]);
        if source_tree_path.is_file() {
            return Some(source_tree_path);
        }

        let program_files_path = env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .map(|base| base.join("PawnIO").join(PAWNIO_DLL_NAME))
            .filter(|path| path.is_file());

        if let Some(path) = program_files_path {
            return Some(path);
        }

        debug!("PawnIO library {} was not found", PAWNIO_DLL_NAME);
        None
    }

    fn ensure_pawnio_installed_and_resolve_dll_path() -> Option<PathBuf> {
        if !pawnio_auto_install_enabled() {
            debug!("PawnIO auto-install disabled by environment");
            return None;
        }

        let install_attempt_succeeded =
            *PAWNIO_AUTO_INSTALL_ATTEMPTED.get_or_init(install_pawnio_runtime_if_missing);
        if !install_attempt_succeeded {
            return None;
        }

        resolve_existing_dll_path()
    }

    fn pawnio_auto_install_enabled() -> bool {
        match env::var("HA_MONITOR_PAWNIO_AUTO_INSTALL") {
            Ok(value) => !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            ),
            Err(_) => true,
        }
    }

    fn install_pawnio_runtime_if_missing() -> bool {
        info!("PawnIO not found, attempting automatic local install");

        let installer_path = match prepare_pawnio_installer() {
            Some(path) => path,
            None => {
                warn!("failed to locate a local PawnIO installer");
                return false;
            }
        };

        let status = match Command::new(&installer_path)
            .args(["-install", "-silent"])
            .status()
        {
            Ok(status) => status,
            Err(error) => {
                warn!(%error, path = %installer_path.display(), "failed to launch PawnIO installer");
                return false;
            }
        };

        let exit_code = status.code().unwrap_or(-1);
        if !matches!(status.code(), Some(0 | 3010)) {
            warn!(exit_code, "PawnIO installer failed");
            return false;
        }

        if exit_code == 3010 {
            info!("PawnIO installer requested reboot to complete installation");
        }

        for _ in 0..PAWNIO_INSTALL_RETRY_COUNT {
            if resolve_existing_dll_path().is_some() {
                info!("PawnIO runtime installed successfully");
                return true;
            }

            thread::sleep(PAWNIO_INSTALL_RETRY_DELAY);
        }

        warn!("PawnIO installer finished but PawnIOLib.dll is still missing");
        false
    }

    fn prepare_pawnio_installer() -> Option<PathBuf> {
        if let Some(path) = bundled_runtime_path(["pawnio", "windows", PAWNIO_INSTALLER_NAME]) {
            return Some(path);
        }

        let source_tree_path =
            source_tree_vendor_path(["pawnio", "windows", PAWNIO_INSTALLER_NAME]);
        if source_tree_path.is_file() {
            return Some(source_tree_path);
        }

        warn!(
            installer_name = PAWNIO_INSTALLER_NAME,
            "PawnIO local installer was not found next to the executable or under vendor/pawnio/windows"
        );
        None
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

    fn open_access_pci_mutex() -> Option<HANDLE> {
        let name = to_wide(ACCESS_PCI_MUTEX_NAME);
        unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
            .ok()
            .or_else(|| {
                debug!("failed to open or create Global\\Access_PCI");
                None
            })
    }

    fn to_wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn decode_zen_temperature(register_value: u32) -> Option<i32> {
        if register_value == 0 || register_value == u32::MAX {
            return None;
        }

        let mut temperature_millicelsius = ((register_value >> ZEN_CUR_TEMP_SHIFT) as i32) * 125;

        if (register_value & ZEN_CUR_TEMP_RANGE_SEL_MASK) != 0
            || (register_value & ZEN_CUR_TEMP_TJ_SEL_MASK) == ZEN_CUR_TEMP_TJ_SEL_MASK
        {
            temperature_millicelsius -= 49_000;
        }

        if (-40_000..=130_000).contains(&temperature_millicelsius) {
            Some(temperature_millicelsius)
        } else {
            None
        }
    }

    fn log_pawnio_error(operation: &str, path: &Path, result: HRESULT) {
        let code = format_hresult(result);
        if result.0 as u32 == 0x8007_0005 {
            warn!(
                operation,
                path = %path.display(),
                hresult = code,
                "PawnIO access denied; run the monitor elevated on Windows to read cpu_package_temp"
            );
        } else {
            debug!(
                operation,
                path = %path.display(),
                hresult = code,
                "PawnIO initialization failed"
            );
        }
    }

    fn format_hresult(result: HRESULT) -> String {
        format!("0x{:08X}", result.0 as u32)
    }
}

#[cfg(target_os = "windows")]
pub use windows_backend::PawnIoCpuTemperatureReader;

#[cfg(not(target_os = "windows"))]
#[derive(Debug, Default)]
pub struct PawnIoCpuTemperatureReader;

#[cfg(not(target_os = "windows"))]
impl PawnIoCpuTemperatureReader {
    pub fn new() -> Option<Self> {
        None
    }

    pub fn read(&mut self) -> Option<f32> {
        None
    }

    pub fn should_recreate_after_failure(&self) -> bool {
        false
    }
}

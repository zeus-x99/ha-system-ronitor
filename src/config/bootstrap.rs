use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

use super::candidate_config_directories_with;

const CONFIG_DIR_ENV: &str = "HA_MONITOR_CONFIG_DIR";
const LOG_DIR_ENV: &str = "HA_MONITOR_LOG_DIR";

#[derive(Debug, Clone, Default)]
pub struct BootstrapOptions {
    pub config_dir: Option<PathBuf>,
    pub log_dir: Option<PathBuf>,
}

impl BootstrapOptions {
    pub fn from_current_process() -> Self {
        Self::from_args(env::args_os())
    }

    pub fn from_args<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString>,
    {
        let mut options = Self {
            config_dir: env::var_os(CONFIG_DIR_ENV).map(PathBuf::from),
            log_dir: env::var_os(LOG_DIR_ENV).map(PathBuf::from),
        };
        let mut args = args.into_iter().map(Into::into);

        let _ = args.next();

        while let Some(arg) = args.next() {
            match arg.to_str() {
                Some("--config-dir") => {
                    if let Some(value) = args.next() {
                        options.config_dir = Some(PathBuf::from(value));
                    }
                }
                Some("--log-dir") => {
                    if let Some(value) = args.next() {
                        options.log_dir = Some(PathBuf::from(value));
                    }
                }
                _ => {}
            }
        }

        options
    }

    pub fn config_directories(&self) -> Vec<PathBuf> {
        if let Some(config_dir) = self.config_dir.clone() {
            vec![config_dir]
        } else {
            candidate_config_directories_with(std::iter::empty())
        }
    }
}

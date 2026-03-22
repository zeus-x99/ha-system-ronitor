mod env;

use clap::Parser;

pub use env::load_env_files;

#[derive(Debug, Parser, Clone)]
#[command(
    author,
    version,
    about = "Cross-platform system monitor for Home Assistant"
)]
pub struct Config {
    #[arg(long, env = "HA_MONITOR_MQTT_HOST")]
    pub mqtt_host: String,

    #[arg(long, env = "HA_MONITOR_MQTT_PORT", default_value_t = 1883)]
    pub mqtt_port: u16,

    #[arg(long, env = "HA_MONITOR_MQTT_USERNAME")]
    pub mqtt_username: Option<String>,

    #[arg(long, env = "HA_MONITOR_MQTT_PASSWORD")]
    pub mqtt_password: Option<String>,

    #[arg(
        long,
        env = "HA_MONITOR_DISCOVERY_PREFIX",
        default_value = "homeassistant"
    )]
    pub discovery_prefix: String,

    #[arg(
        long,
        env = "HA_MONITOR_HOME_ASSISTANT_STATUS_TOPIC",
        default_value = "homeassistant/status"
    )]
    pub home_assistant_status_topic: String,

    #[arg(
        long,
        env = "HA_MONITOR_TOPIC_PREFIX",
        default_value = "monitor/system"
    )]
    pub topic_prefix: String,

    #[arg(long, env = "HA_MONITOR_NODE_ID")]
    pub node_id: Option<String>,

    #[arg(long, env = "HA_MONITOR_DEVICE_NAME")]
    pub device_name: Option<String>,

    #[arg(
        long,
        env = "HA_MONITOR_ENABLE_SHUTDOWN_BUTTON",
        default_value_t = false
    )]
    pub enable_shutdown_button: bool,

    #[arg(long, env = "HA_MONITOR_SHUTDOWN_PAYLOAD", default_value = "shutdown")]
    pub shutdown_payload: String,

    #[arg(long, env = "HA_MONITOR_SHUTDOWN_DRY_RUN", default_value_t = false)]
    pub shutdown_dry_run: bool,

    #[arg(
        long,
        env = "HA_MONITOR_CPU_INTERVAL_SECS",
        default_value_t = 1,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub cpu_interval_secs: u64,

    #[arg(
        long,
        env = "HA_MONITOR_GPU_INTERVAL_SECS",
        default_value_t = 1,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub gpu_interval_secs: u64,

    #[arg(
        long,
        env = "HA_MONITOR_MEMORY_INTERVAL_SECS",
        default_value_t = 5,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub memory_interval_secs: u64,

    #[arg(
        long,
        env = "HA_MONITOR_DISK_INTERVAL_SECS",
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub disk_interval_secs: u64,

    #[arg(
        long,
        env = "HA_MONITOR_CPU_CHANGE_THRESHOLD_PCT",
        default_value_t = 1.0
    )]
    pub cpu_change_threshold_pct: f32,

    #[arg(
        long,
        env = "HA_MONITOR_GPU_USAGE_CHANGE_THRESHOLD_PCT",
        default_value_t = 1.0
    )]
    pub gpu_usage_change_threshold_pct: f32,

    #[arg(
        long,
        env = "HA_MONITOR_GPU_MEMORY_CHANGE_THRESHOLD_MIB",
        default_value_t = 8,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub gpu_memory_change_threshold_mib: u64,

    #[arg(
        long,
        env = "HA_MONITOR_MEMORY_CHANGE_THRESHOLD_MIB",
        default_value_t = 8,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub memory_change_threshold_mib: u64,

    #[arg(
        long,
        env = "HA_MONITOR_DISK_CHANGE_THRESHOLD_MIB",
        default_value_t = 32,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub disk_change_threshold_mib: u64,

    #[arg(long, env = "HA_MONITOR_CPU_SMOOTHING_WINDOW", default_value_t = 5)]
    pub cpu_smoothing_window: usize,

    #[arg(
        long,
        env = "HA_MONITOR_CPU_MAX_SILENCE_SECS",
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub cpu_max_silence_secs: u64,

    #[arg(
        long,
        env = "HA_MONITOR_GPU_MAX_SILENCE_SECS",
        default_value_t = 30,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub gpu_max_silence_secs: u64,

    #[arg(
        long,
        env = "HA_MONITOR_MEMORY_MAX_SILENCE_SECS",
        default_value_t = 120,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub memory_max_silence_secs: u64,

    #[arg(
        long,
        env = "HA_MONITOR_DISK_MAX_SILENCE_SECS",
        default_value_t = 900,
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub disk_max_silence_secs: u64,
}

impl Config {
    pub fn gpu_memory_change_threshold_bytes(&self) -> u64 {
        self.gpu_memory_change_threshold_mib * 1024 * 1024
    }

    pub fn memory_change_threshold_bytes(&self) -> u64 {
        self.memory_change_threshold_mib * 1024 * 1024
    }

    pub fn disk_change_threshold_bytes(&self) -> u64 {
        self.disk_change_threshold_mib * 1024 * 1024
    }
}

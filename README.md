# ha-system-ronitor

Cross-platform system monitor written in Rust, publishing CPU, GPU, memory, and disk metrics to Home Assistant through MQTT Device Discovery.

## Why this design

- Uses `sysinfo` for cross-platform metrics on Windows, Linux, and macOS.
- Uses Home Assistant MQTT device-based discovery so one MQTT device can announce all of its entities in a single retained payload.
- Republishes discovery when Home Assistant sends its MQTT birth message on `homeassistant/status`, using a small per-node stagger to avoid broker spikes.
- Uses one device with multiple sensor components instead of many disconnected entities.

## Metrics

- Global CPU usage
- Best-effort CPU package temperature
- CPU model / OS version / uptime
- Best-effort GPU usage / dedicated memory used / available / total / temperature
- Total memory / used memory / memory usage
- Per-disk total / available / used / usage
- Optional Home Assistant shutdown button that can power off the host

## Refresh strategy

- CPU: high frequency, default every 1 second
- GPU: high frequency, default every 1 second
- Memory: medium frequency, default every 5 seconds
- Disk: low frequency, default every 30 seconds
- State is published only when values change enough to matter, reducing Home Assistant update pressure
- CPU uses a moving average smoothing window before publish, reducing jitter
- Runtime metadata and temperature snapshots are refreshed every 1 second so `cpu_package_temp` tracks the fast CPU publish path
- A max silence timer forces a refresh occasionally even if values stay within thresholds
- Defaults: CPU and GPU usage threshold 1.0%, GPU and memory delta 8 MiB, disk delta 32 MiB
- Defaults: CPU smoothing window 5 samples, max silence 30s / 30s / 120s / 900s

## Temperature support

- `cpu_package_temp` is best-effort
- On Linux this usually works when the host exposes hwmon sensors
- On Windows it uses PawnIO with the signed `AMDFamily17` module for modern AMD Zen CPUs
- If PawnIO is missing on Windows, the monitor tries to run a bundled local PawnIO installer automatically on first start
- For cross-platform consistency, only one whole-CPU temperature entity is published
- The Home Assistant `cpu_package_temp` entity is always announced, and shows no value until a temperature source is available

## GPU support

- On Windows, NVIDIA GPU metrics use `nvml-wrapper`, the Rust wrapper over NVIDIA NVML
- It publishes `gpu_name`, `gpu_usage`, `gpu_memory_used`, `gpu_memory_total`, `gpu_memory_usage`, and `gpu_temperature`
- The current implementation selects the NVIDIA device with the largest reported VRAM
- On platforms without a supported native GPU backend yet, GPU entities are omitted

## Windows CPU temperature

- Windows builds package `vendor/pawnio/windows` next to the executable automatically through `build.rs`
- The project vendors the official signed `AMDFamily17.bin` module from PawnIO Modules `0.2.3`, published on 2026-02-25
- To enable offline auto-install, place `PawnIO_setup.exe` under `vendor/pawnio/windows/`
- `PawnIOLib.dll` is not bundled; the runtime looks for a local `pawnio/windows/PawnIOLib.dll` first, then falls back to `C:\Program Files\PawnIO\PawnIOLib.dll`
- If `PawnIOLib.dll` is still missing, the runtime looks for a bundled local `PawnIO_setup.exe` and runs a silent install
- On Windows, `cpu_package_temp` is read only through PawnIO and the validated AMD SMN temperature register path
- PawnIO device access is restricted to Administrators and SYSTEM on Windows, so the monitor process must run elevated if you want `cpu_package_temp` to appear
- If PawnIO is missing, unsupported on the current CPU, or cannot be opened, the `cpu_package_temp` entity stays unavailable
- Set `HA_MONITOR_PAWNIO_AUTO_INSTALL=false` if you want to disable automatic PawnIO installation

## Remote shutdown

- Disabled by default
- When enabled, Home Assistant shows a button on the same device
- Pressing the button publishes an MQTT command and this agent executes a local shutdown command
- The process usually needs administrator or root privileges for shutdown to succeed
- For safe testing, enable dry-run mode first

## Project structure

- `src/main.rs`: binary entry, only starts the application
- `src/lib.rs`: crate root, re-exports the internal modules
- `src/app/`: application runtime orchestration
- `src/config/`: CLI and environment configuration
- `src/device/`: device identity and MQTT topic layout
- `src/system/`: system metric collection and data models
- `src/integrations/home_assistant/`: Home Assistant discovery integration
- `src/integrations/mqtt/`: MQTT connection and publish helpers
- `src/shared/`: shared utility helpers

## Requirements

- Rust stable with edition 2024 support
- A running MQTT broker already connected to Home Assistant
- Home Assistant MQTT integration enabled

## Run

Create your local config first:

```powershell
Copy-Item .env.example .env
```

Then edit `.env` and start:

```powershell
cargo run --release
```

You can still override values with shell environment variables or CLI flags.

## Run without .env

```powershell
$env:HA_MONITOR_MQTT_HOST="192.168.1.10"
$env:HA_MONITOR_MQTT_PORT="1883"
$env:HA_MONITOR_MQTT_USERNAME="mqtt-user"
$env:HA_MONITOR_MQTT_PASSWORD="mqtt-password"
cargo run --release
```

Or with CLI flags:

```powershell
cargo run --release -- `
  --mqtt-host 192.168.1.10 `
  --mqtt-port 1883 `
  --mqtt-username mqtt-user `
  --mqtt-password mqtt-password `
  --cpu-interval-secs 1 `
  --gpu-interval-secs 1 `
  --memory-interval-secs 5 `
  --disk-interval-secs 30 `
  --cpu-change-threshold-pct 1.0 `
  --gpu-usage-change-threshold-pct 1.0 `
  --gpu-memory-change-threshold-mib 8 `
  --memory-change-threshold-mib 8 `
  --disk-change-threshold-mib 32 `
  --cpu-smoothing-window 5 `
  --cpu-max-silence-secs 30 `
  --gpu-max-silence-secs 30
```

## Configuration

| Flag | Env | Default | Description |
| --- | --- | --- | --- |
| `--mqtt-host` | `HA_MONITOR_MQTT_HOST` | none | MQTT broker host |
| `--mqtt-port` | `HA_MONITOR_MQTT_PORT` | `1883` | MQTT broker port |
| `--mqtt-username` | `HA_MONITOR_MQTT_USERNAME` | none | MQTT username |
| `--mqtt-password` | `HA_MONITOR_MQTT_PASSWORD` | none | MQTT password |
| `--discovery-prefix` | `HA_MONITOR_DISCOVERY_PREFIX` | `homeassistant` | MQTT discovery prefix |
| `--home-assistant-status-topic` | `HA_MONITOR_HOME_ASSISTANT_STATUS_TOPIC` | `homeassistant/status` | Home Assistant birth topic |
| `--topic-prefix` | `HA_MONITOR_TOPIC_PREFIX` | `monitor/system` | State and availability topic prefix |
| `--node-id` | `HA_MONITOR_NODE_ID` | hostname-derived | Stable device node ID |
| `--device-name` | `HA_MONITOR_DEVICE_NAME` | `<hostname> System Monitor` | Device name in Home Assistant |
| `--enable-shutdown-button` | `HA_MONITOR_ENABLE_SHUTDOWN_BUTTON` | `false` | Expose a shutdown button in Home Assistant |
| `--shutdown-payload` | `HA_MONITOR_SHUTDOWN_PAYLOAD` | `shutdown` | Expected MQTT payload for shutdown command |
| `--shutdown-dry-run` | `HA_MONITOR_SHUTDOWN_DRY_RUN` | `false` | Log shutdown action without powering off the host |
| `--cpu-interval-secs` | `HA_MONITOR_CPU_INTERVAL_SECS` | `1` | CPU publish interval |
| `--gpu-interval-secs` | `HA_MONITOR_GPU_INTERVAL_SECS` | `1` | GPU publish interval |
| `--memory-interval-secs` | `HA_MONITOR_MEMORY_INTERVAL_SECS` | `5` | Memory and swap publish interval |
| `--disk-interval-secs` | `HA_MONITOR_DISK_INTERVAL_SECS` | `30` | Disk publish interval |
| `--cpu-change-threshold-pct` | `HA_MONITOR_CPU_CHANGE_THRESHOLD_PCT` | `1.0` | Minimum CPU change before publish |
| `--gpu-usage-change-threshold-pct` | `HA_MONITOR_GPU_USAGE_CHANGE_THRESHOLD_PCT` | `1.0` | Minimum GPU usage change before publish |
| `--gpu-memory-change-threshold-mib` | `HA_MONITOR_GPU_MEMORY_CHANGE_THRESHOLD_MIB` | `8` | Minimum GPU memory change before publish |
| `--memory-change-threshold-mib` | `HA_MONITOR_MEMORY_CHANGE_THRESHOLD_MIB` | `8` | Minimum memory or swap change before publish |
| `--disk-change-threshold-mib` | `HA_MONITOR_DISK_CHANGE_THRESHOLD_MIB` | `32` | Minimum disk change before publish |
| `--cpu-smoothing-window` | `HA_MONITOR_CPU_SMOOTHING_WINDOW` | `5` | Number of CPU samples used for moving average |
| `--cpu-max-silence-secs` | `HA_MONITOR_CPU_MAX_SILENCE_SECS` | `30` | Force CPU publish after this silence window |
| `--gpu-max-silence-secs` | `HA_MONITOR_GPU_MAX_SILENCE_SECS` | `30` | Force GPU publish after this silence window |
| `--memory-max-silence-secs` | `HA_MONITOR_MEMORY_MAX_SILENCE_SECS` | `120` | Force memory publish after this silence window |
| `--disk-max-silence-secs` | `HA_MONITOR_DISK_MAX_SILENCE_SECS` | `900` | Force disk publish after this silence window |

## Home Assistant

1. Enable the MQTT integration in Home Assistant.
2. Make sure Home Assistant can connect to the same MQTT broker.
3. Start this service.
4. Open `Settings -> Devices & services -> MQTT` and the device should appear automatically.

No YAML sensor definitions are required.

The current implementation publishes one retained device discovery payload at `homeassistant/device/<node_id>/config` and automatically publishes migration markers for the older per-entity discovery topics.

To enable the shutdown button:

```env
HA_MONITOR_ENABLE_SHUTDOWN_BUTTON=true
HA_MONITOR_SHUTDOWN_PAYLOAD=shutdown
HA_MONITOR_SHUTDOWN_DRY_RUN=true
```

After validating the button in Home Assistant, change `HA_MONITOR_SHUTDOWN_DRY_RUN=false` and restart the service.

## Development

```powershell
cargo fmt
cargo check
cargo clippy --all-targets --all-features
```

Windows PawnIO CPU temperature probe:

```powershell
cargo run --example windows_pawnio_temp_probe
cargo run --example windows_pawnio_temp_probe -- --count 60 --interval-ms 1000
```

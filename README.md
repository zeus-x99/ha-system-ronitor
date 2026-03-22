# ha-system-ronitor

这是一个使用 Rust 编写的跨平台系统监控程序，通过 MQTT Device Discovery 将 CPU、GPU、内存、磁盘等指标发布到 Home Assistant。

## 设计思路

- 使用 `sysinfo` 采集 Windows、Linux、macOS 上通用的系统指标。
- 使用 Home Assistant MQTT 的“设备级自动发现”，让一个 MQTT 设备一次性声明多个实体。
- 当 Home Assistant 在 `homeassistant/status` 发布 birth 消息时，程序会自动重新发布 discovery。
- 使用单个设备承载多个传感器实体，避免在 HA 中产生大量割裂的独立实体。

## 当前已发布指标

- CPU 总使用率
- 尽力获取的 CPU 整体温度
- CPU 型号 / 操作系统版本 / 系统运行时间
- 尽力获取的 GPU 使用率 / 已用显存 / 可用显存 / 总显存 / 温度
- 内存总量 / 已用内存 / 可用内存 / 内存使用率
- 各磁盘总量 / 可用空间 / 已用空间 / 使用率
- 可选的 Home Assistant 远程关机按钮

## 刷新策略

- CPU：高频刷新，默认每 `1` 秒
- GPU：高频刷新，默认每 `1` 秒
- 内存：中频刷新，默认每 `5` 秒
- 磁盘：低频刷新，默认每 `30` 秒
- 只有当数值变化达到阈值时才发布，降低 Home Assistant 与 MQTT Broker 的压力
- CPU 发布前会经过滑动平均，减少瞬时抖动
- 运行时元数据与温度快照默认每 `1` 秒刷新一次，保证 `cpu_package_temp` 能跟上 CPU 高频发布链路
- 即使长期没有明显变化，也会在“最大发布静默时间”到达后强制补发一次状态
- 默认阈值：CPU/GPU 使用率 `1.0%`、GPU/内存变化 `8 MiB`、磁盘变化 `32 MiB`
- 默认最大发布静默时间：CPU `30s`、GPU `30s`、内存 `120s`、磁盘 `900s`

## 温度支持

- `cpu_package_temp` 为“尽力获取”指标
- 在 Linux 上，如果主机暴露了标准 hwmon 传感器，通常可以读取到 CPU 温度
- 在 Windows 上，当前通过 PawnIO + 已验证的 `AMDFamily17` 模块为现代 AMD Zen 平台读取 CPU 温度
- 如果 Windows 上未安装 PawnIO，程序首次启动时会尝试执行本地打包的 PawnIO 安装器
- 为了保证跨平台一致性，目前只发布一个“整颗 CPU”的温度实体
- Home Assistant 中会始终声明 `cpu_package_temp` 实体；若当前平台无法获取温度，它会保持无值状态

## GPU 支持

- 在 Windows 上，NVIDIA GPU 指标通过 `nvml-wrapper`（NVIDIA NVML 的 Rust 封装）采集
- 当前会发布 `gpu_name`、`gpu_usage`、`gpu_memory_used`、`gpu_memory_available`、`gpu_memory_total`、`gpu_memory_usage`、`gpu_temperature`
- 当前实现会优先选择显存最大的 NVIDIA 设备
- 在暂未接入原生 GPU 后端的平台上，GPU 相关实体会被省略

## Windows CPU 温度实现

- Windows 构建会通过 `build.rs` 自动把 `vendor/pawnio/windows` 打包到可执行文件旁边
- 项目内置了 PawnIO Modules `0.2.3` 中官方签名的 `AMDFamily17.bin` 模块（发布时间为 `2026-02-25`）
- 如需离线自动安装 PawnIO，请将 `PawnIO_setup.exe` 放到 `vendor/pawnio/windows/`
- `PawnIOLib.dll` 不直接内置到仓库；运行时会优先查找本地 `pawnio/windows/PawnIOLib.dll`，找不到时再回退到 `C:\Program Files\PawnIO\PawnIOLib.dll`
- 如果仍然缺少 `PawnIOLib.dll`，运行时会继续查找本地打包的 `PawnIO_setup.exe` 并尝试静默安装
- Windows 下 `cpu_package_temp` 目前只通过 PawnIO + 已验证的 AMD SMN 温度寄存器路径读取
- PawnIO 在 Windows 上默认只允许 Administrators 和 SYSTEM 访问，因此如果你希望读取 `cpu_package_temp`，监控进程需要管理员权限，或以服务形式运行在 `LocalSystem`
- 如果 PawnIO 不存在、当前 CPU 不受支持、或设备打开失败，`cpu_package_temp` 会保持不可用
- 如需禁用自动安装，可设置 `HA_MONITOR_PAWNIO_AUTO_INSTALL=false`

## 远程关机

- 默认关闭
- 启用后，Home Assistant 会在同一设备下显示一个“关机”按钮实体
- 点击按钮后，HA 会发送 MQTT 指令，本程序收到后会在本机执行关机命令
- 真正执行关机通常需要管理员权限或 root 权限
- 建议先开启 `dry_run` 进行安全验证

## Windows 服务

- Windows 构建既可以作为普通控制台程序运行，也可以作为真正的 Windows 服务运行
- 安装服务时应直接使用编译后的可执行文件，而不是 `cargo run`
- 当前默认目录布局遵循 Windows 最佳实践：
  - 程序与运行时文件位于 `C:\Program Files\ha-system-ronitor`
  - 配置位于 `C:\ProgramData\ha-system-ronitor\config\config.toml`
  - 日志位于 `C:\ProgramData\ha-system-ronitor\logs`
- 服务默认以 `LocalSystem` 运行，这也满足 PawnIO 读取 CPU 温度所需的权限
- 服务启动命令会显式带上 `--config-dir` 与 `--log-dir`，避免把运行配置和日志写进 `Program Files`
- `service install` 默认就会使用这套目录布局
- 安装服务时，如果当前目录已有 `config.toml`，会复制过去；否则会用 `config.example.toml` 自动生成
- 现在只支持当前版本的 `config.toml` 结构，不再兼容旧的平铺式配置

```powershell
cargo build --release
.\target\release\ha-system-ronitor.exe service install
.\target\release\ha-system-ronitor.exe service start
.\target\release\ha-system-ronitor.exe service status
```

常用服务命令：

```powershell
.\target\release\ha-system-ronitor.exe service install --binary-dir "D:\Apps\ha-system-ronitor" --config-dir "D:\Data\ha-system-ronitor\config" --log-dir "D:\Data\ha-system-ronitor\logs"
.\target\release\ha-system-ronitor.exe service install --start-mode manual
.\target\release\ha-system-ronitor.exe service install --in-place --config-dir "C:\ProgramData\ha-system-ronitor\config" --log-dir "C:\ProgramData\ha-system-ronitor\logs"
.\target\release\ha-system-ronitor.exe service stop
.\target\release\ha-system-ronitor.exe service restart
.\target\release\ha-system-ronitor.exe service uninstall
```

安装后的 `config.toml` 示例：

```toml
[mqtt]
host = "10.0.0.1"
port = 1883
username = "homeassistant"
password = "change-me"

[home_assistant]
discovery_prefix = "homeassistant"
status_topic = "homeassistant/status"
topic_prefix = "monitor/system"

[sampling.cpu]
interval_secs = 1
smoothing_window = 5
max_silence_secs = 30

[sampling.gpu]
interval_secs = 1
max_silence_secs = 30

[sampling.memory]
interval_secs = 5
max_silence_secs = 120

[sampling.disk]
interval_secs = 30
max_silence_secs = 900

[thresholds.cpu]
usage_pct = 1.0

[thresholds.gpu]
usage_pct = 1.0
memory_change_mib = 8

[thresholds.memory]
change_mib = 8

[thresholds.disk]
change_mib = 32

[shutdown]
enable_button = false
payload = "shutdown"
dry_run = false
```

## 项目结构

- `src/main.rs`：程序入口
- `src/lib.rs`：crate 根模块，对外导出内部模块
- `src/app/`：应用运行时编排
- `src/config/`：CLI、`config.toml` 与环境变量配置处理
- `src/device/`：设备身份与 MQTT 主题结构
- `src/system/`：系统指标采集与数据模型
- `src/integrations/home_assistant/`：Home Assistant 自动发现集成
- `src/integrations/mqtt/`：MQTT 连接与发布辅助逻辑
- `src/shared/`：通用工具函数

## 环境要求

- 支持 Rust Edition 2024 的稳定版 Rust 工具链
- 已运行并可被 Home Assistant 使用的 MQTT Broker
- 已启用 MQTT 集成的 Home Assistant

## Nix Flake

当前仓库会导出：

- `packages.<system>.default`：`ha-system-ronitor` 二进制包
- `apps.<system>.default`：`nix run` 的默认入口
- `nixosModules.default`：可复用的 NixOS 模块

本地构建：

```bash
nix build .#default
```

直接通过环境变量运行：

```bash
HA_MONITOR_MQTT_HOST=127.0.0.1 nix run .#default
```

在其他 NixOS Flake 中引用：

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    ha-system-ronitor.url = "github:zeus-x99/ha-system-ronitor";
  };

  outputs = { nixpkgs, ha-system-ronitor, ... }: {
    nixosConfigurations.router = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ha-system-ronitor.nixosModules.default
        {
          services.ha-system-ronitor = {
            enable = true;
            mqtt.host = "127.0.0.1";
            mqtt.port = 1883;
            mqtt.username = "homeassistant";
            environmentFile = "/run/secrets/ha-system-ronitor.env";
            deviceName = "Router System Monitor";
            topicPrefix = "monitor/system";
          };
        }
      ];
    };
  };
}
```

示例密钥文件：

```bash
HA_MONITOR_MQTT_PASSWORD=your-password
```

如果你的 MQTT Broker 允许匿名访问，可以同时省略 `mqtt.username` 和密码。

## 运行方式

先创建本地配置文件：

```powershell
Copy-Item config.example.toml config.toml
```

`config.example.toml` 中为每一个配置项都写了中文注释。

然后根据你的环境修改配置文件，再启动程序：

```powershell
cargo run --release
```

如果需要，你仍然可以使用系统环境变量或 CLI 参数覆盖 `config.toml` 中的值。

## 不使用 config.toml 直接运行

可以直接使用环境变量：

```powershell
$env:HA_MONITOR_MQTT_HOST="192.168.1.10"
$env:HA_MONITOR_MQTT_PORT="1883"
$env:HA_MONITOR_MQTT_USERNAME="mqtt-user"
$env:HA_MONITOR_MQTT_PASSWORD="mqtt-password"
cargo run --release
```

也可以直接传 CLI 参数：

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

## 配置项说明

| 参数 | 环境变量 | 默认值 | 说明 |
| --- | --- | --- | --- |
| `--mqtt-host` | `HA_MONITOR_MQTT_HOST` | 无 | MQTT Broker 主机地址 |
| `--mqtt-port` | `HA_MONITOR_MQTT_PORT` | `1883` | MQTT Broker 端口 |
| `--mqtt-username` | `HA_MONITOR_MQTT_USERNAME` | 无 | MQTT 用户名 |
| `--mqtt-password` | `HA_MONITOR_MQTT_PASSWORD` | 无 | MQTT 密码 |
| `--discovery-prefix` | `HA_MONITOR_DISCOVERY_PREFIX` | `homeassistant` | MQTT 自动发现前缀 |
| `--home-assistant-status-topic` | `HA_MONITOR_HOME_ASSISTANT_STATUS_TOPIC` | `homeassistant/status` | Home Assistant birth 主题 |
| `--topic-prefix` | `HA_MONITOR_TOPIC_PREFIX` | `monitor/system` | 状态与可用性主题前缀 |
| `--node-id` | `HA_MONITOR_NODE_ID` | 基于主机名生成 | 设备稳定 node_id |
| `--device-name` | `HA_MONITOR_DEVICE_NAME` | `<主机名> System Monitor` | HA 中显示的设备名 |
| `--enable-shutdown-button` | `HA_MONITOR_ENABLE_SHUTDOWN_BUTTON` | `false` | 是否暴露关机按钮 |
| `--shutdown-payload` | `HA_MONITOR_SHUTDOWN_PAYLOAD` | `shutdown` | 触发关机时要求收到的 MQTT payload |
| `--shutdown-dry-run` | `HA_MONITOR_SHUTDOWN_DRY_RUN` | `false` | 仅记录关机动作，不真正执行关机 |
| `--cpu-interval-secs` | `HA_MONITOR_CPU_INTERVAL_SECS` | `1` | CPU 发布间隔 |
| `--gpu-interval-secs` | `HA_MONITOR_GPU_INTERVAL_SECS` | `1` | GPU 发布间隔 |
| `--memory-interval-secs` | `HA_MONITOR_MEMORY_INTERVAL_SECS` | `5` | 内存发布间隔 |
| `--disk-interval-secs` | `HA_MONITOR_DISK_INTERVAL_SECS` | `30` | 磁盘发布间隔 |
| `--cpu-change-threshold-pct` | `HA_MONITOR_CPU_CHANGE_THRESHOLD_PCT` | `1.0` | CPU 最小发布变化阈值 |
| `--gpu-usage-change-threshold-pct` | `HA_MONITOR_GPU_USAGE_CHANGE_THRESHOLD_PCT` | `1.0` | GPU 使用率最小发布变化阈值 |
| `--gpu-memory-change-threshold-mib` | `HA_MONITOR_GPU_MEMORY_CHANGE_THRESHOLD_MIB` | `8` | GPU 显存最小发布变化阈值 |
| `--memory-change-threshold-mib` | `HA_MONITOR_MEMORY_CHANGE_THRESHOLD_MIB` | `8` | 内存最小发布变化阈值 |
| `--disk-change-threshold-mib` | `HA_MONITOR_DISK_CHANGE_THRESHOLD_MIB` | `32` | 磁盘最小发布变化阈值 |
| `--cpu-smoothing-window` | `HA_MONITOR_CPU_SMOOTHING_WINDOW` | `5` | CPU 滑动平均窗口大小 |
| `--cpu-max-silence-secs` | `HA_MONITOR_CPU_MAX_SILENCE_SECS` | `30` | CPU 最大发布静默时间 |
| `--gpu-max-silence-secs` | `HA_MONITOR_GPU_MAX_SILENCE_SECS` | `30` | GPU 最大发布静默时间 |
| `--memory-max-silence-secs` | `HA_MONITOR_MEMORY_MAX_SILENCE_SECS` | `120` | 内存最大发布静默时间 |
| `--disk-max-silence-secs` | `HA_MONITOR_DISK_MAX_SILENCE_SECS` | `900` | 磁盘最大发布静默时间 |

## Home Assistant 接入

1. 在 Home Assistant 中启用 MQTT 集成。
2. 确认 Home Assistant 能连接到和本程序相同的 MQTT Broker。
3. 启动本程序或 Windows 服务。
4. 打开 `设置 -> 设备与服务 -> MQTT`，设备应会自动出现。

不需要手写任何 YAML 传感器定义。

当前实现会把设备级 discovery 保留发布到 `homeassistant/device/<node_id>/config`，同时也会自动发布旧版逐实体 discovery 主题的迁移标记，用于清理历史残留。

如果你要启用关机按钮，可以这样设置：

```toml
[shutdown]
enable_button = true
payload = "shutdown"
dry_run = true
```

在 Home Assistant 中验证按钮行为正常后，再改为：

```toml
[shutdown]
enable_button = true
payload = "shutdown"
dry_run = false
```

然后重启程序或服务。

## 开发命令

```powershell
cargo fmt
cargo check
cargo clippy --all-targets --all-features
```

Windows PawnIO CPU 温度探测示例：

```powershell
cargo run --example windows_pawnio_temp_probe
cargo run --example windows_pawnio_temp_probe -- --count 60 --interval-ms 1000
```

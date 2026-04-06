# ha-system-ronitor

`ha-system-ronitor` 是一个使用 Rust 编写的跨平台系统监控程序，通过 MQTT Device Discovery 将主机指标接入 Home Assistant。

项目当前重点是：

- 目录结构清晰，方便后续继续扩展
- `config.toml` 作为唯一业务配置来源
- Home Assistant 使用设备级 MQTT discovery
- Windows 服务化部署友好
- Windows 上通过 PawnIO 读取 AMD CPU 温度
- NVIDIA GPU 通过 `nvml-wrapper` 采集

## 当前发布的指标

- CPU
  - `cpu_usage`
  - `cpu_package_temp`
  - `cpu_model`
  - `os_version`
- Uptime
  - `uptime`
- GPU
  - `gpu_name`
  - `gpu_usage`
  - `gpu_temperature`
  - `gpu_memory_used`
  - `gpu_memory_available`
  - `gpu_memory_total`
  - `gpu_memory_usage`
- 内存
  - `memory_used`
  - `memory_total`
  - `memory_usage`
- 网络
  - `network_download_rate`
  - `network_upload_rate`
  - `network_total_download`
  - `network_total_upload`
  - 如果配置了 `network.include_interfaces`，还会额外发布对应网卡的同名分项实体
- 磁盘
  - 每个挂载点发布 `used` / `available` / `total` / `usage`
- 可选控制
  - `shutdown_host`

## 配置原则

- `config.toml` 是唯一业务配置来源
- 不再支持通过环境变量或普通 CLI 参数覆盖 `mqtt`、`sampling`、`thresholds`、`shutdown` 等业务配置
- `--config-dir` 和 `--log-dir` 只用于告诉程序去哪里找配置、把日志写到哪里
- Windows 服务和 Nix 模块也遵循同样规则

## 快速开始

1. 复制示例配置：

```powershell
Copy-Item config.example.toml config.toml
```

2. 修改 `config.toml`：

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

[cpu]
enabled = true
sampling_interval_secs = 1
usage_threshold_pct = 1.0

[network]
enabled = true
sampling_interval_secs = 1
include_interfaces = ["Ethernet", "Wi-Fi"]
rate_change_threshold_bps = 10240
total_change_threshold_bytes = 10240
```

3. 启动：

```powershell
cargo run --release
```

如果 `config.toml` 不存在，程序现在会直接报错，而不是再尝试从环境变量或普通 CLI 参数拼配置。

## 刷新策略

- CPU：默认每 `1` 秒采样一次
- GPU：默认每 `1` 秒采样一次
- 内存：默认每 `5` 秒采样一次
- 网络：默认每 `1` 秒采样一次
- Uptime：默认每 `300` 秒发布一次
- 磁盘：默认每 `30` 秒采样一次

所有指标都会在启动或 MQTT 重连后立即发布一次。

后续正常运行时，只有当数值变化达到阈值时才会再次发布：

- CPU / GPU 使用率：默认 `1.0%`
- GPU / 内存变化：默认 `8 MiB`
- 磁盘变化：默认 `32 MiB`

## 网络流量

- 当前通过 `sysinfo` 采集网络流量，支持跨平台
- `network_download_rate` / `network_upload_rate` 表示当前上下行速率，单位是 `B/s`
- `network_total_download` / `network_total_upload` 表示自系统启动以来的累计上下行字节数
- 如果配置了 `network.include_interfaces`，程序会：
  - 只统计这些白名单网卡
  - 额外发布每张网卡自己的上下行速率和累计流量

例如：

```toml
[network]
include_interfaces = ["Ethernet", "Wi-Fi"]
```

这样会额外出现类似这些实体：

- `network_ethernet_download_rate`
- `network_ethernet_upload_rate`
- `network_ethernet_total_download`
- `network_ethernet_total_upload`
- `network_wi_fi_download_rate`
- `network_wi_fi_upload_rate`
- `network_wi_fi_total_download`
- `network_wi_fi_total_upload`

## 温度与 GPU 支持

### CPU 温度

- Linux / 其他支持 `sysinfo` 组件传感器的平台：尽力从标准传感器读取
- Windows：当前通过 PawnIO + `AMDFamily17.bin` 模块读取 AMD Zen 平台 CPU 温度

说明：

- 目前只发布一个整包温度：`cpu_package_temp`
- 如果平台不支持或权限不足，该实体仍会存在，但值可能为空

### Windows 上的 PawnIO

- Windows 构建会自动打包 `vendor/pawnio/windows` 下的运行时文件
- 运行时优先从程序目录附近查找 `PawnIOLib.dll`
- 如果本地同时提供了 `PawnIO_setup.exe`，程序在缺少 DLL 时会尝试静默本地安装
- PawnIO 自动安装可以通过 `HA_MONITOR_PAWNIO_AUTO_INSTALL=false` 禁用

注意：

- Windows 上读取 `cpu_package_temp` 通常需要管理员权限，或者以 `LocalSystem` 方式运行

### GPU

- Windows / Linux 上的 NVIDIA GPU：通过 `nvml-wrapper`
- Linux 上的非 NVIDIA GPU：会尽量使用 sysfs 路径读取基础 GPU 信息
- 如果当前平台拿不到 GPU 数据，对应实体不会创建或不会有值

## Home Assistant 接入

1. 在 Home Assistant 中启用 MQTT 集成
2. 确认 Home Assistant 与本程序连接的是同一个 MQTT Broker
3. 启动本程序或启动 Windows 服务
4. 打开 `设置 -> 设备与服务 -> MQTT`

程序会通过设备级 MQTT discovery 发布到：

```text
homeassistant/device/<node_id>/config
```

当前实现只维护这一套新的设备级 discovery，不再继续维护旧版逐实体 discovery 兼容层。

运行时会遵循 Home Assistant 官方推荐方式：

- 使用 `homeassistant/device/.../config` 设备级 discovery
- 订阅 `homeassistant/status` birth 消息，在 Home Assistant 重连后重新发布 discovery
- 不在正常运行路径里反复执行旧 discovery 清理

如果未来需要从旧的单实体 discovery 迁移，建议按 Home Assistant 官方 MQTT 文档的一次性迁移流程单独处理，而不是放进主循环里长期执行。

如果要启用关机按钮，可以这样设置：

```toml
[shutdown]
enable_button = true
payload = "shutdown"
cancel_payload = "cancel"
delay_secs = 30
dry_run = true
```

这时 Home Assistant 会出现两个按钮：

- `shutdown_host`：收到命令后先进入延迟倒计时，再执行真正关机
- `cancel_shutdown`：在倒计时期间取消这次待执行的关机

如果启用了延迟关机，还会额外发布一个状态值：

- `shutdown_remaining_secs`：未处于待关机状态时恒为 `0`，处于待关机状态时显示剩余倒计时秒数

如果你想保持原来的“立即关机”，可以把 `delay_secs` 设为 `0`。

验证按钮行为正常后，再改成真正执行关机：

```toml
[shutdown]
enable_button = true
payload = "shutdown"
cancel_payload = "cancel"
delay_secs = 30
dry_run = false
```

## Windows 服务

Windows 下既可以作为普通控制台程序运行，也可以安装成服务。

推荐目录布局：

- 程序目录：`C:\Program Files\ha-system-ronitor`
- 配置目录：`C:\ProgramData\ha-system-ronitor\config`
- 日志目录：`C:\ProgramData\ha-system-ronitor\logs`

安装示例：

```powershell
cargo build --release
.\target\release\ha-system-ronitor.exe service install
.\target\release\ha-system-ronitor.exe service start
.\target\release\ha-system-ronitor.exe service status
```

常用命令：

```powershell
.\target\release\ha-system-ronitor.exe service install --binary-dir "D:\Apps\ha-system-ronitor" --config-dir "D:\Data\ha-system-ronitor\config" --log-dir "D:\Data\ha-system-ronitor\logs"
.\target\release\ha-system-ronitor.exe service install --start-mode manual
.\target\release\ha-system-ronitor.exe service install --in-place --config-dir "C:\ProgramData\ha-system-ronitor\config" --log-dir "C:\ProgramData\ha-system-ronitor\logs"
.\target\release\ha-system-ronitor.exe service stop
.\target\release\ha-system-ronitor.exe service restart
.\target\release\ha-system-ronitor.exe service uninstall
```

补充说明：

- 服务安装时会自动准备 `config.toml`
- 服务启动命令只会显式带上 `--config-dir` 和 `--log-dir`
- 服务默认以 `LocalSystem` 运行，这也更适合 PawnIO 温度读取

## Nix Flake

仓库导出了：

- `packages.<system>.default`
- `apps.<system>.default`
- `nixosModules.default`

本地构建：

```bash
nix build .#default
```

NixOS 模块示例：

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
            mqttPasswordFile = "/run/secrets/ha-system-ronitor-mqtt-password";
            settings = {
              mqtt = {
                host = "127.0.0.1";
                port = 1883;
                username = "homeassistant";
              };
              home_assistant = {
                discovery_prefix = "homeassistant";
                status_topic = "homeassistant/status";
                topic_prefix = "monitor/system";
              };
              device = {
                node_id = "router";
                name = "Router System Monitor";
              };
              cpu = {
                enabled = true;
                sampling_interval_secs = 1;
                usage_threshold_pct = 1.0;
              };
              gpu = {
                enabled = true;
                sampling_interval_secs = 1;
                usage_threshold_pct = 1.0;
                memory_change_threshold_mib = 8;
              };
              memory = {
                enabled = true;
                sampling_interval_secs = 5;
                change_threshold_mib = 8;
              };
              uptime = {
                enabled = true;
                sampling_interval_secs = 300;
              };
              disk = {
                enabled = true;
                sampling_interval_secs = 30;
                change_threshold_mib = 32;
                include_paths = [ "/" "/mnt/data" ];
              };
              network = {
                enabled = true;
                sampling_interval_secs = 1;
                include_interfaces = [ "Ethernet" "Wi-Fi" ];
                rate_change_threshold_bps = 10240;
                total_change_threshold_bytes = 10240;
              };
              shutdown = {
                enable_button = false;
                payload = "shutdown";
                dry_run = false;
              };
            };
          };
        }
      ];
    };
  };
}
```

说明：

- Nix 模块会把 `settings` 渲染为 `config.toml`
- MQTT 密码建议通过 `mqttPasswordFile` 提供，避免把密钥写进 Nix store
- 设置了 `mqttPasswordFile` 后，模块会在服务启动前生成运行时 `config.toml`，并显式通过 `--config-dir` 指向它
- `environmentFile` 只适合传递运行时环境变量，比如 `RUST_LOG` 或 `HA_MONITOR_PAWNIO_AUTO_INSTALL`
- 不再通过 `environmentFile` 覆盖业务配置字段

## 项目结构

- `src/main.rs`：程序入口
- `src/app/`：运行时主循环与 MQTT 编排
- `src/config/`：配置目录定位与 `config.toml` 读取
- `src/device/`：设备身份与主题构造
- `src/system/`：CPU / GPU / 内存 / 磁盘采集
- `src/integrations/home_assistant/`：Home Assistant discovery
- `src/integrations/mqtt/`：MQTT 发布逻辑
- `src/shared/`：共享工具函数
- `src/windows_service_host.rs`：Windows 服务安装与托管逻辑

## 开发命令

```powershell
cargo fmt
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
```

Windows PawnIO 温度探测示例：

```powershell
cargo run --example windows_pawnio_temp_probe
```

MQTT 调试示例：

```powershell
cargo run --example mqtt_peek
cargo run --example mqtt_publish -- "monitor/system/test" "hello"
```

## Metric Switches

Each metric group now keeps its own `enabled`, sampling, and threshold settings together.
Disabled groups are removed from MQTT discovery and no longer publish runtime updates.
For disk metrics, `include_paths` must be configured or the disk group stays disabled.

```toml
[host]
enabled = true

[cpu]
enabled = true
sampling_interval_secs = 1
usage_threshold_pct = 1.0

[gpu]
enabled = true
sampling_interval_secs = 1
usage_threshold_pct = 1.0
memory_change_threshold_mib = 8

[memory]
enabled = false
sampling_interval_secs = 5
change_threshold_mib = 8

[uptime]
enabled = true
sampling_interval_secs = 300

[disk]
enabled = true
sampling_interval_secs = 30
change_threshold_mib = 32
include_paths = ["/", "/mnt/data"]

[network]
enabled = false
sampling_interval_secs = 1
rate_change_threshold_bps = 10240
total_change_threshold_bytes = 10240
```

## Tencent Cloud Lighthouse

This project can optionally publish Tencent Cloud Lighthouse traffic package usage into the same
Home Assistant device via MQTT discovery.

Add these fields to `config.toml` when you want to enable it:

```toml
[lighthouse]
enabled = true
sampling_interval_secs = 300
secret_id = "AKID..."
secret_key = "..."
region = "ap-chengdu"
instance_id = "lhins-xxxxxxxx"
# session_token = "..." # optional for STS credentials
# endpoint = "lighthouse.tencentcloudapi.com"
```

When enabled, the integration publishes these additional sensors:

- `lighthouse_instance_id`
- `lighthouse_package_id`
- `lighthouse_used`
- `lighthouse_total`
- `lighthouse_remaining`
- `lighthouse_overflow`
- `lighthouse_usage`
- `lighthouse_status`
- `lighthouse_cycle_start`
- `lighthouse_cycle_end`
- `lighthouse_deadline`

Notes:

- `region` must match the Lighthouse instance region exactly, such as `ap-chengdu`.
- Use a sub-account or temporary STS credentials with minimum permissions where possible.
- If you already tested the API with credentials that were exposed in chat or logs, rotate them before production use.

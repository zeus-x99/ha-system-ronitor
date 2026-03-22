# windows-drivers-rs CPU temperature attempt

Date: 2026-03-16
Host: Windows

## Goal

Try a Windows-native CPU temperature path based on `windows-drivers-rs`, without using the bundled LibreHardwareMonitor runtime approach.

## What was verified on this machine

- Rust toolchain is installed and current enough for the official samples.
- `cargo-make` was installed successfully.
- LLVM `17.0.6` was installed successfully.
- WDK `10.1.26100.6584` was installed successfully.
- The required kernel headers are now present, including:
  - `C:\Program Files (x86)\Windows Kits\10\Include\10.0.26100.0\km\crt`
- User environment variables were configured:
  - `LIBCLANG_PATH=C:\Program Files\LLVM\bin`
  - user `Path` now includes `C:\Program Files\LLVM\bin`

## Attempt performed

The official Microsoft sample repository was cloned locally:

- `C:\Users\zeus\Desktop\tmp\windows-rust-driver-samples-try`

Then the official KMDF echo sample was built:

```powershell
cd C:\Users\zeus\Desktop\tmp\windows-rust-driver-samples-try\general\echo\kmdf\driver\DriverSync
cargo build
```

Build result:

- build now succeeds
- `cargo make` for the official sample also succeeds after WDK + LLVM were installed

## CPU temperature probe added

A Windows-only Rust probe was added to this project:

- `C:\Users\zeus\Desktop\tmp\ha-system-ronitor\examples\windows_thermal_probe.rs`
- `C:\Users\zeus\Desktop\tmp\ha-system-ronitor\examples\windows_kmdf_temp_client.rs`
- `C:\Users\zeus\Desktop\tmp\ha-system-ronitor\experiments\windows_kmdf_cpu_temp_driver`

Run it with:

```powershell
cargo run --example windows_thermal_probe
```

If the current terminal still cannot find `clang`, load the helper first:

```powershell
powershell -ExecutionPolicy Bypass -File C:\Users\zeus\Desktop\tmp\ha-system-ronitor\scripts\enter-driver-env.ps1
```

Or use the command wrapper that injects the LLVM environment for one cargo run:

```powershell
C:\Users\zeus\Desktop\tmp\ha-system-ronitor\scripts\cargo-driver-build.cmd run --example windows_thermal_probe
```

Probe result on this machine:

- no `GUID_DEVICE_THERMAL_ZONE` device was exposed by Windows
- so the standard thermal zone interface currently provides no readable CPU temperature path on this host
- the experimental KMDF driver now builds and packages successfully, but it currently returns a placeholder `no backend` response until a real kernel backend is implemented

## Why this still blocks real CPU package temperature work

`windows-drivers-rs` is a Rust driver development stack, not a ready-made CPU temperature API. To use it for a real CPU package temperature on this hardware, we would still need:

1. a working WDK/eWDK build environment
2. a kernel or framework driver written in Rust
3. a user-mode client that talks to that driver through IOCTL
4. a real temperature source behind the driver

The toolchain part is now fixed, but reading CPU temperature is still hardware-specific:

- the generic Windows thermal model is centered on ACPI thermal zones
- thermal zones are not guaranteed to be the CPU package temperature sensor
- on this desktop, no standard thermal zone device is exposed at all
- on many desktops, the useful CPU package temperature still comes from vendor/private hardware paths

## Practical next step

The next implementation milestone should be:

- install the experimental driver package and validate the IOCTL round-trip end to end
- replace the placeholder backend in the driver with a real kernel backend
- then try a lower-level ACPI or vendor-specific path instead of relying on thermal zones

## Current blocking point

The build and package pipeline works, but this machine still does not expose a real CPU temperature yet.

What was confirmed after packaging:

- `pnputil /add-driver ... /install` currently fails in the normal terminal with `Access is denied`
- `devgen.exe /add /hardwareid root\HA_CPU_TEMP_KMDF` also fails with `0x00000005`
- the generated catalog is now trusted for the current user after importing `WDRLocalTestCert.cer`
- the experimental driver backend still returns a placeholder `STATUS_NO_BACKEND` response even after install, until a real sensor backend is implemented

So the current state is:

- `windows-drivers-rs` toolchain: working
- experimental KMDF package: working
- driver installation from this non-elevated shell: blocked
- real CPU temperature reading: still not available

## Prepared install helper

To make the next step repeatable, the repo now includes:

- `C:\Users\zeus\Desktop\tmp\ha-system-ronitor\scripts\install-kmdf-temp-driver.ps1`
- `C:\Users\zeus\Desktop\tmp\ha-system-ronitor\scripts\install-kmdf-temp-driver.cmd`

What the installer does:

1. checks that the shell is elevated
2. imports `WDRLocalTestCert.cer` into `Root` and `TrustedPublisher`
3. runs `pnputil /add-driver ... /install`
4. removes stale `SWD\DEVGEN\...` and `ROOT\DEVGEN\...` test devices from older attempts
5. removes duplicate installed `ROOT\SAMPLE\000x` devices from older test runs
6. creates or updates the persistent root-enumerated device with `devcon install` or `devcon update`
7. prints the current device state and warns if `TESTSIGNING` does not appear to be enabled

Why the installer now uses `devcon` for the final bind:

- `devgen.exe /add` defaults to the `SWD` bus
- `SWD` devices disconnect after reboot, so they are not suitable for this experiment
- `devgen.exe /add /bus ROOT` does create a persistent root device, but it only creates the devnode
- `devcon install <inf> root\HA_CPU_TEMP_KMDF` creates the persistent root device and binds the INF in one step
- repeated `devcon install` runs can create multiple `ROOT\SAMPLE\000x` instances, so the helper now cleans duplicates first
- after that, `devcon update` can be used for repeat installs against the same hardware ID

Recommended command from an Administrator terminal:

```powershell
C:\Users\zeus\Desktop\tmp\ha-system-ronitor\scripts\install-kmdf-temp-driver.cmd
```

Then validate the user-mode side with:

```powershell
cargo run --example windows_kmdf_temp_client
```

For a quick live validation against Ryzen Master, use:

```powershell
cargo run --example windows_kmdf_temp_watch -- --interval-ms 1000 --count 60
```

That prints a 60-second temperature curve with per-sample deltas so you can compare trend and range side-by-side.

## Recommendation

For this project, `windows-drivers-rs` should still be treated as an experimental branch, not the primary CPU temperature source yet.

## Latest backend experiment

After the KMDF device and IOCTL path were proven, the experimental driver was upgraded from a pure placeholder to a first real AMD backend attempt:

- the driver now checks whether the CPU is an AMD Zen-family CPU
- it scans PCI buses for the AMD Data Fabric function 3 host bridge
- it then tries a Zen SMN package-temperature read using the same register family used by Linux `k10temp`
- because this Windows root-driver experiment does not have Linux `amd_smn_read`, it currently uses the older northbridge index fallback pattern also seen in `zenpower`

Current backend behavior:

- `backend=1` means the AMD NB index path was attempted
- `status=0` means the temperature read decoded successfully
- any non-zero status means the driver still did not get a usable value

Primary implementation references used for this experiment:

- Linux `k10temp`: `drivers/hwmon/k10temp.c`
- `zenpower`: fallback NB-index SMN read path
- Microsoft `HalGetBusDataByOffset` / `HalSetBusDataByOffset` documentation

This backend is still experimental and has not yet been treated as production-safe. In particular, it is trying to reproduce a Linux-oriented fallback path on Windows, so success is hardware-dependent.

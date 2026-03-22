# Windows AMD private CPU temperature path notes

Date: 2026-03-16
Host CPU: AMD Ryzen 7 9800X3D

## Goal

Check whether there is an official AMD private path we can realistically use from this Windows Rust project to read CPU temperature.

## What was checked

- local host CPU identity confirms this is an AMD consumer desktop CPU
- the Windows thermal zone path on this host exposes no readable thermal device
- official AMD management stacks were checked as the next vendor-specific option

## Official AMD findings

### APML

AMD's official APML stack is documented as:

- an out-of-band or sideband management interface
- delivered as a Linux package / library
- targeted at server management flows

Official source:

- https://www.amd.com/en/developer/e-sms/apml-library.html

### E-SMI

AMD's official E-SMI stack is documented as:

- a Linux library and command line tool
- focused on in-band management for AMD Instinct GPUs and AMD EPYC CPUs

Official source:

- https://www.amd.com/en/developer/e-sms/e-smi.html

## Practical conclusion

For this Windows desktop host:

- no standard Windows thermal zone path is available
- the official AMD private management stacks found are Linux-oriented, not Windows desktop SDKs
- so there is currently no official AMD Windows user-mode temperature API we can plug into this Rust project directly

This means a real Windows AMD-private temperature path would most likely require an unofficial low-level driver/backend implemented by us.

That last sentence is an inference from the official sources above plus the local host behavior.

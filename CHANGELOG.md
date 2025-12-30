# Changelog

All notable changes to Splax OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- GitHub Actions CI/CD pipeline for automated builds
- Multi-architecture support: x86_64, aarch64, riscv64
- Capability-based security system (S-CAP)
- Microkernel architecture with userspace services
- Zero-copy IPC channels (S-LINK)
- Service registry (S-ATLAS)
- WASM runtime (S-WAVE)
- Object storage service (S-STORAGE)
- Network stack with IPv4/IPv6 support
- USB 3.0 xHCI driver
- GPU driver with basic rendering
- Sound subsystem with Intel HDA and AC97
- Block device drivers: NVMe, AHCI, VirtIO
- Filesystem support: ext4, FAT32
- ACPI support for power management
- SMP support for multi-core systems
- Deterministic scheduler

### Architecture Support
- **x86_64**: Full support with UEFI/BIOS boot
- **aarch64**: Core support with device tree
- **riscv64**: Initial support with SBI

### Services
- `S-INIT`: System initialization service
- `S-ATLAS`: Service registry and discovery
- `S-CAP`: Capability management service
- `S-GATE`: External gateway/firewall
- `S-LINK`: IPC message routing
- `S-STORAGE`: Object storage backend
- `S-NET`: Network stack service
- `S-GPU`: Graphics service
- `S-DEV`: Device management service

### Tools
- `S-TERM`: Terminal emulator
- `S-CODE`: Code editor

### Security
- No root user - capability-only access control
- Cryptographic capability tokens
- Microkernel isolation for drivers
- WASM sandboxing for applications

## [0.1.0] - 2024-12-30

### Added
- Initial release
- Basic kernel functionality
- Bootloader with Limine support
- Serial console shell
- Memory management (physical and virtual)
- Basic interrupt handling
- Initial driver framework

---

## Version History

| Version | Date | Highlights |
|---------|------|------------|
| 0.1.0 | 2024-12-30 | Initial release |

## Upgrade Notes

### From 0.0.x to 0.1.0
- First public release, no upgrade path needed

---

[Unreleased]: https://github.com/splax-s/splax_os/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/splax-s/splax_os/releases/tag/v0.1.0

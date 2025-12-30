# Changelog

All notable changes to Splax OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Planned
- WiFi driver improvements
- GPU hardware acceleration
- Service hot-reloading
- Live kernel updates
- Distributed IPC across machines

---

## [0.1.0] - 2024-12-30

### 🎉 First Public Release

Splax OS v0.1.0 marks the first public release of our capability-secure, distributed-first operating system.

### Highlights
- **Microkernel Architecture**: 53KB stripped kernel binary
- **Multi-Architecture**: x86_64, aarch64, riscv64 support
- **Zero-Copy IPC**: Fast path with <500ns latency target
- **Async IPC**: Non-blocking message passing with pending operation queuing
- **Service Auto-Restart**: Configurable restart policies with rate limiting

### Kernel (S-CORE)
- Capability-based security system (S-CAP)
- Zero-copy IPC channels with fast path optimization
- Async IPC with PendingId, timeout support
- Deterministic scheduler with SMP support
- Memory manager with no overcommit policy
- IPC benchmarks via `ipcbench` shell command

### Services
- **S-ATLAS**: Service registry with health monitoring and auto-restart
  - RestartPolicy: Never, OnFailure, Always
  - ServiceSupervisor with event logging
- **S-INIT**: System initialization service
- **S-CAP**: Capability management service
- **S-GATE**: External gateway/firewall
- **S-LINK**: IPC message routing
- **S-STORAGE**: VFS and object storage
- **S-NET**: Network stack (TCP/UDP/ICMP)
- **S-GPU**: Graphics and framebuffer
- **S-DEV**: Device management

### Drivers
- **Network**: VirtIO-net, E1000, RTL8139
- **Block**: NVMe, AHCI, VirtIO-blk
- **USB**: xHCI, HID
- **Sound**: Intel HDA, AC97
- **GPU**: Framebuffer, VirtIO-GPU

### Architecture Support
- **x86_64**: Full support with UEFI/BIOS boot
- **aarch64**: Core support with device tree
- **riscv64**: Initial support with SBI

### Documentation
- Architecture documentation
- IPC and microkernel guides
- Build instructions for all platforms
- QEMU testing documentation

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

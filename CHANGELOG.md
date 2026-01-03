# Changelog

All notable changes to Splax OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Capability Revocation Engine**: Advanced capability lifecycle management with:
  - Revocation bloom filter for O(1) revocation checks
  - Delegation chain tracking with depth limits
  - Time-limited capabilities with configurable durations
  - Revocation audit log with comprehensive metadata
  - Cascade revocation for derived capabilities
  - Renewal support for renewable time-limited tokens
- **Secure Key Storage**: Kernel-level cryptographic key management with:
  - Key generation, import, derivation via HKDF
  - ChaCha20-Poly1305 encryption at rest
  - Key usage flags (ENCRYPT, DECRYPT, SIGN, VERIFY, DERIVE)
  - Key rotation with successor tracking
  - Extractable/non-extractable key policies
  - Per-process key ownership and access control
- **Operations Default trait**: Added Default implementation for capability Operations
- **S-TERM Terminal Emulator**: Full ANSI/VT100 terminal emulation with:
  - Complete CSI, OSC, DCS escape sequence parsing
  - 256-color and true color (RGB) support
  - Scrollback buffer with configurable history
  - Alternate screen buffer for full-screen apps
  - Cursor styles (block, underline, bar)
  - Character attributes (bold, italic, underline, etc.)
- **S-CODE Syntax Highlighting**: Multi-language syntax highlighting engine with:
  - Token-based highlighting (keywords, types, strings, comments, etc.)
  - Language registry for Rust, JavaScript, Python, C
  - Dark and light color themes
  - Multi-line comment and string state tracking
- **AOT WASM Compilation**: Ahead-of-time compilation for WebAssembly modules:
  - Serializable compiled module format (.swc)
  - Module cache with LRU eviction
  - Relocation support for runtime patching
  - Cross-function inlining and optimization
- **Boot-time 4GB Identity Mapping**: Extended page tables to map first 4GB of physical memory using 2MB huge pages, covering LAPIC (0xFEE00000), IOAPIC, and PCI config space
- **FSGSBASE Support**: Enabled FSGSBASE instructions (CR4 bit 16) for fast per-CPU data access via `RDGSBASE`/`WRGSBASE`
- **ACPI CPU Enumeration**: Added `cpu_count()`, `get_apic_ids()`, and `bsp_apic_id()` helper functions
- **Multi-core AP Startup**: Implemented INIT-SIPI-SIPI sequence for Application Processor startup
- **Service Launcher**: Kernel-side service spawning for microkernel mode (`kernel/src/process/service.rs`)
- **S-NATIVE Runtime Hooks**: Sandbox management functions for native code execution (`kernel/src/process/native.rs`)
- **Shared Crypto Library**: Consolidated SHA-256, SHA-512, HMAC implementations in `lib/crypto`
- **Conditional GPU Init**: GPU initialization is now feature-gated for microkernel mode

### Changed
- Boot.S now uses LLVM-compatible section directives
- GPU init only runs when `not(microkernel)` or `monolithic_gpu` feature is set
- LAPIC access works via boot-time identity mapping (no additional page table setup needed)

### Fixed
- LAPIC `current_apic_id()` return type (u32 → u8)
- CPUID inline assembly for FSGSBASE detection (rbx is LLVM reserved)
- Boot.S syntax for cross-platform LLVM assembler compatibility

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

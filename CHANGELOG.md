# Changelog

All notable changes to Splax OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **S-PKG Package Manager Enhancements**:
  - Built-in package registry with 35+ packages (vim, git, grep, curl, python, node, etc.)
  - Package search, install, remove, update, upgrade commands
  - Package info and dependency tracking
  - Repository management system
- **Vim Text Editor**: Full vi-compatible editor implementation:
  - Modal editing (normal, insert, visual, command-line modes)
  - Movement commands (h/j/k/l, w/b/e, 0/$, gg/G)
  - Editing commands (i/a/o/O, x, dd, yy, p/P)
  - Search and replace (/pattern, :s/old/new/)
  - Undo/redo support
  - Visual mode selection
  - Ex command mode (:w, :q, :wq, :e, :set)
  - Multiple buffer support
- **Shell Package Commands**: New `pkg` command for package management
- **Binary Execution**: `run` command to execute installed package binaries
- **Comprehensive Unit Test Suite**: Full test framework with:
  - Test macros (assert_eq, assert_ok, assert_err, skip)
  - Test suites for memory, capability, IPC, scheduler, filesystem, network, crypto, process
  - Test case metadata with isolation and timeout support
  - Filtered test execution by module
- **Fuzzing Infrastructure**: Coverage-guided fuzzing with:
  - Fuzz targets for WASM parser, ELF parser, network packets, filesystem, IPC, capability, crypto
  - Corpus management with coverage tracking
  - Input mutation strategies (bit flip, byte flip, arithmetic, havoc)
  - Sanitizer integration (ASAN, MSAN, UBSAN, TSAN)
  - Crash detection and categorization
- **Performance Benchmark Suite**: Comprehensive benchmarks covering:
  - IPC benchmarks (empty roundtrip, small/large messages, zero-copy)
  - Memory benchmarks (allocation, page faults, memcpy)
  - Scheduler benchmarks (context switch, mutex, spinlock)
  - Network benchmarks (packet parsing, checksum, socket lookup)
  - Storage benchmarks (4KB read/write, sequential, random)
  - Crypto benchmarks (SHA-256, AES-GCM, ChaCha20, Ed25519)
  - WASM benchmarks (call overhead, memory access, instantiation)
- **User Manual**: Complete user documentation including:
  - Getting started guide
  - Shell command reference
  - Package management
  - Service management
  - Networking guide
  - Security and capability management
  - Troubleshooting guide
- **Tutorial Series**: 8 progressive tutorials:
  - Hello World
  - File Operations
  - Capability Basics
  - Writing Your First Service
  - IPC Communication
  - Network Programming
  - Container Deployment
  - Building a Full Application
- **API Documentation**: Comprehensive API reference for:
  - System calls
  - Capability API
  - IPC API
  - File System API
  - Network API
  - Process API
  - Memory API
  - Crypto API
  - Service API
  - WASM API
- **Release Management**: Complete release infrastructure with:
  - Semantic versioning
  - Release channels (nightly, alpha, beta, RC, stable, LTS)
  - LTS policy with 3-year support
  - Security advisory process
  - Package repository structure
  - GPG signing for releases
- **Community Documentation**: Community guidelines including:
  - Communication channels (Discord, Matrix, forums, mailing lists)
  - Contribution guide
  - Governance model
  - Sponsorship information
- **Kubernetes Node Support (S-KUBELET)**: Full Kubernetes node agent with:
  - CRI shim for container runtime interface
  - CNI plugin for container networking
  - Pod lifecycle management with phases
  - Container state tracking and resource limits
  - Node info reporting and registration
  - Volume mounting and port mappings
- **S-CLUSTER Orchestrator**: Native container orchestration with:
  - Deployment management with replicas and selectors
  - Rolling update and recreate strategies
  - Horizontal Pod Autoscaler (HPA) with CPU/memory metrics
  - Service definitions with ClusterIP and NodePort types
  - Node affinity, anti-affinity, and tolerations
  - Pod templates with init containers
- **Service Discovery**: DNS-based service discovery with:
  - Internal DNS server for service resolution
  - Service registry with endpoints
  - Health checking with configurable intervals
  - A, AAAA, SRV, CNAME, TXT record support
  - Watch callbacks for service changes
  - Automatic endpoint management
- **Load Balancing**: Advanced L4/L7 load balancing with:
  - Multiple algorithms: round-robin, weighted, least connections, consistent hash
  - Session affinity with client IP, cookie, and header support
  - Circuit breaker pattern for cascade failure prevention
  - Zone-aware backend selection
  - Backend health monitoring and automatic failover
  - Connection tracking and resource limits
- **Control Flow Integrity (CFI)**: Hardware-assisted control flow protection with:
  - Shadow stack implementation with per-CPU stacks
  - Landing pad registry for indirect call validation
  - Intel CET and ARM BTI/PAC hardware support detection
  - Configurable policies: Permissive, Enforcing, Strict
  - Per-thread CFI context tracking
- **Memory Tagging Extension (MTE)**: AArch64 memory tagging for spatial safety:
  - Hardware tag generation and checking
  - Synchronous, asynchronous, and asymmetric modes
  - Tagged allocation with automatic coloring
  - MTE capability detection and runtime configuration
  - Kernel and userspace memory tagging support
- **Formal Verification Framework**: Separation logic for capability model:
  - Verified capability tokens with mathematical proofs
  - Separation assertions (disjoint, implies, resources)
  - Property verification (transitivity, non-forgeability, revocation)
  - Delegation chain verification
  - Proof obligation tracking
- **Service Mesh Integration**: Istio-compatible service mesh in kernel:
  - Sidecar proxy with transparent traffic interception
  - Client-side load balancing (round-robin, weighted, least connections)
  - Circuit breaker pattern with configurable thresholds
  - Health checking and endpoint management
  - Traffic metrics collection
- **Intel Integrated Graphics Driver**: Gen9+ Intel GPU support with:
  - Ring buffer command submission
  - GTT (Graphics Translation Table) management
  - Display pipe configuration
  - Power state management
  - Multi-display support
- **AMD GPU Driver**: RDNA/RDNA2/RDNA3 GPU support with:
  - SDMA ring buffer engine
  - GART (Graphics Address Remapping Table)
  - DCN display controller support
  - Multi-monitor configuration
  - GPU power management
- **Wayland Compositor**: Full Wayland protocol implementation with:
  - XDG shell toplevel and popup support
  - SHM buffer management for CPU rendering
  - DMA-BUF support for GPU buffers
  - Input event handling (keyboard, pointer, touch)
  - Subsurface composition
  - Double-buffered state management
- **Low-Latency Audio Engine**: Real-time audio processing with:
  - Lock-free ring buffer for zero-copy audio
  - Audio graph processing with multiple node types
  - Gain, mixer, delay, and filter nodes
  - Biquad filter implementation
  - Configurable buffer sizes and sample rates
  - Priority scheduling for audio threads
- **USB Video Class (UVC) Driver**: USB camera support with:
  - UVC descriptor parsing
  - Multiple video format support (YUYV, MJPEG, H.264)
  - Camera and processing unit controls
  - Video streaming with frame callbacks
  - Resolution and frame rate configuration

### Changed
- Security initialization now includes CFI on x86_64 and MTE on aarch64
- GPU initialization includes Intel and AMD driver probing
- Sound initialization includes low-latency audio engine setup
- USB initialization includes UVC driver registration
- Network initialization includes service mesh setup
- Capability system includes formal verification hooks
- VGA boot output shows all new subsystem status

### Fixed
- CapabilityToken now has `value()` accessor method for verification
- CfiPolicy enum uses `Enforcing` variant correctly

---

### Added (Previous)
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

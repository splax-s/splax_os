# Splax OS Roadmap

This document outlines the development roadmap for Splax OS. Our goal is to create a production-grade, capability-secure, distributed-first operating system.

## Vision

> "Where your laptop feels like a cloud region, nothing runs unless you ask, and security is built in, not bolted on."

## Current Status: Alpha

Splax OS is currently in **alpha** stage. The kernel boots, basic drivers work, and the microkernel architecture is functional. We are actively working toward a stable beta release.

---

## Phase 1: Foundation ✅ (Complete)

### Kernel Core
- [x] Multi-architecture support (x86_64, aarch64, riscv64)
- [x] Physical and virtual memory management
- [x] Interrupt handling and exception management
- [x] Basic scheduler with priority queues
- [x] Serial console and debugging infrastructure

### Boot
- [x] Limine bootloader integration
- [x] UEFI and BIOS boot support
- [x] Multiboot2 compatibility
- [x] Device tree parsing (aarch64/riscv64)

### Security Foundation
- [x] Capability token structure (S-CAP)
- [x] Basic capability checking
- [x] Process isolation

---

## Phase 2: Core Services ✅ (Complete)

### IPC & Messaging
- [x] Zero-copy IPC channels
- [x] Message passing infrastructure
- [x] Asynchronous message handling
- [x] Fast path IPC (<500ns target)
- [x] IPC benchmarks (`ipcbench` command)

### Service Framework
- [x] Service registry (S-ATLAS)
- [x] Service lifecycle management
- [x] Service health monitoring
- [x] Automatic service restart
- [x] Restart policies (Never, OnFailure, Always)
- [x] Service event logging

### Storage
- [x] Block device abstraction
- [x] NVMe driver
- [x] AHCI/SATA driver
- [x] VirtIO block driver
- [ ] Object storage API
- [ ] Content-addressed storage
- [ ] Distributed storage protocol

---

## Phase 3: Networking (Q1 2025)

### Network Stack
- [x] E1000 driver
- [x] VirtIO network driver
- [x] IPv4 support
- [x] IPv6 support
- [ ] TCP optimization
- [ ] UDP multicast
- [ ] Network namespaces

### Distributed Features
- [ ] Service mesh integration
- [ ] Capability-based network ACLs
- [ ] Encrypted inter-node communication
- [ ] Distributed capability delegation

---

## Phase 4: Userspace (Q2 2025)

### WASM Runtime
- [x] Basic WASM interpreter
- [ ] WASI implementation
- [ ] JIT compilation
- [ ] Ahead-of-time compilation
- [ ] Capability-based WASM permissions

### Native Runtime
- [x] ELF loader
- [ ] Dynamic linking
- [ ] Shared libraries
- [ ] Native sandbox

### Shell & Tools
- [x] Serial console shell
- [ ] Full terminal emulator (S-TERM)
- [ ] Code editor (S-CODE)
- [ ] Package manager

---

## Phase 5: Hardware Support (Q2-Q3 2025)

### Graphics
- [x] Basic framebuffer
- [x] VirtIO GPU
- [ ] Intel integrated graphics
- [ ] AMD GPU support
- [ ] Wayland-compatible compositor

### Audio
- [x] Intel HDA driver
- [x] AC97 driver
- [ ] Audio mixing
- [ ] Low-latency audio

### USB
- [x] xHCI (USB 3.0) controller
- [x] USB HID (keyboard/mouse)
- [ ] USB mass storage
- [ ] USB audio
- [ ] USB video

### Input
- [x] PS/2 keyboard
- [x] PS/2 mouse
- [ ] Touchpad support
- [ ] Multi-touch support

---

## Phase 6: Security Hardening (Q3 2025)

### Capability System
- [ ] Formal verification of capability model
- [ ] Capability revocation
- [ ] Capability delegation chains
- [ ] Time-limited capabilities

### Isolation
- [ ] Memory tagging (aarch64 MTE)
- [ ] Control flow integrity
- [ ] Stack canaries
- [ ] ASLR

### Cryptography
- [x] Random number generation
- [x] Hash functions (SHA-256, SHA-3)
- [x] Symmetric encryption (AES-GCM, ChaCha20-Poly1305)
- [ ] Asymmetric cryptography
- [ ] TLS support
- [ ] Secure key storage

---

## Phase 7: Cloud Native (Q4 2025)

### Container Support
- [ ] OCI-compatible runtime
- [ ] Capability-based container isolation
- [ ] Container networking
- [ ] Container storage

### Orchestration
- [ ] Kubernetes node support
- [ ] Custom orchestrator (S-CLUSTER)
- [ ] Service discovery
- [ ] Load balancing

### Observability
- [ ] Metrics collection
- [ ] Distributed tracing
- [ ] Log aggregation
- [ ] Health checks

---

## Phase 8: Stability & Polish (2026)

### Testing
- [ ] Comprehensive unit tests
- [ ] Integration test suite
- [ ] Fuzzing infrastructure
- [ ] Performance benchmarks

### Documentation
- [ ] API documentation
- [ ] Architecture guide
- [ ] User manual
- [ ] Tutorial series

### Community
- [ ] Package repository
- [ ] Community forums
- [ ] Regular releases
- [ ] LTS support

---

## Future Considerations

These items are on our radar but not yet scheduled:

- **Real-time scheduling**: Hard real-time guarantees
- **Formal verification**: Formally verified kernel core
- **Hardware security modules**: TPM and HSM integration
- **Unikernels**: Single-application kernel configurations
- **WebGPU**: GPU compute via WebGPU API
- **Persistence**: Persistent memory support
- **FPGA**: FPGA acceleration framework

---

## How to Contribute

We welcome contributions at any phase! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### Priority Areas
1. **Testing**: We need more tests everywhere
2. **Documentation**: Help us explain complex systems
3. **Drivers**: USB, audio, and graphics drivers
4. **Security**: Audit and harden the capability system

### Getting Started
- Pick an unchecked item from any phase
- Open an issue to discuss your approach
- Submit a PR with your implementation

---

## Release Schedule

| Version | Target Date | Focus | Status |
|---------|-------------|-------|--------|
| 0.1.0 | Dec 2024 | Initial alpha release | ✅ Released |
| 0.2.0 | Feb 2025 | Object storage, distributed IPC | 🔄 Next |
| 0.3.0 | Apr 2025 | WASM JIT, native sandbox | Planned |
| 0.4.0 | Jun 2025 | Graphics acceleration | Planned |
| 0.5.0 | Aug 2025 | Security hardening | Planned |
| 1.0.0 | Q1 2026 | First stable release | Planned |

---

*This roadmap is a living document and will be updated as priorities evolve.*

*Last updated: December 2024*

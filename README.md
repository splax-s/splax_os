# Splax OS

> *"Where your laptop feels like a cloud region, nothing runs unless you ask, and security is built in, not bolted on."*

[![Build Status](https://github.com/splax/splax_os/actions/workflows/ci.yml/badge.svg)](https://github.com/splax/splax_os/actions)
[![License](https://img.shields.io/badge/license-Proprietary-red.svg)](LICENSE)

⚠️ **PROPRIETARY SOFTWARE** - All rights reserved. See [LICENSE](LICENSE) for terms.

Splax OS is a **production-grade, capability-secure, distributed-first operating system** built from scratch in Rust. It reimagines operating system design with modern principles:

- **🔐 Capability-Based Security (S-CAP)**: No users, groups, or root. Every operation requires an explicit, unforgeable capability token.
- **🧱 Microkernel Architecture**: Tiny trusted kernel (~15K LOC), everything else in userspace services.
- **🌐 Distributed-First**: Designed for cloud-native workloads from day one.
- **⚡ Deterministic Execution**: Same inputs → same outputs. No swap, no overcommit.
- **🦀 Memory Safe**: 100% Rust, zero `unsafe` outside hardware abstraction.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        USERSPACE                                │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐           │
│  │ S-TERM   │ │ S-CODE   │ │ S-WAVE   │ │ S-NATIVE │           │
│  │  (CLI)   │ │ (Editor) │ │  (WASM)  │ │(Sandbox) │           │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └────┬─────┘           │
│       │            │            │            │                  │
│  ┌────┴────────────┴────────────┴────────────┴────┐            │
│  │              S-LINK (Internal Messaging)        │            │
│  └────┬────────────┬────────────┬────────────┬────┘            │
│       │            │            │            │                  │
│  ┌────┴─────┐ ┌────┴─────┐ ┌────┴─────┐ ┌────┴─────┐           │
│  │ S-ATLAS  │ │ S-GATE   │ │S-STORAGE │ │   ...    │           │
│  │(Registry)│ │(Gateway) │ │ (Objects)│ │          │           │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘           │
├─────────────────────────────────────────────────────────────────┤
│                    S-CORE (Microkernel)                         │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐           │
│  │   S-CAP  │ │   IPC    │ │ Scheduler│ │  Memory  │           │
│  │(Capabil.)│ │(Zero-cp) │ │  (Det.)  │ │ Manager  │           │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘           │
├─────────────────────────────────────────────────────────────────┤
│                      HARDWARE (x86_64 / aarch64)                │
└─────────────────────────────────────────────────────────────────┘
```

## 10 Non-Negotiable Design Constraints

1. **NO POSIX** - Fresh API design, no legacy baggage
2. **Capability-Only Security** - Every operation gated by S-CAP tokens
3. **Microkernel** - Kernel does: scheduling, memory, IPC, capabilities. Nothing else.
4. **No Global Mutable State** - All state is capability-protected
5. **Cross-Architecture** - x86_64 and aarch64 from day one
6. **Deterministic Execution** - Reproducible builds, predictable scheduling
7. **Zero-Copy IPC** - Shared memory regions with capability transfer
8. **Service-Oriented** - Everything above kernel is a restartable service
9. **Object Storage** - No hierarchical filesystem, content-addressed objects
10. **Headless-First** - CLI-first, GUI optional

## Quick Start

### Prerequisites

- Rust nightly toolchain
- QEMU for testing
- xorriso for ISO creation
- (Optional) Cross-compilation toolchains for aarch64

### Setup

```bash
# Clone the repository
git clone https://github.com/splax-s/splax_os.git
cd splax_os

# Install Rust nightly and components
rustup override set nightly
rustup component add rust-src llvm-tools-preview

# Build the kernel
cargo build -p splax_kernel --bin splax_kernel --release \
    --target x86_64-unknown-none \
    -Zbuild-std=core,alloc \
    -Zbuild-std-features=compiler-builtins-mem

# Create bootable ISO
cp target/x86_64-unknown-none/release/splax_kernel target/iso/iso_root/boot/
xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin -no-emul-boot \
    -boot-load-size 4 -boot-info-table --efi-boot boot/limine/limine-uefi-cd.bin \
    -efi-boot-part --efi-boot-image --protective-msdos-label \
    target/iso/iso_root -o target/iso/splax.iso

# Run in QEMU
qemu-system-x86_64 -cdrom target/iso/splax.iso -m 256M -serial stdio -no-reboot
```

### Development

```bash
# Check compilation (all crates)
cargo check

# Build in release mode
cargo build -p splax_kernel --bin splax_kernel --release \
    --target x86_64-unknown-none \
    -Zbuild-std=core,alloc \
    -Zbuild-std-features=compiler-builtins-mem

# Run tests
cargo test --workspace --exclude splax_kernel --exclude splax_bootloader
```

## Project Structure

```
splax_os/
├── bootloader/          # UEFI bootloader
├── kernel/              # S-CORE microkernel
│   └── src/
│       ├── arch/        # Architecture-specific (x86_64, aarch64)
│       ├── cap/         # S-CAP capability system
│       ├── ipc/         # Inter-process communication
│       ├── mm/          # Memory management
│       └── sched/       # Deterministic scheduler
├── services/            # Userspace services
│   ├── atlas/           # Service registry
│   ├── link/            # Internal messaging
│   ├── gate/            # External gateway (TCP/HTTP)
│   └── storage/         # Object storage
├── runtime/             # Execution runtimes
│   ├── wave/            # WASM runtime
│   └── native/          # Native sandbox
├── tools/               # Developer tools
│   ├── term/            # S-TERM CLI
│   └── code/            # S-CODE editor
├── tests/               # Integration tests
├── scripts/             # Build and test scripts
└── docs/                # Documentation
```

## Implementation Status

### ✅ Completed

| Component | Status | Description |
|-----------|--------|-------------|
| **S-CORE Kernel** | ✅ Done | Boots on x86_64 and aarch64, full subsystem initialization |
| **x86_64 Support** | ✅ Done | IDT, GDT, LAPIC, PIC, PIT, serial, VGA, keyboard, paging |
| **aarch64 Support** | ✅ Done | GIC, PL011 UART, Generic Timer, MMU, exceptions |
| **S-CAP** | ✅ Done | Capability tokens, grant/check/revoke, recursive revocation, audit |
| **Memory Manager** | ✅ Done | Frame allocator, heap (free-list), page tables, no overcommit |
| **Scheduler** | ✅ Done | Priority-based, SMP-aware, per-CPU work queues, IPIs |
| **S-LINK** | ✅ Done | IPC channels, zero-copy messaging, capability transfer |
| **S-ATLAS** | ✅ Done | Service registry, discovery, health monitoring |
| **S-GATE** | ✅ Done | TCP/HTTP gateway, routing, TLS, S-LINK integration |
| **S-STORAGE** | ✅ Done | VFS, content-addressed objects, deduplication |
| **S-WAVE** | ✅ Done | WASM module loading, bytecode interpreter, host functions |
| **S-TERM** | ✅ Done | CLI commands, kernel shell integration |
| **Network Stack** | ✅ Done | TCP/IP, UDP, ARP, ICMP, DNS, DHCP, SSH client, firewall, QoS |
| **Network Drivers** | ✅ Done | VirtIO-net, E1000, RTL8139, WiFi framework |
| **Block Layer** | ✅ Done | VirtIO-blk, NVMe, AHCI, partitions, I/O scheduler |
| **Filesystems** | ✅ Done | VFS, RamFS, ext4 (read), FAT32, SplaxFS, ProcFS, SysFS, DevFS |
| **Crypto** | ✅ Done | SHA-1/256/512, AES-256-CBC, ChaCha20, HMAC, HKDF, PBKDF2, RNG |
| **Sound** | ✅ Done | AC97, Intel HDA, VirtIO-snd, AudioDevice trait |
| **USB** | ✅ Done | xHCI host controller, HID (keyboard/mouse) |
| **PCI** | ✅ Done | Bus enumeration, device discovery, BAR handling, MSI/MSI-X |
| **GPU** | ✅ Done | Framebuffer, console, 2D primitives, text rendering |
| **Documentation** | ✅ Done | Architecture docs, API reference, build instructions |

### 📋 Planned
| **GUI (S-CANVAS)** | 📋 Planned | Windowing system, compositor |
| **RISC-V Support** | 📋 Planned | Third architecture target |

## Running Splax OS

### Prerequisites

- Rust nightly toolchain with `rust-src` component
- QEMU (x86_64 and optionally aarch64)
- xorriso (for ISO creation)
- Limine bootloader (included in `target/iso/`)

### Building the Kernel

```bash
# Install Rust nightly and required components
rustup override set nightly
rustup component add rust-src llvm-tools-preview

# Build for x86_64 (release mode)
cargo build -p splax_kernel --bin splax_kernel --release \
    --target x86_64-unknown-none \
    -Zbuild-std=core,alloc \
    -Zbuild-std-features=compiler-builtins-mem

# Check for compilation errors
cargo check
```

### Creating Bootable ISO

```bash
# Copy kernel to ISO structure
cp target/x86_64-unknown-none/release/splax_kernel target/iso/iso_root/boot/

# Create ISO with Limine bootloader
xorriso -as mkisofs \
    -b boot/limine/limine-bios-cd.bin \
    -no-emul-boot -boot-load-size 4 -boot-info-table \
    --efi-boot boot/limine/limine-uefi-cd.bin \
    -efi-boot-part --efi-boot-image \
    --protective-msdos-label \
    target/iso/iso_root -o target/iso/splax.iso
```

### Running in QEMU

```bash
# Basic run with serial output
qemu-system-x86_64 -cdrom target/iso/splax.iso -m 256M -serial stdio -no-reboot

# With networking (E1000)
qemu-system-x86_64 -cdrom target/iso/splax.iso -m 512M -serial stdio \
    -device e1000,netdev=net0 -netdev user,id=net0

# With VirtIO devices
qemu-system-x86_64 -cdrom target/iso/splax.iso -m 512M -serial stdio \
    -device virtio-net-pci,netdev=net0 -netdev user,id=net0 \
    -drive file=disk.img,if=virtio,format=raw
```

### Quick Start (All-in-One)

```bash
# Build, create ISO, and run
cargo build -p splax_kernel --bin splax_kernel --release \
    --target x86_64-unknown-none \
    -Zbuild-std=core,alloc \
    -Zbuild-std-features=compiler-builtins-mem && \
cp target/x86_64-unknown-none/release/splax_kernel target/iso/iso_root/boot/ && \
xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin -no-emul-boot \
    -boot-load-size 4 -boot-info-table --efi-boot boot/limine/limine-uefi-cd.bin \
    -efi-boot-part --efi-boot-image --protective-msdos-label \
    target/iso/iso_root -o target/iso/splax.iso 2>/dev/null && \
qemu-system-x86_64 -cdrom target/iso/splax.iso -m 256M -serial stdio -no-reboot
```

### Kernel Shell Commands

Once booted, use the interactive shell:

| Command | Description |
|---------|-------------|
| `help` | Show available commands |
| `services` | List registered services |
| `channels` | List IPC channels |
| `cap` | Show capability system status |
| `memory` | Show memory usage |
| `wave` | Show S-WAVE WASM runtime status |
| `version` | Show version info |
| `clear` | Clear screen |

## Roadmap

### Phase 1: Foundation (Weeks 1-4) ✅
- [x] Project structure
- [x] Build system
- [x] Multiboot2 bootloader
- [x] Kernel entry (x86_64)
- [x] Basic memory management
- [x] S-CAP implementation

### Phase 2: Kernel Core (Weeks 5-8) ✅
- [x] Deterministic scheduler
- [x] IPC channels
- [x] VGA display
- [x] Keyboard input
- [x] Interrupt handling

### Phase 3: Services (Weeks 9-12) ✅
- [x] S-ATLAS service registry
- [x] S-LINK messaging
- [x] S-STORAGE objects
- [x] S-GATE networking

### Phase 4: Runtimes & Tools (Weeks 13-16) ✅
- [x] S-WAVE WASM runtime
- [x] Host function bindings
- [x] S-TERM CLI
- [x] Kernel shell integration

### Phase 5: Polish & Ports (Weeks 17+) ✅
- [x] aarch64 port (GIC, UART, Timer, MMU, Exceptions)
- [x] SMP support (per-CPU work queues, IPIs)
- [x] Full network stack (TCP/IP, UDP, DNS, SSH, WiFi framework)
- [x] Block storage (VirtIO-blk, NVMe, AHCI)
- [x] Filesystems (VFS, RamFS, ext4, FAT32, SplaxFS, ProcFS, SysFS, DevFS)
- [x] Crypto (SHA-256/512, AES-256-CBC, ChaCha20, HMAC, HKDF, PBKDF2)
- [x] Sound (AC97, HDA, VirtIO-snd)
- [x] USB (xHCI, HID)
- [ ] GUI (S-CANVAS)

### Building for aarch64

```bash
# Build for aarch64
cargo build -p splax_kernel --bin splax_kernel_aarch64 --release \
    --target aarch64-unknown-none \
    -Zbuild-std=core,alloc \
    -Zbuild-std-features=compiler-builtins-mem

# Run in QEMU (virt machine with Cortex-A72)
qemu-system-aarch64 -M virt -cpu cortex-a72 -m 512M \
    -kernel target/aarch64-unknown-none/release/splax_kernel_aarch64 \
    -nographic
```

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](docs/CONTRIBUTING.md) for guidelines.

### Code Style

- All code must be `#![no_std]` compatible
- No `unsafe` outside `arch/` modules (with rare exceptions)
- All public APIs require capability tokens
- Use `spin::Mutex` for synchronization
- Explicit error types (no `Box<dyn Error>`)

## License

Splax OS is dual-licensed under:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Acknowledgments

Splax OS draws inspiration from:
- seL4 (capability security)
- Fuchsia (microkernel design)
- Redox OS (Rust OS development)
- Plan 9 (distributed systems)

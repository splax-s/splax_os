# Splax OS

> *"Where your laptop feels like a cloud region, nothing runs unless you ask, and security is built in, not bolted on."*

[![Build Status](https://github.com/splax/splax_os/actions/workflows/ci.yml/badge.svg)](https://github.com/splax/splax_os/actions)
[![License](https://img.shields.io/badge/license-Apache--2.0%2FMIT-blue.svg)](LICENSE)

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
- (Optional) Cross-compilation toolchains for aarch64

### Setup

```bash
# Clone the repository
git clone https://github.com/splax/splax_os.git
cd splax_os

# Install Rust nightly
rustup override set nightly
rustup component add rust-src llvm-tools-preview

# Build for x86_64
./scripts/build.sh x86_64

# Build for aarch64
./scripts/build.sh aarch64

# Run in QEMU
./scripts/qemu.sh x86_64
```

### Development

```bash
# Check compilation
cargo check

# Run tests
./scripts/test.sh unit

# Build in release mode
./scripts/build.sh x86_64 --release
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
| **S-CORE Kernel** | ✅ Done | Boots on x86_64 and aarch64, VGA/UART output |
| **x86_64 Support** | ✅ Done | IDT, GDT, serial, VGA, keyboard, paging |
| **aarch64 Support** | ✅ Done | GIC, PL011 UART, Generic Timer, MMU, exceptions |
| **S-CAP** | ✅ Done | Capability tokens, grant/check/revoke, audit logging |
| **Memory Manager** | ✅ Done | Frame allocator, no-overcommit, page tables |
| **Scheduler** | ✅ Done | Priority-based, deterministic, 4 priority classes |
| **S-LINK** | ✅ Done | IPC channels, message passing, capability transfer |
| **S-ATLAS** | ✅ Done | Service registry, discovery, health monitoring |
| **S-GATE** | ✅ Done | TCP/HTTP gateway, routing, S-LINK integration |
| **S-STORAGE** | ✅ Done | Content-addressed objects, deduplication |
| **S-WAVE** | ✅ Done | WASM module loading, host functions, execution |
| **S-TERM** | ✅ Done | CLI commands, kernel shell integration |
| **Testing** | ✅ Done | 30+ integration tests across all components |
| **Documentation** | ✅ Done | Architecture docs, API reference |

### 🔄 In Progress
| **aarch64** | 📋 Planned | ARM64 port |

## Running Splax OS

### Quick Start

```bash
# Build the kernel
cargo build -p splax_kernel --bin splax_kernel \
    --target x86_64-unknown-none \
    -Z build-std=core,alloc \
    -Z build-std-features=compiler-builtins-mem \
    --release

# Create bootable ISO (requires i686-elf-grub)
cp target/x86_64-unknown-none/release/splax_kernel target/iso/boot/
i686-elf-grub-mkrescue -o target/splax.iso target/iso

# Run in QEMU
qemu-system-x86_64 -cdrom target/splax.iso -serial stdio -m 512M
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

### Phase 5: Polish & Ports (Weeks 17+) 🔄
- [x] aarch64 port (GIC, UART, Timer, MMU, Exceptions)
- [ ] SMP support
- [ ] Network stack
- [ ] Persistent storage
- [ ] GUI (S-CANVAS)

### Running on aarch64

```bash
# Build for aarch64
cargo kbuild-arm

# Run in QEMU
./scripts/qemu-aarch64.sh

# Or manually:
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

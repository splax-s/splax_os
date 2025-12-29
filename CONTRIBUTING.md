# Contributing to Splax OS

First off, thank you for considering contributing to Splax OS! 🎉

Splax OS is an ambitious project to build a **capability-secure, microkernel-based operating system** from scratch in Rust. We welcome contributions from developers of all skill levels, whether you're fixing a typo, implementing a new driver, or designing a core subsystem.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [How to Contribute](#how-to-contribute)
- [Coding Guidelines](#coding-guidelines)
- [Architecture Overview](#architecture-overview)
- [Pull Request Process](#pull-request-process)
- [Issue Guidelines](#issue-guidelines)
- [Community](#community)

## Code of Conduct

This project adheres to the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code. Please report unacceptable behavior to the maintainers.

## Getting Started

### Prerequisites

Before you begin, ensure you have:

- **Rust nightly toolchain** (we use unstable features for OS development)
- **QEMU** for testing (x86_64, optionally aarch64 and riscv64)
- **Git** for version control
- **A Unix-like environment** (Linux, macOS, or WSL on Windows)

### Quick Setup

```bash
# Clone the repository
git clone https://github.com/splax-s/splax_os.git
cd splax_os

# Install Rust nightly
rustup override set nightly
rustup component add rust-src llvm-tools-preview

# Build the kernel
./scripts/splax build

# Run in QEMU
./scripts/splax run
```

## Development Setup

### Recommended Tools

| Tool | Purpose | Install |
|------|---------|---------|
| `rust-analyzer` | IDE support | VS Code extension |
| `cargo-watch` | Auto-rebuild on changes | `cargo install cargo-watch` |
| `cargo-bloat` | Binary size analysis | `cargo install cargo-bloat` |
| `gdb` / `lldb` | Debugging | System package manager |

### Editor Setup

We recommend VS Code with these extensions:
- `rust-analyzer` - Rust language support
- `Even Better TOML` - Cargo.toml support
- `CodeLLDB` - Debugging support
- `x86_64 Assembly` - Assembly syntax highlighting

### Building for Different Architectures

```bash
# x86_64 (default - microkernel mode)
./scripts/splax build

# x86_64 with all monolithic drivers (for testing hardware)
cargo build -p splax_kernel --bin splax_kernel --release \
    --target x86_64-unknown-none \
    --no-default-features \
    --features "monolithic_net,monolithic_usb,monolithic_sound,monolithic_gpu,monolithic_fs" \
    -Zbuild-std=core,alloc \
    -Zbuild-std-features=compiler-builtins-mem

# aarch64
cargo build -p splax_kernel --bin splax_kernel_aarch64 --release \
    --target aarch64-unknown-none \
    -Zbuild-std=core,alloc \
    -Zbuild-std-features=compiler-builtins-mem

# RISC-V
cargo build -p splax_kernel --bin splax_kernel_riscv64 --release \
    --target riscv64gc-unknown-none-elf \
    -Zbuild-std=core,alloc \
    -Zbuild-std-features=compiler-builtins-mem
```

### Running in QEMU

```bash
# Basic run (microkernel mode)
qemu-system-x86_64 -cdrom target/splax.iso -m 256M -serial stdio -no-reboot

# With E1000 network adapter
qemu-system-x86_64 -cdrom target/splax.iso -m 512M -serial stdio -no-reboot \
    -device e1000,netdev=net0 -netdev user,id=net0

# With VirtIO devices (network + block)
qemu-system-x86_64 -cdrom target/splax.iso -m 512M -serial stdio -no-reboot \
    -device virtio-net-pci,netdev=net0 -netdev user,id=net0 \
    -drive file=disk.img,if=virtio,format=raw

# Full hardware emulation (all drivers, no disk images needed)
qemu-system-x86_64 -cdrom target/splax.iso -m 1G -serial stdio -no-reboot \
    -device e1000,netdev=net0 -netdev user,id=net0,hostfwd=tcp::2222-:22 \
    -device intel-hda -device hda-duplex \
    -device qemu-xhci -device usb-kbd -device usb-mouse \
    -device VGA

# Full hardware emulation with disk images (optional - create images first)
qemu-system-x86_64 -cdrom target/splax.iso -m 1G -serial stdio -no-reboot \
    -device e1000,netdev=net0 -netdev user,id=net0,hostfwd=tcp::2222-:22 \
    -device virtio-blk-pci,drive=hd0 -drive file=disk.img,id=hd0,if=none,format=raw \
    -device intel-hda -device hda-duplex \
    -device qemu-xhci -device usb-kbd -device usb-mouse \
    -device VGA

# NVMe storage
qemu-system-x86_64 -cdrom target/splax.iso -m 512M -serial stdio -no-reboot \
    -device nvme,drive=nvme0,serial=deadbeef \
    -drive file=nvme.img,id=nvme0,if=none,format=raw

# AHCI/SATA storage
qemu-system-x86_64 -cdrom target/splax.iso -m 512M -serial stdio -no-reboot \
    -device ahci,id=ahci \
    -device ide-hd,drive=sata0,bus=ahci.0 \
    -drive file=sata.img,id=sata0,if=none,format=raw

# With sound (AC97)
qemu-system-x86_64 -cdrom target/splax.iso -m 512M -serial stdio -no-reboot \
    -device AC97

# With sound (Intel HDA)
qemu-system-x86_64 -cdrom target/splax.iso -m 512M -serial stdio -no-reboot \
    -device intel-hda -device hda-duplex

# aarch64 in QEMU
qemu-system-aarch64 -M virt -cpu cortex-a72 -m 512M \
    -kernel target/aarch64-unknown-none/release/splax_kernel_aarch64 \
    -nographic

# RISC-V in QEMU
qemu-system-riscv64 -M virt -m 512M -bios default \
    -kernel target/riscv64gc-unknown-none-elf/release/splax_kernel_riscv64 \
    -nographic
```

### Creating Disk Images for Testing

```bash
# Create a blank disk image (100MB)
qemu-img create -f raw disk.img 100M

# Create a FAT32 formatted disk (for FAT32 driver testing)
dd if=/dev/zero of=fat32.img bs=1M count=100
mkfs.vfat -F 32 fat32.img

# Create an ext4 formatted disk (for ext4 driver testing)
dd if=/dev/zero of=ext4.img bs=1M count=100
mkfs.ext4 ext4.img
```

## How to Contribute

### Types of Contributions

We welcome many types of contributions:

#### 🐛 Bug Fixes
Found a bug? Check if there's an existing issue. If not, open one, then submit a PR with the fix.

#### ✨ New Features
Want to add something new? Please open an issue first to discuss the design. This helps avoid wasted effort.

#### 📖 Documentation
Documentation improvements are always welcome! This includes:
- README updates
- Code comments
- Architecture documentation
- Tutorials and guides

#### 🧪 Tests
More tests = more confidence. We especially need:
- Unit tests for kernel subsystems
- Integration tests
- Fuzzing harnesses for parsers and protocol handlers

#### 🔧 Tooling
Improvements to build scripts, CI/CD, or development tools.

### Good First Issues

Look for issues labeled [`good first issue`](https://github.com/splax-s/splax_os/labels/good%20first%20issue) - these are specifically chosen for newcomers.

### Areas We Need Help

| Area | Difficulty | Description |
|------|------------|-------------|
| Documentation | Easy | Improve docs, add examples |
| USB Drivers | Medium | Add support for more USB device classes |
| Network Drivers | Medium | Intel i210/i225, Realtek NICs |
| Filesystems | Medium | NTFS read support, exFAT |
| RISC-V | Hard | Complete the RISC-V port |
| SMP | Hard | Multi-core scheduler improvements |
| GPU | Hard | Basic GPU driver (virtio-gpu, Intel) |

## Coding Guidelines

### Rust Style

We follow the standard Rust style guidelines with some additions:

```rust
// ✅ Good: Descriptive names, clear structure
pub struct CapabilityToken {
    /// Unique identifier for this capability
    id: u64,
    /// Operations permitted by this capability
    operations: Operations,
    /// Expiration timestamp (0 = never expires)
    expires_at: u64,
}

// ❌ Bad: Cryptic names, no documentation
pub struct Cap {
    i: u64,
    o: u32,
    e: u64,
}
```

### Key Principles

1. **No `unsafe` outside `arch/` modules** (with rare, documented exceptions)
2. **All public APIs require capability tokens**
3. **Explicit error types** - no `Box<dyn Error>` or panics
4. **`#![no_std]` compatible** - this is an OS kernel
5. **Document everything** - especially public items

### Code Style

```rust
// Module-level documentation
//! This module implements the S-CAP capability system.
//!
//! Capabilities are unforgeable tokens that grant access to resources.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// Creates a new capability with the specified operations.
///
/// # Arguments
/// * `owner` - Process ID of the capability owner
/// * `ops` - Operations permitted by this capability
///
/// # Returns
/// A new capability token, or an error if creation failed.
///
/// # Example
/// ```
/// let cap = create_capability(pid, Operations::READ | Operations::WRITE)?;
/// ```
pub fn create_capability(owner: ProcessId, ops: Operations) -> Result<CapabilityToken, CapError> {
    // Implementation
}
```

### Commit Messages

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `style`: Formatting, no code change
- `refactor`: Code change that neither fixes nor adds
- `perf`: Performance improvement
- `test`: Adding tests
- `chore`: Build process or auxiliary tools

**Examples:**
```
feat(net): add RTL8169 gigabit ethernet driver

Implements full RTL8169 driver with:
- PCI device detection
- DMA ring buffer setup
- Interrupt handling
- Basic packet TX/RX

Closes #123
```

```
fix(sched): prevent deadlock in SMP scheduler

The previous implementation could deadlock when two CPUs
simultaneously tried to steal work from each other.

Fixes #456
```

## Architecture Overview

Understanding the architecture helps you contribute effectively:

```
┌─────────────────────────────────────────────────────────────────┐
│                        USERSPACE                                │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐           │
│  │ S-TERM   │ │ S-CODE   │ │ S-WAVE   │ │ S-NATIVE │           │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └────┬─────┘           │
│       └────────────┴────────────┴────────────┘                  │
│                         ↓ S-LINK IPC                            │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐           │
│  │ S-ATLAS  │ │ S-GATE   │ │S-STORAGE │ │ S-GPU    │           │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘           │
├─────────────────────────────────────────────────────────────────┤
│                    S-CORE (Microkernel)                         │
│       S-CAP │ IPC │ Scheduler │ Memory │ Interrupts            │
├─────────────────────────────────────────────────────────────────┤
│                         HARDWARE                                │
└─────────────────────────────────────────────────────────────────┘
```

### Key Subsystems

| Subsystem | Location | Description |
|-----------|----------|-------------|
| S-CAP | `kernel/src/cap/` | Capability-based security |
| IPC | `kernel/src/ipc/` | Inter-process communication |
| Scheduler | `kernel/src/sched/` | Deterministic scheduling |
| Memory | `kernel/src/mm/` | Physical and virtual memory |
| Network | `kernel/src/net/` | TCP/IP stack |
| Block | `kernel/src/block/` | Storage abstraction |
| Filesystem | `kernel/src/fs/` | VFS and filesystems |

## Pull Request Process

### Before Submitting

1. **Run the tests**: `cargo test --workspace --exclude splax_kernel --exclude splax_bootloader`
2. **Check formatting**: `cargo fmt --check`
3. **Run clippy**: `cargo clippy --workspace`
4. **Build the kernel**: `./scripts/splax build`
5. **Test in QEMU**: `./scripts/splax run`

### PR Checklist

- [ ] Code follows our style guidelines
- [ ] Self-reviewed the code
- [ ] Added tests for new functionality
- [ ] Updated documentation if needed
- [ ] Commit messages follow conventional commits
- [ ] All CI checks pass

### Review Process

1. Submit your PR
2. Automated checks run (CI, formatting, tests)
3. A maintainer reviews the code
4. Address any feedback
5. Once approved, a maintainer merges the PR

### PR Tips

- **Keep PRs small** - easier to review, faster to merge
- **One feature per PR** - don't bundle unrelated changes
- **Write a good description** - explain what and why
- **Link related issues** - use "Closes #123" or "Fixes #456"

## Issue Guidelines

### Bug Reports

Include:
1. **Description**: What happened?
2. **Expected**: What should have happened?
3. **Steps to reproduce**: How can we see the bug?
4. **Environment**: OS, Rust version, QEMU version
5. **Logs/Output**: Any error messages or screenshots

### Feature Requests

Include:
1. **Problem**: What problem does this solve?
2. **Solution**: What do you propose?
3. **Alternatives**: What else did you consider?
4. **Context**: Any other relevant information

### Issue Labels

| Label | Description |
|-------|-------------|
| `bug` | Something isn't working |
| `enhancement` | New feature request |
| `good first issue` | Good for newcomers |
| `help wanted` | Extra attention needed |
| `documentation` | Documentation improvements |
| `question` | Further information requested |
| `wontfix` | This won't be worked on |

## Community

### Getting Help

- **GitHub Issues**: For bugs and feature requests
- **GitHub Discussions**: For questions and ideas
- **Discord**: [Join our Discord server](https://discord.gg/splaxos) (coming soon)

### Recognition

Contributors are recognized in:
- The `CONTRIBUTORS.md` file
- Release notes
- The project README (for significant contributions)

### Maintainers

Current maintainers:
- [@splax-s](https://github.com/splax-s) - Project lead

---

## Thank You! 🙏

Every contribution, no matter how small, helps make Splax OS better. We appreciate your time and effort!

If you have questions, don't hesitate to ask. We're here to help you contribute successfully.

Happy hacking! 🦀🖥️

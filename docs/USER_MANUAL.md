# Splax OS User Manual

Welcome to Splax OS! This comprehensive guide will help you get started with the operating system and explore its features.

## Table of Contents

1. [Introduction](#introduction)
2. [Getting Started](#getting-started)
3. [Core Concepts](#core-concepts)
4. [Shell Commands](#shell-commands)
5. [Package Management](#package-management)
6. [Service Management](#service-management)
7. [Networking](#networking)
8. [Security](#security)
9. [Development](#development)
10. [Troubleshooting](#troubleshooting)

---

## Introduction

Splax OS is a capability-secure, distributed-first operating system designed for modern computing. It features:

- **Capability-based security**: Fine-grained access control without ambient authority
- **Microkernel architecture**: Minimal kernel with services in userspace
- **WebAssembly first**: Run WASM applications natively
- **Cloud-native**: Built-in container orchestration and service mesh
- **Multi-architecture**: Supports x86_64, aarch64, and riscv64

### Philosophy

> "Where your laptop feels like a cloud region, nothing runs unless you ask, and security is built in, not bolted on."

---

## Getting Started

### System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| CPU | x86_64/aarch64/riscv64 | Multi-core |
| RAM | 256 MB | 2 GB+ |
| Storage | 100 MB | 4 GB+ |
| Firmware | UEFI or BIOS | UEFI |

### Installation

#### From ISO

1. Download the latest ISO from the releases page
2. Write to USB or mount in VM:
   ```bash
   dd if=splax.iso of=/dev/sdX bs=4M status=progress
   ```
3. Boot from the media
4. Run the installer:
   ```
   splax> install
   ```

#### From Source

```bash
# Clone the repository
git clone https://github.com/splax-s/splax_os.git
cd splax_os

# Build and create ISO
./scripts/splax build
./scripts/splax iso

# Run in QEMU
./scripts/splax run
```

### First Boot

When Splax OS first boots, you'll see the boot screen followed by the shell prompt:

```
Splax OS v0.1.0 (alpha)
Initializing kernel...
Starting services...

splax>
```

---

## Core Concepts

### Capabilities (S-CAP)

Capabilities are unforgeable tokens that grant specific access rights. Unlike traditional permission systems, capabilities:

- Cannot be forged or guessed
- Can be delegated to other processes
- Can be attenuated (reduced in scope)
- Can expire or be revoked

```
# View your capabilities
splax> cap list

# Delegate a capability
splax> cap delegate file:/home/user/doc.txt --read-only --to process:1234
```

### Services

Splax OS uses a microkernel design where most functionality runs as userspace services:

| Service | Description |
|---------|-------------|
| S-ATLAS | Service registry and discovery |
| S-LINK | IPC messaging |
| S-STORAGE | Object storage |
| S-NET | Networking |
| S-GATE | External access gateway |
| S-DEV | Device drivers |

### IPC Channels

Processes communicate through typed IPC channels:

```
# Create a channel
splax> ipc create my-channel

# Send a message
splax> ipc send my-channel "Hello, World!"

# Receive messages
splax> ipc recv my-channel
```

---

## Shell Commands

### Navigation

```bash
# List directory contents
ls [path]

# Change directory
cd <path>

# Print working directory
pwd

# Show file contents
cat <file>

# Create directory
mkdir <path>

# Remove file/directory
rm <path>
```

### System Information

```bash
# System status
status

# Memory usage
mem

# CPU information
cpuinfo

# Process list
ps

# Uptime
uptime
```

### Process Management

```bash
# Run a program
run <program> [args...]

# Background execution
run <program> &

# Kill a process
kill <pid>

# Send signal
signal <pid> <signal>
```

### File Operations

```bash
# Copy file
cp <source> <dest>

# Move/rename
mv <source> <dest>

# Create empty file
touch <file>

# File permissions (capability-based)
cap grant <file> <capability>
```

---

## Package Management

Splax OS uses `pkg` for package management.

### Installing Packages

```bash
# Search for packages
pkg search <query>

# Install a package
pkg install <package>

# Install specific version
pkg install <package>@<version>

# Install from WASM
pkg install ./app.wasm
```

### Managing Packages

```bash
# List installed packages
pkg list

# Update package index
pkg update

# Upgrade all packages
pkg upgrade

# Remove a package
pkg remove <package>
```

### Package Sources

```bash
# Add a source
pkg source add <url>

# List sources
pkg source list

# Remove source
pkg source remove <name>
```

---

## Service Management

### Viewing Services

```bash
# List all services
svc list

# Service status
svc status <name>

# Service info
svc info <name>
```

### Controlling Services

```bash
# Start a service
svc start <name>

# Stop a service
svc stop <name>

# Restart a service
svc restart <name>

# Enable at boot
svc enable <name>

# Disable at boot
svc disable <name>
```

### Service Logs

```bash
# View service logs
svc logs <name>

# Follow logs
svc logs <name> --follow

# Filter by time
svc logs <name> --since 1h
```

---

## Networking

### Configuration

```bash
# Show network interfaces
net list

# Configure interface
net config eth0 --ip 192.168.1.100/24 --gateway 192.168.1.1

# Enable DHCP
net config eth0 --dhcp

# Show routing table
net route
```

### Connectivity

```bash
# Ping a host
ping <host>

# DNS lookup
dns resolve <hostname>

# HTTP request
http get <url>

# Open socket
socket connect <host>:<port>
```

### Firewall

```bash
# Show firewall rules
fw list

# Allow incoming port
fw allow in tcp 22

# Block outgoing
fw deny out udp 53

# Enable firewall
fw enable
```

### Service Mesh

```bash
# List mesh services
mesh list

# Register service
mesh register my-service --port 8080

# Route traffic
mesh route my-service --to backend:8080
```

---

## Security

### Capability Management

```bash
# Create capability
cap create <type> <resource> <permissions>

# Grant capability
cap grant <cap-id> --to <process>

# Revoke capability
cap revoke <cap-id>

# Attenuate (reduce permissions)
cap attenuate <cap-id> --remove write
```

### Cryptography

```bash
# Generate key pair
crypto keygen ed25519 --output my-key

# Sign data
crypto sign --key my-key --input data.txt

# Verify signature
crypto verify --key my-key.pub --input data.txt --sig data.sig

# Encrypt file
crypto encrypt --key password --input secret.txt

# Decrypt file
crypto decrypt --key password --input secret.txt.enc
```

### TLS

```bash
# Generate certificate
tls cert generate --cn example.com

# Show certificate info
tls cert show server.crt
```

---

## Development

### Building Applications

Splax OS primarily runs WebAssembly applications:

```bash
# Compile Rust to WASM
cargo build --target wasm32-wasi --release

# Run WASM application
run app.wasm

# With arguments
run app.wasm arg1 arg2
```

### Debugging

```bash
# Debug a process
debug <pid>

# Set breakpoint
(debug) break main

# Continue
(debug) continue

# Step
(debug) step

# Print variable
(debug) print var_name
```

### Performance

```bash
# Profile a process
perf record <pid>

# Show profile
perf report

# System benchmarks
bench run all
```

### Testing

```bash
# Run unit tests
test unit

# Run integration tests
test integration

# Run specific test
test unit --filter memory
```

---

## Troubleshooting

### Common Issues

#### Boot Failure

If the system fails to boot:

1. Check boot media integrity
2. Verify UEFI/BIOS settings
3. Try safe mode: `splax --safe`

#### Service Not Starting

```bash
# Check service status
svc status <name>

# View error logs
svc logs <name> --level error

# Restart with verbose output
svc start <name> --verbose
```

#### Network Issues

```bash
# Check interface status
net status

# Test connectivity
ping 8.8.8.8

# Check DNS
dns resolve google.com

# View network logs
log show --filter net
```

#### Permission Denied

In Splax OS, "permission denied" usually means missing capability:

```bash
# Check capabilities
cap list

# Request capability from service
cap request file:/path/to/resource --permissions read
```

### Getting Help

```bash
# Command help
<command> --help

# Full manual
man <command>

# System documentation
help

# Online resources
help --online
```

### Reporting Bugs

Please report bugs at: https://github.com/splax-s/splax_os/issues

Include:
- System version (`version`)
- Hardware info (`cpuinfo`)
- Steps to reproduce
- Error messages (`log show --since boot`)

---

## Appendix

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| Ctrl+C | Interrupt process |
| Ctrl+D | EOF / Logout |
| Ctrl+Z | Suspend process |
| Ctrl+L | Clear screen |
| Tab | Command completion |
| ↑/↓ | Command history |

### Environment Variables

| Variable | Description |
|----------|-------------|
| HOME | Home directory |
| PATH | Command search path |
| SPLAX_CAPS | Capability space |
| TERM | Terminal type |
| LANG | Locale |

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid usage |
| 126 | Permission denied |
| 127 | Command not found |
| 130 | Interrupted (Ctrl+C) |

---

*Last updated: January 2026*
*Splax OS Version: 0.1.0-alpha*

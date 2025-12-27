# SPLAX OS - MASTER DEVELOPMENT BLUEPRINT (v3.0)

> **This is your single source of truth.** Inspired by Linux kernel architecture, reimagined with Rust-first design and capability-based security.

## 🎯 THE VISION: LINUX REIMAGINED IN RUST

We're taking the best architectural patterns from the Linux kernel (35+ years of battle-tested design) and rebuilding them from scratch in pure Rust with:

- **Memory safety by default** (no CVEs from use-after-free, buffer overflows)
- **Capability-based security** (replace POSIX permissions entirely)
- **Modern async/await patterns** (no callback hell from Linux)
- **Zero-cost abstractions** (Rust's guarantees without runtime overhead)

---

## 🏗️ ARCHITECTURE: LINUX SUBSYSTEMS → SPLAX MODULES

### Core Subsystem Mapping

| Linux Subsystem | Splax Module | Status | Improvements |
|-----------------|--------------|--------|--------------|
| `kernel/` | `kernel/src/` | ✅ Active | Pure Rust, no C bindings |
| `mm/` | `kernel/src/mm/` | ✅ Active | RAII memory management |
| `fs/` | `kernel/src/fs/` | ✅ Active | VFS + SplaxFS + RamFS + ProcFS + SysFS + DevFS |
| `net/` | `kernel/src/net/` | ✅ Active | Async networking, SSH, WiFi framework |
| `drivers/` | `kernel/src/drivers/` | 🔄 Basic | VirtIO-net, VirtIO-blk, E1000, RTL8139 |
| `ipc/` | `kernel/src/ipc/` | ✅ Active | S-LINK zero-copy channels |
| `security/` | `kernel/src/cap/` | ✅ Active | S-CAP replaces LSM/SELinux |
| `sched/` | `kernel/src/sched/` | ✅ Active | Deterministic, SMP-aware |
| `block/` | `kernel/src/block/` | ✅ Active | VirtIO-blk, block device abstraction |
| `crypto/` | `kernel/src/crypto/` | 📋 Planned | Safe crypto primitives |
| `sound/` | `kernel/src/sound/` | 📋 Phase 4 | Audio subsystem |
| `gpu/drm/` | `kernel/src/gpu/` | 📋 Phase 4 | Graphics subsystem |

---

## 📋 COMPREHENSIVE DEVELOPMENT ROADMAP

### PHASE 1: KERNEL FOUNDATION (Months 1-3) ✅ MOSTLY COMPLETE

**Goal**: Bootable kernel with core subsystems

| Week | Focus | Linux Equivalent | Deliverable | Status |
|------|-------|------------------|-------------|--------|
| 1-2 | Bootloader | `arch/*/boot/` | UEFI/Multiboot2 loader | ✅ Done |
| 3-4 | Memory Manager | `mm/` | Frame allocator, heap, paging | ✅ Done |
| 5-6 | Interrupts/Timers | `kernel/irq/`, `kernel/time/` | IDT, PIC, APIC, timer | ✅ Done |
| 7-8 | Scheduler | `kernel/sched/` | Process scheduler, SMP | ✅ Done |
| 9-10 | Capability System | `security/` | S-CAP tokens, enforcement | ✅ Done |
| 11-12 | IPC | `ipc/` | S-LINK channels, messaging | ✅ Done |

---

### PHASE 2: NETWORKING & DRIVERS (Months 4-6) 🔄 IN PROGRESS

**Goal**: Full network stack, driver framework, storage

#### 2.1 Network Stack (Linux `net/`)

| Component | Linux Path | Splax Path | Status |
|-----------|------------|------------|--------|
| Ethernet | `net/ethernet/` | `net/ethernet.rs` | ✅ Done |
| IPv4 | `net/ipv4/` | `net/ip.rs` | ✅ Done |
| ARP | `net/ipv4/arp.c` | `net/arp.rs` | ✅ Done |
| ICMP | `net/ipv4/icmp.c` | `net/icmp.rs` | ✅ Done |
| TCP | `net/ipv4/tcp*.c` | `net/tcp.rs` | ✅ Done |
| UDP | `net/ipv4/udp.c` | `net/udp.rs` | ✅ Done |
| DNS | `(userspace)` | `net/dns.rs` | ✅ Done |
| SSH | `(userspace)` | `net/ssh.rs` | ✅ Done |
| Socket API | `net/socket.c` | `net/socket.rs` | ✅ Done |
| IPv6 | `net/ipv6/` | `net/ipv6.rs` | 📋 Planned |
| Netfilter | `net/netfilter/` | `net/firewall.rs` | 📋 Planned |
| Traffic Control | `net/sched/` | `net/qos.rs` | 📋 Planned |

#### 2.2 Driver Framework (Linux `drivers/`)

| Component | Linux Path | Splax Path | Status |
|-----------|------------|------------|--------|
| Driver Core | `drivers/base/` | `drivers/mod.rs` | 🔄 Basic |
| VirtIO Net | `drivers/virtio/` | `net/virtio.rs` | ✅ Done |
| VirtIO Block | `drivers/block/virtio_blk.c` | `block/virtio_blk.rs` | ✅ Done |
| E1000 | `drivers/net/e1000/` | `net/e1000.rs` | ✅ Done |
| RTL8139 | `drivers/net/rtl8139.c` | `net/rtl8139.rs` | ✅ Done |
| WiFi | `drivers/net/wireless/` | `net/wifi.rs` | 🔄 Framework |
| NVMe | `drivers/nvme/` | `block/nvme.rs` | ✅ Done |
| AHCI/SATA | `drivers/ata/` | `block/ahci.rs` | ✅ Done |
| USB Core | `drivers/usb/core/` | `usb/mod.rs` | ✅ Done |
| USB HID | `drivers/hid/` | `usb/hid.rs` | ✅ Done |
| PCI | `drivers/pci/` | `drivers/pci.rs` | 📋 Planned |

#### 2.3 Block Layer (Linux `block/`)

| Component | Linux Path | Splax Path | Status |
|-----------|------------|------------|--------|
| Block Core | `block/blk-core.c` | `block/mod.rs` | ✅ Done |
| VirtIO Block | `block/virtio_blk.c` | `block/virtio_blk.rs` | ✅ Done |
| I/O Scheduler | `block/elevator.c` | `block/scheduler.rs` | 📋 Planned |
| Partitions | `block/partitions/` | `block/partitions.rs` | 📋 Planned |
| Bio Layer | `block/bio.c` | `block/bio.rs` | 📋 Planned |

---

### PHASE 3: FILESYSTEMS & STORAGE (Months 7-9)

**Goal**: Production filesystem, persistent storage

#### 3.1 Virtual Filesystem (Linux `fs/`)

| Component | Linux Path | Splax Path | Description |
|-----------|------------|------------|-------------|
| VFS Core | `fs/` | `fs/vfs.rs` | Abstract filesystem interface |
| Inode/Dentry | `fs/inode.c`, `fs/dcache.c` | `fs/inode.rs` | File metadata, directory cache |
| Page Cache | `mm/filemap.c` | `fs/pagecache.rs` | Cached file I/O |
| Buffer Cache | `fs/buffer.c` | `fs/buffer.rs` | Block buffer management |
| Mount System | `fs/namespace.c` | `fs/mount.rs` | Capability-gated mounts |

#### 3.2 Filesystem Implementations

| Filesystem | Linux Path | Splax Path | Priority |
|------------|------------|------------|----------|
| RamFS | `fs/ramfs/` | `fs/ramfs.rs` | ✅ Done |
| SplaxFS | N/A | `fs/splaxfs.rs` | ✅ Done - Native FS |
| VFS Core | `fs/` | `fs/vfs.rs` | ✅ Done |
| Procfs | `fs/proc/` | `fs/procfs.rs` | ✅ Done |
| Sysfs | `fs/sysfs/` | `fs/sysfs.rs` | ✅ Done |
| Devfs | `fs/devpts/` | `fs/devfs.rs` | ✅ Done |
| ext4 (read) | `fs/ext4/` | `fs/ext4_ro.rs` | 📋 Planned |
| FAT32 | `fs/fat/` | `fs/fat32.rs` | 📋 Planned |
| ISO9660 | `fs/isofs/` | `fs/iso9660.rs` | 📋 Low Priority |

#### 3.3 SplaxFS Native Filesystem Design

```rust
// Capability-aware filesystem with built-in encryption
pub struct SplaxFS {
    superblock: SuperBlock,
    encryption_key: Option<EncryptionKey>,
    capability_table: CapabilityTable,
}

// Features:
// - Copy-on-write (like btrfs/ZFS)
// - Built-in encryption (like LUKS, but integrated)
// - Capability tokens stored in extended attributes
// - Snapshots and rollback
// - Compression (LZ4/Zstd)
// - Checksumming (XXHash3)
```

---

### PHASE 4: PROCESS & EXECUTION (Months 10-12)

**Goal**: Full process model, binary loading, signals

#### 4.1 Process Management (Linux `kernel/`)

| Component | Linux Path | Splax Path | Description |
|-----------|------------|------------|-------------|
| Process Core | `kernel/fork.c` | `process/mod.rs` | ✅ Basic process structs |
| Task Struct | `include/linux/sched.h` | `process/task.rs` | Task state machine |
| Exec | `fs/exec.c` | `process/exec.rs` | Binary loading |
| Exit | `kernel/exit.c` | `process/exit.rs` | Clean shutdown |
| Wait | `kernel/exit.c` | `process/wait.rs` | Child reaping |
| Signals | `kernel/signal.c` | `process/signal.rs` | Async events |
| Namespaces | `kernel/nsproxy.c` | `process/namespace.rs` | Isolation |
| Cgroups | `kernel/cgroup/` | `process/cgroup.rs` | Resource limits |

#### 4.2 Binary Format Support

| Format | Linux Path | Splax Path | Priority |
|--------|------------|------------|----------|
| ELF | `fs/binfmt_elf.c` | `process/elf.rs` | ✅ Done (loader) |
| WASM | N/A | `runtime/wave/` (S-WAVE) | ✅ Done (interpreter) |
| Script | `fs/binfmt_script.c` | `exec/script.rs` | 📋 Planned |
| Flat | `fs/binfmt_flat.c` | `exec/flat.rs` | 📋 Low Priority |

#### 4.3 S-WAVE Runtime (WASM)

```rust
// WebAssembly runtime with capability injection
// IMPLEMENTED in runtime/wave/src/lib.rs
pub struct Wave {
    config: WaveConfig,
    modules: BTreeMap<ModuleId, Module>,
    instances: BTreeMap<InstanceId, Instance>,
}

impl Wave {
    // Load and validate WASM module
    pub fn load(&self, wasm_bytes: Vec<u8>, name: Option<String>, cap: &CapabilityToken) -> Result<ModuleId>;
    
    // Instantiate with capability bindings
    pub fn instantiate(&self, module_id: ModuleId, capability_bindings: Vec<(HostFunction, CapabilityToken)>) -> Result<InstanceId>;
}

// Bytecode interpreter supports:
// - Control flow (block, loop, if, br, br_if, return)
// - Local/global variables
// - Memory operations (load/store i32, i64)
// - i32/i64 arithmetic and comparison
// - Type conversions
// - 20+ host functions for system calls
```

---

### PHASE 5: USERSPACE SERVICES (Months 13-15)

**Goal**: System services, init system, service manager

#### 5.1 Core Services

| Service | Linux Equivalent | Splax Service | Description |
|---------|------------------|---------------|-------------|
| Init | `systemd/init` | S-INIT | First process, service manager |
| Device Manager | `udevd` | S-DEV | Device hotplug, driver loading |
| Network Manager | `NetworkManager` | S-NET | Network configuration |
| Logger | `journald` | S-LOG | Structured logging |
| DNS Resolver | `systemd-resolved` | S-DNS | Local DNS cache |
| Time Sync | `systemd-timesyncd` | S-TIME | NTP client |
| SSH Daemon | `sshd` | S-SSHD | Remote access |
| HTTP Gateway | `nginx/haproxy` | S-GATE | External API gateway |

#### 5.2 S-INIT Design

```rust
// Declarative service definitions (no shell scripts)
pub struct ServiceDefinition {
    name: String,
    binary: BinaryRef,           // Path or WASM module
    capabilities: Vec<Capability>, // What it's allowed to do
    dependencies: Vec<String>,   // Wait for these first
    restart_policy: RestartPolicy,
    resource_limits: ResourceLimits,
}

// Boot sequence:
// 1. Kernel starts S-INIT with root capability
// 2. S-INIT reads service manifests from S-STORAGE
// 3. Topological sort by dependencies
// 4. Parallel startup with capability delegation
// 5. Health monitoring and restart
```

---

### PHASE 6: HARDWARE & PLATFORM (Months 16-18)

**Goal**: Broad hardware support, platform drivers

#### 6.1 Platform Support (Linux `arch/`)

| Architecture | Linux Path | Splax Path | Status |
|--------------|------------|------------|--------|
| x86_64 | `arch/x86/` | `arch/x86_64/` | ✅ Active |
| AArch64 | `arch/arm64/` | `arch/aarch64/` | ✅ Basic |
| RISC-V | `arch/riscv/` | `arch/riscv/` | 📋 Planned |

#### 6.2 Hardware Drivers

| Category | Examples | Priority |
|----------|----------|----------|
| Serial | 16550 UART, PL011 | ✅ Done |
| Display | VGA text, framebuffer | ✅ Done |
| Keyboard | PS/2, USB HID | ✅ Done |
| Network | VirtIO, e1000, RTL8139 | ✅ Done |
| Storage | VirtIO-blk, AHCI, NVMe | ✅ Done |
| Graphics | Simple FB, VirtIO-GPU | 📋 Phase 4 |
| Audio | HDA, VirtIO-snd | 📋 Phase 4 |
| USB | xHCI | ✅ Done |

#### 6.3 ACPI & Power Management (Linux `drivers/acpi/`)

```rust
pub struct AcpiSubsystem {
    tables: AcpiTables,
    power_states: PowerStateManager,
    thermal_zones: Vec<ThermalZone>,
    battery: Option<BatteryInfo>,
}

impl AcpiSubsystem {
    pub fn parse_tables(&mut self, rsdp: PhysAddr) -> Result<()>;
    pub fn enter_sleep_state(&self, state: SleepState) -> Result<()>;
    pub fn shutdown(&self) -> !;
    pub fn reboot(&self) -> !;
}
```

---

### PHASE 7: SECURITY HARDENING (Months 19-21)

**Goal**: Production-grade security

#### 7.1 Security Features

| Feature | Linux Equivalent | Splax Implementation |
|---------|------------------|----------------------|
| Capabilities | `security/commoncap.c` | S-CAP (core system) |
| Sandboxing | seccomp-bpf | S-SANDBOX (WASM + caps) |
| MAC | SELinux/AppArmor | S-CAP policies |
| Audit | `kernel/audit.c` | S-AUDIT (built-in) |
| Crypto | `crypto/` | S-CRYPTO |
| Secure Boot | UEFI Secure Boot | S-BOOT verification |
| Memory Safety | ASLR, Stack Canaries | Rust + W^X + ASLR |

#### 7.2 S-CAP Policy Language

```rust
// Declarative capability policies
capability "network.socket.tcp" {
    grants: [
        { service: "s-gate", operations: ["create", "bind", "listen", "accept"] },
        { service: "s-ssh", operations: ["create", "connect"] },
    ],
    auditing: "always",
    ratelimit: "1000/sec",
}

capability "fs.read" {
    grants: [
        { service: "*", paths: ["/data/${service}/**"] },
    ],
    deny: [
        { paths: ["/secrets/**"] },
    ],
}
```

---

### PHASE 8: CLOUD & CONTAINERS (Months 22-24)

**Goal**: Container runtime, orchestration support

#### 8.1 Containerization

| Feature | Linux/Docker | Splax |
|---------|--------------|-------|
| Namespaces | `kernel/nsproxy.c` | S-NAMESPACE |
| Cgroups | `kernel/cgroup/` | S-CGROUP |
| Overlay FS | `fs/overlayfs/` | S-OVERLAY |
| Container Runtime | containerd/runc | S-CONTAINER |
| Image Format | OCI | S-IMAGE (capability-aware) |

#### 8.2 Orchestration

```rust
// Native Kubernetes-style orchestration
pub struct SplaxOrchestrator {
    scheduler: WorkloadScheduler,
    network: PodNetwork,
    storage: VolumeManager,
    capability_broker: CapabilityBroker,
}

// Pods get capabilities instead of root
impl Pod {
    pub fn deploy(&self, caps: CapabilitySet) -> Result<PodHandle>;
}
```

---

### PHASE 9: INSTALLATION & DEPLOYMENT (Months 25-27)

**Goal**: Production-ready installation system

#### 9.1 S-INSTALL: Installation System

```rust
// Declarative installation configuration
pub struct InstallConfig {
    target_disk: DiskDescriptor,        // Where to install
    partitioning: PartitionScheme,      // How to partition
    filesystem: FilesystemChoice,       // SplaxFS recommended
    encryption: Option<EncryptionConfig>, // Full disk encryption
    bootloader: BootloaderChoice,       // UEFI or Legacy BIOS
    services: Vec<ServiceManifest>,     // Initial services to install
    network: NetworkConfig,             // Initial network setup
}

pub enum PartitionScheme {
    AutoErase,           // Wipe disk, auto-partition
    DualBoot(Vec<Partition>), // Preserve existing partitions
    Manual(Vec<PartitionDef>), // User-defined layout
    Recovery,            // Install to recovery partition
}

impl Installer {
    pub fn validate(&self, config: &InstallConfig) -> Result<ValidationReport>;
    pub fn install(&self, config: InstallConfig) -> Result<InstallReport>;
    pub fn create_recovery(&self) -> Result<RecoveryImage>;
}
```

#### 9.2 Installation Modes

| Mode | Description | Use Case |
|------|-------------|----------|
| **Live Boot** | Run from ISO/USB without installing | Testing, recovery |
| **Full Install** | Install to disk, replace existing | Dedicated Splax machine |
| **Dual Boot** | Install alongside existing OS | Gradual migration |
| **Container** | Run Splax kernel in container | Development, CI/CD |
| **VM Image** | Pre-built VM images | Cloud deployment |
| **Embedded** | Minimal install for IoT/embedded | Raspberry Pi, routers |

#### 9.3 Installation Process

```
┌─────────────────────────────────────────────────────────────────┐
│                     S-INSTALL WIZARD                            │
├─────────────────────────────────────────────────────────────────┤
│  1. Boot from ISO/USB                                           │
│  2. Hardware detection (CPU, RAM, disks, network)               │
│  3. Choose installation mode                                    │
│  4. Configure partitioning (auto or manual)                     │
│  5. Set encryption passphrase (recommended)                     │
│  6. Configure initial capabilities (admin token)                │
│  7. Select services to install                                  │
│  8. Network configuration (gateway setup)                       │
│  9. Install bootloader (UEFI/BIOS)                             │
│ 10. First boot into S-INIT                                      │
└─────────────────────────────────────────────────────────────────┘
```

#### 9.4 Post-Install Configuration

```rust
// First boot setup (no /etc, uses S-STORAGE)
pub struct FirstBootConfig {
    admin_capability: CapabilityToken,  // Root-equivalent cap
    hostname: String,
    timezone: Timezone,
    locale: Locale,
    ssh_enabled: bool,
    ssh_keys: Vec<PublicKey>,
    gateway_config: GatewayConfig,
}
```

---

### PHASE 10: APPLICATION PORTING & COMPATIBILITY (Months 28-30)

**Goal**: Seamless porting of existing applications

#### 10.1 S-PORT: Application Porting Layer

Splax is NOT POSIX-compatible by design, but we provide **seamless porting tools**:

```rust
// Porting layer translates common patterns to Splax equivalents
pub struct PortingLayer {
    syscall_translator: SyscallTranslator,  // Map POSIX → S-CAP
    path_mapper: PathMapper,                 // /etc → S-STORAGE
    network_adapter: NetworkAdapter,         // Ports → S-GATE services
    capability_inferrer: CapabilityInferrer, // Detect needed caps
}

// Example: Port a Node.js app
// Original: app.listen(3000)
// Ported:   s_gate::register_service("my-app", handler, caps)
```

#### 10.2 Porting Strategies

| Original Pattern | Splax Equivalent | Porting Tool |
|------------------|------------------|--------------|
| `listen(port)` | `s_gate::register_http_service()` | S-PORT auto-translate |
| `fork()` | `s_process::spawn()` | Compile-time rewrite |
| `open("/etc/config")` | `s_storage::get_config()` | Path remapping |
| `socket()` + `connect()` | `s_link::connect_service()` | Network adapter |
| `setuid(0)` | `cap_table.elevate(token)` | Capability mapping |
| `pthread_create()` | `s_process::spawn_thread()` | Thread adapter |
| Environment vars | `s_config::get()` | Config translation |

#### 10.3 WASM Porting (Preferred)

```rust
// Best porting path: Compile to WASM
// - Automatic sandboxing
// - Capability injection at load time
// - Cross-architecture by default

pub struct WasmPort {
    source: SourceLanguage,  // Rust, C, Go, Python, JS
    wasm_module: Vec<u8>,
    capabilities_needed: Vec<CapabilityRequirement>,
    host_bindings: Vec<HostBinding>,
}

// Supported source languages for WASM compilation:
// ✅ Rust (native)
// ✅ C/C++ (via Emscripten/wasi-sdk)
// ✅ Go (via TinyGo)
// ✅ Python (via Pyodide)
// ✅ JavaScript (via wasm-bindgen)
// ✅ AssemblyScript
```

#### 10.4 Native Porting (When WASM isn't suitable)

```rust
// For performance-critical apps, native ELF is supported
// but runs in S-NATIVE sandbox with strict capabilities

pub struct NativePort {
    elf_binary: Vec<u8>,
    sandbox_config: SandboxConfig,
    capabilities: CapabilitySet,
    resource_limits: ResourceLimits,
}

// Native apps CANNOT:
// - Access raw network (must use S-GATE)
// - Access filesystem directly (must use S-STORAGE)
// - Create child processes without capability
// - Use privileged CPU instructions
```

---

## 📁 ENHANCED FILE STRUCTURE

```
/splax/
├── Cargo.toml                          # Workspace root
├── splax_kernel.json                   # x86_64 target spec
├── splax_kernel_aarch64.json           # ARM64 target spec
├── splax_kernel_riscv.json             # RISC-V target spec (planned)
│
├── kernel/                             # S-CORE KERNEL
│   ├── Cargo.toml
│   ├── linker-x86_64.ld
│   ├── linker-aarch64.ld
│   └── src/
│       ├── lib.rs                      # Kernel entry
│       ├── main.rs                     # x86_64 entry
│       ├── main_aarch64.rs             # ARM64 entry
│       │
│       ├── arch/                       # ARCHITECTURE (Linux: arch/)
│       │   ├── mod.rs
│       │   ├── x86_64/
│       │   │   ├── mod.rs
│       │   │   ├── boot.rs             # Multiboot2
│       │   │   ├── gdt.rs              # GDT/TSS
│       │   │   ├── idt.rs              # Interrupts
│       │   │   ├── paging.rs           # Page tables
│       │   │   ├── lapic.rs            # Local APIC
│       │   │   ├── ioapic.rs           # I/O APIC
│       │   │   └── serial.rs           # UART
│       │   └── aarch64/
│       │       ├── mod.rs
│       │       └── ...
│       │
│       ├── mm/                         # MEMORY (Linux: mm/)
│       │   ├── mod.rs
│       │   ├── frame.rs                # Physical frame allocator
│       │   ├── heap.rs                 # Kernel heap
│       │   ├── vmm.rs                  # Virtual memory manager
│       │   ├── slab.rs                 # Slab allocator (planned)
│       │   └── page_cache.rs           # Page cache (planned)
│       │
│       ├── sched/                      # SCHEDULER (Linux: kernel/sched/)
│       │   ├── mod.rs
│       │   ├── smp.rs                  # Multi-processor support
│       │   ├── cfs.rs                  # Fair scheduler (planned)
│       │   └── rt.rs                   # Real-time scheduler (planned)
│       │
│       ├── cap/                        # CAPABILITIES (Linux: security/)
│       │   ├── mod.rs                  # S-CAP system
│       │   ├── token.rs                # Capability tokens
│       │   ├── table.rs                # Capability table
│       │   ├── audit.rs                # Audit logging
│       │   └── policy.rs               # Policy engine (planned)
│       │
│       ├── ipc/                        # IPC (Linux: ipc/)
│       │   ├── mod.rs                  # S-LINK channels
│       │   ├── channel.rs              # Ring buffer channels
│       │   ├── signal.rs               # Signals (planned)
│       │   └── shm.rs                  # Shared memory (planned)
│       │
│       ├── fs/                         # FILESYSTEM (Linux: fs/)
│       │   ├── mod.rs                  # VFS core
│       │   ├── vfs.rs                  # Virtual filesystem (planned)
│       │   ├── ramfs.rs                # RAM filesystem
│       │   ├── devfs.rs                # Device filesystem (planned)
│       │   ├── procfs.rs               # Process filesystem (planned)
│       │   └── splaxfs/                # Native filesystem (planned)
│       │
│       ├── net/                        # NETWORKING (Linux: net/)
│       │   ├── mod.rs                  # Network stack
│       │   ├── device.rs               # Network device abstraction
│       │   ├── ethernet.rs             # Ethernet frames
│       │   ├── arp.rs                  # ARP protocol
│       │   ├── ip.rs                   # IPv4
│       │   ├── icmp.rs                 # ICMP (ping)
│       │   ├── tcp.rs                  # TCP
│       │   ├── udp.rs                  # UDP
│       │   ├── dns.rs                  # DNS client
│       │   ├── ssh.rs                  # SSH client/server
│       │   ├── socket.rs               # Socket API
│       │   ├── virtio.rs               # VirtIO-net driver
│       │   ├── firewall.rs             # Packet filtering (planned)
│       │   └── ipv6.rs                 # IPv6 (planned)
│       │
│       ├── drivers/                    # DRIVERS (Linux: drivers/)
│       │   ├── mod.rs                  # Driver framework (planned)
│       │   ├── pci.rs                  # PCI bus (planned)
│       │   ├── usb/                    # USB subsystem (planned)
│       │   ├── block/                  # Block devices (planned)
│       │   └── gpu/                    # Graphics (planned)
│       │
│       ├── block/                      # BLOCK LAYER (Linux: block/)
│       │   ├── mod.rs                  # Block device core (planned)
│       │   └── scheduler.rs            # I/O scheduler (planned)
│       │
│       ├── process/                    # PROCESS (Linux: kernel/)
│       │   ├── mod.rs                  # Process management
│       │   ├── task.rs                 # Task struct
│       │   ├── exec.rs                 # Binary loading (planned)
│       │   └── signal.rs               # Signals (planned)
│       │
│       ├── crypto/                     # CRYPTO (Linux: crypto/)
│       │   ├── mod.rs                  # Crypto framework (planned)
│       │   ├── hash.rs                 # Hash functions (planned)
│       │   └── cipher.rs               # Encryption (planned)
│       │
│       └── time/                       # TIME (Linux: kernel/time/)
│           ├── mod.rs                  # Timekeeping
│           └── timer.rs                # Timer management
│
├── services/                           # USERSPACE SERVICES
│   ├── atlas/                          # S-ATLAS: Service registry
│   ├── link/                           # S-LINK: IPC library
│   ├── gate/                           # S-GATE: HTTP/TCP gateway
│   ├── storage/                        # S-STORAGE: Object storage
│   ├── init/                           # S-INIT: Init system (planned)
│   ├── dev/                            # S-DEV: Device manager (planned)
│   └── log/                            # S-LOG: Logger (planned)
│
├── runtime/                            # EXECUTION RUNTIMES
│   ├── wave/                           # S-WAVE: WASM runtime
│   └── native/                         # S-NATIVE: Native sandbox
│
├── tools/                              # DEVELOPER TOOLS
│   ├── term/                           # S-TERM: Terminal
│   └── code/                           # S-CODE: Editor
│
├── tests/                              # Integration tests
├── scripts/                            # Build scripts
└── docs/                               # Documentation
```

---

## 🔧 BUILD SYSTEM COMPARISON

### Linux Build System
```makefile
# Linux uses Kbuild (complex Makefile system)
make menuconfig    # Configure
make -j$(nproc)    # Build
make modules       # Build modules
make install       # Install
```

### Splax Build System
```bash
# Splax uses Cargo (Rust's native build)
cargo build --release --target x86_64-unknown-none  # Build kernel
./scripts/build-iso.sh                               # Create ISO
./scripts/qemu.sh                                    # Test in QEMU
```

---

## 🆚 KEY DIFFERENCES FROM LINUX

| Aspect | Linux | Splax |
|--------|-------|-------|
| **Language** | C (99%), Rust (1%) | Rust (100%) |
| **Security Model** | POSIX + LSM | Capability-only (S-CAP) |
| **IPC** | Pipes, sockets, shared mem | S-LINK channels |
| **Drivers** | Loadable modules (.ko) | Compiled-in or WASM |
| **Init** | systemd/sysvinit | S-INIT (declarative) |
| **Filesystem** | ext4, XFS, btrfs | SplaxFS (native) |
| **Apps** | ELF binaries | WASM (S-WAVE) + ELF |
| **Config** | /etc files | Capability-gated storage |
| **Shell** | bash/zsh | S-TERM (integrated) |
| **Users** | UID/GID | Capabilities only |
| **Networking** | Ports (0-65535) | Services via S-GATE |
| **Installation** | Complex installers | S-INSTALL (declarative) |

---

## 🌐 NETWORKING: SERVICES NOT PORTS

### The Problem with Ports

Traditional networking uses port numbers (0-65535) which are:
- **Arbitrary**: Why is HTTP on 80? SSH on 22? Historical accident.
- **Conflicting**: Two apps can't bind to same port
- **Security nightmare**: Port scanning, firewall rules, NAT traversal
- **Not self-documenting**: `curl localhost:3847` - what service is that?

### The Splax Solution: Named Services via S-GATE

```
┌─────────────────────────────────────────────────────────────────┐
│                    EXTERNAL WORLD (Internet)                    │
│                          ↓ ↑                                    │
│              ┌──────────────────────┐                          │
│              │       S-GATE         │  ← Single gateway        │
│              │  (External Gateway)  │    handles ALL external  │
│              └──────────────────────┘    traffic               │
│                    ↓         ↑                                 │
│         ┌─────────────────────────────────┐                    │
│         │         S-LINK (IPC)            │                    │
│         │   Capability-bound channels     │                    │
│         └─────────────────────────────────┘                    │
│              ↓         ↓         ↓                             │
│         ┌────────┐ ┌────────┐ ┌────────┐                       │
│         │Service │ │Service │ │Service │  ← Internal services  │
│         │  "api" │ │ "web"  │ │ "auth" │    (no ports!)        │
│         └────────┘ └────────┘ └────────┘                       │
└─────────────────────────────────────────────────────────────────┘
```

### Service Registration (No Ports!)

```rust
// ❌ WRONG (Linux way)
// let listener = TcpListener::bind("0.0.0.0:8080")?;

// ✅ CORRECT (Splax way)
// Services register with S-GATE by NAME, not port
s_gate::register_service(ServiceConfig {
    name: "my-api",                      // Service name (like DNS)
    protocol: Protocol::Http,            // HTTP, gRPC, WebSocket
    handler: my_handler,                 // Request handler
    capabilities: vec![cap_network],     // Required capability
    rate_limit: Some(RateLimit::new(1000, Duration::from_secs(1))),
    auth: AuthConfig::CapabilityRequired,
});

// External access: https://my-splax-host/services/my-api/
// Internal access: s_link::connect("my-api", cap)?
```

### Service Discovery (S-ATLAS)

```rust
// Find services by name, not by remembering port numbers
let auth_service = s_atlas::discover("auth-service", my_cap)?;

// Connect via S-LINK (internal IPC)
let channel = s_link::connect(&auth_service, my_cap)?;

// Or request via S-GATE (if external)
let response = s_gate::request("auth-service", "/validate", my_cap)?;
```

### External Gateway Configuration

```rust
// S-GATE exposes services to the outside world
pub struct GatewayConfig {
    // External binding (this IS a port, but only S-GATE uses it)
    listen: GatewayListen::Auto,  // Auto-selects available port
    
    // TLS termination
    tls: TlsConfig::AutoCert { domain: "splax.example.com" },
    
    // Service routing
    routes: vec![
        Route::new("/api/*", "api-service"),
        Route::new("/web/*", "web-service"),
        Route::new("/ws/*", "websocket-service"),
    ],
    
    // Firewall rules (capability-based)
    firewall: FirewallConfig {
        default_policy: Policy::Deny,
        rules: vec![
            Rule::allow("api-service").from_capability(public_cap),
            Rule::allow("admin-service").from_capability(admin_cap),
        ],
    },
}
```

### Benefits of Service-Based Networking

| Aspect | Port-Based (Linux) | Service-Based (Splax) |
|--------|-------------------|----------------------|
| **Discovery** | Manual (know the port) | Automatic (S-ATLAS) |
| **Conflicts** | Port already in use | Names are unique |
| **Security** | Firewall rules by port | Capability tokens |
| **Documentation** | External docs needed | Self-describing |
| **Load Balancing** | External LB needed | Built into S-GATE |
| **TLS** | Per-service config | Centralized in S-GATE |
| **Rate Limiting** | Per-service | Centralized policy |

---

## 🏗️ MICROKERNEL ARCHITECTURE: WHY IT MATTERS

### Linux Monolithic vs Splax Microkernel

```
LINUX (Monolithic)                    SPLAX (Microkernel)
┌─────────────────────┐               ┌─────────────────────┐
│    Applications     │               │    Applications     │
├─────────────────────┤               ├─────────────────────┤
│                     │               │   System Services   │
│                     │               │ ┌─────┬─────┬─────┐ │
│      Kernel         │               │ │FS   │Net  │Drv  │ │
│  (Everything here)  │               │ └─────┴─────┴─────┘ │
│  - Filesystems      │               ├─────────────────────┤
│  - Network Stack    │               │     S-CORE Kernel   │
│  - Drivers          │               │  - Scheduling       │
│  - Security         │               │  - Memory           │
│  - IPC              │               │  - IPC (S-LINK)     │
│  - Scheduling       │               │  - Capabilities     │
│                     │               │  (NOTHING ELSE!)    │
└─────────────────────┘               └─────────────────────┘
```

### What Lives WHERE

| Component | Linux (in kernel) | Splax Location |
|-----------|-------------------|----------------|
| Scheduler | ✅ Kernel | ✅ S-CORE (kernel) |
| Memory Manager | ✅ Kernel | ✅ S-CORE (kernel) |
| IPC | ✅ Kernel | ✅ S-CORE (kernel) |
| Capabilities | ❌ LSM module | ✅ S-CORE (kernel) |
| Filesystem | ✅ Kernel (VFS) | 📦 S-STORAGE (userspace) |
| Network Stack | ✅ Kernel | 📦 S-NET (userspace) |
| Device Drivers | ✅ Kernel modules | 📦 S-DEV (userspace) |
| TCP/IP | ✅ Kernel | 📦 S-NET service |
| Graphics | ✅ DRM/KMS | 📦 S-GPU service |

### Why Microkernel?

1. **Fault Isolation**: Network driver crashes? Only S-NET restarts, not the whole system.
2. **Security**: Drivers can't access kernel memory (capability-gated).
3. **Updatability**: Update filesystem service without rebooting.
4. **Simplicity**: S-CORE is ~10K lines, Linux kernel is 30M+ lines.
5. **Verification**: Small kernel can be formally verified.

### IPC Performance (The Classic Concern)

"But microkernels are slow because of IPC!"

```rust
// S-LINK uses zero-copy shared memory with capability tokens
// Measured latency: <2µs for cross-service calls

pub struct SLinkChannel {
    shared_buffer: SharedMemory,  // Zero-copy
    capability: CapabilityToken,  // Security
    ring_buffer: RingBuffer,      // Lock-free
}

// Benchmark: S-LINK vs Linux pipe
// S-LINK:    1.8µs average
// Linux pipe: 3.2µs average
// We're FASTER because: no syscall overhead, no copy
```

---

## 🎯 CURRENT STATUS & NEXT STEPS

### Completed ✅
- [x] x86_64 bootloader (Multiboot2)
- [x] Memory management (frame allocator, heap, paging)
- [x] Interrupt handling (IDT, PIC, keyboard, timer)
- [x] VGA text mode output
- [x] Serial console (COM1) with ring buffer input
- [x] Basic scheduler with SMP support
- [x] Capability system (S-CAP) with tokens and audit
- [x] IPC channels (S-LINK) with zero-copy messaging
- [x] Full network stack (Ethernet, IP, TCP, UDP, ICMP)
- [x] VirtIO-net driver
- [x] E1000 network driver
- [x] RTL8139 network driver
- [x] WiFi driver framework
- [x] Ping, traceroute, DNS tools
- [x] SSH client/server (basic)
- [x] Dual shell (VGA + Serial) with 40+ commands
- [x] Block device abstraction layer
- [x] VirtIO-blk driver (basic)
- [x] RamFS filesystem
- [x] SplaxFS native filesystem (disk-backed)
- [x] ProcFS (process filesystem)
- [x] SysFS (system filesystem)
- [x] DevFS (device filesystem)
- [x] VFS layer with mount system
- [x] ELF loader (basic)
- [x] Process management with signals
- [x] S-WAVE WASM runtime with bytecode interpreter
- [x] S-WAVE: SIMD, threads, atomics, JIT (WASM 2.0+)
- [x] S-WAVE: VFS integration (load .wasm from filesystem)
- [x] S-WAVE: WASM validation from files
- [x] S-WAVE: Test WASM modules in /bin/
- [x] S-WAVE: 60+ host functions (process, memory, fs, net, thread, sync, cap, service, time, sys, debug)
- [x] Userspace process execution (Ring 3 transition, full ELF exec)
- [x] S-INIT service manager (PID 1, service/dependency/restart logic)
- [x] USB subsystem (core types, descriptors, xHCI driver)
- [x] USB HID keyboard driver (scancode translation, LED support)
- [x] S-WAVE: full function execution in kernel (Wave::call() integration)
- [x] SplaxFS journaling and recovery (write-ahead log, transactions)
- [x] NVMe storage driver (queue management, namespace support)
- [x] AHCI/SATA storage driver (FIS, port management, DMA)

### In Progress 🔄
- [ ] S-INSTALL installer system
- [ ] Graphics/framebuffer subsystem
- [ ] Audio subsystem (basic)

### Next Milestones 📋
1. **Week 1-2**: NVMe/AHCI storage drivers
2. **Week 3-4**: S-INSTALL installer system
3. **Week 5-6**: Graphics/framebuffer basics
4. **Week 7-8**: Audio subsystem (basic)

---

## ✅ SPLAX PATTERNS

**COPILOT: ALWAYS USE THESE**

- **Access Control**: `cap_table.check(process_id, token, "operation")?`
- **Inter-Process Communication**: `s_link::Channel::create(sender, receiver, cap_token)`
- **Service Discovery**: `s_atlas::discover("auth-service", discovery_token)`
- **External Communication**: `s_gate::TcpGateway::new(port, internal_service, firewall_rules)`
- **Storage**: `s_storage::Object::create(data, capabilities)` (returns an `ObjectId`)
- **Errors**: Use explicit `thiserror`-style enums. No generic `Box<dyn Error>`.
- **Configuration**: Declarative structs passed to constructors, not global config files.

---

## 🚫 FORBIDDEN PATTERNS

**COPILOT: NEVER SUGGEST THESE**

- `fork()`, `exec()`, `sudo`, `chmod`, `chown`
- `listen(port)`, `bind(port)`, `accept()` in app code (Use S-GATE services)
- `TcpListener::bind("0.0.0.0:8080")` (Use S-GATE named services)
- Port numbers in application code (Services are named, not numbered)
- Global `static mut` variables (use `spin::Mutex` or `OnceCell`)
- File paths like `/etc/config.toml` (Use S-STORAGE objects)
- Environment variable configuration (`env::var`)
- `println!` in kernel (Use serial/VGA output macros)
- Dynamic linking (`dlopen`)
- Any C code or FFI to C libraries
- Direct socket creation in apps (Use S-LINK for internal, S-GATE for external)

---

## 🏁 THE MISSION

> **"Take the best of Linux. Throw away the legacy. Build it in Rust. Make it secure by default."**

### Core Principles (Non-Negotiable)

1. **Microkernel Architecture**: S-CORE handles only scheduling, memory, IPC, and capabilities. Everything else runs as userspace services.

2. **Services, Not Ports**: Applications don't bind to port numbers. They register named services with S-GATE. No more port conflicts, no more port scanning, no more firewall rule nightmares.

3. **Capability-Only Security**: No users, no groups, no root. Every resource access requires a cryptographic capability token.

4. **Seamless Porting**: While we're not POSIX-compatible, S-PORT tools make porting existing apps straightforward. WASM is the preferred path.

5. **Installable OS**: S-INSTALL makes Splax a real OS you can install on bare metal, not just a research project.

We are not here to be compatible with Linux. We are here to make something better.

**Now, build.**

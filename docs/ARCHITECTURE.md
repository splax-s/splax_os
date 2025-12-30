# Splax OS Architecture

> Technical deep-dive into Splax OS internals

## Overview

Splax OS is a capability-secure, microkernel-based operating system written in Rust. This document describes the architecture and design decisions.

## System Layers

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           USER APPLICATIONS                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                   │
│  │ WASM Modules │  │ Native Apps  │  │  CLI Tools   │                   │
│  │  (S-WAVE)    │  │ (S-NATIVE)   │  │  (S-TERM)    │                   │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘                   │
│         └─────────────────┴─────────────────┘                           │
│                           │                                              │
├───────────────────────────┼──────────────────────────────────────────────┤
│                    SYSTEM SERVICES                                       │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │   S-ATLAS    │  │   S-LINK     │  │   S-GATE     │  │  S-STORAGE   │ │
│  │   Registry   │  │     IPC      │  │   Gateway    │  │   Objects    │ │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘ │
│         └─────────────────┴─────────────────┴─────────────────┘         │
│                           │                                              │
├───────────────────────────┼──────────────────────────────────────────────┤
│                      S-CORE KERNEL                                       │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │    S-CAP     │  │   S-LINK     │  │  Scheduler   │  │   Memory     │ │
│  │ Capabilities │  │  IPC Core    │  │ Deterministic│  │   Manager    │ │
│  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘ │
├─────────────────────────────────────────────────────────────────────────┤
│                    HARDWARE ABSTRACTION                                  │
│             x86_64 (GDT, IDT, Paging) │ aarch64 (MMU, GIC)              │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## S-CORE: The Microkernel

The kernel is intentionally minimal (~53KB stripped, v0.1.0). It handles only:

### 1. CPU Scheduling

**Location:** `kernel/src/sched/mod.rs`

- **Deterministic Priority Scheduler**: Tasks sorted by priority, predictable execution order
- **Priority Classes**:
  - Realtime (0-63): Immediate execution, minimal preemption
  - Interactive (64-127): Low latency for user interaction
  - Normal (128-191): Standard workloads
  - Background (192-255): Best-effort, yields frequently
- **Time Slices**: Higher priority = longer quantum

```rust
pub struct Scheduler {
    ready_queues: [TaskQueue; 256],  // One queue per priority
    current_task: Option<TaskId>,
    tick_count: u64,
}
```

### 2. Memory Management

**Location:** `kernel/src/mm/mod.rs`

- **No Overcommit**: Physical memory is always backed
- **No Swap**: Deterministic, no disk I/O surprises
- **Frame Allocator**: Bitmap-based physical page allocator
- **Page Tables**: x86_64 4-level paging, aarch64 4-level translation

```rust
pub struct MemoryManager {
    frame_allocator: FrameAllocator,
    kernel_page_table: PageTable,
    total_memory: u64,
    used_memory: u64,
}
```

### 3. Capability System (S-CAP)

**Location:** `kernel/src/cap/mod.rs`

The heart of Splax security. NO operation proceeds without a valid capability.

```rust
pub struct Capability {
    id: CapabilityId,
    resource: ResourceId,
    operations: Operations,    // Bitmask of allowed operations
    parent: Option<CapabilityId>,
    delegation_depth: u32,
    revoked: bool,
}
```

**Key Properties:**
- **Unforgeable**: Cryptographic tokens, not names
- **Delegatable**: Can grant subset of your permissions
- **Revocable**: Parent can revoke all children
- **Auditable**: All grants/checks logged

**Operations Bitmask:**
```rust
bitflags! {
    pub struct Operations: u64 {
        const READ    = 1 << 0;
        const WRITE   = 1 << 1;
        const EXECUTE = 1 << 2;
        const GRANT   = 1 << 3;
        const REVOKE  = 1 << 4;
        // ... more
    }
}
```

### 4. IPC Primitives

**Location:** `kernel/src/ipc/mod.rs`

Low-level channel infrastructure for S-LINK:

```rust
pub struct Channel {
    id: ChannelId,
    sender_cap: CapabilityToken,
    receiver_cap: CapabilityToken,
    buffer: RingBuffer<Message>,
    state: ChannelState,
}
```

- **Zero-Copy**: Shared memory regions with capability transfer
- **Bounded Buffers**: No unbounded growth
- **Capability Passing**: Transfer capabilities across channels

---

## System Services

### S-ATLAS: Service Registry

**Location:** `services/atlas/src/lib.rs`

Central registry for service discovery:

```rust
pub struct ServiceRegistry {
    services: BTreeMap<ServiceId, ServiceRecord>,
    by_name: BTreeMap<String, Vec<ServiceId>>,
    by_namespace: BTreeMap<String, Vec<ServiceId>>,
}

pub struct ServiceRecord {
    id: ServiceId,
    name: String,
    namespace: String,
    version: Version,
    capabilities: Vec<CapabilityRequirement>,
    channel: ChannelId,
    health: HealthStatus,
}
```

**Features:**
- Service registration with capability requirements
- Discovery by name, namespace, or capabilities
- Health monitoring with automatic deregistration
- Dependency injection via capability routing

### S-LINK: IPC Channels

**Location:** `services/link/src/lib.rs`

High-level messaging on top of kernel IPC:

```rust
pub struct Link {
    channels: BTreeMap<ChannelId, Channel>,
    pending: BTreeMap<ChannelId, VecDeque<Message>>,
}

pub struct Message {
    id: MessageId,
    sender: ChannelEndpoint,
    payload: Vec<u8>,
    capabilities: Vec<CapabilityToken>,
}
```

**Message Patterns:**
- Request/Response with correlation IDs
- Fire-and-forget notifications
- Streaming with backpressure
- Capability transfer

### S-GATE: External Gateway

**Location:** `services/gate/src/lib.rs`

Bridge to external world (TCP, HTTP):

```rust
pub struct Gateway {
    id: GatewayId,
    protocol: Protocol,
    listeners: Vec<Listener>,
    routes: Vec<Route>,
}

pub struct Route {
    pattern: RoutePattern,
    target: ChannelId,
    required_caps: Vec<CapabilityType>,
}
```

**Protocols:**
- TCP: Raw socket emulation
- HTTP: Request/response routing
- Future: gRPC, WebSocket

### S-STORAGE: Object Storage

**Location:** `services/storage/src/lib.rs`

Content-addressed, capability-gated object storage:

```rust
pub struct Storage {
    objects: BTreeMap<ObjectId, StoredObject>,
    by_hash: BTreeMap<ContentHash, ObjectId>,
    namespaces: BTreeMap<String, NamespaceConfig>,
}

pub struct StoredObject {
    id: ObjectId,
    hash: ContentHash,
    data: Vec<u8>,
    metadata: ObjectMetadata,
    ref_count: u32,  // For deduplication
}
```

**Features:**
- Content-addressed: Same data = same ID
- Automatic deduplication
- Capability-gated read/write/delete
- Namespace isolation

---

## Application Runtimes

### S-WAVE: WASM Runtime

**Location:** `runtime/wave/src/lib.rs`

Primary application execution environment:

```rust
pub struct Wave {
    modules: BTreeMap<ModuleId, Module>,
    instances: BTreeMap<InstanceId, Instance>,
    config: WaveConfig,
}

pub struct Instance {
    id: InstanceId,
    module_id: ModuleId,
    host_functions: Vec<BoundHostFunction>,
    memory: Vec<u8>,
    state: InstanceState,
    steps_executed: u64,
}
```

**Host Functions:**
| Function | Capability Required | Signature |
|----------|-------------------|-----------|
| `s_link_send` | `channel:send` | `(i32, i32, i32) -> i32` |
| `s_link_receive` | `channel:receive` | `(i32, i32, i32) -> i32` |
| `s_storage_read` | `storage:read` | `(i32, i32, i32, i32) -> i32` |
| `s_storage_write` | `storage:write` | `(i32, i32, i32, i32) -> i32` |
| `s_log` | `log:write` | `(i32, i32, i32) -> ()` |
| `s_time_now` | `time:read` | `() -> i64` |
| `s_sleep` | `process:suspend` | `(i64) -> ()` |

**Security Model:**
- No ambient authority
- All imports require explicit capability binding
- Execution step limits for determinism
- Memory isolation per instance

### S-NATIVE: Native Sandbox

**Location:** `runtime/native/src/lib.rs`

For performance-critical native code:

```rust
pub struct NativeSandbox {
    processes: BTreeMap<ProcessId, NativeProcess>,
    config: SandboxConfig,
}

pub struct NativeProcess {
    id: ProcessId,
    capabilities: Vec<CapabilityToken>,
    memory_limit: usize,
    syscall_filter: SyscallFilter,
}
```

- Stricter sandboxing than WASM
- Explicit syscall filtering
- Memory limits enforced
- Required for drivers, performance code

---

## Hardware Abstraction

### x86_64

**Location:** `kernel/src/arch/x86_64/`

| Component | File | Purpose |
|-----------|------|---------|
| GDT | `gdt.rs` | Segment descriptors, TSS |
| IDT | `idt.rs` | Interrupt descriptor table |
| Paging | `paging.rs` | 4-level page tables |
| Interrupts | `interrupts.rs` | Exception/IRQ handlers |
| Serial | `serial.rs` | Debug output |
| VGA | `vga.rs` | Text mode display |
| Keyboard | `keyboard.rs` | PS/2 keyboard input |

### aarch64

**Location:** `kernel/src/arch/aarch64/`

| Component | File | Purpose |
|-----------|------|---------|
| Exceptions | `exceptions.rs` | EL1 exception vectors, ESR parsing |
| GIC | `gic.rs` | Generic Interrupt Controller v2 |
| MMU | `mmu.rs` | 4-level page tables, 4KB granule |
| Timer | `timer.rs` | ARM Generic Timer for tick scheduling |
| UART | `uart.rs` | PL011 serial console |
| Boot | `boot.S` | Stack setup, BSS clear, vector table |

**QEMU virt Memory Map:**
```
0x0800_0000  GICD (Distributor)
0x0801_0000  GICC (CPU Interface)
0x0900_0000  UART0 (PL011)
0x4000_0000  RAM start
0x4008_0000  Kernel load address
```

**aarch64 Boot Sequence:**
1. **QEMU** loads kernel ELF at 0x4008_0000
2. **boot.S** clears BSS, sets up 64KB stack
3. **kernel_main()** initializes:
   - PL011 UART (serial debug)
   - GICv2 distributor and CPU interface
   - ARM Generic Timer (10ms tick)
   - Exception vectors at EL1
4. **S-CAP** capability table initialized
5. **S-LINK** IPC channels ready
6. **S-ATLAS** service registry starts
7. **Scheduler** enables interrupts
8. **Shell** prompt ready

---

## Boot Process

### x86_64 Boot Sequence

1. **GRUB** loads kernel ELF at 1MB
2. **boot.S** sets up initial stack, enables long mode
3. **kernel_main()** initializes:
   - Serial output (debug)
   - GDT with TSS
   - IDT with handlers
   - PIC remapping
   - VGA text mode
4. **S-CAP** capability table initialized
5. **S-LINK** IPC channels ready
6. **S-ATLAS** service registry starts
7. **Scheduler** enables interrupts
8. **Shell** prompt ready

### Memory Layout (x86_64)

```
0xFFFF_FFFF_FFFF_FFFF ┌────────────────────┐
                      │   Kernel Stack     │
0xFFFF_8000_0100_0000 ├────────────────────┤
                      │   Kernel Heap      │
0xFFFF_8000_0080_0000 ├────────────────────┤
                      │   Kernel Code      │
0xFFFF_8000_0010_0000 ├────────────────────┤
                      │   ...              │
0x0000_0000_0010_0000 ├────────────────────┤ 1MB
                      │   Kernel (phys)    │
0x0000_0000_0000_0000 └────────────────────┘
```

---

## Security Model

### Capability Flow

```
┌─────────────┐     grant(subset)     ┌─────────────┐
│   Kernel    │ ───────────────────▶  │  S-ATLAS    │
│ (root cap)  │                       │             │
└─────────────┘                       └──────┬──────┘
                                             │
                                      grant(channel:create)
                                             │
                                             ▼
                                      ┌─────────────┐
                                      │  S-LINK     │
                                      │             │
                                      └──────┬──────┘
                                             │
                                      grant(channel:send)
                                             │
                                             ▼
                                      ┌─────────────┐
                                      │ WASM Module │
                                      │             │
                                      └─────────────┘
```

### Audit Trail

Every capability operation is logged:

```rust
struct AuditEntry {
    operation: AuditOperation,  // Grant, Check, Revoke
    token: CapabilityToken,
    actor: ProcessId,
    resource: Option<ResourceId>,
    result: AuditResult,        // Allowed, Denied
    timestamp: u64,
}
```

---

## Design Decisions

### Why No POSIX?

- **Clean slate**: No legacy baggage
- **Capability-native**: POSIX assumes ambient authority
- **Simpler**: No emulation layers

### Why Microkernel?

- **Minimal TCB**: Less code in kernel = fewer bugs
- **Isolation**: Drivers crash without taking down OS
- **Flexibility**: Services can be restarted independently

### Why WASM First?

- **Security**: Sandboxed by design
- **Portability**: Same binary on x86_64 and aarch64
- **Determinism**: Predictable execution

### Why Content-Addressed Storage?

- **Deduplication**: Same content stored once
- **Integrity**: Hash verifies content
- **Simplicity**: No hierarchy to manage

---

## Performance Considerations

### IPC Latency

- Zero-copy for large messages
- Direct capability transfer
- Bounded buffers prevent memory exhaustion

### Scheduler Fairness

- Deterministic priority ordering
- No random jitter
- Predictable time slices

### Memory

- No swap = no disk latency surprises
- No overcommit = no OOM kills
- Frame allocator is O(1) for common case

---

## Future Work

1. **SMP Support**: Multi-core scheduling
2. **NUMA Awareness**: Memory locality optimization
3. **GPU Support**: Compute capabilities
4. **Network Stack**: Full TCP/IP in S-GATE
5. **Persistence**: S-STORAGE to disk
6. **GUI**: S-CANVAS graphics service

---

## References

- [seL4 Microkernel](https://sel4.systems/)
- [Capability-based Security](https://en.wikipedia.org/wiki/Capability-based_security)
- [WebAssembly Specification](https://webassembly.github.io/spec/)
- [Rust OS Development](https://os.phil-opp.com/)

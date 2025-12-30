# Hybrid Microkernel Migration Guide

## Overview

This document describes the hybrid microkernel architecture of Splax OS and the migration path from monolithic to microkernel design.

## Architecture

### S-CORE (Kernel)

The kernel contains only essential primitives:

| Component | Size | Responsibility |
|-----------|------|----------------|
| `arch/` | ~5KB | CPU init, interrupts, paging |
| `mm/` | ~3KB | Frame allocator, page tables |
| `sched/` | ~2KB | Scheduler, context switch |
| `cap/` | ~2KB | Capability checking |
| `ipc/` | ~3KB | S-LINK channels, fast path |
| `syscall/` | ~2KB | Syscall dispatch |
| **Total** | **~17KB** | Target: <50KB |

### Userspace Services

All higher-level functionality runs as userspace services:

```
┌────────────────────────────────────────────────────────────┐
│                     S-CORE (Kernel)                        │
│  [Scheduler] [Memory] [IPC Channels] [Capabilities]        │
└────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌────────────────────────────────────────────────────────────┐
│                  S-INIT (PID 1)                            │
└────────────────────────────────────────────────────────────┘
                          │
        ┌─────────────────┼─────────────────┐
        ▼                 ▼                 ▼
┌───────────────┐ ┌───────────────┐ ┌───────────────┐
│   S-STORAGE   │ │     S-DEV     │ │    S-GPU      │
│  VFS, Block   │ │   Drivers     │ │ Framebuffer   │
└───────────────┘ └───────────────┘ └───────────────┘
        │                 │                 │
        └────────┬────────┴─────────────────┘
                 ▼
         ┌───────────────┐
         │     S-NET     │
         │  TCP/IP Stack │
         └───────────────┘
                 │
        ┌────────┴────────┐
        ▼                 ▼
┌───────────────┐ ┌───────────────┐
│    S-GATE     │ │   S-ATLAS     │
│   Gateway     │ │    GUI/WM     │
└───────────────┘ └───────────────┘
```

## Service Descriptions

### S-STORAGE (`services/storage/`)

**Responsibility**: Virtual filesystem, block devices, filesystem drivers

**Components**:
- `vfs_server.rs` - VFS RPC server
- `vfs_protocol.rs` - IPC message definitions
- `splaxfs.rs` - Native filesystem
- `ramfs.rs` - RAM-based filesystem
- `ext4.rs` - ext4 read support
- `fat32.rs` - FAT32 support

**IPC Channel**: `STORAGE_VFS (0x53544F52)`

### S-DEV (`services/dev/`)

**Responsibility**: Device drivers in userspace

**Components**:
- `lib.rs` - Service main, device registry
- `driver.rs` - Driver trait framework
- `usb.rs` - USB host controller, devices
- `sound.rs` - Audio streams, mixer
- `input.rs` - Keyboard, mouse handling
- `irq.rs` - IRQ forwarding, MSI

**IPC Channel**: `DEV_DRIVER (0x44455600)`

### S-NET (`services/net/`)

**Responsibility**: Network stack, sockets, firewall

**Components**:
- `lib.rs` - Service main, socket IPC
- `socket.rs` - BSD socket abstraction
- `tcp.rs` - TCP state machine (RFC 793)
- `udp.rs` - UDP datagrams
- `icmp.rs` - ICMP messages, ping
- `ip.rs` - IPv4 routing, fragmentation
- `firewall.rs` - Packet filtering, NAT
- `config.rs` - DHCP, interface config

**IPC Channel**: `NET_SOCKET (0x4E455400)`

### S-GPU (`services/gpu/`)

**Responsibility**: Graphics, framebuffer, rendering

**Components**:
- `lib.rs` - GPU service, device management
- `framebuffer.rs` - Pixel operations
- `console.rs` - Text console, ANSI parser
- `renderer.rs` - 2D drawing primitives

**IPC Channel**: `GPU_FB (0x47505500)`

### S-GATE (`services/gate/`)

**Responsibility**: External network gateway, firewall

**IPC Channel**: `GATE_GW (0x47415445)`

### S-ATLAS (`services/atlas/`)

**Responsibility**: Window manager, compositor

**IPC Channel**: `ATLAS_WM (0x41544C53)`

## IPC Protocol

### Message Format

```rust
#[repr(C)]
pub struct FastMessage {
    pub tag: u64,      // Message type
    pub data0: u64,    // Payload word 0
    pub data1: u64,    // Payload word 1
    pub data2: u64,    // Payload word 2
    pub data3: u64,    // Payload word 3
    pub data4: u64,    // Payload word 4
    pub data5: u64,    // Payload word 5
    pub data6: u64,    // Capability or extra data
}
```

### Common Tags

| Service | Operation | Tag |
|---------|-----------|-----|
| VFS | Open | `0x0001` |
| VFS | Close | `0x0002` |
| VFS | Read | `0x0003` |
| VFS | Write | `0x0004` |
| Socket | Create | `0x0100` |
| Socket | Bind | `0x0101` |
| Socket | Connect | `0x0102` |
| Socket | Send | `0x0103` |
| Socket | Recv | `0x0104` |
| Device | IOCTL | `0x0200` |
| Device | IRQ | `0x0203` |
| Reply | OK | `0x8000` |
| Reply | Error | `0x8001` |

### Fast Path

For latency-critical operations, use the lock-free SPSC ring buffer:

```rust
use crate::ipc::fastpath::{FastMessage, SpscRing};

// Send on fast path
let msg = FastMessage::from_bytes(tags::VFS_READ, &request_data);
endpoint.send(msg)?;

// Receive reply
let reply = endpoint.recv()?;
```

**Performance Targets**:
- Small messages (≤64 bytes): <500ns
- Service call round-trip: <2µs

## Boot Sequence

S-INIT starts services in dependency order:

1. **Group 1** (no dependencies): S-STORAGE, S-DEV, S-GPU
2. **Group 2** (depends on Group 1): S-NET
3. **Group 3** (depends on S-NET or S-GPU): S-GATE, S-ATLAS

```rust
use splax_init::microkernel::{CoreService, ServiceBootstrap};

let mut bootstrap = ServiceBootstrap::new();

while !bootstrap.is_complete() {
    let ready = bootstrap.next_to_start();
    for service in ready {
        // Start service via syscall
        if start_service(service).is_ok() {
            bootstrap.mark_started(service);
        }
    }
}
```

## Kernel Stubs

The kernel contains stubs that forward requests to userspace:

### Network Stub (`kernel/src/net/stub.rs`)

```rust
// Syscall forwarding to S-NET
pub fn sys_socket(domain: u32, sock_type: u32, protocol: u32) -> Result<u64, NetStubError> {
    NET_STUB.lock().socket_create(domain, sock_type, protocol)
}
```

### Device Stub (`kernel/src/dev_stub.rs`)

```rust
// IRQ forwarding to S-DEV
pub fn forward_irq(irq: u8) {
    DEV_STUB.lock().irq_notify(irq);
}
```

## Migration Checklist

### Phase A: VFS Migration ✅ COMPLETE
- [x] Create VFS server in S-STORAGE
- [x] Define VFS RPC protocol
- [x] Implement kernel VFS stub

### Phase B: Network Migration ✅ COMPLETE
- [x] Create S-NET service
- [x] Implement socket RPC protocol
- [x] Move TCP/UDP/ICMP to userspace
- [x] Move firewall to userspace
- [x] Add routing and DHCP

### Phase C: Driver Migration ✅ COMPLETE
- [x] Create S-DEV service
- [x] Implement IRQ forwarding
- [x] Move USB to userspace
- [x] Move sound to userspace
- [x] Move input to userspace
- [x] Create S-GPU service

### Phase D: Finalization 📋 IN PROGRESS
- [x] Create IPC fast path
- [x] Update S-INIT boot sequence
- [ ] Remove dead kernel code
- [ ] Profile and tune
- [ ] Target: <50KB kernel

## Capability System

Every service operation requires a capability token:

```rust
// Request capability from S-CAP
let cap = cap_table.request(process_id, "net:socket:create")?;

// Use capability in IPC
socket_channel.send(SocketCreate { cap, domain, sock_type })?;
```

## Debugging

### Service Logs

Each service writes to its own log channel:

```
dmesg | grep "s-net"
```

### IPC Statistics

```rust
use crate::ipc::fastpath::IPC_STATS;

println!("Fast sends: {}", IPC_STATS.fast_sends.load(Ordering::Relaxed));
println!("Slow path ratio: {:.2}%", IPC_STATS.slow_path_ratio() * 100.0);
```

### Performance Tracing

```rust
#[cfg(feature = "tracing")]
use crate::trace::ipc_trace;

ipc_trace!("VFS_READ", start_cycles, end_cycles);
```

## Testing

### Unit Tests

```bash
cargo test -p splax-kernel --features test
cargo test -p splax-storage
cargo test -p splax-net
cargo test -p splax-dev
cargo test -p splax-gpu
```

### Integration Tests

```bash
# Boot with test harness
./scripts/test-microkernel.sh

# Verify service startup
assert_service_running "s-storage"
assert_service_running "s-net"
assert_service_running "s-dev"
```

### Benchmarks

```bash
# IPC latency benchmark
cargo bench -p splax-kernel --bench ipc_latency

# Service call benchmark
cargo bench -p splax-kernel --bench service_call
```

## Service Supervision (New in v0.1.0)

S-ATLAS now includes a `ServiceSupervisor` for automatic health monitoring and restart of failed services.

### Restart Policies

```rust
use splax_atlas::{RestartPolicy, RestartConfig};

// Available policies
pub enum RestartPolicy {
    Never,     // Never restart (one-shot services)
    OnFailure, // Restart only on failure (exit code != 0)
    Always,    // Always restart (daemon services)
}

// Configuration
pub struct RestartConfig {
    pub max_restarts: u32,     // Max restarts before giving up (default: 3)
    pub restart_window: u64,   // Time window in ms (default: 60000)
    pub restart_delay: u64,    // Delay before restart in ms (default: 1000)
}
```

### Configuring Restart Policies

```rust
use splax_atlas::supervisor::ServiceSupervisor;

let supervisor = ServiceSupervisor::new();

// Configure S-NET to always restart with defaults
supervisor.configure_restart("s-net", RestartPolicy::Always, None);

// Configure S-STORAGE with custom limits
supervisor.configure_restart("s-storage", RestartPolicy::OnFailure, Some(RestartConfig {
    max_restarts: 5,
    restart_window: 30000,  // 30 seconds
    restart_delay: 500,     // 500ms
}));
```

### Service Events

The supervisor logs all service lifecycle events:

```rust
pub enum ServiceEventType {
    Registered,    // Service added to registry
    Healthy,       // Health check passed
    Degraded,      // Health check shows degradation
    Unhealthy,     // Health check failed
    Restarting,    // Restart initiated
    Restarted,     // Restart successful
    RestartFailed, // Restart limit exceeded
    Unregistered,  // Service removed
    Draining,      // Service draining connections
}

// Get recent events
let events = supervisor.get_events("s-net", 10);
for event in events {
    println!("[{}] {}: {:?}", event.timestamp, event.service_id, event.event_type);
}
```

### Health Check Loop

Run periodic health checks in the service manager:

```rust
// In S-ATLAS main loop
loop {
    // Check all services and restart if needed
    for service_id in atlas.list_services()? {
        supervisor.check_and_restart(&service_id, &atlas)?;
    }
    
    // Sleep between checks
    sleep(Duration::from_millis(5000));
}
```

### Restart Statistics

```rust
// Get restart count for a service
let restarts = supervisor.get_restart_stats("s-net");
println!("S-NET has restarted {} times", restarts);
```

## Troubleshooting

### Service Won't Start

1. Check dependencies: `splax-ctl deps s-net`
2. Check executable exists: `ls /sbin/s-net`
3. Check logs: `dmesg | grep s-net`

### IPC Timeouts

1. Check if service is running
2. Increase timeout in config
3. Check for deadlocks in service

### Performance Issues

1. Check IPC stats for slow path fallback
2. Profile with `perf` or built-in tracing
3. Ensure fast path is being used for hot paths

## Future Work

- [ ] Add more WiFi drivers to S-DEV
- [ ] GPU hardware acceleration in S-GPU
- [ ] Service hot-reloading
- [ ] Live kernel update support
- [ ] Distributed IPC across machines

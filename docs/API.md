# Splax OS API Reference

This document provides comprehensive API documentation for Splax OS system calls, services, and libraries.

## Table of Contents

1. [System Calls](#system-calls)
2. [Capability API](#capability-api)
3. [IPC API](#ipc-api)
4. [File System API](#file-system-api)
5. [Network API](#network-api)
6. [Process API](#process-api)
7. [Memory API](#memory-api)
8. [Crypto API](#crypto-api)
9. [Service API](#service-api)
10. [WASM API](#wasm-api)

---

## System Calls

### Overview

Splax OS provides a minimal system call interface. Most functionality is accessed through IPC to userspace services.

### Core System Calls

#### `sys_exit`

Terminate the current process.

```rust
fn sys_exit(code: i32) -> !
```

**Parameters:**
- `code`: Exit status code

**Returns:** Does not return

---

#### `sys_yield`

Yield the CPU to other threads.

```rust
fn sys_yield() -> ()
```

**Returns:** Nothing

---

#### `sys_time`

Get current system time.

```rust
fn sys_time() -> u64
```

**Returns:** Nanoseconds since boot

---

#### `sys_sleep`

Sleep for a duration.

```rust
fn sys_sleep(ns: u64) -> ()
```

**Parameters:**
- `ns`: Duration in nanoseconds

---

## Capability API

### Types

#### `CapabilityToken`

```rust
pub struct CapabilityToken {
    /// Unique identifier
    pub id: u128,
    /// Resource this capability grants access to
    pub resource: Resource,
    /// Permissions granted
    pub permissions: Permissions,
    /// Expiry time (0 = never)
    pub expires: u64,
    /// Cryptographic signature
    pub signature: [u8; 64],
}
```

#### `Resource`

```rust
pub enum Resource {
    /// File system resource
    File(PathBuf),
    /// Network resource
    Network { address: IpAddr, port: u16 },
    /// Service resource
    Service(String),
    /// Memory region
    Memory { start: usize, size: usize },
    /// Device
    Device(String),
    /// All resources (wildcard)
    All,
}
```

#### `Permissions`

```rust
bitflags! {
    pub struct Permissions: u32 {
        const READ    = 0b0001;
        const WRITE   = 0b0010;
        const EXECUTE = 0b0100;
        const DELETE  = 0b1000;
        const GRANT   = 0b10000;
        const ALL     = 0b11111;
    }
}
```

### Functions

#### `cap_create`

Create a new capability.

```rust
fn cap_create(
    resource: Resource,
    permissions: Permissions,
    expires: Option<Duration>,
) -> Result<CapabilityToken, CapError>
```

**Parameters:**
- `resource`: Resource to grant access to
- `permissions`: Permissions to grant
- `expires`: Optional expiration duration

**Returns:** New capability token or error

---

#### `cap_verify`

Verify a capability is valid.

```rust
fn cap_verify(cap: &CapabilityToken) -> Result<(), CapError>
```

**Parameters:**
- `cap`: Capability to verify

**Returns:** Ok if valid, error otherwise

---

#### `cap_delegate`

Delegate a capability to another process.

```rust
fn cap_delegate(
    cap: &CapabilityToken,
    target: ProcessId,
    attenuation: Option<Permissions>,
) -> Result<CapabilityToken, CapError>
```

**Parameters:**
- `cap`: Capability to delegate
- `target`: Target process
- `attenuation`: Optional permission reduction

**Returns:** Delegated capability

---

#### `cap_revoke`

Revoke a capability.

```rust
fn cap_revoke(cap: &CapabilityToken) -> Result<(), CapError>
```

**Parameters:**
- `cap`: Capability to revoke

---

## IPC API

### Types

#### `Channel`

```rust
pub struct Channel {
    pub id: ChannelId,
    pub endpoint: Endpoint,
    pub caps: Vec<CapabilityToken>,
}
```

#### `Message`

```rust
pub struct Message {
    /// Message type
    pub msg_type: MessageType,
    /// Payload data
    pub payload: Vec<u8>,
    /// Attached capabilities
    pub caps: Vec<CapabilityToken>,
}
```

### Functions

#### `ipc_create_channel`

Create a new IPC channel.

```rust
fn ipc_create_channel() -> Result<(Channel, Channel), IpcError>
```

**Returns:** Tuple of (sender, receiver) endpoints

---

#### `ipc_send`

Send a message on a channel.

```rust
fn ipc_send(
    channel: &Channel,
    message: &Message,
) -> Result<(), IpcError>
```

**Parameters:**
- `channel`: Channel to send on
- `message`: Message to send

---

#### `ipc_receive`

Receive a message from a channel.

```rust
fn ipc_receive(
    channel: &Channel,
    timeout: Option<Duration>,
) -> Result<Message, IpcError>
```

**Parameters:**
- `channel`: Channel to receive from
- `timeout`: Optional timeout

**Returns:** Received message

---

#### `ipc_call`

Send a request and wait for response.

```rust
fn ipc_call<T: Serialize, R: DeserializeOwned>(
    service: &str,
    request: &T,
) -> Result<R, IpcError>
```

**Parameters:**
- `service`: Service name
- `request`: Request payload

**Returns:** Response from service

---

## File System API

### Types

#### `FileHandle`

```rust
pub struct FileHandle {
    pub fd: u64,
    pub path: PathBuf,
    pub mode: OpenMode,
    pub cap: CapabilityToken,
}
```

#### `OpenMode`

```rust
pub struct OpenMode {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
    pub append: bool,
}
```

### Functions

#### `fs_open`

Open a file.

```rust
fn fs_open(
    path: &Path,
    mode: OpenMode,
    cap: &CapabilityToken,
) -> Result<FileHandle, FsError>
```

**Parameters:**
- `path`: File path
- `mode`: Open mode
- `cap`: File capability

**Returns:** File handle

---

#### `fs_read`

Read from a file.

```rust
fn fs_read(
    handle: &FileHandle,
    buffer: &mut [u8],
) -> Result<usize, FsError>
```

**Parameters:**
- `handle`: File handle
- `buffer`: Buffer to read into

**Returns:** Bytes read

---

#### `fs_write`

Write to a file.

```rust
fn fs_write(
    handle: &FileHandle,
    data: &[u8],
) -> Result<usize, FsError>
```

**Parameters:**
- `handle`: File handle
- `data`: Data to write

**Returns:** Bytes written

---

#### `fs_close`

Close a file handle.

```rust
fn fs_close(handle: FileHandle) -> Result<(), FsError>
```

---

#### `fs_stat`

Get file metadata.

```rust
fn fs_stat(path: &Path, cap: &CapabilityToken) -> Result<Metadata, FsError>
```

**Returns:** File metadata

---

## Network API

### Types

#### `SocketHandle`

```rust
pub struct SocketHandle {
    pub id: u64,
    pub socket_type: SocketType,
    pub local_addr: Option<SocketAddr>,
    pub remote_addr: Option<SocketAddr>,
}
```

#### `SocketType`

```rust
pub enum SocketType {
    Tcp,
    Udp,
    Unix,
    Raw,
}
```

### Functions

#### `net_socket`

Create a new socket.

```rust
fn net_socket(
    socket_type: SocketType,
    cap: &CapabilityToken,
) -> Result<SocketHandle, NetError>
```

---

#### `net_bind`

Bind socket to an address.

```rust
fn net_bind(
    socket: &SocketHandle,
    addr: SocketAddr,
) -> Result<(), NetError>
```

---

#### `net_listen`

Listen for incoming connections.

```rust
fn net_listen(
    socket: &SocketHandle,
    backlog: u32,
) -> Result<(), NetError>
```

---

#### `net_accept`

Accept an incoming connection.

```rust
fn net_accept(
    socket: &SocketHandle,
) -> Result<SocketHandle, NetError>
```

---

#### `net_connect`

Connect to a remote address.

```rust
fn net_connect(
    socket: &SocketHandle,
    addr: SocketAddr,
) -> Result<(), NetError>
```

---

#### `net_send`

Send data on a socket.

```rust
fn net_send(
    socket: &SocketHandle,
    data: &[u8],
    flags: SendFlags,
) -> Result<usize, NetError>
```

---

#### `net_recv`

Receive data from a socket.

```rust
fn net_recv(
    socket: &SocketHandle,
    buffer: &mut [u8],
    flags: RecvFlags,
) -> Result<usize, NetError>
```

---

## Process API

### Types

#### `ProcessId`

```rust
pub struct ProcessId(pub u64);
```

#### `ThreadId`

```rust
pub struct ThreadId(pub u64);
```

#### `ProcessInfo`

```rust
pub struct ProcessInfo {
    pub pid: ProcessId,
    pub parent: ProcessId,
    pub name: String,
    pub state: ProcessState,
    pub caps: Vec<CapabilityToken>,
}
```

### Functions

#### `proc_spawn`

Spawn a new process.

```rust
fn proc_spawn(
    binary: &[u8],
    args: &[String],
    caps: &[CapabilityToken],
) -> Result<ProcessId, ProcError>
```

**Parameters:**
- `binary`: WASM or ELF binary
- `args`: Command line arguments
- `caps`: Capabilities to grant

**Returns:** Process ID

---

#### `proc_wait`

Wait for a process to exit.

```rust
fn proc_wait(
    pid: ProcessId,
    timeout: Option<Duration>,
) -> Result<ExitStatus, ProcError>
```

---

#### `proc_kill`

Terminate a process.

```rust
fn proc_kill(
    pid: ProcessId,
    signal: Signal,
) -> Result<(), ProcError>
```

---

#### `proc_self`

Get current process ID.

```rust
fn proc_self() -> ProcessId
```

---

## Memory API

### Functions

#### `mem_alloc`

Allocate memory.

```rust
fn mem_alloc(
    size: usize,
    align: usize,
) -> Result<*mut u8, MemError>
```

---

#### `mem_free`

Free allocated memory.

```rust
fn mem_free(ptr: *mut u8, size: usize) -> Result<(), MemError>
```

---

#### `mem_map`

Map memory region.

```rust
fn mem_map(
    addr: Option<usize>,
    size: usize,
    prot: Protection,
    flags: MapFlags,
) -> Result<*mut u8, MemError>
```

---

#### `mem_unmap`

Unmap memory region.

```rust
fn mem_unmap(
    addr: *mut u8,
    size: usize,
) -> Result<(), MemError>
```

---

#### `mem_protect`

Change memory protection.

```rust
fn mem_protect(
    addr: *mut u8,
    size: usize,
    prot: Protection,
) -> Result<(), MemError>
```

---

## Crypto API

### Hash Functions

#### `crypto_sha256`

Compute SHA-256 hash.

```rust
fn crypto_sha256(data: &[u8]) -> [u8; 32]
```

---

#### `crypto_sha3_256`

Compute SHA3-256 hash.

```rust
fn crypto_sha3_256(data: &[u8]) -> [u8; 32]
```

---

### Symmetric Encryption

#### `crypto_aes_gcm_encrypt`

Encrypt with AES-256-GCM.

```rust
fn crypto_aes_gcm_encrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError>
```

---

#### `crypto_aes_gcm_decrypt`

Decrypt with AES-256-GCM.

```rust
fn crypto_aes_gcm_decrypt(
    key: &[u8; 32],
    nonce: &[u8; 12],
    ciphertext: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError>
```

---

### Asymmetric Cryptography

#### `crypto_ed25519_keygen`

Generate Ed25519 key pair.

```rust
fn crypto_ed25519_keygen() -> (Ed25519PrivateKey, Ed25519PublicKey)
```

---

#### `crypto_ed25519_sign`

Sign with Ed25519.

```rust
fn crypto_ed25519_sign(
    key: &Ed25519PrivateKey,
    message: &[u8],
) -> [u8; 64]
```

---

#### `crypto_ed25519_verify`

Verify Ed25519 signature.

```rust
fn crypto_ed25519_verify(
    key: &Ed25519PublicKey,
    message: &[u8],
    signature: &[u8; 64],
) -> bool
```

---

## Service API

### Types

#### `Service`

```rust
pub trait Service {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn handle(&mut self, msg: Message) -> Response;
    fn start(&mut self) {}
    fn stop(&mut self) {}
}
```

#### `ServiceBuilder`

```rust
pub struct ServiceBuilder<S> {
    name: String,
    version: String,
    handler: Option<S>,
    caps: Vec<CapabilityRequest>,
}
```

### Functions

#### `service_register`

Register a service with S-ATLAS.

```rust
fn service_register(
    name: &str,
    endpoint: &str,
) -> Result<ServiceId, ServiceError>
```

---

#### `service_discover`

Discover a service.

```rust
fn service_discover(
    name: &str,
) -> Result<ServiceInfo, ServiceError>
```

---

#### `service_health`

Report service health.

```rust
fn service_health(
    status: HealthStatus,
) -> Result<(), ServiceError>
```

---

## WASM API

### Types

#### `WasmModule`

```rust
pub struct WasmModule {
    pub name: String,
    pub bytes: Vec<u8>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}
```

#### `WasmInstance`

```rust
pub struct WasmInstance {
    pub module: WasmModule,
    pub memory: WasmMemory,
    pub globals: Vec<WasmGlobal>,
}
```

### Functions

#### `wasm_compile`

Compile WASM module.

```rust
fn wasm_compile(
    bytes: &[u8],
) -> Result<WasmModule, WasmError>
```

---

#### `wasm_instantiate`

Instantiate WASM module.

```rust
fn wasm_instantiate(
    module: &WasmModule,
    imports: &Imports,
) -> Result<WasmInstance, WasmError>
```

---

#### `wasm_call`

Call WASM function.

```rust
fn wasm_call(
    instance: &WasmInstance,
    func: &str,
    args: &[WasmValue],
) -> Result<Vec<WasmValue>, WasmError>
```

---

## Error Types

### Common Error Codes

| Code | Name | Description |
|------|------|-------------|
| 0 | OK | Success |
| 1 | EPERM | Permission denied |
| 2 | ENOENT | No such entity |
| 3 | EINTR | Interrupted |
| 4 | EIO | I/O error |
| 5 | ENOMEM | Out of memory |
| 6 | EACCES | Access denied |
| 7 | EEXIST | Already exists |
| 8 | EINVAL | Invalid argument |
| 9 | EBUSY | Resource busy |
| 10 | ETIMEDOUT | Timeout |

---

*API Reference Version: 1.0*
*Last Updated: January 2026*

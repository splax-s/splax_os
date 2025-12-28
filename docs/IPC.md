# IPC and S-LINK Documentation

## Overview

Splax OS implements a two-layer inter-process communication (IPC) system:

1. **Kernel IPC Primitives** (`kernel/src/ipc/`) - Low-level zero-copy channels
2. **S-LINK Service Layer** (`services/link/`) - High-level service messaging

```text
┌─────────────────────────────────────────────────────────────┐
│                       Services                              │
│  (S-STORAGE, S-GATE, S-ATLAS, User Services)               │
├─────────────────────────────────────────────────────────────┤
│                    S-LINK Layer                             │
│  - Service addressing                                       │
│  - Request/Response patterns                                │
│  - Message routing                                          │
├─────────────────────────────────────────────────────────────┤
│                  Kernel IPC Primitives                      │
│  - Zero-copy message passing                                │
│  - Capability-gated channels                                │
│  - Bounded buffers                                          │
├─────────────────────────────────────────────────────────────┤
│                   S-CAP (Capabilities)                      │
│  - Token-based access control                               │
│  - Operation permissions                                    │
└─────────────────────────────────────────────────────────────┘
```

---

## Design Principles

### Zero-Copy Messaging

Large messages are passed by reference using shared memory, not copied between processes:

```rust
pub enum MessageData {
    /// Inline data (small messages, copied)
    Inline(Vec<u8>),
    /// Shared memory reference (large messages, zero-copy)
    SharedRef {
        addr: u64,  // Physical address of shared memory
        size: usize,
    },
}
```

### Capability-Gated Access

Every IPC operation requires a capability token - you cannot send or receive without explicit authorization:

```rust
pub fn send(
    channel_id: ChannelId,
    sender: ProcessId,
    message: Message,
    cap_token: &CapabilityToken,  // Required for authorization
) -> Result<(), IpcError>
```

### Bounded Buffers

No unbounded queues - explicit backpressure prevents resource exhaustion:

```rust
fn is_full(&self) -> bool {
    self.message_count >= self.buffer.len()
}

fn send(&mut self, message: Message) -> Result<(), IpcError> {
    if self.is_full() {
        return Err(IpcError::BufferFull);
    }
    // ...
}
```

### Deterministic Delivery

Message delivery order is guaranteed via sequence numbers:

```rust
pub struct Message {
    /// Sequence number for ordering
    pub sequence: u64,
    // ...
}
```

---

## Kernel IPC Primitives

### Configuration

```rust
/// IPC configuration
#[derive(Debug, Clone)]
pub struct IpcConfig {
    /// Maximum message size in bytes
    pub max_message_size: usize,    // Default: 64 KB
    /// Maximum number of channels
    pub max_channels: usize,         // Default: 65536
    /// Default channel buffer size (number of messages)
    pub default_buffer_size: usize,  // Default: 16
}
```

### Channel Identifier

```rust
/// Channel identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelId(pub u64);

impl ChannelId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}
```

### Message Structure

```rust
/// A message passed through IPC
#[derive(Debug, Clone)]
pub struct Message {
    /// Sender process
    pub sender: ProcessId,
    /// Message data
    pub data: MessageData,
    /// Optional capability being transferred
    pub capability: Option<CapabilityToken>,
    /// Sequence number for ordering
    pub sequence: u64,
}

impl Message {
    /// Creates a new inline message
    pub fn inline(sender: ProcessId, data: Vec<u8>) -> Self {
        Self {
            sender,
            data: MessageData::Inline(data),
            capability: None,
            sequence: 0,
        }
    }

    /// Creates a new shared memory message
    pub fn shared(sender: ProcessId, addr: u64, size: usize) -> Self {
        Self {
            sender,
            data: MessageData::SharedRef { addr, size },
            capability: None,
            sequence: 0,
        }
    }

    /// Attaches a capability to transfer
    pub fn with_capability(mut self, cap: CapabilityToken) -> Self {
        self.capability = Some(cap);
        self
    }
}
```

### Endpoint Types

```rust
/// Channel endpoint type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointType {
    /// Can only send
    Sender,
    /// Can only receive
    Receiver,
    /// Can both send and receive
    Bidirectional,
}
```

### Channel Types

The IPC subsystem supports three channel patterns:

| Type | Description | Use Case |
|------|-------------|----------|
| **Unidirectional** | One sender, one receiver | Event streams, logs |
| **Bidirectional** | Two endpoints, both can send/receive | Request/response |
| **Broadcast** | One sender, multiple receivers | Event notifications |

### IPC Manager

```rust
/// The IPC manager
pub struct IpcManager {
    config: IpcConfig,
    /// All channels
    channels: Mutex<BTreeMap<ChannelId, Channel>>,
    /// Next channel ID
    next_channel_id: Mutex<u64>,
    /// Capability table reference for access checks
    cap_table: Option<*const CapabilityTable>,
}
```

### Core Operations

#### Create Channel

```rust
impl IpcManager {
    /// Creates a new channel between two processes
    pub fn create_channel(
        &self,
        sender: ProcessId,
        receiver: ProcessId,
        cap_token: &CapabilityToken,
    ) -> Result<ChannelId, IpcError> {
        let mut next_id = self.next_channel_id.lock();
        let id = ChannelId::new(*next_id);
        *next_id += 1;

        let channel = Channel::new(
            id, sender, receiver, 
            self.config.default_buffer_size
        );

        let mut channels = self.channels.lock();
        if channels.len() >= self.config.max_channels {
            return Err(IpcError::TooManyChannels);
        }
        channels.insert(id, channel);

        Ok(id)
    }
}
```

#### Send Message

```rust
impl IpcManager {
    /// Sends a message on a channel
    pub fn send(
        &self,
        channel_id: ChannelId,
        sender: ProcessId,
        message: Message,
        cap_token: &CapabilityToken,
    ) -> Result<(), IpcError> {
        let mut channels = self.channels.lock();
        let channel = channels
            .get_mut(&channel_id)
            .ok_or(IpcError::ChannelNotFound)?;

        // Verify sender
        if channel.sender != sender {
            return Err(IpcError::NotAuthorized);
        }

        // Check message size
        if let MessageData::Inline(ref data) = message.data {
            if data.len() > self.config.max_message_size {
                return Err(IpcError::MessageTooLarge);
            }
        }

        channel.send(message)
    }
}
```

#### Receive Message

```rust
impl IpcManager {
    /// Receives a message from a channel
    pub fn receive(
        &self,
        channel_id: ChannelId,
        receiver: ProcessId,
        cap_token: &CapabilityToken,
    ) -> Result<Message, IpcError> {
        let mut channels = self.channels.lock();
        let channel = channels
            .get_mut(&channel_id)
            .ok_or(IpcError::ChannelNotFound)?;

        // Verify receiver
        if channel.receiver != receiver {
            return Err(IpcError::NotAuthorized);
        }

        channel.receive()
    }
}
```

#### Close Channel

```rust
impl IpcManager {
    /// Closes a channel
    pub fn close(
        &self,
        channel_id: ChannelId,
        closer: ProcessId,
        cap_token: &CapabilityToken,
    ) -> Result<(), IpcError> {
        let mut channels = self.channels.lock();
        let channel = channels
            .get_mut(&channel_id)
            .ok_or(IpcError::ChannelNotFound)?;

        // Either endpoint can close
        if channel.sender != closer && channel.receiver != closer {
            return Err(IpcError::NotAuthorized);
        }

        channel.closed = true;
        Ok(())
    }
}
```

### Channel Statistics

```rust
/// Channel statistics
#[derive(Debug, Clone)]
pub struct ChannelStats {
    pub id: ChannelId,
    pub sender: ProcessId,
    pub receiver: ProcessId,
    pub pending_messages: usize,
    pub buffer_size: usize,
    pub total_sent: u64,
    pub closed: bool,
}
```

### IPC Errors

```rust
/// IPC errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    ChannelNotFound,     // Channel not found
    NotAuthorized,       // Not authorized for this operation
    ChannelClosed,       // Channel is closed
    BufferFull,          // Channel buffer is full
    BufferEmpty,         // Channel buffer is empty
    MessageTooLarge,     // Message exceeds maximum size
    TooManyChannels,     // Too many channels
    InvalidCapability,   // Invalid capability
}
```

### Global IPC Manager

```rust
/// Global IPC manager instance
pub static IPC_MANAGER: Lazy<IpcManager> = Lazy::new(|| {
    IpcManager::new(IpcConfig::default())
});

/// Quick send function for kernel use
pub fn send(
    channel_id: ChannelId,
    sender: ProcessId,
    data: Vec<u8>,
    cap_token: &CapabilityToken,
) -> Result<(), IpcError> {
    let msg = Message::inline(sender, data);
    IPC_MANAGER.send(channel_id, sender, msg, cap_token)
}

/// Quick receive function for kernel use
pub fn receive(
    channel_id: ChannelId,
    receiver: ProcessId,
    cap_token: &CapabilityToken,
) -> Result<Message, IpcError> {
    IPC_MANAGER.receive(channel_id, receiver, cap_token)
}
```

---

## S-LINK Service Layer

S-LINK provides high-level messaging between services, built on top of the kernel IPC primitives. It adds routing, service addressing, and request/response patterns.

### Design Philosophy

- **Capability-Bound**: Every channel requires explicit capability tokens
- **Service-Addressed**: Messages go to service names, not process IDs
- **Request/Response**: Built-in correlation for RPC-style communication
- **Zero-Copy Ready**: Large payloads use shared memory references

### Channel Types

| Type | Description | Use Case |
|------|-------------|----------|
| **Direct** | Point-to-point between two services | Service-to-service calls |
| **Topic** | Pub/sub for event broadcasting | System events |
| **Request** | RPC-style request/response | Synchronous operations |

### Identifiers

```rust
/// Channel identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelId(pub u64);

/// Message identifier for correlation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageId(pub u64);

/// Service identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceId(pub u64);
```

### Message Structure

```rust
/// A message in S-LINK
#[derive(Debug, Clone)]
pub struct Message {
    /// Unique message ID
    pub id: MessageId,
    /// Source service
    pub source: String,
    /// Destination service
    pub destination: String,
    /// Message type
    pub message_type: MessageType,
    /// Payload
    pub payload: Payload,
    /// Correlation ID (for request/response)
    pub correlation_id: Option<MessageId>,
    /// Timestamp (cycles)
    pub timestamp: u64,
}
```

### Message Types

```rust
/// Message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// One-way message
    Send,
    /// Request expecting a response
    Request,
    /// Response to a request
    Response,
    /// Error response
    Error,
    /// Event notification
    Event,
}
```

### Payload Types

```rust
/// Message payload
#[derive(Debug, Clone)]
pub enum Payload {
    /// Inline binary data
    Binary(Vec<u8>),
    /// Inline text data
    Text(String),
    /// Shared memory reference
    SharedMemory {
        addr: u64,
        size: usize,
    },
    /// Empty payload
    Empty,
}

impl Payload {
    pub fn binary(data: Vec<u8>) -> Self {
        Self::Binary(data)
    }

    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    pub fn shared(addr: u64, size: usize) -> Self {
        Self::SharedMemory { addr, size }
    }
}
```

### Channel Configuration

```rust
/// Channel configuration
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Maximum message size
    pub max_message_size: usize,     // Default: 64 KB
    /// Buffer size (number of messages)
    pub buffer_size: usize,           // Default: 32
    /// Request timeout in cycles
    pub default_timeout: u64,         // Default: ~5 seconds at 1GHz
}
```

### Channel Structure

```rust
/// A capability-bound communication channel
pub struct Channel {
    /// Channel ID
    id: ChannelId,
    /// Local service name
    local_service: String,
    /// Remote service name
    remote_service: String,
    /// Channel configuration
    config: ChannelConfig,
    /// Outbound message buffer
    outbound: Mutex<Vec<Message>>,
    /// Inbound message buffer
    inbound: Mutex<Vec<Message>>,
    /// Next message ID
    next_message_id: Mutex<u64>,
    /// Pending requests awaiting response
    pending_requests: Mutex<BTreeMap<MessageId, PendingRequest>>,
    /// Channel is open
    open: Mutex<bool>,
}
```

### Channel Operations

#### Send (One-Way)

```rust
impl Channel {
    /// Sends a one-way message
    pub fn send(&self, payload: Payload, cap_token: &CapabilityToken) 
        -> Result<(), LinkError> 
    {
        if !*self.open.lock() {
            return Err(LinkError::ChannelClosed);
        }

        let message = self.create_message(MessageType::Send, payload, None);
        self.outbound.lock().push(message);
        Ok(())
    }
}
```

#### Request (RPC-Style)

```rust
impl Channel {
    /// Sends a request and waits for response
    pub fn request(
        &self,
        payload: Payload,
        timeout: Option<u64>,
        cap_token: &CapabilityToken,
    ) -> Result<Message, LinkError> {
        if !*self.open.lock() {
            return Err(LinkError::ChannelClosed);
        }

        let message = self.create_message(MessageType::Request, payload, None);
        let msg_id = message.id;
        let timeout_cycles = timeout.unwrap_or(self.config.default_timeout);

        let start_time = Self::get_timestamp();

        // Track pending request
        self.pending_requests.lock().insert(
            msg_id,
            PendingRequest { sent_at: start_time, timeout: timeout_cycles },
        );

        self.outbound.lock().push(message);

        // Poll for response until timeout
        loop {
            // Check for response in inbound queue
            {
                let mut inbound = self.inbound.lock();
                let response_idx = inbound.iter().position(|m| {
                    m.message_type == MessageType::Response && 
                    m.correlation_id == Some(msg_id)
                });
                
                if let Some(idx) = response_idx {
                    self.pending_requests.lock().remove(&msg_id);
                    return Ok(inbound.remove(idx));
                }
            }

            // Check for timeout
            let elapsed = Self::get_timestamp().saturating_sub(start_time);
            if elapsed >= timeout_cycles {
                self.pending_requests.lock().remove(&msg_id);
                return Err(LinkError::Timeout);
            }

            Self::pause_hint();
        }
    }
}
```

#### Receive

```rust
impl Channel {
    /// Receives the next inbound message
    pub fn receive(&self, cap_token: &CapabilityToken) -> Result<Message, LinkError> {
        let mut inbound = self.inbound.lock();
        inbound.pop().ok_or(LinkError::NoMessage)
    }
}
```

#### Respond

```rust
impl Channel {
    /// Responds to a request
    pub fn respond(
        &self,
        request_id: MessageId,
        payload: Payload,
        cap_token: &CapabilityToken,
    ) -> Result<(), LinkError> {
        if !*self.open.lock() {
            return Err(LinkError::ChannelClosed);
        }

        let message = self.create_message(
            MessageType::Response, 
            payload, 
            Some(request_id)
        );
        self.outbound.lock().push(message);
        Ok(())
    }
}
```

### S-LINK Router

The router manages all channels and handles message routing:

```rust
/// The S-LINK router manages all channels
pub struct LinkRouter {
    /// All channels indexed by ID
    channels: Mutex<BTreeMap<ChannelId, Channel>>,
    /// Channels indexed by (local, remote) service pair
    by_endpoint: Mutex<BTreeMap<(String, String), ChannelId>>,
    /// Next channel ID
    next_channel_id: Mutex<u64>,
    /// Default channel config
    default_config: ChannelConfig,
}
```

#### Create Channel

```rust
impl LinkRouter {
    /// Creates a channel between two services
    pub fn create_channel(
        &self,
        local_service: impl Into<String>,
        remote_service: impl Into<String>,
        cap_token: &CapabilityToken,
    ) -> Result<ChannelId, LinkError> {
        let local = local_service.into();
        let remote = remote_service.into();

        // Check if channel already exists
        let key = (local.clone(), remote.clone());
        if self.by_endpoint.lock().contains_key(&key) {
            return Err(LinkError::ChannelExists);
        }

        // Generate ID
        let mut next_id = self.next_channel_id.lock();
        let id = ChannelId::new(*next_id);
        *next_id += 1;

        // Create channel
        let channel = Channel::new(
            id, 
            local.clone(), 
            remote.clone(), 
            self.default_config.clone()
        );

        self.channels.lock().insert(id, channel);
        self.by_endpoint.lock().insert(key, id);

        Ok(id)
    }
}
```

#### Route Message

```rust
impl LinkRouter {
    /// Routes a message to its destination
    pub fn route(&self, message: Message) -> Result<(), LinkError> {
        let key = (message.source.clone(), message.destination.clone());
        let channel_id = self
            .by_endpoint
            .lock()
            .get(&key)
            .copied()
            .ok_or(LinkError::NoRoute)?;

        let channels = self.channels.lock();
        let channel = channels.get(&channel_id).ok_or(LinkError::NoRoute)?;

        channel.outbound.lock().push(message);
        Ok(())
    }
}
```

#### Find Channel

```rust
impl LinkRouter {
    /// Finds a channel by endpoint
    pub fn find_channel(&self, local: &str, remote: &str) -> Option<ChannelId> {
        self.by_endpoint
            .lock()
            .get(&(local.to_string(), remote.to_string()))
            .copied()
    }

    /// Lists all channels
    pub fn list_channels(&self) -> Vec<ChannelStats> {
        self.channels
            .lock()
            .values()
            .map(|c| c.stats())
            .collect()
    }
}
```

### S-LINK Errors

```rust
/// S-LINK errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    ChannelClosed,      // Channel is closed
    ChannelExists,      // Channel already exists
    NoRoute,            // No route to destination
    Timeout,            // Request timed out
    NoMessage,          // No message available
    MessageTooLarge,    // Message too large
    InvalidCapability,  // Invalid capability
    ChannelNotFound,    // Channel not found
}
```

### Channel Statistics

```rust
/// Channel statistics
#[derive(Debug, Clone)]
pub struct ChannelStats {
    pub id: ChannelId,
    pub local_service: String,
    pub remote_service: String,
    pub outbound_pending: usize,
    pub inbound_pending: usize,
    pub pending_requests: usize,
    pub open: bool,
}
```

---

## Architecture Integration

### VFS Integration

The VFS stub uses S-LINK to communicate with the S-STORAGE service:

```text
┌─────────────────────────────────────────┐
│              Application                 │
├─────────────────────────────────────────┤
│              VFS Stub                    │
│  (kernel/src/fs/vfs_stub.rs)            │
├─────────────────────────────────────────┤
│              S-LINK IPC                  │
├─────────────────────────────────────────┤
│            S-STORAGE Service             │
│  (services/storage/)                     │
└─────────────────────────────────────────┘
```

### S-GATE Integration

External requests are routed to internal services via S-LINK:

```text
External Client → TCP/HTTP → S-GATE → S-LINK → Internal Service
```

```rust
/// HTTP request serialized for S-LINK transport
pub struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
```

### WAVE Runtime Integration

WebAssembly modules can access S-LINK through host functions:

```rust
pub enum HostFunction {
    /// Send message on S-LINK channel: (channel_id, ptr, len) -> i32
    SLinkSend,
    /// Receive message from S-LINK channel: (channel_id, ptr, max_len) -> i32
    SLinkReceive,
}
```

---

## Usage Examples

### Create and Use a Channel (Kernel)

```rust
use crate::ipc;

let sender = ProcessId::new(1);
let receiver = ProcessId::new(2);
let cap = CapabilityToken::new([1, 2, 3, 4]);

// Create channel
let channel_id = ipc::create_channel(sender, receiver, &cap)?;

// Send message
ipc::send(channel_id, sender, vec![1, 2, 3, 4], &cap)?;

// Receive message
let msg = ipc::receive(channel_id, receiver, &cap)?;
```

### Service Communication (S-LINK)

```rust
use splax_link::{Channel, Payload, ChannelConfig};

// Create channel to another service
let router = LinkRouter::new(ChannelConfig::default());
let channel_id = router.create_channel(
    "my-service", 
    "storage-service", 
    &cap_token
)?;

// Send request and wait for response
let response = channel.request(
    Payload::text("read /etc/config"),
    Some(5_000_000_000),  // 5 second timeout
    &cap_token
)?;
```

### Zero-Copy Large Message

```rust
// Share large buffer via shared memory
let shared_addr = allocate_shared_memory(1024 * 1024);  // 1 MB
copy_data_to_shared(shared_addr, &large_data);

let msg = Message::shared(sender, shared_addr, large_data.len());
IPC_MANAGER.send(channel_id, sender, msg, &cap_token)?;
```

---

## Shell Commands

```text
channels         - List all S-LINK channels
channels stats   - Show channel statistics  
channels create  - Create a new channel
channels close   - Close a channel
ipc stats        - Show IPC subsystem statistics
```

---

## File Structure

```text
kernel/src/ipc/
└── mod.rs              # Kernel IPC primitives

services/link/
├── Cargo.toml
└── src/
    └── lib.rs          # S-LINK service layer

services/gate/src/
├── network.rs          # S-LINK message types for S-GATE
└── http.rs             # HTTP to S-LINK bridge
```

---

## Performance Considerations

### Inline vs. Shared Memory

| Message Size | Recommended | Reason |
|--------------|-------------|--------|
| < 4 KB | Inline | Low overhead, no TLB flush |
| 4 KB - 64 KB | Either | Depends on copy cost vs. mapping cost |
| > 64 KB | Shared Memory | Avoids large copy |

### Backpressure

When a channel buffer is full, senders receive `BufferFull` error. Strategies:

1. **Retry with backoff**: Exponential backoff before retry
2. **Drop oldest**: For lossy channels (events)
3. **Block**: Wait for space (requires async support)

### Timeout Handling

Request timeouts are tracked per-message:

```rust
struct PendingRequest {
    sent_at: u64,
    timeout: u64,
}
```

---

## Future Work

- [ ] Async/await support for non-blocking operations
- [ ] Priority message queues
- [ ] Flow control with credits
- [ ] Channel persistence across service restarts
- [ ] Multicast/broadcast channels
- [ ] Message tracing and debugging
- [ ] Quality of Service (QoS) guarantees
- [ ] Cross-CPU optimizations (per-CPU queues)

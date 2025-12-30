//! # Distributed IPC
//!
//! Cross-node IPC for Splax clusters. Extends S-LINK for transparent
//! inter-node communication with capability-based security.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Local Process                               │
//! │                  ipc::send(channel, message)                        │
//! └───────────────────────────────┬─────────────────────────────────────┘
//!                                 │
//! ┌───────────────────────────────▼─────────────────────────────────────┐
//! │                      S-LINK Router                                  │
//! │   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐    │
//! │   │ Local Channels  │  │ Remote Channels │  │ Capability      │    │
//! │   │ (same node)     │  │ (cross-node)    │  │ Validator       │    │
//! │   └─────────────────┘  └────────┬────────┘  └─────────────────┘    │
//! └─────────────────────────────────┼───────────────────────────────────┘
//!                                   │
//! ┌─────────────────────────────────▼───────────────────────────────────┐
//! │                    Transport Layer                                  │
//! │   ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐    │
//! │   │ TCP Transport   │  │ QUIC Transport  │  │ Encryption      │    │
//! │   │ (reliable)      │  │ (low latency)   │  │ (ChaCha20)      │    │
//! │   └─────────────────┘  └─────────────────┘  └─────────────────┘    │
//! └─────────────────────────────────┬───────────────────────────────────┘
//!                                   │
//!                            ┌──────▼──────┐
//!                            │  Network    │
//!                            │  (TCP/UDP)  │
//!                            └─────────────┘
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// Node identifier in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

impl NodeId {
    /// Local node ID (placeholder).
    pub const LOCAL: NodeId = NodeId(0);

    /// Creates a new node ID.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Checks if this is the local node.
    pub fn is_local(&self) -> bool {
        self.0 == 0 || self.0 == LOCAL_NODE_ID.load(core::sync::atomic::Ordering::Relaxed)
    }
}

/// Local node ID (set at init).
static LOCAL_NODE_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Sets the local node ID.
pub fn set_local_node_id(id: u64) {
    LOCAL_NODE_ID.store(id, core::sync::atomic::Ordering::Relaxed);
}

/// Gets the local node ID.
pub fn local_node_id() -> NodeId {
    NodeId(LOCAL_NODE_ID.load(core::sync::atomic::Ordering::Relaxed))
}

/// Remote channel endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RemoteEndpoint {
    /// Node ID.
    pub node: NodeId,
    /// Channel ID on the remote node.
    pub channel: u64,
}

impl RemoteEndpoint {
    /// Creates a new remote endpoint.
    pub fn new(node: NodeId, channel: u64) -> Self {
        Self { node, channel }
    }
}

/// Distributed channel ID (globally unique).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlobalChannelId {
    /// Origin node.
    pub node: NodeId,
    /// Local channel ID.
    pub channel: u64,
}

impl GlobalChannelId {
    /// Creates a new global channel ID.
    pub fn new(node: NodeId, channel: u64) -> Self {
        Self { node, channel }
    }

    /// Creates from local channel.
    pub fn local(channel: u64) -> Self {
        Self {
            node: local_node_id(),
            channel,
        }
    }
}

/// Distributed IPC message.
#[derive(Debug, Clone)]
pub struct DistributedMessage {
    /// Message ID.
    pub id: u64,
    /// Source endpoint.
    pub source: GlobalChannelId,
    /// Destination endpoint.
    pub dest: GlobalChannelId,
    /// Message payload.
    pub payload: Vec<u8>,
    /// Attached capabilities (serialized).
    pub capabilities: Vec<SerializedCapability>,
    /// Message flags.
    pub flags: MessageFlags,
    /// Timestamp (for ordering).
    pub timestamp: u64,
}

impl DistributedMessage {
    /// Maximum payload size.
    pub const MAX_PAYLOAD: usize = 64 * 1024;

    /// Creates a new message.
    pub fn new(source: GlobalChannelId, dest: GlobalChannelId, payload: Vec<u8>) -> Self {
        static NEXT_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

        Self {
            id: NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed),
            source,
            dest,
            payload,
            capabilities: Vec::new(),
            flags: MessageFlags::empty(),
            timestamp: 0, // Would use real timestamp
        }
    }

    /// Attaches a capability.
    pub fn with_capability(mut self, cap: SerializedCapability) -> Self {
        self.capabilities.push(cap);
        self
    }

    /// Sets flags.
    pub fn with_flags(mut self, flags: MessageFlags) -> Self {
        self.flags = flags;
        self
    }

    /// Serializes the message for network transmission.
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();

        // Header
        data.extend_from_slice(&self.id.to_le_bytes());
        data.extend_from_slice(&self.source.node.0.to_le_bytes());
        data.extend_from_slice(&self.source.channel.to_le_bytes());
        data.extend_from_slice(&self.dest.node.0.to_le_bytes());
        data.extend_from_slice(&self.dest.channel.to_le_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.extend_from_slice(&(self.flags.bits() as u32).to_le_bytes());

        // Payload length and data
        data.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        data.extend_from_slice(&self.payload);

        // Capabilities
        data.extend_from_slice(&(self.capabilities.len() as u32).to_le_bytes());
        for cap in &self.capabilities {
            let cap_data = cap.serialize();
            data.extend_from_slice(&(cap_data.len() as u32).to_le_bytes());
            data.extend_from_slice(&cap_data);
        }

        data
    }

    /// Deserializes from network data.
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 60 {
            return None;
        }

        let mut offset = 0;

        let id = u64::from_le_bytes(data[offset..offset + 8].try_into().ok()?);
        offset += 8;

        let src_node = u64::from_le_bytes(data[offset..offset + 8].try_into().ok()?);
        offset += 8;
        let src_channel = u64::from_le_bytes(data[offset..offset + 8].try_into().ok()?);
        offset += 8;

        let dest_node = u64::from_le_bytes(data[offset..offset + 8].try_into().ok()?);
        offset += 8;
        let dest_channel = u64::from_le_bytes(data[offset..offset + 8].try_into().ok()?);
        offset += 8;

        let timestamp = u64::from_le_bytes(data[offset..offset + 8].try_into().ok()?);
        offset += 8;

        let flags_bits = u32::from_le_bytes(data[offset..offset + 4].try_into().ok()?);
        offset += 4;

        let payload_len = u32::from_le_bytes(data[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;

        if data.len() < offset + payload_len {
            return None;
        }

        let payload = data[offset..offset + payload_len].to_vec();
        offset += payload_len;

        let cap_count = u32::from_le_bytes(data[offset..offset + 4].try_into().ok()?) as usize;
        offset += 4;

        let mut capabilities = Vec::with_capacity(cap_count);
        for _ in 0..cap_count {
            if data.len() < offset + 4 {
                return None;
            }
            let cap_len = u32::from_le_bytes(data[offset..offset + 4].try_into().ok()?) as usize;
            offset += 4;

            if data.len() < offset + cap_len {
                return None;
            }
            if let Some(cap) = SerializedCapability::deserialize(&data[offset..offset + cap_len]) {
                capabilities.push(cap);
            }
            offset += cap_len;
        }

        Some(Self {
            id,
            source: GlobalChannelId::new(NodeId(src_node), src_channel),
            dest: GlobalChannelId::new(NodeId(dest_node), dest_channel),
            payload,
            capabilities,
            flags: MessageFlags::from_bits_truncate(flags_bits),
            timestamp,
        })
    }
}

bitflags::bitflags! {
    /// Message flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MessageFlags: u32 {
        /// Request/response - expects a reply.
        const REQUEST = 1 << 0;
        /// This is a response to a request.
        const RESPONSE = 1 << 1;
        /// High priority - use fast path.
        const PRIORITY = 1 << 2;
        /// Encrypted payload.
        const ENCRYPTED = 1 << 3;
        /// Compressed payload.
        const COMPRESSED = 1 << 4;
        /// One-way message, no ack needed.
        const ONEWAY = 1 << 5;
        /// Stream message (part of larger stream).
        const STREAM = 1 << 6;
        /// End of stream.
        const END_STREAM = 1 << 7;
    }
}

/// Serialized capability for cross-node transfer.
#[derive(Debug, Clone)]
pub struct SerializedCapability {
    /// Capability token.
    pub token: [u8; 32],
    /// Resource type.
    pub resource_type: u16,
    /// Operations mask.
    pub operations: u64,
    /// Expiration timestamp (0 = no expiry).
    pub expires: u64,
    /// Origin node.
    pub origin_node: NodeId,
    /// Signature for verification.
    pub signature: [u8; 64],
}

impl SerializedCapability {
    /// Serializes to bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(128);
        data.extend_from_slice(&self.token);
        data.extend_from_slice(&self.resource_type.to_le_bytes());
        data.extend_from_slice(&self.operations.to_le_bytes());
        data.extend_from_slice(&self.expires.to_le_bytes());
        data.extend_from_slice(&self.origin_node.0.to_le_bytes());
        data.extend_from_slice(&self.signature);
        data
    }

    /// Deserializes from bytes.
    pub fn deserialize(data: &[u8]) -> Option<Self> {
        if data.len() < 122 {
            return None;
        }

        let mut token = [0u8; 32];
        token.copy_from_slice(&data[0..32]);

        let resource_type = u16::from_le_bytes(data[32..34].try_into().ok()?);
        let operations = u64::from_le_bytes(data[34..42].try_into().ok()?);
        let expires = u64::from_le_bytes(data[42..50].try_into().ok()?);
        let origin_node = u64::from_le_bytes(data[50..58].try_into().ok()?);

        let mut signature = [0u8; 64];
        signature.copy_from_slice(&data[58..122]);

        Some(Self {
            token,
            resource_type,
            operations,
            expires,
            origin_node: NodeId(origin_node),
            signature,
        })
    }
}

/// Remote node connection.
#[derive(Debug)]
pub struct RemoteNode {
    /// Node ID.
    pub id: NodeId,
    /// Node address.
    pub address: NodeAddress,
    /// Connection state.
    pub state: ConnectionState,
    /// Shared encryption key.
    pub session_key: Option<[u8; 32]>,
    /// Message sequence number.
    pub send_seq: u64,
    /// Last received sequence.
    pub recv_seq: u64,
    /// Pending acknowledgments.
    pub pending_acks: Vec<u64>,
    /// Round-trip time estimate (ms).
    pub rtt_ms: u32,
}

/// Node network address.
#[derive(Debug, Clone)]
pub struct NodeAddress {
    pub ip: [u8; 4],
    pub port: u16,
}

impl NodeAddress {
    pub fn new(ip: [u8; 4], port: u16) -> Self {
        Self { ip, port }
    }
}

/// Connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected.
    Disconnected,
    /// Handshake in progress.
    Connecting,
    /// Connected and ready.
    Connected,
    /// Reconnecting after failure.
    Reconnecting,
    /// Failed and not retrying.
    Failed,
}

/// Distributed IPC router.
pub struct DistributedRouter {
    /// Remote node connections.
    nodes: Mutex<BTreeMap<NodeId, RemoteNode>>,
    /// Channel routing table (global channel -> handler).
    routes: Mutex<BTreeMap<GlobalChannelId, ChannelRoute>>,
    /// Pending requests (for request/response pattern).
    pending_requests: Mutex<BTreeMap<u64, PendingRequest>>,
    /// Message handlers.
    handlers: Mutex<BTreeMap<u64, MessageHandler>>,
    /// Configuration.
    config: RouterConfig,
}

/// Channel route.
#[derive(Debug, Clone)]
pub struct ChannelRoute {
    /// Local handler ID (if local).
    pub local_handler: Option<u64>,
    /// Remote endpoint (if remote).
    pub remote: Option<RemoteEndpoint>,
    /// Route type.
    pub route_type: RouteType,
}

/// Route type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteType {
    /// Direct local channel.
    Local,
    /// Remote channel on another node.
    Remote,
    /// Replicated across nodes.
    Replicated,
    /// Load balanced across nodes.
    LoadBalanced,
}

/// Message handler function type.
pub type MessageHandler = fn(DistributedMessage) -> Option<DistributedMessage>;

/// Pending request.
#[derive(Debug)]
struct PendingRequest {
    message_id: u64,
    dest: GlobalChannelId,
    sent_at: u64,
    timeout_ms: u64,
    response: Option<DistributedMessage>,
}

/// Router configuration.
#[derive(Debug, Clone)]
pub struct RouterConfig {
    /// Request timeout (ms).
    pub request_timeout_ms: u64,
    /// Maximum retries.
    pub max_retries: u8,
    /// Enable encryption.
    pub encryption_enabled: bool,
    /// Enable compression.
    pub compression_enabled: bool,
    /// Maximum message size.
    pub max_message_size: usize,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            request_timeout_ms: 5000,
            max_retries: 3,
            encryption_enabled: true,
            compression_enabled: false,
            max_message_size: 64 * 1024,
        }
    }
}

impl DistributedRouter {
    /// Creates a new router.
    pub fn new(config: RouterConfig) -> Self {
        Self {
            nodes: Mutex::new(BTreeMap::new()),
            routes: Mutex::new(BTreeMap::new()),
            pending_requests: Mutex::new(BTreeMap::new()),
            handlers: Mutex::new(BTreeMap::new()),
            config,
        }
    }

    /// Registers a remote node.
    pub fn add_node(&self, id: NodeId, address: NodeAddress) {
        let node = RemoteNode {
            id,
            address,
            state: ConnectionState::Disconnected,
            session_key: None,
            send_seq: 0,
            recv_seq: 0,
            pending_acks: Vec::new(),
            rtt_ms: 0,
        };
        self.nodes.lock().insert(id, node);
    }

    /// Removes a node.
    pub fn remove_node(&self, id: NodeId) {
        self.nodes.lock().remove(&id);
    }

    /// Registers a local channel handler.
    pub fn register_handler(&self, channel: u64, handler: MessageHandler) {
        self.handlers.lock().insert(channel, handler);

        let route = ChannelRoute {
            local_handler: Some(channel),
            remote: None,
            route_type: RouteType::Local,
        };
        self.routes.lock().insert(GlobalChannelId::local(channel), route);
    }

    /// Registers a route to a remote channel.
    pub fn register_remote_route(&self, local_channel: u64, remote: RemoteEndpoint) {
        let route = ChannelRoute {
            local_handler: None,
            remote: Some(remote),
            route_type: RouteType::Remote,
        };
        self.routes.lock().insert(GlobalChannelId::local(local_channel), route);
    }

    /// Sends a message.
    pub fn send(&self, msg: DistributedMessage) -> Result<(), DistributedError> {
        if msg.payload.len() > self.config.max_message_size {
            return Err(DistributedError::MessageTooLarge);
        }

        // Check if destination is local
        if msg.dest.node.is_local() {
            return self.deliver_local(msg);
        }

        // Send to remote node
        self.send_remote(msg)
    }

    /// Sends a request and waits for response.
    pub fn request(&self, mut msg: DistributedMessage) -> Result<DistributedMessage, DistributedError> {
        msg.flags |= MessageFlags::REQUEST;

        // Register pending request
        let request = PendingRequest {
            message_id: msg.id,
            dest: msg.dest,
            sent_at: 0, // Would use real timestamp
            timeout_ms: self.config.request_timeout_ms,
            response: None,
        };
        self.pending_requests.lock().insert(msg.id, request);

        // Send message
        self.send(msg.clone())?;

        // In async version, would await response
        // For now, return error (would need async runtime)
        Err(DistributedError::Timeout)
    }

    /// Delivers a message locally.
    fn deliver_local(&self, msg: DistributedMessage) -> Result<(), DistributedError> {
        let handlers = self.handlers.lock();
        let handler = handlers
            .get(&msg.dest.channel)
            .ok_or(DistributedError::ChannelNotFound)?;

        // Call handler
        if let Some(response) = handler(msg) {
            // Would send response back
            let _ = self.send(response);
        }

        Ok(())
    }

    /// Sends to a remote node.
    fn send_remote(&self, msg: DistributedMessage) -> Result<(), DistributedError> {
        let mut nodes = self.nodes.lock();
        let node = nodes
            .get_mut(&msg.dest.node)
            .ok_or(DistributedError::NodeNotFound)?;

        if node.state != ConnectionState::Connected {
            return Err(DistributedError::NotConnected);
        }

        // Serialize message
        let mut data = msg.serialize();

        // Encrypt if enabled
        if self.config.encryption_enabled {
            if let Some(key) = &node.session_key {
                data = encrypt_message(&data, key);
            }
        }

        // Update sequence
        node.send_seq += 1;

        // Would send via network here
        // self.transport.send(node.address, data);

        Ok(())
    }

    /// Handles an incoming message from the network.
    pub fn handle_incoming(&self, from_node: NodeId, data: &[u8]) -> Result<(), DistributedError> {
        // Decrypt if needed
        let data = if self.config.encryption_enabled {
            let nodes = self.nodes.lock();
            if let Some(node) = nodes.get(&from_node) {
                if let Some(key) = &node.session_key {
                    decrypt_message(data, key)
                } else {
                    data.to_vec()
                }
            } else {
                data.to_vec()
            }
        } else {
            data.to_vec()
        };

        // Deserialize
        let msg = DistributedMessage::deserialize(&data)
            .ok_or(DistributedError::InvalidMessage)?;

        // Check if this is a response
        if msg.flags.contains(MessageFlags::RESPONSE) {
            let mut pending = self.pending_requests.lock();
            if let Some(request) = pending.get_mut(&msg.id) {
                request.response = Some(msg);
                return Ok(());
            }
        }

        // Deliver to local handler
        self.deliver_local(msg)
    }

    /// Gets connected node count.
    pub fn connected_nodes(&self) -> usize {
        self.nodes
            .lock()
            .values()
            .filter(|n| n.state == ConnectionState::Connected)
            .count()
    }

    /// Gets router statistics.
    pub fn stats(&self) -> RouterStats {
        let nodes = self.nodes.lock();
        RouterStats {
            total_nodes: nodes.len(),
            connected_nodes: nodes.values().filter(|n| n.state == ConnectionState::Connected).count(),
            registered_handlers: self.handlers.lock().len(),
            pending_requests: self.pending_requests.lock().len(),
        }
    }
}

/// Router statistics.
#[derive(Debug, Clone)]
pub struct RouterStats {
    pub total_nodes: usize,
    pub connected_nodes: usize,
    pub registered_handlers: usize,
    pub pending_requests: usize,
}

/// Distributed IPC errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistributedError {
    /// Destination channel not found.
    ChannelNotFound,
    /// Remote node not found.
    NodeNotFound,
    /// Not connected to remote node.
    NotConnected,
    /// Request timed out.
    Timeout,
    /// Message too large.
    MessageTooLarge,
    /// Invalid message format.
    InvalidMessage,
    /// Capability validation failed.
    CapabilityDenied,
    /// Encryption error.
    EncryptionError,
    /// Network error.
    NetworkError,
}

/// Placeholder encryption (would use ChaCha20).
fn encrypt_message(data: &[u8], _key: &[u8; 32]) -> Vec<u8> {
    // Would use kernel crypto module
    data.to_vec()
}

/// Placeholder decryption.
fn decrypt_message(data: &[u8], _key: &[u8; 32]) -> Vec<u8> {
    data.to_vec()
}

/// Global distributed router.
static DISTRIBUTED_ROUTER: Mutex<Option<DistributedRouter>> = Mutex::new(None);

/// Initializes the distributed IPC subsystem.
pub fn init(node_id: u64, config: RouterConfig) {
    set_local_node_id(node_id);
    *DISTRIBUTED_ROUTER.lock() = Some(DistributedRouter::new(config));
}

/// Gets the distributed router.
pub fn router() -> &'static Mutex<Option<DistributedRouter>> {
    &DISTRIBUTED_ROUTER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialize_deserialize() {
        let msg = DistributedMessage::new(
            GlobalChannelId::new(NodeId(1), 100),
            GlobalChannelId::new(NodeId(2), 200),
            b"Hello, distributed!".to_vec(),
        );

        let data = msg.serialize();
        let msg2 = DistributedMessage::deserialize(&data).unwrap();

        assert_eq!(msg.id, msg2.id);
        assert_eq!(msg.source, msg2.source);
        assert_eq!(msg.dest, msg2.dest);
        assert_eq!(msg.payload, msg2.payload);
    }

    #[test]
    fn test_router() {
        let router = DistributedRouter::new(RouterConfig::default());

        router.add_node(NodeId(1), NodeAddress::new([192, 168, 1, 1], 8080));
        router.add_node(NodeId(2), NodeAddress::new([192, 168, 1, 2], 8080));

        assert_eq!(router.stats().total_nodes, 2);
    }

    #[test]
    fn test_capability_serialization() {
        let cap = SerializedCapability {
            token: [0x42; 32],
            resource_type: 1,
            operations: 0xFF,
            expires: 0,
            origin_node: NodeId(1),
            signature: [0; 64],
        };

        let data = cap.serialize();
        let cap2 = SerializedCapability::deserialize(&data).unwrap();

        assert_eq!(cap.token, cap2.token);
        assert_eq!(cap.resource_type, cap2.resource_type);
        assert_eq!(cap.operations, cap2.operations);
    }
}

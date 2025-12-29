//! # S-LINK: Internal Capability Messaging
//!
//! S-LINK provides high-level messaging between services, built on top of
//! the kernel's IPC primitives. It adds routing, service addressing, and
//! request/response patterns.
//!
//! ## Design Philosophy
//!
//! - **Capability-Bound**: Every channel requires explicit capability tokens
//! - **Service-Addressed**: Messages go to service names, not process IDs
//! - **Request/Response**: Built-in correlation for RPC-style communication
//! - **Zero-Copy Ready**: Large payloads use shared memory references
//!
//! ## Channel Types
//!
//! - **Direct**: Point-to-point between two services
//! - **Topic**: Pub/sub for event broadcasting
//! - **Request**: RPC-style request/response
//!
//! ## Example
//!
//! ```ignore
//! // Create a channel to another service
//! let channel = s_link::Channel::create(
//!     "auth-service",
//!     my_cap_token,
//! )?;
//!
//! // Send a request
//! let response = channel.request(message, timeout)?;
//! ```

#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use spin::Mutex;

// Import shared capability token
pub use splax_cap::{CapabilityToken, Operations, Permission};

/// Channel identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelId(pub u64);

impl ChannelId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Message identifier for correlation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MessageId(pub u64);

/// Service identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceId(pub u64);

/// A message in S-LINK.
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

/// Message types.
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

/// Message payload.
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

/// Channel configuration.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    /// Maximum message size
    pub max_message_size: usize,
    /// Buffer size (number of messages)
    pub buffer_size: usize,
    /// Request timeout in cycles
    pub default_timeout: u64,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            max_message_size: 64 * 1024,
            buffer_size: 32,
            default_timeout: 5_000_000_000, // ~5 seconds at 1GHz
        }
    }
}

/// A capability-bound communication channel.
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

struct PendingRequest {
    sent_at: u64,
    timeout: u64,
}

impl Channel {
    /// Creates a new channel.
    pub fn new(
        id: ChannelId,
        local_service: String,
        remote_service: String,
        config: ChannelConfig,
    ) -> Self {
        Self {
            id,
            local_service,
            remote_service,
            config,
            outbound: Mutex::new(Vec::new()),
            inbound: Mutex::new(Vec::new()),
            next_message_id: Mutex::new(1),
            pending_requests: Mutex::new(BTreeMap::new()),
            open: Mutex::new(true),
        }
    }

    /// Sends a one-way message.
    pub fn send(&self, payload: Payload, _cap_token: &CapabilityToken) -> Result<(), LinkError> {
        if !*self.open.lock() {
            return Err(LinkError::ChannelClosed);
        }

        let message = self.create_message(MessageType::Send, payload, None);
        self.outbound.lock().push(message);
        Ok(())
    }

    /// Sends a request and waits for response.
    pub fn request(
        &self,
        payload: Payload,
        timeout: Option<u64>,
        _cap_token: &CapabilityToken,
    ) -> Result<Message, LinkError> {
        if !*self.open.lock() {
            return Err(LinkError::ChannelClosed);
        }

        let message = self.create_message(MessageType::Request, payload, None);
        let msg_id = message.id;
        let timeout_cycles = timeout.unwrap_or(self.config.default_timeout);

        // Get current timestamp for timeout tracking
        let start_time = Self::get_timestamp();

        // Track pending request
        self.pending_requests.lock().insert(
            msg_id,
            PendingRequest {
                sent_at: start_time,
                timeout: timeout_cycles,
            },
        );

        self.outbound.lock().push(message);

        // Poll for response until timeout
        loop {
            // Check for response in inbound queue
            {
                let mut inbound = self.inbound.lock();
                // Find response matching our request ID
                let response_idx = inbound.iter().position(|m| {
                    m.message_type == MessageType::Response && m.correlation_id == Some(msg_id)
                });
                
                if let Some(idx) = response_idx {
                    // Found our response - remove from pending and return
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

            // Brief pause before next poll (avoid busy-spin)
            Self::pause_hint();
        }
    }

    /// Get current timestamp (CPU cycles).
    #[cfg(target_arch = "x86_64")]
    fn get_timestamp() -> u64 {
        unsafe { core::arch::x86_64::_rdtsc() }
    }

    #[cfg(target_arch = "aarch64")]
    fn get_timestamp() -> u64 {
        let cnt: u64;
        unsafe {
            core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt, options(nostack, nomem));
        }
        cnt
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    fn get_timestamp() -> u64 {
        0 // Fallback - would need external time source
    }

    /// CPU pause hint for spin-wait loops.
    #[cfg(target_arch = "x86_64")]
    fn pause_hint() {
        unsafe { core::arch::x86_64::_mm_pause() }
    }

    #[cfg(target_arch = "aarch64")]
    fn pause_hint() {
        unsafe {
            core::arch::asm!("yield", options(nostack, nomem));
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    fn pause_hint() {
        // No-op on other architectures
    }

    /// Receives the next inbound message.
    pub fn receive(&self, _cap_token: &CapabilityToken) -> Result<Message, LinkError> {
        let mut inbound = self.inbound.lock();
        inbound.pop().ok_or(LinkError::NoMessage)
    }

    /// Responds to a request.
    pub fn respond(
        &self,
        request_id: MessageId,
        payload: Payload,
        _cap_token: &CapabilityToken,
    ) -> Result<(), LinkError> {
        if !*self.open.lock() {
            return Err(LinkError::ChannelClosed);
        }

        let message = self.create_message(MessageType::Response, payload, Some(request_id));
        self.outbound.lock().push(message);
        Ok(())
    }

    /// Closes the channel.
    pub fn close(&self, _cap_token: &CapabilityToken) -> Result<(), LinkError> {
        *self.open.lock() = false;
        Ok(())
    }

    /// Gets channel statistics.
    pub fn stats(&self) -> ChannelStats {
        ChannelStats {
            id: self.id,
            local_service: self.local_service.clone(),
            remote_service: self.remote_service.clone(),
            outbound_pending: self.outbound.lock().len(),
            inbound_pending: self.inbound.lock().len(),
            pending_requests: self.pending_requests.lock().len(),
            open: *self.open.lock(),
        }
    }

    fn create_message(
        &self,
        message_type: MessageType,
        payload: Payload,
        correlation_id: Option<MessageId>,
    ) -> Message {
        let mut next_id = self.next_message_id.lock();
        let id = MessageId(*next_id);
        *next_id += 1;

        Message {
            id,
            source: self.local_service.clone(),
            destination: self.remote_service.clone(),
            message_type,
            payload,
            correlation_id,
            timestamp: Self::get_timestamp(),
        }
    }
}

/// Channel statistics.
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

/// The S-LINK router manages all channels.
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

impl LinkRouter {
    /// Creates a new router.
    pub fn new(default_config: ChannelConfig) -> Self {
        Self {
            channels: Mutex::new(BTreeMap::new()),
            by_endpoint: Mutex::new(BTreeMap::new()),
            next_channel_id: Mutex::new(1),
            default_config,
        }
    }

    /// Creates a channel between two services.
    pub fn create_channel(
        &self,
        local_service: impl Into<String>,
        remote_service: impl Into<String>,
        _cap_token: &CapabilityToken,
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
        let channel = Channel::new(id, local.clone(), remote.clone(), self.default_config.clone());

        self.channels.lock().insert(id, channel);
        self.by_endpoint.lock().insert(key, id);

        Ok(id)
    }

    /// Gets a channel by ID.
    pub fn get_channel(&self, id: ChannelId) -> Option<ChannelId> {
        if self.channels.lock().contains_key(&id) {
            Some(id)
        } else {
            None
        }
    }

    /// Finds a channel by endpoint.
    pub fn find_channel(&self, local: &str, remote: &str) -> Option<ChannelId> {
        self.by_endpoint
            .lock()
            .get(&(local.to_string(), remote.to_string()))
            .copied()
    }

    /// Routes a message to its destination.
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

    /// Lists all channels.
    pub fn list_channels(&self) -> Vec<ChannelStats> {
        self.channels
            .lock()
            .values()
            .map(|c| c.stats())
            .collect()
    }
}

/// S-LINK errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    /// Channel is closed
    ChannelClosed,
    /// Channel already exists
    ChannelExists,
    /// No route to destination
    NoRoute,
    /// Request timed out
    Timeout,
    /// No message available
    NoMessage,
    /// Message too large
    MessageTooLarge,
    /// Invalid capability
    InvalidCapability,
    /// Channel not found
    ChannelNotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken::new([1, 2, 3, 4])
    }

    #[test]
    fn test_channel_creation() {
        let router = LinkRouter::new(ChannelConfig::default());
        let token = dummy_token();

        let id = router
            .create_channel("service-a", "service-b", &token)
            .expect("should create channel");

        assert!(router.get_channel(id).is_some());
    }

    #[test]
    fn test_duplicate_channel_fails() {
        let router = LinkRouter::new(ChannelConfig::default());
        let token = dummy_token();

        router
            .create_channel("service-a", "service-b", &token)
            .expect("first should succeed");

        let result = router.create_channel("service-a", "service-b", &token);
        assert_eq!(result, Err(LinkError::ChannelExists));
    }
}

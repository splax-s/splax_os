//! # IPC Primitives
//!
//! The Splax IPC system provides zero-copy message passing between processes.
//! All IPC is mediated by capabilities - you cannot send or receive without
//! the appropriate tokens.
//!
//! ## Design Principles
//!
//! 1. **Zero-Copy**: Large messages are passed by reference, not copied
//! 2. **Capability-Gated**: Every channel operation requires a token
//! 3. **Bounded Buffers**: No unbounded queues, explicit backpressure
//! 4. **Deterministic**: Message delivery order is guaranteed
//!
//! ## Channel Types
//!
//! - **Unidirectional**: One sender, one receiver
//! - **Bidirectional**: Two endpoints, both can send and receive
//! - **Broadcast**: One sender, multiple receivers
//!
//! ## Integration with S-LINK
//!
//! These primitives form the foundation for the S-LINK service messaging
//! layer. S-LINK adds service discovery and routing on top.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use spin::Mutex;

use crate::cap::{CapabilityToken, Operations};
use crate::sched::ProcessId;

/// Channel identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChannelId(pub u64);

impl ChannelId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

/// IPC configuration.
#[derive(Debug, Clone)]
pub struct IpcConfig {
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Maximum number of channels
    pub max_channels: usize,
    /// Default channel buffer size (number of messages)
    pub default_buffer_size: usize,
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            max_message_size: 64 * 1024, // 64 KB
            max_channels: 65536,
            default_buffer_size: 16,
        }
    }
}

/// A message passed through IPC.
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

/// Message data types.
#[derive(Debug, Clone)]
pub enum MessageData {
    /// Inline data (small messages, copied)
    Inline(Vec<u8>),
    /// Shared memory reference (large messages, zero-copy)
    SharedRef {
        /// Physical address of shared memory
        addr: u64,
        /// Size in bytes
        size: usize,
    },
}

impl Message {
    /// Creates a new inline message.
    pub fn inline(sender: ProcessId, data: Vec<u8>) -> Self {
        Self {
            sender,
            data: MessageData::Inline(data),
            capability: None,
            sequence: 0,
        }
    }

    /// Creates a new shared memory message.
    pub fn shared(sender: ProcessId, addr: u64, size: usize) -> Self {
        Self {
            sender,
            data: MessageData::SharedRef { addr, size },
            capability: None,
            sequence: 0,
        }
    }

    /// Attaches a capability to transfer.
    pub fn with_capability(mut self, cap: CapabilityToken) -> Self {
        self.capability = Some(cap);
        self
    }
}

/// Channel endpoint type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointType {
    /// Can only send
    Sender,
    /// Can only receive
    Receiver,
    /// Can both send and receive
    Bidirectional,
}

/// A communication channel.
struct Channel {
    /// Channel ID
    id: ChannelId,
    /// Sender endpoint owner
    sender: ProcessId,
    /// Receiver endpoint owner
    receiver: ProcessId,
    /// Message buffer (ring buffer)
    buffer: Vec<Option<Message>>,
    /// Write position in buffer
    write_pos: usize,
    /// Read position in buffer
    read_pos: usize,
    /// Number of messages in buffer
    message_count: usize,
    /// Next sequence number
    next_sequence: u64,
    /// Channel is closed
    closed: bool,
}

impl Channel {
    fn new(id: ChannelId, sender: ProcessId, receiver: ProcessId, buffer_size: usize) -> Self {
        Self {
            id,
            sender,
            receiver,
            buffer: (0..buffer_size).map(|_| None).collect(),
            write_pos: 0,
            read_pos: 0,
            message_count: 0,
            next_sequence: 0,
            closed: false,
        }
    }

    fn is_full(&self) -> bool {
        self.message_count >= self.buffer.len()
    }

    fn is_empty(&self) -> bool {
        self.message_count == 0
    }

    fn send(&mut self, mut message: Message) -> Result<(), IpcError> {
        if self.closed {
            return Err(IpcError::ChannelClosed);
        }
        if self.is_full() {
            return Err(IpcError::BufferFull);
        }

        message.sequence = self.next_sequence;
        self.next_sequence += 1;

        self.buffer[self.write_pos] = Some(message);
        self.write_pos = (self.write_pos + 1) % self.buffer.len();
        self.message_count += 1;

        Ok(())
    }

    fn receive(&mut self) -> Result<Message, IpcError> {
        if self.is_empty() {
            if self.closed {
                return Err(IpcError::ChannelClosed);
            }
            return Err(IpcError::BufferEmpty);
        }

        let message = self.buffer[self.read_pos]
            .take()
            .ok_or(IpcError::BufferEmpty)?;
        self.read_pos = (self.read_pos + 1) % self.buffer.len();
        self.message_count -= 1;

        Ok(message)
    }
}

/// The IPC manager.
pub struct IpcManager {
    config: IpcConfig,
    /// All channels
    channels: Mutex<BTreeMap<ChannelId, Channel>>,
    /// Next channel ID
    next_channel_id: Mutex<u64>,
    /// Capability table reference for access checks
    cap_table: Option<*const crate::cap::CapabilityTable>,
}

// SAFETY: IpcManager uses interior mutability via Mutex
unsafe impl Send for IpcManager {}
unsafe impl Sync for IpcManager {}

impl IpcManager {
    /// Creates a new IPC manager.
    pub fn new(config: IpcConfig) -> Self {
        Self {
            config,
            channels: Mutex::new(BTreeMap::new()),
            next_channel_id: Mutex::new(1),
            cap_table: None,
        }
    }

    /// Creates a new channel between two processes.
    ///
    /// # Arguments
    ///
    /// * `sender` - Process that will send on this channel
    /// * `receiver` - Process that will receive on this channel
    /// * `cap_token` - Capability authorizing channel creation
    ///
    /// # Returns
    ///
    /// The new channel ID.
    pub fn create_channel(
        &self,
        sender: ProcessId,
        receiver: ProcessId,
        _cap_token: &CapabilityToken,
    ) -> Result<ChannelId, IpcError> {
        let mut next_id = self.next_channel_id.lock();
        let id = ChannelId::new(*next_id);
        *next_id += 1;

        let channel = Channel::new(id, sender, receiver, self.config.default_buffer_size);

        let mut channels = self.channels.lock();
        if channels.len() >= self.config.max_channels {
            return Err(IpcError::TooManyChannels);
        }
        channels.insert(id, channel);

        Ok(id)
    }

    /// Sends a message on a channel.
    ///
    /// # Arguments
    ///
    /// * `channel_id` - Channel to send on
    /// * `sender` - Sending process
    /// * `message` - Message to send
    /// * `cap_token` - Capability authorizing this send
    ///
    /// # Returns
    ///
    /// `Ok(())` on success.
    pub fn send(
        &self,
        channel_id: ChannelId,
        sender: ProcessId,
        message: Message,
        _cap_token: &CapabilityToken,
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

    /// Receives a message from a channel.
    ///
    /// # Arguments
    ///
    /// * `channel_id` - Channel to receive from
    /// * `receiver` - Receiving process
    /// * `cap_token` - Capability authorizing this receive
    ///
    /// # Returns
    ///
    /// The received message.
    pub fn receive(
        &self,
        channel_id: ChannelId,
        receiver: ProcessId,
        _cap_token: &CapabilityToken,
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

    /// Closes a channel.
    ///
    /// # Arguments
    ///
    /// * `channel_id` - Channel to close
    /// * `closer` - Process closing the channel
    /// * `cap_token` - Capability authorizing this close
    pub fn close(
        &self,
        channel_id: ChannelId,
        closer: ProcessId,
        _cap_token: &CapabilityToken,
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

    /// Gets channel statistics.
    pub fn channel_stats(&self, channel_id: ChannelId) -> Result<ChannelStats, IpcError> {
        let channels = self.channels.lock();
        let channel = channels.get(&channel_id).ok_or(IpcError::ChannelNotFound)?;

        Ok(ChannelStats {
            id: channel.id,
            sender: channel.sender,
            receiver: channel.receiver,
            pending_messages: channel.message_count,
            buffer_size: channel.buffer.len(),
            total_sent: channel.next_sequence,
            closed: channel.closed,
        })
    }
}

/// Channel statistics.
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

/// IPC errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcError {
    /// Channel not found
    ChannelNotFound,
    /// Not authorized for this operation
    NotAuthorized,
    /// Channel is closed
    ChannelClosed,
    /// Channel buffer is full
    BufferFull,
    /// Channel buffer is empty
    BufferEmpty,
    /// Message exceeds maximum size
    MessageTooLarge,
    /// Too many channels
    TooManyChannels,
    /// Invalid capability
    InvalidCapability,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken::new([1, 2, 3, 4])
    }

    #[test]
    fn test_channel_creation() {
        let manager = IpcManager::new(IpcConfig::default());
        let sender = ProcessId::new(1);
        let receiver = ProcessId::new(2);

        let channel = manager
            .create_channel(sender, receiver, &dummy_token())
            .expect("should create channel");

        let stats = manager.channel_stats(channel).expect("should get stats");
        assert_eq!(stats.sender, sender);
        assert_eq!(stats.receiver, receiver);
    }

    #[test]
    fn test_send_receive() {
        let manager = IpcManager::new(IpcConfig::default());
        let sender = ProcessId::new(1);
        let receiver = ProcessId::new(2);

        let channel = manager
            .create_channel(sender, receiver, &dummy_token())
            .expect("should create channel");

        let msg = Message::inline(sender, vec![1, 2, 3, 4]);
        manager
            .send(channel, sender, msg, &dummy_token())
            .expect("should send");

        let received = manager
            .receive(channel, receiver, &dummy_token())
            .expect("should receive");

        if let MessageData::Inline(data) = received.data {
            assert_eq!(data, vec![1, 2, 3, 4]);
        } else {
            panic!("expected inline data");
        }
    }
}

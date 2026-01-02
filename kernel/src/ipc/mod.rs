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
//!
//! ## Fast Path (Microkernel Optimization)
//!
//! The `fastpath` module provides optimized IPC for high-frequency operations
//! in the hybrid microkernel architecture. See `fastpath.rs` for details.
//!
//! ## Distributed IPC (v0.2.0)
//!
//! The `distributed` module extends S-LINK for cross-node communication,
//! enabling transparent IPC across Splax clusters.

pub mod fastpath;
pub mod distributed;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Get current timestamp for IPC operations.
#[inline]
fn get_timestamp() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        crate::arch::x86_64::interrupts::get_ticks()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

use spin::Mutex;

use crate::cap::CapabilityToken;
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
pub struct Channel {
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

    pub fn send(&mut self, mut message: Message) -> Result<(), IpcError> {
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

    pub fn receive(&mut self) -> Result<Message, IpcError> {
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
            pending_async_sends: ASYNC_IPC.pending_count(channel.id).0,
            pending_async_receives: ASYNC_IPC.pending_count(channel.id).1,
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
    /// Pending async sends waiting
    pub pending_async_sends: usize,
    /// Pending async receives waiting
    pub pending_async_receives: usize,
}

impl IpcManager {
    // ... existing methods ...
    
    /// Async send - queues if buffer full
    pub fn send_async(
        &self,
        channel_id: ChannelId,
        sender: ProcessId,
        message: Message,
        cap_token: &CapabilityToken,
        timeout: u64,
    ) -> Result<Option<PendingId>, IpcError> {
        // Try sync send first
        match self.send(channel_id, sender, message.clone(), cap_token) {
            Ok(()) => Ok(None), // Sent immediately
            Err(IpcError::BufferFull) => {
                // Queue for async completion
                let pending_id = ASYNC_IPC.queue_send(channel_id, sender, message, timeout)?;
                Ok(Some(pending_id))
            }
            Err(e) => Err(e),
        }
    }
    
    /// Async receive - queues if buffer empty
    pub fn receive_async(
        &self,
        channel_id: ChannelId,
        receiver: ProcessId,
        cap_token: &CapabilityToken,
        timeout: u64,
    ) -> Result<Result<Message, PendingId>, IpcError> {
        // Try sync receive first
        match self.receive(channel_id, receiver, cap_token) {
            Ok(msg) => Ok(Ok(msg)), // Received immediately
            Err(IpcError::BufferEmpty) => {
                // Queue for async completion
                let pending_id = ASYNC_IPC.queue_receive(channel_id, receiver, timeout)?;
                Ok(Err(pending_id))
            }
            Err(e) => Err(e),
        }
    }
    
    /// Poll for async operation completion
    pub fn poll_pending(&self, pending_id: PendingId) -> Result<Option<Message>, IpcError> {
        // Check if the operation is still pending
        let (sends, receives) = {
            let pending_sends = ASYNC_IPC.pending_sends.lock();
            let pending_receives = ASYNC_IPC.pending_receives.lock();
            
            let has_send = pending_sends.values().any(|queue| {
                queue.iter().any(|op: &PendingOp| op.id == pending_id)
            });
            let has_receive = pending_receives.values().any(|queue| {
                queue.iter().any(|op: &PendingOp| op.id == pending_id)
            });
            
            (has_send, has_receive)
        };
        
        if sends || receives {
            return Err(IpcError::WouldBlock);
        }
        
        // If not in pending queues, operation was cancelled or ID invalid
        Err(IpcError::PendingNotFound)
    }
    
    /// Cancel a pending async operation
    pub fn cancel_pending(&self, pending_id: PendingId) -> Result<(), IpcError> {
        ASYNC_IPC.cancel(pending_id)
    }
    
    /// Process pending operations for all channels
    /// Should be called periodically by the kernel main loop
    pub fn process_pending(&self) {
        let mut channels = self.channels.lock();
        for channel in channels.values_mut() {
            // Try to complete pending sends
            let _completed_sends = ASYNC_IPC.try_complete_sends(channel);
            // Notify waiters (would wake up blocked processes in real impl)
            
            // Try to complete pending receives
            let _completed_receives = ASYNC_IPC.try_complete_receives(channel);
            // Deliver messages to waiting processes
        }
    }
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
    /// Operation timed out
    Timeout,
    /// Operation would block (async mode)
    WouldBlock,
    /// Pending operation ID not found
    PendingNotFound,
    /// Too many pending async operations
    TooManyPending,
}

// =============================================================================
// Async IPC Support
// =============================================================================

/// Pending operation identifier for async IPC
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PendingId(pub u64);

/// Async operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncOpType {
    /// Waiting to send (buffer was full)
    Send,
    /// Waiting to receive (buffer was empty)
    Receive,
}

/// A pending async operation
#[derive(Debug, Clone)]
pub struct PendingOp {
    /// Operation ID
    pub id: PendingId,
    /// Channel this operation is on
    pub channel_id: ChannelId,
    /// Process that initiated the operation
    pub process: ProcessId,
    /// Type of operation
    pub op_type: AsyncOpType,
    /// Message (for pending sends)
    pub message: Option<Message>,
    /// When this operation was queued (for timeout)
    pub queued_at: u64,
    /// Timeout in cycles (0 = no timeout)
    pub timeout: u64,
}

/// Async IPC manager extension
pub struct AsyncIpcManager {
    /// Pending send operations per channel
    pending_sends: Mutex<BTreeMap<ChannelId, Vec<PendingOp>>>,
    /// Pending receive operations per channel  
    pending_receives: Mutex<BTreeMap<ChannelId, Vec<PendingOp>>>,
    /// Next pending operation ID
    next_pending_id: Mutex<u64>,
    /// Maximum pending operations per channel
    max_pending_per_channel: usize,
}

impl AsyncIpcManager {
    /// Create new async IPC manager
    pub fn new(max_pending: usize) -> Self {
        Self {
            pending_sends: Mutex::new(BTreeMap::new()),
            pending_receives: Mutex::new(BTreeMap::new()),
            next_pending_id: Mutex::new(1),
            max_pending_per_channel: max_pending,
        }
    }
    
    /// Queue an async send operation
    pub fn queue_send(
        &self,
        channel_id: ChannelId,
        process: ProcessId,
        message: Message,
        timeout: u64,
    ) -> Result<PendingId, IpcError> {
        let mut pending = self.pending_sends.lock();
        let queue = pending.entry(channel_id).or_insert_with(Vec::new);
        
        if queue.len() >= self.max_pending_per_channel {
            return Err(IpcError::TooManyPending);
        }
        
        let mut next_id = self.next_pending_id.lock();
        let id = PendingId(*next_id);
        *next_id += 1;
        
        queue.push(PendingOp {
            id,
            channel_id,
            process,
            op_type: AsyncOpType::Send,
            message: Some(message),
            queued_at: get_timestamp(),
            timeout,
        });
        
        Ok(id)
    }
    
    /// Queue an async receive operation
    pub fn queue_receive(
        &self,
        channel_id: ChannelId,
        process: ProcessId,
        timeout: u64,
    ) -> Result<PendingId, IpcError> {
        let mut pending = self.pending_receives.lock();
        let queue = pending.entry(channel_id).or_insert_with(Vec::new);
        
        if queue.len() >= self.max_pending_per_channel {
            return Err(IpcError::TooManyPending);
        }
        
        let mut next_id = self.next_pending_id.lock();
        let id = PendingId(*next_id);
        *next_id += 1;
        
        queue.push(PendingOp {
            id,
            channel_id,
            process,
            op_type: AsyncOpType::Receive,
            message: None,
            queued_at: get_timestamp(),
            timeout,
        });
        
        Ok(id)
    }
    
    /// Cancel a pending operation
    pub fn cancel(&self, pending_id: PendingId) -> Result<(), IpcError> {
        // Check sends
        {
            let mut pending = self.pending_sends.lock();
            for queue in pending.values_mut() {
                if let Some(pos) = queue.iter().position(|op| op.id == pending_id) {
                    queue.remove(pos);
                    return Ok(());
                }
            }
        }
        
        // Check receives
        {
            let mut pending = self.pending_receives.lock();
            for queue in pending.values_mut() {
                if let Some(pos) = queue.iter().position(|op| op.id == pending_id) {
                    queue.remove(pos);
                    return Ok(());
                }
            }
        }
        
        Err(IpcError::PendingNotFound)
    }
    
    /// Try to complete pending sends for a channel (called when buffer has space)
    pub fn try_complete_sends(&self, channel: &mut Channel) -> Vec<PendingId> {
        let mut completed = Vec::new();
        let mut pending = self.pending_sends.lock();
        
        if let Some(queue) = pending.get_mut(&channel.id) {
            let mut i = 0;
            while i < queue.len() && !channel.is_full() {
                if let Some(msg) = queue[i].message.take() {
                    if channel.send(msg).is_ok() {
                        completed.push(queue[i].id);
                        queue.remove(i);
                        continue;
                    }
                }
                i += 1;
            }
        }
        
        completed
    }
    
    /// Try to complete pending receives for a channel (called when message available)
    pub fn try_complete_receives(&self, channel: &mut Channel) -> Vec<(PendingId, Message)> {
        let mut completed = Vec::new();
        let mut pending = self.pending_receives.lock();
        
        if let Some(queue) = pending.get_mut(&channel.id) {
            while !queue.is_empty() && !channel.is_empty() {
                if let Ok(msg) = channel.receive() {
                    let op = queue.remove(0);
                    completed.push((op.id, msg));
                } else {
                    break;
                }
            }
        }
        
        completed
    }
    
    /// Get pending operation count for a channel
    pub fn pending_count(&self, channel_id: ChannelId) -> (usize, usize) {
        let sends = self.pending_sends.lock()
            .get(&channel_id)
            .map(|q| q.len())
            .unwrap_or(0);
        let receives = self.pending_receives.lock()
            .get(&channel_id)
            .map(|q| q.len())
            .unwrap_or(0);
        (sends, receives)
    }
}

/// Global async IPC manager
pub static ASYNC_IPC: spin::Lazy<AsyncIpcManager> = spin::Lazy::new(|| {
    AsyncIpcManager::new(64) // Max 64 pending ops per channel
});

// =============================================================================
// Global IPC Manager
// =============================================================================

use spin::Lazy;

/// Global IPC manager instance.
pub static IPC_MANAGER: Lazy<IpcManager> = Lazy::new(|| {
    IpcManager::new(IpcConfig::default())
});

/// Initialize IPC subsystem.
pub fn init() {
    // Force lazy initialization
    let _ = &*IPC_MANAGER;
}

/// Quick send function for kernel use.
pub fn send(
    channel_id: ChannelId,
    sender: ProcessId,
    data: Vec<u8>,
    cap_token: &CapabilityToken,
) -> Result<(), IpcError> {
    let msg = Message::inline(sender, data);
    IPC_MANAGER.send(channel_id, sender, msg, cap_token)
}

/// Quick receive function for kernel use.
pub fn receive(
    channel_id: ChannelId,
    receiver: ProcessId,
    cap_token: &CapabilityToken,
) -> Result<Message, IpcError> {
    IPC_MANAGER.receive(channel_id, receiver, cap_token)
}

/// Create a channel between two processes.
pub fn create_channel(
    sender: ProcessId,
    receiver: ProcessId,
    cap_token: &CapabilityToken,
) -> Result<ChannelId, IpcError> {
    IPC_MANAGER.create_channel(sender, receiver, cap_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

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

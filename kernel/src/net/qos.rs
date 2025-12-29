//! # Traffic Control / Quality of Service (QoS)
//!
//! Packet scheduling and traffic shaping for network QoS.
//! Based on Linux `net/sched/` architecture but simplified for microkernel.
//!
//! ## Design
//!
//! Traffic control operates on the egress path:
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              Application                    │
//! ├─────────────────────────────────────────────┤
//! │              Socket Layer                   │
//! ├─────────────────────────────────────────────┤
//! │           TCP/UDP/IP Stack                  │
//! ├─────────────────────────────────────────────┤
//! │         Traffic Control (QoS)               │
//! │  ┌─────────────┐  ┌─────────────┐           │
//! │  │  Classifier │──│   Qdisc     │           │
//! │  └─────────────┘  └─────────────┘           │
//! ├─────────────────────────────────────────────┤
//! │           Network Device                    │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ## Components
//!
//! - **Qdisc**: Queuing discipline (scheduler)
//! - **Class**: Traffic class for hierarchical scheduling
//! - **Filter**: Classifier for matching packets to classes
//!
//! ## Scheduling Disciplines
//!
//! - **FIFO**: First-in-first-out (default)
//! - **Prio**: Priority-based scheduling
//! - **TBF**: Token Bucket Filter (rate limiting)
//! - **HTB**: Hierarchical Token Bucket

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

/// Packet priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PacketPriority {
    /// Background traffic (bulk downloads)
    Background = 0,
    /// Best effort (default)
    BestEffort = 1,
    /// Excellent effort (video streaming)
    ExcellentEffort = 2,
    /// Critical applications (VoIP signaling)
    Critical = 3,
    /// Video
    Video = 4,
    /// Voice
    Voice = 5,
    /// Internetwork control
    InternetControl = 6,
    /// Network control (routing protocols)
    NetworkControl = 7,
}

impl Default for PacketPriority {
    fn default() -> Self {
        PacketPriority::BestEffort
    }
}

impl From<u8> for PacketPriority {
    fn from(val: u8) -> Self {
        match val {
            0 => PacketPriority::Background,
            1 => PacketPriority::BestEffort,
            2 => PacketPriority::ExcellentEffort,
            3 => PacketPriority::Critical,
            4 => PacketPriority::Video,
            5 => PacketPriority::Voice,
            6 => PacketPriority::InternetControl,
            7 => PacketPriority::NetworkControl,
            _ => PacketPriority::BestEffort,
        }
    }
}

/// Packet metadata for QoS decisions
#[derive(Debug, Clone)]
pub struct PacketInfo {
    /// Packet priority
    pub priority: PacketPriority,
    /// Packet length in bytes
    pub length: usize,
    /// Source port (for TCP/UDP)
    pub src_port: u16,
    /// Destination port
    pub dst_port: u16,
    /// IP protocol number
    pub protocol: u8,
    /// DSCP/TOS field
    pub dscp: u8,
    /// Timestamp (arrival time)
    pub timestamp: u64,
    /// Mark for internal use
    pub mark: u32,
}

impl PacketInfo {
    /// Creates a new packet info
    pub fn new(length: usize) -> Self {
        Self {
            priority: PacketPriority::BestEffort,
            length,
            src_port: 0,
            dst_port: 0,
            protocol: 0,
            dscp: 0,
            timestamp: 0,
            mark: 0,
        }
    }

    /// Sets priority from DSCP
    pub fn set_dscp(&mut self, dscp: u8) {
        self.dscp = dscp;
        // Map DSCP to priority (simplified)
        self.priority = match dscp >> 3 {
            0 => PacketPriority::BestEffort,
            1 => PacketPriority::Background,
            2 | 3 => PacketPriority::ExcellentEffort,
            4 => PacketPriority::Video,
            5 => PacketPriority::Voice,
            6 => PacketPriority::InternetControl,
            7 => PacketPriority::NetworkControl,
            _ => PacketPriority::BestEffort,
        };
    }
}

/// A queued packet
pub struct QueuedPacket {
    /// Packet data
    pub data: Vec<u8>,
    /// Packet metadata
    pub info: PacketInfo,
}

impl QueuedPacket {
    /// Creates a new queued packet
    pub fn new(data: Vec<u8>, info: PacketInfo) -> Self {
        Self { data, info }
    }
}

// ============================================================================
// Queuing Discipline Trait
// ============================================================================

/// Queuing discipline trait
pub trait Qdisc: Send + Sync {
    /// Returns the qdisc name
    fn name(&self) -> &'static str;

    /// Enqueues a packet
    fn enqueue(&mut self, packet: QueuedPacket) -> Result<(), QdiscError>;

    /// Dequeues a packet for transmission
    fn dequeue(&mut self) -> Option<QueuedPacket>;

    /// Peeks at the next packet without removing it
    fn peek(&self) -> Option<&QueuedPacket>;

    /// Returns the number of queued packets
    fn len(&self) -> usize;

    /// Checks if the queue is empty
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns queue statistics
    fn stats(&self) -> QdiscStats;

    /// Resets the qdisc
    fn reset(&mut self);
}

/// Qdisc errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QdiscError {
    /// Queue is full
    QueueFull,
    /// Packet too large
    PacketTooLarge,
    /// Rate limit exceeded
    RateLimited,
    /// Invalid configuration
    InvalidConfig,
}

/// Qdisc statistics
#[derive(Debug, Default, Clone)]
pub struct QdiscStats {
    /// Packets enqueued
    pub packets_enqueued: u64,
    /// Packets dequeued
    pub packets_dequeued: u64,
    /// Packets dropped
    pub packets_dropped: u64,
    /// Bytes enqueued
    pub bytes_enqueued: u64,
    /// Bytes dequeued
    pub bytes_dequeued: u64,
    /// Current queue depth (packets)
    pub queue_depth: usize,
    /// Current queue depth (bytes)
    pub queue_bytes: usize,
    /// Overlimits (rate/buffer exceeded)
    pub overlimits: u64,
}

// ============================================================================
// FIFO Qdisc (Default)
// ============================================================================

/// Simple FIFO qdisc
pub struct FifoQdisc {
    queue: VecDeque<QueuedPacket>,
    max_packets: usize,
    stats: QdiscStats,
}

impl FifoQdisc {
    /// Creates a new FIFO qdisc
    pub fn new(max_packets: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(max_packets),
            max_packets,
            stats: QdiscStats::default(),
        }
    }
}

impl Default for FifoQdisc {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl Qdisc for FifoQdisc {
    fn name(&self) -> &'static str {
        "pfifo"
    }

    fn enqueue(&mut self, packet: QueuedPacket) -> Result<(), QdiscError> {
        if self.queue.len() >= self.max_packets {
            self.stats.packets_dropped += 1;
            return Err(QdiscError::QueueFull);
        }

        self.stats.packets_enqueued += 1;
        self.stats.bytes_enqueued += packet.info.length as u64;
        self.stats.queue_bytes += packet.info.length;
        self.queue.push_back(packet);
        self.stats.queue_depth = self.queue.len();

        Ok(())
    }

    fn dequeue(&mut self) -> Option<QueuedPacket> {
        let packet = self.queue.pop_front()?;
        self.stats.packets_dequeued += 1;
        self.stats.bytes_dequeued += packet.info.length as u64;
        self.stats.queue_bytes = self.stats.queue_bytes.saturating_sub(packet.info.length);
        self.stats.queue_depth = self.queue.len();
        Some(packet)
    }

    fn peek(&self) -> Option<&QueuedPacket> {
        self.queue.front()
    }

    fn len(&self) -> usize {
        self.queue.len()
    }

    fn stats(&self) -> QdiscStats {
        self.stats.clone()
    }

    fn reset(&mut self) {
        self.queue.clear();
        self.stats = QdiscStats::default();
    }
}

// ============================================================================
// Priority Qdisc
// ============================================================================

/// Priority qdisc with multiple bands
pub struct PrioQdisc {
    /// Priority bands (highest priority first)
    bands: Vec<VecDeque<QueuedPacket>>,
    /// Maximum packets per band
    max_per_band: usize,
    stats: QdiscStats,
}

impl PrioQdisc {
    /// Creates a new priority qdisc
    pub fn new(num_bands: usize, max_per_band: usize) -> Self {
        Self {
            bands: (0..num_bands).map(|_| VecDeque::new()).collect(),
            max_per_band,
            stats: QdiscStats::default(),
        }
    }

    /// Maps packet priority to band
    fn priority_to_band(&self, priority: PacketPriority) -> usize {
        let band = match priority {
            PacketPriority::NetworkControl | PacketPriority::InternetControl => 0,
            PacketPriority::Voice | PacketPriority::Video => 1,
            PacketPriority::Critical | PacketPriority::ExcellentEffort => 2,
            PacketPriority::BestEffort => 3,
            PacketPriority::Background => 4,
        };
        band.min(self.bands.len() - 1)
    }
}

impl Default for PrioQdisc {
    fn default() -> Self {
        Self::new(5, 200)
    }
}

impl Qdisc for PrioQdisc {
    fn name(&self) -> &'static str {
        "prio"
    }

    fn enqueue(&mut self, packet: QueuedPacket) -> Result<(), QdiscError> {
        let band = self.priority_to_band(packet.info.priority);

        if self.bands[band].len() >= self.max_per_band {
            self.stats.packets_dropped += 1;
            return Err(QdiscError::QueueFull);
        }

        self.stats.packets_enqueued += 1;
        self.stats.bytes_enqueued += packet.info.length as u64;
        self.stats.queue_bytes += packet.info.length;
        self.bands[band].push_back(packet);
        self.stats.queue_depth = self.bands.iter().map(|b| b.len()).sum();

        Ok(())
    }

    fn dequeue(&mut self) -> Option<QueuedPacket> {
        // Serve highest priority band first
        for band in &mut self.bands {
            if let Some(packet) = band.pop_front() {
                self.stats.packets_dequeued += 1;
                self.stats.bytes_dequeued += packet.info.length as u64;
                self.stats.queue_bytes = self.stats.queue_bytes.saturating_sub(packet.info.length);
                self.stats.queue_depth = self.bands.iter().map(|b| b.len()).sum();
                return Some(packet);
            }
        }
        None
    }

    fn peek(&self) -> Option<&QueuedPacket> {
        for band in &self.bands {
            if let Some(packet) = band.front() {
                return Some(packet);
            }
        }
        None
    }

    fn len(&self) -> usize {
        self.bands.iter().map(|b| b.len()).sum()
    }

    fn stats(&self) -> QdiscStats {
        self.stats.clone()
    }

    fn reset(&mut self) {
        for band in &mut self.bands {
            band.clear();
        }
        self.stats = QdiscStats::default();
    }
}

// ============================================================================
// Token Bucket Filter (TBF)
// ============================================================================

/// Token Bucket Filter for rate limiting
pub struct TbfQdisc {
    /// Inner queue
    queue: VecDeque<QueuedPacket>,
    /// Rate in bytes per second
    rate: u64,
    /// Bucket size in bytes
    bucket_size: u64,
    /// Current tokens
    tokens: u64,
    /// Last update time
    last_update: u64,
    /// Maximum queue size
    max_queue: usize,
    stats: QdiscStats,
}

impl TbfQdisc {
    /// Creates a new TBF qdisc
    ///
    /// # Arguments
    /// * `rate` - Rate limit in bytes per second
    /// * `bucket_size` - Maximum burst size in bytes
    /// * `max_queue` - Maximum queue depth
    pub fn new(rate: u64, bucket_size: u64, max_queue: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            rate,
            bucket_size,
            tokens: bucket_size,
            last_update: 0,
            max_queue,
            stats: QdiscStats::default(),
        }
    }

    /// Updates token count based on elapsed time
    fn update_tokens(&mut self, now: u64) {
        if now > self.last_update {
            let elapsed = now - self.last_update;
            // Add tokens for elapsed time (simplified: assume microseconds)
            let new_tokens = (self.rate * elapsed) / 1_000_000;
            self.tokens = (self.tokens + new_tokens).min(self.bucket_size);
            self.last_update = now;
        }
    }

    /// Checks if packet can be transmitted
    fn can_transmit(&self, packet_len: usize) -> bool {
        self.tokens >= packet_len as u64
    }
}

impl Qdisc for TbfQdisc {
    fn name(&self) -> &'static str {
        "tbf"
    }

    fn enqueue(&mut self, packet: QueuedPacket) -> Result<(), QdiscError> {
        if self.queue.len() >= self.max_queue {
            self.stats.packets_dropped += 1;
            return Err(QdiscError::QueueFull);
        }

        self.stats.packets_enqueued += 1;
        self.stats.bytes_enqueued += packet.info.length as u64;
        self.stats.queue_bytes += packet.info.length;
        self.queue.push_back(packet);
        self.stats.queue_depth = self.queue.len();

        Ok(())
    }

    fn dequeue(&mut self) -> Option<QueuedPacket> {
        // Get current time using the architecture's cycle counter
        let now = crate::arch::read_cycle_counter();
        self.update_tokens(now);

        if let Some(packet) = self.queue.front() {
            if self.can_transmit(packet.info.length) {
                let packet = self.queue.pop_front().unwrap();
                self.tokens -= packet.info.length as u64;
                self.stats.packets_dequeued += 1;
                self.stats.bytes_dequeued += packet.info.length as u64;
                self.stats.queue_bytes = self.stats.queue_bytes.saturating_sub(packet.info.length);
                self.stats.queue_depth = self.queue.len();
                return Some(packet);
            } else {
                self.stats.overlimits += 1;
            }
        }
        None
    }

    fn peek(&self) -> Option<&QueuedPacket> {
        self.queue.front()
    }

    fn len(&self) -> usize {
        self.queue.len()
    }

    fn stats(&self) -> QdiscStats {
        self.stats.clone()
    }

    fn reset(&mut self) {
        self.queue.clear();
        self.tokens = self.bucket_size;
        self.stats = QdiscStats::default();
    }
}

// ============================================================================
// Hierarchical Token Bucket (HTB)
// ============================================================================

/// HTB class
pub struct HtbClass {
    /// Class ID
    pub id: u32,
    /// Parent class ID (0 for root)
    pub parent: u32,
    /// Guaranteed rate
    pub rate: u64,
    /// Ceiling rate (maximum)
    pub ceil: u64,
    /// Priority
    pub priority: u8,
    /// Quantum
    pub quantum: u32,
    /// Current tokens
    tokens: u64,
    /// Ceiling tokens
    ctokens: u64,
    /// Queue for this class
    queue: VecDeque<QueuedPacket>,
}

impl HtbClass {
    /// Creates a new HTB class
    pub fn new(id: u32, parent: u32, rate: u64, ceil: u64) -> Self {
        Self {
            id,
            parent,
            rate,
            ceil,
            priority: 0,
            quantum: 10000,
            tokens: ceil,
            ctokens: ceil,
            queue: VecDeque::new(),
        }
    }
}

/// Hierarchical Token Bucket qdisc
pub struct HtbQdisc {
    /// Root classes
    classes: BTreeMap<u32, HtbClass>,
    /// Default class ID
    default_class: u32,
    stats: QdiscStats,
}

impl HtbQdisc {
    /// Creates a new HTB qdisc
    pub fn new() -> Self {
        Self {
            classes: BTreeMap::new(),
            default_class: 0,
            stats: QdiscStats::default(),
        }
    }

    /// Adds a class
    pub fn add_class(&mut self, class: HtbClass) {
        if self.default_class == 0 {
            self.default_class = class.id;
        }
        self.classes.insert(class.id, class);
    }

    /// Sets the default class
    pub fn set_default(&mut self, class_id: u32) {
        self.default_class = class_id;
    }

    /// Finds the appropriate traffic class for a packet using classifiers.
    ///
    /// Classification is based on the PacketPriority enum which maps to 
    /// traffic classes. PacketPriority follows 802.1p/DiffServ conventions:
    /// - 0: Background (bulk)
    /// - 1: Best Effort (default)
    /// - 2: Excellent Effort (video streaming)
    /// - 3: Critical (VoIP signaling)
    /// - 4: Video
    /// - 5: Voice (highest user priority)
    /// - 6: Internetwork Control
    /// - 7: Network Control (routing protocols)
    fn find_class_for_packet(&self, packet: &QueuedPacket) -> u32 {
        // Map PacketPriority to traffic class
        // We attempt to match higher priority packets to specific classes
        let class_for_priority = match packet.info.priority {
            PacketPriority::NetworkControl | PacketPriority::InternetControl => 1,
            PacketPriority::Voice => 2,
            PacketPriority::Video => 3,
            PacketPriority::Critical => 4,
            PacketPriority::ExcellentEffort => 5,
            PacketPriority::BestEffort | PacketPriority::Background => 0,
        };
        
        // If we found a priority-based class that exists, use it
        if class_for_priority > 0 {
            if self.classes.contains_key(&class_for_priority) {
                return class_for_priority;
            }
        }
        
        // Fall back to default class
        self.default_class
    }
}

impl Default for HtbQdisc {
    fn default() -> Self {
        Self::new()
    }
}

impl Qdisc for HtbQdisc {
    fn name(&self) -> &'static str {
        "htb"
    }

    fn enqueue(&mut self, packet: QueuedPacket) -> Result<(), QdiscError> {
        let class_id = self.find_class_for_packet(&packet);
        
        if let Some(class) = self.classes.get_mut(&class_id) {
            self.stats.packets_enqueued += 1;
            self.stats.bytes_enqueued += packet.info.length as u64;
            self.stats.queue_bytes += packet.info.length;
            class.queue.push_back(packet);
            self.stats.queue_depth = self.classes.values().map(|c| c.queue.len()).sum();
            Ok(())
        } else {
            self.stats.packets_dropped += 1;
            Err(QdiscError::InvalidConfig)
        }
    }

    fn dequeue(&mut self) -> Option<QueuedPacket> {
        // Find class with tokens and packets
        // Simplified: round-robin through classes
        for class in self.classes.values_mut() {
            if let Some(packet) = class.queue.pop_front() {
                self.stats.packets_dequeued += 1;
                self.stats.bytes_dequeued += packet.info.length as u64;
                self.stats.queue_bytes = self.stats.queue_bytes.saturating_sub(packet.info.length);
                self.stats.queue_depth = self.classes.values().map(|c| c.queue.len()).sum();
                return Some(packet);
            }
        }
        None
    }

    fn peek(&self) -> Option<&QueuedPacket> {
        for class in self.classes.values() {
            if let Some(packet) = class.queue.front() {
                return Some(packet);
            }
        }
        None
    }

    fn len(&self) -> usize {
        self.classes.values().map(|c| c.queue.len()).sum()
    }

    fn stats(&self) -> QdiscStats {
        self.stats.clone()
    }

    fn reset(&mut self) {
        for class in self.classes.values_mut() {
            class.queue.clear();
            class.tokens = class.ceil;
            class.ctokens = class.ceil;
        }
        self.stats = QdiscStats::default();
    }
}

// ============================================================================
// Traffic Control Manager
// ============================================================================

/// Traffic control manager for a network interface
pub struct TrafficControl {
    /// Interface name
    interface: String,
    /// Root qdisc
    qdisc: Box<dyn Qdisc>,
    /// Enabled flag
    enabled: bool,
}

impl TrafficControl {
    /// Creates new traffic control for an interface
    pub fn new(interface: &str) -> Self {
        Self {
            interface: String::from(interface),
            qdisc: Box::new(FifoQdisc::default()),
            enabled: true,
        }
    }

    /// Sets the qdisc
    pub fn set_qdisc(&mut self, qdisc: Box<dyn Qdisc>) {
        self.qdisc = qdisc;
    }

    /// Enqueues a packet for transmission
    pub fn enqueue(&mut self, data: Vec<u8>, info: PacketInfo) -> Result<(), QdiscError> {
        if !self.enabled {
            return Ok(()); // Pass through
        }
        self.qdisc.enqueue(QueuedPacket::new(data, info))
    }

    /// Dequeues the next packet for transmission
    pub fn dequeue(&mut self) -> Option<QueuedPacket> {
        self.qdisc.dequeue()
    }

    /// Returns qdisc statistics
    pub fn stats(&self) -> QdiscStats {
        self.qdisc.stats()
    }

    /// Enables/disables traffic control
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Returns the interface name
    pub fn interface(&self) -> &str {
        &self.interface
    }
}

/// Global traffic control instances
static TC_INSTANCES: Mutex<BTreeMap<String, TrafficControl>> = Mutex::new(BTreeMap::new());

/// Gets or creates traffic control for an interface
pub fn get_tc(interface: &str) -> Result<(), QdiscError> {
    let mut instances = TC_INSTANCES.lock();
    if !instances.contains_key(interface) {
        instances.insert(String::from(interface), TrafficControl::new(interface));
    }
    Ok(())
}

/// Sets the qdisc for an interface
pub fn set_qdisc(interface: &str, qdisc_type: &str) -> Result<(), QdiscError> {
    let mut instances = TC_INSTANCES.lock();
    
    if let Some(tc) = instances.get_mut(interface) {
        let qdisc: Box<dyn Qdisc> = match qdisc_type {
            "pfifo" => Box::new(FifoQdisc::default()),
            "prio" => Box::new(PrioQdisc::default()),
            "tbf" => Box::new(TbfQdisc::new(1_000_000, 10_000, 1000)),
            "htb" => Box::new(HtbQdisc::default()),
            _ => return Err(QdiscError::InvalidConfig),
        };
        tc.set_qdisc(qdisc);
        Ok(())
    } else {
        Err(QdiscError::InvalidConfig)
    }
}

/// Initializes the QoS subsystem
pub fn init() {
    crate::serial_println!("[QoS] Traffic control subsystem initialized");
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_fifo_qdisc() {
        let mut qdisc = FifoQdisc::new(10);
        
        for i in 0..5 {
            let info = PacketInfo::new(100);
            let packet = QueuedPacket::new(vec![i; 100], info);
            assert!(qdisc.enqueue(packet).is_ok());
        }
        
        assert_eq!(qdisc.len(), 5);
        
        let packet = qdisc.dequeue().unwrap();
        assert_eq!(packet.data[0], 0);
    }

    #[test]
    fn test_prio_qdisc() {
        let mut qdisc = PrioQdisc::default();
        
        // Add low priority packet
        let mut info = PacketInfo::new(100);
        info.priority = PacketPriority::Background;
        qdisc.enqueue(QueuedPacket::new(vec![1], info)).unwrap();
        
        // Add high priority packet
        let mut info = PacketInfo::new(100);
        info.priority = PacketPriority::Voice;
        qdisc.enqueue(QueuedPacket::new(vec![2], info)).unwrap();
        
        // High priority should come out first
        let packet = qdisc.dequeue().unwrap();
        assert_eq!(packet.data[0], 2);
    }
}

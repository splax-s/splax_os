//! IPC Fast Path Optimizations
//!
//! This module provides optimized IPC paths for common microkernel operations.
//! These are critical for performance as every syscall in a microkernel goes
//! through IPC to reach userspace services.
//!
//! ## Optimization Strategies
//!
//! 1. **Register-based Fast Path**: Small messages passed in registers
//! 2. **Lock-free Ring Buffers**: Avoid mutex contention on hot paths
//! 3. **CPU-local Channels**: Reduce cache line bouncing
//! 4. **Batched Operations**: Coalesce multiple small messages
//! 5. **Zero-copy Shared Memory**: Large transfers via page sharing
//!
//! ## Performance Targets
//!
//! | Operation | Target Latency |
//! |-----------|---------------|
//! | Small msg (≤64 bytes) | <500ns |
//! | Medium msg (≤4KB) | <1µs |
//! | Large msg (page sharing) | <2µs |
//! | Service call round-trip | <2µs |

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use core::cell::UnsafeCell;
use alloc::vec::Vec;
use alloc::sync::Arc;
use spin::Mutex;

use crate::sched::ProcessId;
use super::{ChannelId, Message, MessageData, IpcError};

/// Maximum size for register-based fast path (fits in syscall registers)
pub const FASTPATH_MAX_SIZE: usize = 64;

/// Number of slots in lock-free ring buffer
pub const RING_BUFFER_SIZE: usize = 256;

/// Fast path message - fits in CPU registers
#[repr(C, align(64))]  // Cache line aligned
#[derive(Clone, Copy)]
pub struct FastMessage {
    /// Message type/tag
    pub tag: u64,
    /// First data word
    pub data0: u64,
    /// Second data word
    pub data1: u64,
    /// Third data word
    pub data2: u64,
    /// Fourth data word
    pub data3: u64,
    /// Fifth data word
    pub data4: u64,
    /// Sixth data word
    pub data5: u64,
    /// Seventh data word (capability or extra data)
    pub data6: u64,
}

impl FastMessage {
    /// Create empty message
    pub const fn empty() -> Self {
        Self {
            tag: 0,
            data0: 0,
            data1: 0,
            data2: 0,
            data3: 0,
            data4: 0,
            data5: 0,
            data6: 0,
        }
    }

    /// Create message with tag
    pub const fn new(tag: u64) -> Self {
        Self {
            tag,
            ..Self::empty()
        }
    }

    /// Copy data from byte slice
    pub fn from_bytes(tag: u64, data: &[u8]) -> Self {
        let mut msg = Self::new(tag);
        let len = data.len().min(56);

        // Copy up to 56 bytes (7 * 8)
        let words = unsafe {
            core::slice::from_raw_parts_mut(
                &mut msg.data0 as *mut u64 as *mut u8,
                56
            )
        };
        words[..len].copy_from_slice(&data[..len]);

        msg
    }

    /// Extract data to byte slice
    pub fn to_bytes(&self, buffer: &mut [u8]) -> usize {
        let len = buffer.len().min(56);
        let words = unsafe {
            core::slice::from_raw_parts(
                &self.data0 as *const u64 as *const u8,
                56
            )
        };
        buffer[..len].copy_from_slice(&words[..len]);
        len
    }
}

/// Lock-free single-producer single-consumer ring buffer
#[repr(C, align(128))]  // Two cache lines to prevent false sharing
pub struct SpscRing {
    /// Write position (producer)
    write_pos: AtomicUsize,
    /// Padding to separate cache lines
    _pad1: [u64; 7],
    /// Read position (consumer)
    read_pos: AtomicUsize,
    /// Padding
    _pad2: [u64; 7],
    /// Message slots
    slots: UnsafeCell<[FastMessage; RING_BUFFER_SIZE]>,
    /// Sequence numbers for each slot (for ordering)
    sequences: [AtomicU64; RING_BUFFER_SIZE],
}

// SAFETY: SpscRing is designed for single-producer single-consumer use
unsafe impl Sync for SpscRing {}
unsafe impl Send for SpscRing {}

impl SpscRing {
    /// Create a new ring buffer
    pub fn new() -> Self {
        const EMPTY: FastMessage = FastMessage::empty();
        const ZERO: AtomicU64 = AtomicU64::new(0);

        Self {
            write_pos: AtomicUsize::new(0),
            _pad1: [0; 7],
            read_pos: AtomicUsize::new(0),
            _pad2: [0; 7],
            slots: UnsafeCell::new([EMPTY; RING_BUFFER_SIZE]),
            sequences: [ZERO; RING_BUFFER_SIZE],
        }
    }

    /// Try to send a message (non-blocking)
    pub fn try_send(&self, msg: FastMessage) -> Result<(), FastMessage> {
        let write = self.write_pos.load(Ordering::Relaxed);
        let read = self.read_pos.load(Ordering::Acquire);

        // Check if buffer is full
        if write.wrapping_sub(read) >= RING_BUFFER_SIZE {
            return Err(msg);
        }

        let slot = write & (RING_BUFFER_SIZE - 1);

        // Write message
        unsafe {
            (*self.slots.get())[slot] = msg;
        }

        // Memory barrier before updating write position
        core::sync::atomic::fence(Ordering::Release);

        self.write_pos.store(write.wrapping_add(1), Ordering::Release);

        Ok(())
    }

    /// Try to receive a message (non-blocking)
    pub fn try_recv(&self) -> Option<FastMessage> {
        let read = self.read_pos.load(Ordering::Relaxed);
        let write = self.write_pos.load(Ordering::Acquire);

        // Check if buffer is empty
        if read == write {
            return None;
        }

        let slot = read & (RING_BUFFER_SIZE - 1);

        // Read message
        let msg = unsafe { (*self.slots.get())[slot] };

        // Memory barrier before updating read position
        core::sync::atomic::fence(Ordering::Release);

        self.read_pos.store(read.wrapping_add(1), Ordering::Release);

        Some(msg)
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        let read = self.read_pos.load(Ordering::Relaxed);
        let write = self.write_pos.load(Ordering::Acquire);
        read == write
    }

    /// Check if buffer is full
    pub fn is_full(&self) -> bool {
        let write = self.write_pos.load(Ordering::Relaxed);
        let read = self.read_pos.load(Ordering::Acquire);
        write.wrapping_sub(read) >= RING_BUFFER_SIZE
    }

    /// Get number of messages in buffer
    pub fn len(&self) -> usize {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        write.wrapping_sub(read)
    }
}

impl Default for SpscRing {
    fn default() -> Self {
        Self::new()
    }
}

/// Fast IPC endpoint for a specific service
pub struct FastEndpoint {
    /// Service ID
    pub service_id: u64,
    /// Send ring buffer (to peer)
    pub tx: Arc<SpscRing>,
    /// Receive ring buffer (from peer)
    pub rx: Arc<SpscRing>,
    /// Owner process
    pub owner: ProcessId,
    /// Connected peer process
    pub peer: ProcessId,
}

impl FastEndpoint {
    /// Create a new endpoint pair
    pub fn create_pair(
        service_id: u64,
        client: ProcessId,
        server: ProcessId,
    ) -> (Self, Self) {
        // Create shared ring buffers
        // client_to_server is TX for client, RX for server
        // server_to_client is TX for server, RX for client
        let client_to_server = Arc::new(SpscRing::new());
        let server_to_client = Arc::new(SpscRing::new());
        
        // Client endpoint
        let client_ep = Self {
            service_id,
            tx: Arc::clone(&client_to_server),
            rx: Arc::clone(&server_to_client),
            owner: client,
            peer: server,
        };

        // Server endpoint (swapped - server reads from client_to_server, writes to server_to_client)
        let server_ep = Self {
            service_id,
            tx: Arc::clone(&server_to_client),
            rx: Arc::clone(&client_to_server),
            owner: server,
            peer: client,
        };

        (client_ep, server_ep)
    }

    /// Send a fast message
    pub fn send(&self, msg: FastMessage) -> Result<(), IpcError> {
        self.tx.try_send(msg)
            .map_err(|_| IpcError::BufferFull)
    }

    /// Receive a fast message
    pub fn recv(&self) -> Result<FastMessage, IpcError> {
        self.rx.try_recv()
            .ok_or(IpcError::BufferEmpty)
    }

    /// Send and wait for reply (synchronous call)
    pub fn call(&self, request: FastMessage) -> Result<FastMessage, IpcError> {
        self.send(request)?;

        // Spin waiting for reply (in real impl, would yield to scheduler)
        let mut spins = 0;
        loop {
            if let Some(reply) = self.rx.try_recv() {
                return Ok(reply);
            }

            spins += 1;
            if spins > 10000 {
                // Yield to scheduler after spinning
                core::hint::spin_loop();
            }
            if spins > 1000000 {
                return Err(IpcError::Timeout);
            }
        }
    }
}

/// Common message tags for system services
pub mod tags {
    // VFS operations (S-STORAGE)
    pub const VFS_OPEN: u64 = 0x0001;
    pub const VFS_CLOSE: u64 = 0x0002;
    pub const VFS_READ: u64 = 0x0003;
    pub const VFS_WRITE: u64 = 0x0004;
    pub const VFS_STAT: u64 = 0x0005;
    pub const VFS_READDIR: u64 = 0x0006;

    // Socket operations (S-NET)
    pub const SOCK_CREATE: u64 = 0x0100;
    pub const SOCK_BIND: u64 = 0x0101;
    pub const SOCK_CONNECT: u64 = 0x0102;
    pub const SOCK_SEND: u64 = 0x0103;
    pub const SOCK_RECV: u64 = 0x0104;
    pub const SOCK_CLOSE: u64 = 0x0105;

    // Device operations (S-DEV)
    pub const DEV_IOCTL: u64 = 0x0200;
    pub const DEV_READ: u64 = 0x0201;
    pub const DEV_WRITE: u64 = 0x0202;
    pub const DEV_IRQ: u64 = 0x0203;

    // GPU operations (S-GPU)
    pub const GPU_SWAP: u64 = 0x0300;
    pub const GPU_BLIT: u64 = 0x0301;
    pub const GPU_CLEAR: u64 = 0x0302;

    // Response codes
    pub const REPLY_OK: u64 = 0x8000;
    pub const REPLY_ERROR: u64 = 0x8001;
}

/// Batch multiple small messages together
pub struct MessageBatch {
    messages: Vec<FastMessage>,
    max_size: usize,
}

impl MessageBatch {
    /// Create a new batch
    pub fn new(max_size: usize) -> Self {
        Self {
            messages: Vec::with_capacity(max_size),
            max_size,
        }
    }

    /// Add a message to the batch
    pub fn push(&mut self, msg: FastMessage) -> bool {
        if self.messages.len() < self.max_size {
            self.messages.push(msg);
            true
        } else {
            false
        }
    }

    /// Check if batch is full
    pub fn is_full(&self) -> bool {
        self.messages.len() >= self.max_size
    }

    /// Check if batch is empty
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Get number of messages
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// Send all messages in batch
    pub fn send_all(&mut self, endpoint: &FastEndpoint) -> Result<usize, IpcError> {
        let mut sent = 0;
        for msg in self.messages.drain(..) {
            endpoint.send(msg)?;
            sent += 1;
        }
        Ok(sent)
    }

    /// Clear the batch
    pub fn clear(&mut self) {
        self.messages.clear();
    }
}

/// Per-CPU IPC endpoint cache
#[repr(C, align(64))]
pub struct CpuLocalEndpoints {
    /// Endpoint to S-STORAGE (VFS)
    pub storage: Option<*const FastEndpoint>,
    /// Endpoint to S-NET
    pub net: Option<*const FastEndpoint>,
    /// Endpoint to S-DEV
    pub dev: Option<*const FastEndpoint>,
    /// Endpoint to S-GPU
    pub gpu: Option<*const FastEndpoint>,
}

// SAFETY: Pointers are only accessed from the owning CPU
unsafe impl Sync for CpuLocalEndpoints {}
unsafe impl Send for CpuLocalEndpoints {}

impl CpuLocalEndpoints {
    /// Create new CPU-local endpoint cache
    pub const fn new() -> Self {
        Self {
            storage: None,
            net: None,
            dev: None,
            gpu: None,
        }
    }
}

/// Statistics for IPC performance monitoring
#[derive(Debug, Default)]
pub struct IpcStats {
    /// Number of fast path messages sent
    pub fast_sends: AtomicU64,
    /// Number of fast path messages received
    pub fast_recvs: AtomicU64,
    /// Number of slow path messages (fell back to regular IPC)
    pub slow_path: AtomicU64,
    /// Total cycles spent in IPC
    pub total_cycles: AtomicU64,
    /// Number of buffer full events
    pub buffer_full: AtomicU64,
    /// Number of buffer empty events
    pub buffer_empty: AtomicU64,
}

impl IpcStats {
    /// Create new stats
    pub const fn new() -> Self {
        Self {
            fast_sends: AtomicU64::new(0),
            fast_recvs: AtomicU64::new(0),
            slow_path: AtomicU64::new(0),
            total_cycles: AtomicU64::new(0),
            buffer_full: AtomicU64::new(0),
            buffer_empty: AtomicU64::new(0),
        }
    }

    /// Record a fast send
    pub fn record_fast_send(&self) {
        self.fast_sends.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a fast receive
    pub fn record_fast_recv(&self) {
        self.fast_recvs.fetch_add(1, Ordering::Relaxed);
    }

    /// Record falling back to slow path
    pub fn record_slow_path(&self) {
        self.slow_path.fetch_add(1, Ordering::Relaxed);
    }

    /// Get total messages
    pub fn total_messages(&self) -> u64 {
        self.fast_sends.load(Ordering::Relaxed)
            + self.fast_recvs.load(Ordering::Relaxed)
    }

    /// Get slow path ratio
    pub fn slow_path_ratio(&self) -> f64 {
        let total = self.total_messages();
        if total == 0 {
            0.0
        } else {
            self.slow_path.load(Ordering::Relaxed) as f64 / total as f64
        }
    }
}

/// Global IPC statistics
pub static IPC_STATS: IpcStats = IpcStats::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_message() {
        let data = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let msg = FastMessage::from_bytes(0x1234, &data);
        assert_eq!(msg.tag, 0x1234);

        let mut buffer = [0u8; 16];
        let len = msg.to_bytes(&mut buffer);
        assert_eq!(len, 16);
        assert_eq!(&buffer[..8], &data);
    }

    #[test]
    fn test_spsc_ring() {
        let ring = SpscRing::new();

        // Empty initially
        assert!(ring.is_empty());
        assert!(!ring.is_full());
        assert_eq!(ring.len(), 0);

        // Send a message
        let msg = FastMessage::new(42);
        assert!(ring.try_send(msg).is_ok());
        assert!(!ring.is_empty());
        assert_eq!(ring.len(), 1);

        // Receive the message
        let recv = ring.try_recv();
        assert!(recv.is_some());
        assert_eq!(recv.unwrap().tag, 42);
        assert!(ring.is_empty());
    }
}

// =============================================================================
// IPC Benchmarks
// =============================================================================

/// Benchmark results for IPC operations
#[derive(Debug, Clone, Copy)]
pub struct IpcBenchmarkResult {
    /// Operation name
    pub name: &'static str,
    /// Number of iterations
    pub iterations: u64,
    /// Total cycles
    pub total_cycles: u64,
    /// Average cycles per operation
    pub avg_cycles: u64,
    /// Estimated nanoseconds (assuming 3GHz)
    pub estimated_ns: u64,
}

impl IpcBenchmarkResult {
    /// Calculate average from total
    pub fn new(name: &'static str, iterations: u64, total_cycles: u64) -> Self {
        let avg_cycles = total_cycles / iterations.max(1);
        // Assume ~3GHz CPU for estimation
        let estimated_ns = avg_cycles / 3;
        Self {
            name,
            iterations,
            total_cycles,
            avg_cycles,
            estimated_ns,
        }
    }
}

/// Run IPC benchmarks (call from kernel shell or test harness)
pub fn run_ipc_benchmarks() -> [IpcBenchmarkResult; 4] {
    let iterations = 10_000u64;
    
    // Benchmark 1: FastMessage creation
    let start = read_tsc();
    for i in 0..iterations {
        let msg = FastMessage::new(i);
        core::hint::black_box(msg);
    }
    let end = read_tsc();
    let fast_msg_create = IpcBenchmarkResult::new("FastMessage::new", iterations, end - start);
    
    // Benchmark 2: FastMessage from_bytes
    let data = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let start = read_tsc();
    for i in 0..iterations {
        let msg = FastMessage::from_bytes(i, &data);
        core::hint::black_box(msg);
    }
    let end = read_tsc();
    let fast_msg_bytes = IpcBenchmarkResult::new("FastMessage::from_bytes", iterations, end - start);
    
    // Benchmark 3: SPSC ring send/recv (no contention)
    let ring = SpscRing::new();
    let msg = FastMessage::new(42);
    let start = read_tsc();
    for _ in 0..iterations {
        let _ = ring.try_send(msg);
        let recv = ring.try_recv();
        core::hint::black_box(recv);
    }
    let end = read_tsc();
    let spsc_roundtrip = IpcBenchmarkResult::new("SPSC send+recv", iterations, end - start);
    
    // Benchmark 4: FastEndpoint send/recv
    let (client_ep, _server_ep) = FastEndpoint::create_pair(
        0x1234,
        ProcessId::new(1),
        ProcessId::new(2)
    );
    let start = read_tsc();
    for i in 0..iterations {
        let msg = FastMessage::new(i);
        let _ = client_ep.send(msg);
        let recv = client_ep.recv();
        core::hint::black_box(recv);
    }
    let end = read_tsc();
    let endpoint_roundtrip = IpcBenchmarkResult::new("Endpoint send+recv", iterations, end - start);
    
    [fast_msg_create, fast_msg_bytes, spsc_roundtrip, endpoint_roundtrip]
}

/// Read CPU timestamp counter
#[inline]
fn read_tsc() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let lo: u32;
        let hi: u32;
        unsafe {
            core::arch::asm!(
                "rdtsc",
                out("eax") lo,
                out("edx") hi,
                options(nostack, nomem)
            );
        }
        ((hi as u64) << 32) | (lo as u64)
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0 // Placeholder for other architectures
    }
}

//! # Block I/O Scheduler
//!
//! I/O scheduling for block devices, providing fair and efficient
//! access to storage while minimizing seek time and maximizing throughput.
//!
//! ## Schedulers
//!
//! - **NoOp**: First-come-first-served, best for SSDs/NVMe
//! - **Deadline**: Guarantees maximum latency per request
//! - **CFQ**: Completely Fair Queuing for spinning disks
//!
//! ## Design
//!
//! The scheduler sits between the filesystem and block device, batching
//! and reordering requests for optimal performance.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cmp::Ordering;
use spin::Mutex;

use super::{BlockRequest, BlockRequestType, SECTOR_SIZE};

/// Request priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Background I/O (prefetch, writeback)
    Background = 0,
    /// Normal user I/O
    Normal = 1,
    /// Real-time I/O (audio, video)
    Realtime = 2,
    /// System-critical I/O (journal, metadata)
    Critical = 3,
}

impl Default for Priority {
    fn default() -> Self {
        Priority::Normal
    }
}

/// Request direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Read,
    Write,
}

/// Scheduled I/O request
#[derive(Debug)]
pub struct SchedRequest {
    /// Original request
    pub request: BlockRequest,
    /// Request priority
    pub priority: Priority,
    /// Submission timestamp (in ticks)
    pub submit_time: u64,
    /// Deadline (absolute time by which request must complete)
    pub deadline: Option<u64>,
    /// Process ID that submitted this request
    pub pid: u64,
}

impl SchedRequest {
    /// Creates a new scheduled request
    pub fn new(request: BlockRequest, priority: Priority, now: u64) -> Self {
        Self {
            request,
            priority,
            submit_time: now,
            deadline: None,
            pid: 0,
        }
    }

    /// Creates a request with a deadline
    pub fn with_deadline(request: BlockRequest, priority: Priority, now: u64, deadline: u64) -> Self {
        Self {
            request,
            priority,
            submit_time: now,
            deadline: Some(deadline),
            pid: 0,
        }
    }

    /// Gets the sector number for sorting
    pub fn sector(&self) -> u64 {
        self.request.sector
    }

    /// Gets the request direction
    pub fn direction(&self) -> Direction {
        match self.request.request_type {
            BlockRequestType::Write | BlockRequestType::Flush => Direction::Write,
            _ => Direction::Read,
        }
    }
}

/// I/O Scheduler trait
pub trait IoScheduler: Send + Sync {
    /// Scheduler name
    fn name(&self) -> &'static str;

    /// Adds a request to the scheduler
    fn add_request(&mut self, request: SchedRequest);

    /// Gets the next request to dispatch
    fn next_request(&mut self) -> Option<SchedRequest>;

    /// Checks if the scheduler has pending requests
    fn has_pending(&self) -> bool;

    /// Returns the number of pending requests
    fn pending_count(&self) -> usize;

    /// Merges requests if possible (returns true if merged)
    fn merge_request(&mut self, request: &SchedRequest) -> bool {
        let _ = request;
        false
    }
}

// ============================================================================
// NoOp Scheduler (Best for SSDs/NVMe)
// ============================================================================

/// No-operation scheduler - FIFO ordering
///
/// Simply processes requests in submission order. Best for devices
/// with no seek penalty (SSDs, NVMe).
pub struct NoOpScheduler {
    queue: VecDeque<SchedRequest>,
}

impl NoOpScheduler {
    /// Creates a new NoOp scheduler
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }
}

impl Default for NoOpScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl IoScheduler for NoOpScheduler {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn add_request(&mut self, request: SchedRequest) {
        self.queue.push_back(request);
    }

    fn next_request(&mut self) -> Option<SchedRequest> {
        self.queue.pop_front()
    }

    fn has_pending(&self) -> bool {
        !self.queue.is_empty()
    }

    fn pending_count(&self) -> usize {
        self.queue.len()
    }
}

// ============================================================================
// Deadline Scheduler
// ============================================================================

/// Deadline scheduler - guarantees maximum latency
///
/// Maintains separate queues for reads and writes, sorted by sector.
/// Uses FIFO queues to enforce deadlines when requests are about to expire.
pub struct DeadlineScheduler {
    /// Read requests sorted by sector
    read_sorted: BTreeMap<u64, SchedRequest>,
    /// Write requests sorted by sector
    write_sorted: BTreeMap<u64, SchedRequest>,
    /// Read FIFO for deadline enforcement
    read_fifo: VecDeque<u64>,
    /// Write FIFO for deadline enforcement  
    write_fifo: VecDeque<u64>,
    /// Current head position (for elevator algorithm)
    head_position: u64,
    /// Currently serving reads
    serving_reads: bool,
    /// Read deadline in ticks (default: ~500ms)
    read_deadline: u64,
    /// Write deadline in ticks (default: ~5s)
    write_deadline: u64,
    /// Writes starving counter
    writes_starved: usize,
    /// Maximum writes to starve before forcing write batch
    write_starve_limit: usize,
}

impl DeadlineScheduler {
    /// Creates a new deadline scheduler
    pub fn new() -> Self {
        Self {
            read_sorted: BTreeMap::new(),
            write_sorted: BTreeMap::new(),
            read_fifo: VecDeque::new(),
            write_fifo: VecDeque::new(),
            head_position: 0,
            serving_reads: true,
            read_deadline: 500,
            write_deadline: 5000,
            writes_starved: 0,
            write_starve_limit: 16,
        }
    }

    fn dispatch_read(&mut self) -> Option<SchedRequest> {
        // Check if oldest read has expired
        if let Some(&sector) = self.read_fifo.front() {
            if let Some(req) = self.read_sorted.get(&sector) {
                if let Some(deadline) = req.deadline {
                    // If deadline passed, dispatch immediately
                    if deadline <= req.submit_time {
                        self.read_fifo.pop_front();
                        return self.read_sorted.remove(&sector);
                    }
                }
            }
        }

        // Otherwise dispatch nearest to head
        let next_sector = self.read_sorted
            .range(self.head_position..)
            .next()
            .map(|(&s, _)| s)
            .or_else(|| self.read_sorted.keys().next().copied());

        if let Some(sector) = next_sector {
            self.head_position = sector;
            self.read_fifo.retain(|&s| s != sector);
            return self.read_sorted.remove(&sector);
        }

        None
    }

    fn dispatch_write(&mut self) -> Option<SchedRequest> {
        // Check if oldest write has expired
        if let Some(&sector) = self.write_fifo.front() {
            if let Some(req) = self.write_sorted.get(&sector) {
                if let Some(deadline) = req.deadline {
                    if deadline <= req.submit_time {
                        self.write_fifo.pop_front();
                        return self.write_sorted.remove(&sector);
                    }
                }
            }
        }

        // Otherwise dispatch nearest to head
        let next_sector = self.write_sorted
            .range(self.head_position..)
            .next()
            .map(|(&s, _)| s)
            .or_else(|| self.write_sorted.keys().next().copied());

        if let Some(sector) = next_sector {
            self.head_position = sector;
            self.write_fifo.retain(|&s| s != sector);
            return self.write_sorted.remove(&sector);
        }

        None
    }
}

impl Default for DeadlineScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl IoScheduler for DeadlineScheduler {
    fn name(&self) -> &'static str {
        "deadline"
    }

    fn add_request(&mut self, request: SchedRequest) {
        let sector = request.sector();
        match request.direction() {
            Direction::Read => {
                self.read_fifo.push_back(sector);
                self.read_sorted.insert(sector, request);
            }
            Direction::Write => {
                self.write_fifo.push_back(sector);
                self.write_sorted.insert(sector, request);
            }
        }
    }

    fn next_request(&mut self) -> Option<SchedRequest> {
        // Prefer reads, but don't starve writes
        if self.serving_reads {
            if !self.read_sorted.is_empty() {
                self.writes_starved += 1;
                if self.writes_starved >= self.write_starve_limit && !self.write_sorted.is_empty() {
                    self.serving_reads = false;
                    self.writes_starved = 0;
                } else {
                    return self.dispatch_read();
                }
            } else {
                self.serving_reads = false;
            }
        }

        if !self.serving_reads {
            if !self.write_sorted.is_empty() {
                let req = self.dispatch_write();
                if self.write_sorted.is_empty() {
                    self.serving_reads = true;
                }
                return req;
            } else {
                self.serving_reads = true;
                return self.dispatch_read();
            }
        }

        None
    }

    fn has_pending(&self) -> bool {
        !self.read_sorted.is_empty() || !self.write_sorted.is_empty()
    }

    fn pending_count(&self) -> usize {
        self.read_sorted.len() + self.write_sorted.len()
    }
}

// ============================================================================
// CFQ (Completely Fair Queuing) Scheduler
// ============================================================================

/// Per-process I/O queue for CFQ
struct CfqQueue {
    requests: VecDeque<SchedRequest>,
    time_slice: u64,
    used_time: u64,
}

impl CfqQueue {
    fn new() -> Self {
        Self {
            requests: VecDeque::new(),
            time_slice: 100, // Default time slice
            used_time: 0,
        }
    }
}

/// Completely Fair Queuing scheduler
///
/// Provides fair I/O bandwidth distribution between processes.
/// Each process gets a time slice for I/O operations.
pub struct CfqScheduler {
    /// Per-process queues
    queues: BTreeMap<u64, CfqQueue>,
    /// Currently active process
    active_pid: Option<u64>,
    /// Round-robin order
    round_robin: VecDeque<u64>,
    /// Idle queue for requests without a process
    idle_queue: VecDeque<SchedRequest>,
}

impl CfqScheduler {
    /// Creates a new CFQ scheduler
    pub fn new() -> Self {
        Self {
            queues: BTreeMap::new(),
            active_pid: None,
            round_robin: VecDeque::new(),
            idle_queue: VecDeque::new(),
        }
    }

    fn get_or_create_queue(&mut self, pid: u64) -> &mut CfqQueue {
        if !self.queues.contains_key(&pid) {
            self.queues.insert(pid, CfqQueue::new());
            self.round_robin.push_back(pid);
        }
        self.queues.get_mut(&pid).unwrap()
    }
}

impl Default for CfqScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl IoScheduler for CfqScheduler {
    fn name(&self) -> &'static str {
        "cfq"
    }

    fn add_request(&mut self, request: SchedRequest) {
        let pid = request.pid;
        if pid == 0 {
            self.idle_queue.push_back(request);
        } else {
            self.get_or_create_queue(pid).requests.push_back(request);
        }
    }

    fn next_request(&mut self) -> Option<SchedRequest> {
        // Try active process first
        if let Some(pid) = self.active_pid {
            if let Some(queue) = self.queues.get_mut(&pid) {
                if let Some(req) = queue.requests.pop_front() {
                    queue.used_time += 1;
                    if queue.used_time >= queue.time_slice {
                        queue.used_time = 0;
                        self.active_pid = None;
                    }
                    return Some(req);
                } else {
                    // Queue empty, move to next
                    self.active_pid = None;
                }
            }
        }

        // Round-robin to next process
        while let Some(pid) = self.round_robin.pop_front() {
            if let Some(queue) = self.queues.get(&pid) {
                if !queue.requests.is_empty() {
                    self.round_robin.push_back(pid);
                    self.active_pid = Some(pid);
                    return self.next_request();
                }
            }
            // Remove empty queues
            self.queues.remove(&pid);
        }

        // Fallback to idle queue
        self.idle_queue.pop_front()
    }

    fn has_pending(&self) -> bool {
        !self.idle_queue.is_empty() || self.queues.values().any(|q| !q.requests.is_empty())
    }

    fn pending_count(&self) -> usize {
        self.idle_queue.len() + self.queues.values().map(|q| q.requests.len()).sum::<usize>()
    }
}

// ============================================================================
// Scheduler Management
// ============================================================================

/// Available scheduler types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerType {
    NoOp,
    Deadline,
    Cfq,
}

impl SchedulerType {
    /// Creates a scheduler instance
    pub fn create(self) -> Box<dyn IoScheduler> {
        match self {
            SchedulerType::NoOp => Box::new(NoOpScheduler::new()),
            SchedulerType::Deadline => Box::new(DeadlineScheduler::new()),
            SchedulerType::Cfq => Box::new(CfqScheduler::new()),
        }
    }
}

/// Global scheduler registry (per-device schedulers)
static SCHEDULERS: Mutex<BTreeMap<&'static str, Box<dyn IoScheduler>>> = Mutex::new(BTreeMap::new());

/// Sets the scheduler for a device
pub fn set_scheduler(device: &'static str, scheduler: Box<dyn IoScheduler>) {
    let mut schedulers = SCHEDULERS.lock();
    schedulers.insert(device, scheduler);
}

/// Gets the default scheduler type for a device type
pub fn default_scheduler_for_device(device_type: &str) -> SchedulerType {
    match device_type {
        "nvme" | "virtio" => SchedulerType::NoOp,
        "ahci" | "ide" => SchedulerType::Deadline,
        _ => SchedulerType::Deadline,
    }
}

/// Scheduler statistics
#[derive(Debug, Default, Clone)]
pub struct SchedulerStats {
    /// Requests submitted
    pub submitted: u64,
    /// Requests completed
    pub completed: u64,
    /// Requests merged
    pub merged: u64,
    /// Total wait time (ticks)
    pub total_wait_time: u64,
}

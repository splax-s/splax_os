//! # Multi-Core Scheduler Extensions
//!
//! This module extends the base scheduler with SMP support.
//!
//! ## Features
//!
//! - Per-CPU run queues for locality
//! - CPU affinity for processes
//! - Load balancing across cores
//! - Work stealing for idle CPUs

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use spin::Mutex;

use crate::smp::{CpuId, IpiType, MAX_CPUS};

use super::{ProcessId, ProcessState, SchedulingClass};

/// CPU affinity mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuMask {
    bits: [u64; 4],  // Support up to 256 CPUs
}

impl CpuMask {
    /// Creates a mask with all CPUs allowed.
    pub const fn all() -> Self {
        Self { bits: [!0; 4] }
    }

    /// Creates an empty mask.
    pub const fn none() -> Self {
        Self { bits: [0; 4] }
    }

    /// Creates a mask with a single CPU.
    pub fn single(cpu: CpuId) -> Self {
        let mut mask = Self::none();
        mask.set(cpu);
        mask
    }

    /// Sets a CPU in the mask.
    pub fn set(&mut self, cpu: CpuId) {
        let idx = cpu.as_u32() as usize;
        if idx < 256 {
            self.bits[idx / 64] |= 1 << (idx % 64);
        }
    }

    /// Clears a CPU from the mask.
    pub fn clear(&mut self, cpu: CpuId) {
        let idx = cpu.as_u32() as usize;
        if idx < 256 {
            self.bits[idx / 64] &= !(1 << (idx % 64));
        }
    }

    /// Checks if a CPU is in the mask.
    pub fn contains(&self, cpu: CpuId) -> bool {
        let idx = cpu.as_u32() as usize;
        if idx < 256 {
            (self.bits[idx / 64] & (1 << (idx % 64))) != 0
        } else {
            false
        }
    }

    /// Returns the first CPU in the mask.
    pub fn first(&self) -> Option<CpuId> {
        for (word_idx, &word) in self.bits.iter().enumerate() {
            if word != 0 {
                let bit_idx = word.trailing_zeros();
                return Some(CpuId::new((word_idx * 64 + bit_idx as usize) as u32));
            }
        }
        None
    }

    /// Returns the number of CPUs in the mask.
    pub fn count(&self) -> u32 {
        self.bits.iter().map(|w| w.count_ones()).sum()
    }
}

impl Default for CpuMask {
    fn default() -> Self {
        Self::all()
    }
}

/// Per-CPU run queue.
pub struct CpuRunQueue {
    /// CPU this queue belongs to.
    pub cpu_id: CpuId,
    /// Number of runnable tasks.
    pub nr_running: AtomicU32,
    /// Total load (for balancing).
    pub load: AtomicU64,
    /// Real-time queue (FIFO).
    realtime: Mutex<VecDeque<ProcessId>>,
    /// Interactive queue.
    interactive: Mutex<VecDeque<ProcessId>>,
    /// Background queue.
    background: Mutex<VecDeque<ProcessId>>,
}

impl CpuRunQueue {
    /// Creates a new run queue for a CPU.
    pub const fn new(cpu_id: CpuId) -> Self {
        Self {
            cpu_id,
            nr_running: AtomicU32::new(0),
            load: AtomicU64::new(0),
            realtime: Mutex::new(VecDeque::new()),
            interactive: Mutex::new(VecDeque::new()),
            background: Mutex::new(VecDeque::new()),
        }
    }

    /// Enqueues a process.
    pub fn enqueue(&self, pid: ProcessId, class: SchedulingClass, priority: u8) {
        match class {
            SchedulingClass::Realtime => {
                self.realtime.lock().push_back(pid);
            }
            SchedulingClass::Interactive => {
                // Insert sorted by priority
                let mut queue = self.interactive.lock();
                let pos = queue.iter().position(|_| true).unwrap_or(queue.len());
                queue.insert(pos, pid);
            }
            SchedulingClass::Background => {
                self.background.lock().push_back(pid);
            }
        }
        self.nr_running.fetch_add(1, Ordering::Relaxed);
        self.load.fetch_add(priority as u64 + 1, Ordering::Relaxed);
    }

    /// Dequeues the next process to run.
    pub fn dequeue(&self) -> Option<ProcessId> {
        // Check realtime first
        if let Some(pid) = self.realtime.lock().pop_front() {
            self.nr_running.fetch_sub(1, Ordering::Relaxed);
            return Some(pid);
        }

        // Then interactive
        if let Some(pid) = self.interactive.lock().pop_front() {
            self.nr_running.fetch_sub(1, Ordering::Relaxed);
            return Some(pid);
        }

        // Then background
        if let Some(pid) = self.background.lock().pop_front() {
            self.nr_running.fetch_sub(1, Ordering::Relaxed);
            return Some(pid);
        }

        None
    }

    /// Steals a process from this queue (for work stealing).
    pub fn steal(&self) -> Option<(ProcessId, SchedulingClass)> {
        // Only steal from background queue to avoid priority inversion
        if let Some(pid) = self.background.lock().pop_back() {
            self.nr_running.fetch_sub(1, Ordering::Relaxed);
            return Some((pid, SchedulingClass::Background));
        }

        // Can also steal from interactive if desperate
        if let Some(pid) = self.interactive.lock().pop_back() {
            self.nr_running.fetch_sub(1, Ordering::Relaxed);
            return Some((pid, SchedulingClass::Interactive));
        }

        None
    }

    /// Returns true if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.nr_running.load(Ordering::Relaxed) == 0
    }

    /// Returns the current load.
    pub fn get_load(&self) -> u64 {
        self.load.load(Ordering::Relaxed)
    }
}

/// Global SMP scheduler state.
pub struct SmpScheduler {
    /// Per-CPU run queues.
    run_queues: [CpuRunQueue; MAX_CPUS],
    /// Number of active CPUs.
    nr_cpus: AtomicU32,
    /// Load balancing tick counter.
    balance_tick: AtomicU64,
}

impl SmpScheduler {
    /// Creates a new SMP scheduler.
    pub const fn new() -> Self {
        const INIT_QUEUE: CpuRunQueue = CpuRunQueue::new(CpuId::BSP);
        Self {
            run_queues: [INIT_QUEUE; MAX_CPUS],
            nr_cpus: AtomicU32::new(1),
            balance_tick: AtomicU64::new(0),
        }
    }

    /// Gets the run queue for a CPU.
    pub fn get_run_queue(&self, cpu: CpuId) -> &CpuRunQueue {
        &self.run_queues[cpu.as_index()]
    }

    /// Enqueues a process on the best CPU.
    pub fn enqueue(
        &self,
        pid: ProcessId,
        class: SchedulingClass,
        priority: u8,
        affinity: CpuMask,
        last_cpu: Option<CpuId>,
    ) {
        let cpu = self.select_cpu(affinity, last_cpu);
        self.run_queues[cpu.as_index()].enqueue(pid, class, priority);
    }

    /// Selects the best CPU for a task.
    fn select_cpu(&self, affinity: CpuMask, last_cpu: Option<CpuId>) -> CpuId {
        let nr_cpus = self.nr_cpus.load(Ordering::Relaxed) as usize;

        // Prefer last CPU for cache locality
        if let Some(last) = last_cpu {
            if affinity.contains(last) {
                return last;
            }
        }

        // Find the least loaded CPU in the affinity mask
        let mut best_cpu = CpuId::BSP;
        let mut best_load = u64::MAX;

        for i in 0..nr_cpus {
            let cpu = CpuId::new(i as u32);
            if affinity.contains(cpu) {
                let load = self.run_queues[i].get_load();
                if load < best_load {
                    best_load = load;
                    best_cpu = cpu;
                }
            }
        }

        best_cpu
    }

    /// Attempts work stealing for an idle CPU.
    pub fn try_steal(&self, idle_cpu: CpuId) -> Option<(ProcessId, SchedulingClass)> {
        let nr_cpus = self.nr_cpus.load(Ordering::Relaxed) as usize;
        let idle_idx = idle_cpu.as_index();

        // Find the most loaded CPU and steal from it
        let mut busiest_idx = 0;
        let mut busiest_load = 0u64;

        for i in 0..nr_cpus {
            if i != idle_idx {
                let load = self.run_queues[i].get_load();
                if load > busiest_load {
                    busiest_load = load;
                    busiest_idx = i;
                }
            }
        }

        // Only steal if there's a significant imbalance
        if busiest_load > 0 {
            self.run_queues[busiest_idx].steal()
        } else {
            None
        }
    }

    /// Performs periodic load balancing.
    pub fn load_balance(&self) {
        let tick = self.balance_tick.fetch_add(1, Ordering::Relaxed);

        // Only balance every 100 ticks
        if tick % 100 != 0 {
            return;
        }

        let nr_cpus = self.nr_cpus.load(Ordering::Relaxed) as usize;
        if nr_cpus <= 1 {
            return;
        }

        // Calculate average load
        let total_load: u64 = (0..nr_cpus)
            .map(|i| self.run_queues[i].get_load())
            .sum();
        let avg_load = total_load / nr_cpus as u64;

        // Find imbalanced CPUs
        for i in 0..nr_cpus {
            let load = self.run_queues[i].get_load();
            if load < avg_load / 2 {
                // This CPU is underloaded, try to steal work
                if let Some((pid, class)) = self.try_steal(CpuId::new(i as u32)) {
                    // Re-enqueue on the underloaded CPU
                    self.run_queues[i].enqueue(pid, class, 128);

                    // Send IPI to wake the CPU if needed
                    crate::smp::send_ipi(CpuId::new(i as u32), IpiType::Reschedule);
                }
            }
        }
    }

    /// Called when a CPU goes online.
    pub fn cpu_online(&self, _cpu: CpuId) {
        self.nr_cpus.fetch_add(1, Ordering::SeqCst);
    }

    /// Called when a CPU goes offline.
    pub fn cpu_offline(&self, cpu: CpuId) {
        self.nr_cpus.fetch_sub(1, Ordering::SeqCst);

        // Migrate all tasks from the offline CPU
        let rq = &self.run_queues[cpu.as_index()];
        while let Some((pid, class)) = rq.steal() {
            // Re-enqueue on BSP
            self.run_queues[0].enqueue(pid, class, 128);
        }
    }
}

/// Global SMP scheduler instance.
static SMP_SCHEDULER: SmpScheduler = SmpScheduler::new();

/// Gets the global SMP scheduler.
pub fn smp_scheduler() -> &'static SmpScheduler {
    &SMP_SCHEDULER
}

/// Per-process scheduling data for SMP.
#[derive(Debug, Clone)]
pub struct SmpProcessData {
    /// CPU affinity mask.
    pub affinity: CpuMask,
    /// Last CPU this process ran on.
    pub last_cpu: Option<CpuId>,
    /// Migration count.
    pub migrations: u64,
}

impl Default for SmpProcessData {
    fn default() -> Self {
        Self {
            affinity: CpuMask::all(),
            last_cpu: None,
            migrations: 0,
        }
    }
}

/// Returns the number of CPUs detected in the system.
pub fn cpu_count() -> u32 {
    crate::smp::smp_state().num_cpus()
}

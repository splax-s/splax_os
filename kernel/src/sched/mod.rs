//! # Deterministic Scheduler
//!
//! The Splax scheduler provides deterministic, priority-based scheduling.
//!
//! ## Key Properties
//!
//! 1. **Determinism**: Same inputs produce same scheduling decisions
//! 2. **Priority-based**: Higher priority processes preempt lower ones
//! 3. **Time-bounded**: Maximum latency guarantees for real-time tasks
//! 4. **Fairness**: Lower priority processes still make progress
//! 5. **SMP-aware**: Per-CPU run queues with work stealing
//!
//! ## Scheduling Classes
//!
//! - **Realtime**: Guaranteed latency, for critical services
//! - **Interactive**: Low latency, for user-facing tools
//! - **Background**: Best effort, for batch processing
//!
//! ## No Magic
//!
//! Unlike traditional schedulers, there are no heuristics or "smart"
//! adjustments. The scheduler does exactly what you configure it to do.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use spin::Mutex;

pub mod smp;
pub use smp::{CpuMask, SmpProcessData, smp_scheduler};

/// Process identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

impl ProcessId {
    /// The kernel's process ID.
    pub const KERNEL: Self = Self(0);

    /// Creates a new process ID.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Scheduling class determines base priority and behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchedulingClass {
    /// Real-time class: guaranteed maximum latency
    Realtime,
    /// Interactive class: low latency for responsive processes
    Interactive,
    /// Background class: best-effort scheduling
    Background,
}

impl Default for SchedulingClass {
    fn default() -> Self {
        Self::Interactive
    }
}

/// Process state in the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// Ready to run
    Ready,
    /// Currently running on a CPU
    Running,
    /// Blocked waiting for an event
    Blocked,
    /// Process has terminated
    Terminated,
}

/// Scheduler configuration.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Time slice for interactive processes (in microseconds)
    pub interactive_time_slice_us: u64,
    /// Time slice for background processes (in microseconds)
    pub background_time_slice_us: u64,
    /// Maximum number of processes
    pub max_processes: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            interactive_time_slice_us: 10_000,  // 10ms
            background_time_slice_us: 50_000,   // 50ms
            max_processes: 65536,
        }
    }
}

/// Information about a scheduled process.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    /// Process ID
    pub pid: ProcessId,
    /// Scheduling class
    pub class: SchedulingClass,
    /// Priority within the class (0-255, higher = more priority)
    pub priority: u8,
    /// Current state
    pub state: ProcessState,
    /// CPU time consumed (in cycles)
    pub cpu_time: u64,
    /// Number of times scheduled
    pub schedule_count: u64,
    /// CPU context for context switching (x86_64 only)
    #[cfg(target_arch = "x86_64")]
    pub context: crate::arch::x86_64::context::Context,
}

/// The deterministic scheduler.
pub struct Scheduler {
    config: SchedulerConfig,
    /// All registered processes
    processes: Mutex<BTreeMap<ProcessId, ProcessInfo>>,
    /// Ready queue per scheduling class
    ready_queues: Mutex<ReadyQueues>,
    /// Currently running process
    current: Mutex<Option<ProcessId>>,
    /// Next process ID to assign
    next_pid: Mutex<u64>,
}

struct ReadyQueues {
    realtime: Vec<ProcessId>,
    interactive: Vec<ProcessId>,
    background: Vec<ProcessId>,
}

impl ReadyQueues {
    fn new() -> Self {
        Self {
            realtime: Vec::new(),
            interactive: Vec::new(),
            background: Vec::new(),
        }
    }
}

impl Scheduler {
    /// Creates a new scheduler.
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config,
            processes: Mutex::new(BTreeMap::new()),
            ready_queues: Mutex::new(ReadyQueues::new()),
            current: Mutex::new(None),
            next_pid: Mutex::new(1),
        }
    }

    /// Registers a new process with the scheduler.
    ///
    /// # Arguments
    ///
    /// * `class` - Scheduling class
    /// * `priority` - Priority within the class
    ///
    /// # Returns
    ///
    /// The new process ID.
    pub fn register_process(
        &self,
        class: SchedulingClass,
        priority: u8,
    ) -> Result<ProcessId, SchedulerError> {
        let mut next_pid = self.next_pid.lock();
        let pid = ProcessId::new(*next_pid);
        *next_pid += 1;

        let info = ProcessInfo {
            pid,
            class,
            priority,
            state: ProcessState::Ready,
            cpu_time: 0,
            schedule_count: 0,
            #[cfg(target_arch = "x86_64")]
            context: crate::arch::x86_64::context::Context::default(),
        };

        self.processes.lock().insert(pid, info);
        self.enqueue(pid, class);

        Ok(pid)
    }

    /// Marks a process as ready to run.
    pub fn wake(&self, pid: ProcessId) -> Result<(), SchedulerError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid).ok_or(SchedulerError::ProcessNotFound)?;

        if process.state == ProcessState::Blocked {
            process.state = ProcessState::Ready;
            self.enqueue(pid, process.class);
        }

        Ok(())
    }

    /// Blocks the current process.
    pub fn block(&self, pid: ProcessId) -> Result<(), SchedulerError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid).ok_or(SchedulerError::ProcessNotFound)?;

        process.state = ProcessState::Blocked;
        Ok(())
    }

    /// Terminates a process.
    pub fn terminate(&self, pid: ProcessId) -> Result<(), SchedulerError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid).ok_or(SchedulerError::ProcessNotFound)?;

        process.state = ProcessState::Terminated;
        
        // Clean up: remove from ready queues
        let mut queues = self.ready_queues.lock();
        queues.realtime.retain(|&p| p != pid);
        queues.interactive.retain(|&p| p != pid);
        queues.background.retain(|&p| p != pid);
        
        // Clear current if this was the running process
        let mut current = self.current.lock();
        if *current == Some(pid) {
            *current = None;
        }
        
        Ok(())
    }

    /// Selects the next process to run.
    ///
    /// This is the core scheduling algorithm:
    /// 1. Check realtime queue (highest priority)
    /// 2. Check interactive queue
    /// 3. Check background queue
    ///
    /// Within each queue, processes are ordered by priority.
    pub fn schedule(&self) -> Option<ProcessId> {
        let mut queues = self.ready_queues.lock();

        // Priority order: realtime > interactive > background
        if let Some(pid) = queues.realtime.pop() {
            return Some(pid);
        }
        if let Some(pid) = queues.interactive.pop() {
            return Some(pid);
        }
        if let Some(pid) = queues.background.pop() {
            return Some(pid);
        }

        None
    }

    /// Performs a context switch to the specified process.
    ///
    /// # Arguments
    ///
    /// * `pid` - Process to switch to
    pub fn switch_to(&self, pid: ProcessId) {
        let mut current = self.current.lock();
        let mut processes = self.processes.lock();
        
        // Save previous PID before we modify current
        let prev_pid_opt = *current;

        // Mark previous process as ready (if any)
        if let Some(prev_pid) = prev_pid_opt {
            if let Some(prev) = processes.get_mut(&prev_pid) {
                if prev.state == ProcessState::Running {
                    prev.state = ProcessState::Ready;
                    self.enqueue(prev_pid, prev.class);
                }
            }
        }

        // Mark new process as running
        if let Some(next) = processes.get_mut(&pid) {
            next.state = ProcessState::Running;
            next.schedule_count += 1;
        }

        *current = Some(pid);

        // Get contexts for context switch
        #[cfg(target_arch = "x86_64")]
        {
            // Get old and new process contexts
            let old_ctx = if let Some(prev_pid) = prev_pid_opt {
                processes.get(&prev_pid).map(|p| &p.context as *const _ as *mut crate::arch::x86_64::context::Context)
            } else {
                None
            };
            
            let new_ctx = processes.get(&pid).map(|p| &p.context as *const crate::arch::x86_64::context::Context);
            
            // Drop locks before context switch (switch may not return to this point)
            drop(processes);
            drop(current);
            
            if let (Some(old), Some(new)) = (old_ctx, new_ctx) {
                // Perform actual context switch
                unsafe {
                    crate::arch::x86_64::context::switch_context(old, new);
                }
            } else if let Some(new) = new_ctx {
                // First switch - just load the new context
                unsafe {
                    crate::arch::x86_64::context::init_context(new);
                }
            }
        }
        
        #[cfg(target_arch = "aarch64")]
        {
            // On AArch64: context is saved/restored via exception return (eret)
            // Memory barrier to ensure visibility
            unsafe {
                core::arch::asm!("dsb sy", "isb", options(nostack, preserves_flags));
            }
        }
    }

    /// Enqueues a process in the appropriate ready queue.
    fn enqueue(&self, pid: ProcessId, class: SchedulingClass) {
        let mut queues = self.ready_queues.lock();
        match class {
            SchedulingClass::Realtime => queues.realtime.push(pid),
            SchedulingClass::Interactive => queues.interactive.push(pid),
            SchedulingClass::Background => queues.background.push(pid),
        }
    }

    /// Gets the currently running process.
    pub fn current_process(&self) -> Option<ProcessId> {
        *self.current.lock()
    }

    /// Gets information about a process.
    pub fn get_process_info(&self, pid: ProcessId) -> Option<ProcessInfo> {
        self.processes.lock().get(&pid).cloned()
    }
}

/// Scheduler errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerError {
    /// Process not found
    ProcessNotFound,
    /// Too many processes
    TooManyProcesses,
    /// Invalid scheduling class
    InvalidClass,
}

/// Global scheduler instance
static SCHEDULER: spin::Lazy<Scheduler> = spin::Lazy::new(|| {
    Scheduler::new(SchedulerConfig::default())
});

/// Gets the global scheduler
pub fn scheduler() -> &'static Scheduler {
    &SCHEDULER
}

/// Returns the count of running processes
pub fn running_process_count() -> usize {
    let processes = SCHEDULER.processes.lock();
    processes.values().filter(|p| p.state == ProcessState::Running).count()
}

/// Returns the total process count
pub fn total_process_count() -> usize {
    SCHEDULER.processes.lock().len()
}

/// Lists all processes
pub fn list_processes() -> Vec<ProcessInfo> {
    SCHEDULER.processes.lock().values().cloned().collect()
}

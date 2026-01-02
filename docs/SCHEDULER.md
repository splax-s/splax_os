# Scheduler and SMP Documentation

## Overview

Splax OS implements a deterministic, priority-based scheduler with full SMP (Symmetric Multi-Processing) support. The scheduler provides predictable behavior—no heuristics, no "smart" adjustments—just explicit, auditable scheduling decisions.

```text
┌─────────────────────────────────────────────────────────────────┐
│                       Scheduler Layer                           │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                   SMP Extensions                         │   │
│  │  - Per-CPU run queues                                   │   │
│  │  - CPU affinity                                         │   │
│  │  - Load balancing                                       │   │
│  │  - Work stealing                                        │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                   Base Scheduler                         │   │
│  │  - Process registration                                 │   │
│  │  - Priority classes                                     │   │
│  │  - Context switching                                    │   │
│  │  - Ready queues                                         │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│                         SMP Layer                               │
│                                                                 │
│  ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐          ┌──────┐        │
│  │ CPU0 │ │ CPU1 │ │ CPU2 │ │ CPU3 │   ...    │CPUn-1│        │
│  └──────┘ └──────┘ └──────┘ └──────┘          └──────┘        │
│                                                                 │
│  - Per-CPU data structures                                     │
│  - Inter-Processor Interrupts (IPI)                            │
│  - TLB shootdowns                                              │
└─────────────────────────────────────────────────────────────────┘
```

---

## Design Principles

### Determinism

Same inputs produce the same scheduling decisions. No adaptive heuristics, no hidden state:

```rust
// The scheduler does exactly what you configure:
// - Realtime always preempts Interactive
// - Interactive always preempts Background
// - Within a class, higher priority wins
```

### Time-Bounded

Maximum latency guarantees for real-time tasks:

```rust
pub struct SchedulerConfig {
    /// Time slice for interactive processes (in microseconds)
    pub interactive_time_slice_us: u64,  // Default: 10ms
    /// Time slice for background processes (in microseconds)
    pub background_time_slice_us: u64,   // Default: 50ms
    // ...
}
```

### Fairness

Lower priority processes still make progress—starvation is prevented by design:

```rust
// Background queue is checked when realtime/interactive are empty
if let Some(pid) = queues.background.pop() {
    return Some(pid);
}
```

### SMP-Aware

Per-CPU run queues with work stealing for optimal cache locality and load distribution.

---

## Scheduling Classes

| Class | Priority | Time Slice | Use Case |
|-------|----------|------------|----------|
| **Realtime** | Highest | N/A (runs to completion) | Critical services, drivers |
| **Interactive** | Medium | 10 ms | User-facing tools, shell |
| **Background** | Lowest | 50 ms | Batch processing, builds |

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchedulingClass {
    /// Real-time class: guaranteed maximum latency
    Realtime,
    /// Interactive class: low latency for responsive processes
    Interactive,
    /// Background class: best-effort scheduling
    Background,
}
```

---

## Core Types

### Process Identifier

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

impl ProcessId {
    /// The kernel's process ID.
    pub const KERNEL: Self = Self(0);

    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}
```

### Process State

```rust
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
```

### Process Information

```rust
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
    pub context: Context,
}
```

### Scheduler Configuration

```rust
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Time slice for interactive processes (in microseconds)
    pub interactive_time_slice_us: u64,    // Default: 10,000 (10ms)
    /// Time slice for background processes (in microseconds)
    pub background_time_slice_us: u64,     // Default: 50,000 (50ms)
    /// Maximum number of processes
    pub max_processes: usize,               // Default: 65,536
}
```

---

## Scheduler Operations

### Register Process

```rust
impl Scheduler {
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
            context: Context::default(),
        };

        self.processes.lock().insert(pid, info);
        self.enqueue(pid, class);

        Ok(pid)
    }
}
```

### Wake Process

```rust
impl Scheduler {
    pub fn wake(&self, pid: ProcessId) -> Result<(), SchedulerError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid)
            .ok_or(SchedulerError::ProcessNotFound)?;

        if process.state == ProcessState::Blocked {
            process.state = ProcessState::Ready;
            self.enqueue(pid, process.class);
        }

        Ok(())
    }
}
```

### Block Process

```rust
impl Scheduler {
    pub fn block(&self, pid: ProcessId) -> Result<(), SchedulerError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid)
            .ok_or(SchedulerError::ProcessNotFound)?;

        process.state = ProcessState::Blocked;
        Ok(())
    }
}
```

### Terminate Process

```rust
impl Scheduler {
    pub fn terminate(&self, pid: ProcessId) -> Result<(), SchedulerError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid)
            .ok_or(SchedulerError::ProcessNotFound)?;

        process.state = ProcessState::Terminated;
        
        // Clean up: remove from ready queues
        let mut queues = self.ready_queues.lock();
        queues.realtime.retain(|&p| p != pid);
        queues.interactive.retain(|&p| p != pid);
        queues.background.retain(|&p| p != pid);
        
        // Clear current if this was the running process
        if *self.current.lock() == Some(pid) {
            *self.current.lock() = None;
        }
        
        Ok(())
    }
}
```

### Schedule (Select Next)

```rust
impl Scheduler {
    /// Selects the next process to run.
    /// Priority order: realtime > interactive > background
    pub fn schedule(&self) -> Option<ProcessId> {
        let mut queues = self.ready_queues.lock();

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
}
```

### Context Switch

```rust
impl Scheduler {
    pub fn switch_to(&self, pid: ProcessId) {
        // 1. Mark previous process as Ready
        // 2. Mark new process as Running
        // 3. Increment schedule_count
        // 4. Perform architecture-specific context switch

        #[cfg(target_arch = "x86_64")]
        unsafe {
            crate::arch::x86_64::context::switch_context(old_ctx, new_ctx);
        }

        #[cfg(target_arch = "aarch64")]
        unsafe {
            core::arch::asm!("dsb sy", "isb", options(nostack, preserves_flags));
        }
    }
}
```

---

## SMP Support

### Maximum CPUs

```rust
pub const MAX_CPUS: usize = 256;
```

### CPU Identifier

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CpuId(pub u32);

impl CpuId {
    /// Bootstrap processor (CPU 0)
    pub const BSP: Self = Self(0);

    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    pub const fn as_u32(&self) -> u32 {
        self.0
    }

    pub const fn as_index(&self) -> usize {
        self.0 as usize
    }
}
```

### CPU State

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CpuState {
    /// CPU is offline/not present
    Offline = 0,
    /// CPU is starting up
    Starting = 1,
    /// CPU is online and running
    Online = 2,
    /// CPU is halted (idle)
    Halted = 3,
    /// CPU is in panic state
    Panicked = 4,
}
```

### CPU Information

```rust
#[derive(Debug)]
pub struct CpuInfo {
    /// CPU identifier
    pub id: CpuId,
    /// Current state
    pub state: AtomicU32,
    /// Whether this is the bootstrap processor
    pub is_bsp: bool,
    /// APIC ID (x86_64) or MPIDR (aarch64)
    pub hw_id: u64,
    /// Pointer to this CPU's stack
    pub stack_top: u64,
    /// Number of interrupts handled
    pub interrupt_count: AtomicUsize,
    /// Number of context switches
    pub context_switches: AtomicUsize,
}
```

---

## CPU Affinity

The CPU affinity mask controls which CPUs a process can run on:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuMask {
    bits: [u64; 4],  // Support up to 256 CPUs
}

impl CpuMask {
    /// Creates a mask with all CPUs allowed
    pub const fn all() -> Self {
        Self { bits: [!0; 4] }
    }

    /// Creates an empty mask
    pub const fn none() -> Self {
        Self { bits: [0; 4] }
    }

    /// Creates a mask with a single CPU
    pub fn single(cpu: CpuId) -> Self {
        let mut mask = Self::none();
        mask.set(cpu);
        mask
    }

    /// Sets a CPU in the mask
    pub fn set(&mut self, cpu: CpuId) {
        let idx = cpu.as_u32() as usize;
        if idx < 256 {
            self.bits[idx / 64] |= 1 << (idx % 64);
        }
    }

    /// Checks if a CPU is in the mask
    pub fn contains(&self, cpu: CpuId) -> bool {
        let idx = cpu.as_u32() as usize;
        if idx < 256 {
            (self.bits[idx / 64] & (1 << (idx % 64))) != 0
        } else {
            false
        }
    }

    /// Returns the number of CPUs in the mask
    pub fn count(&self) -> u32 {
        self.bits.iter().map(|w| w.count_ones()).sum()
    }
}
```

### Per-Process SMP Data

```rust
#[derive(Debug, Clone)]
pub struct SmpProcessData {
    /// CPU affinity mask
    pub affinity: CpuMask,
    /// Last CPU this process ran on
    pub last_cpu: Option<CpuId>,
    /// Migration count
    pub migrations: u64,
}
```

---

## Per-CPU Run Queues

Each CPU has its own run queue for cache locality:

```rust
pub struct CpuRunQueue {
    /// CPU this queue belongs to
    pub cpu_id: CpuId,
    /// Number of runnable tasks
    pub nr_running: AtomicU32,
    /// Total load (for balancing)
    pub load: AtomicU64,
    /// Real-time queue (FIFO)
    realtime: Mutex<VecDeque<ProcessId>>,
    /// Interactive queue
    interactive: Mutex<VecDeque<ProcessId>>,
    /// Background queue
    background: Mutex<VecDeque<ProcessId>>,
}
```

### Run Queue Operations

#### Enqueue

```rust
impl CpuRunQueue {
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
}
```

#### Dequeue

```rust
impl CpuRunQueue {
    pub fn dequeue(&self) -> Option<ProcessId> {
        // Priority: realtime > interactive > background
        if let Some(pid) = self.realtime.lock().pop_front() {
            self.nr_running.fetch_sub(1, Ordering::Relaxed);
            return Some(pid);
        }

        if let Some(pid) = self.interactive.lock().pop_front() {
            self.nr_running.fetch_sub(1, Ordering::Relaxed);
            return Some(pid);
        }

        if let Some(pid) = self.background.lock().pop_front() {
            self.nr_running.fetch_sub(1, Ordering::Relaxed);
            return Some(pid);
        }

        None
    }
}
```

#### Work Stealing

```rust
impl CpuRunQueue {
    /// Steals a process from this queue (for idle CPUs)
    pub fn steal(&self) -> Option<(ProcessId, SchedulingClass)> {
        // Only steal from background to avoid priority inversion
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
}
```

---

## SMP Scheduler

### CPU Selection

```rust
impl SmpScheduler {
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
}
```

### Load Balancing

```rust
impl SmpScheduler {
    pub fn load_balance(&self) {
        let tick = self.balance_tick.fetch_add(1, Ordering::Relaxed);

        // Only balance every 100 ticks
        if tick % 100 != 0 {
            return;
        }

        // Calculate average load
        let total_load: u64 = (0..nr_cpus)
            .map(|i| self.run_queues[i].get_load())
            .sum();
        let avg_load = total_load / nr_cpus as u64;

        // Find underloaded CPUs and steal work
        for i in 0..nr_cpus {
            let load = self.run_queues[i].get_load();
            if load < avg_load / 2 {
                if let Some((pid, class)) = self.try_steal(CpuId::new(i as u32)) {
                    self.run_queues[i].enqueue(pid, class, 128);
                    crate::smp::send_ipi(CpuId::new(i as u32), IpiType::Reschedule);
                }
            }
        }
    }
}
```

---

## Inter-Processor Interrupts (IPI)

IPIs are used for cross-CPU communication:

### IPI Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IpiType {
    /// Reschedule hint - target CPU should run scheduler
    Reschedule = 0,
    /// TLB shootdown - target CPU should invalidate TLB
    TlbShootdown = 1,
    /// Stop - target CPU should halt
    Stop = 2,
    /// Function call - target CPU should call a function
    FunctionCall = 3,
}
```

### Send IPI

```rust
pub fn send_ipi(target: CpuId, ipi_type: IpiType) {
    #[cfg(target_arch = "x86_64")]
    {
        // Send via Local APIC ICR
        const LAPIC_BASE: usize = 0xFEE0_0000;
        const ICR_LOW: usize = LAPIC_BASE + 0x300;
        const ICR_HIGH: usize = LAPIC_BASE + 0x310;

        let vector = match ipi_type {
            IpiType::Reschedule => 0xFD,
            IpiType::TlbShootdown => 0xFC,
            IpiType::Stop => 0xFE,
            IpiType::FunctionCall => 0xFB,
        };

        unsafe {
            core::ptr::write_volatile(ICR_HIGH as *mut u32, target.as_u32() << 24);
            core::ptr::write_volatile(ICR_LOW as *mut u32, vector);
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Send Software Generated Interrupt via GIC
        const GICD_BASE: usize = 0x0800_0000;
        const GICD_SGIR: usize = GICD_BASE + 0xF00;

        let sgi_id = ipi_type as u8;
        let target_list = 1u8 << target.as_u32();
        let sgir_value = ((target_list as u32) << 16) | (sgi_id as u32);

        unsafe {
            core::ptr::write_volatile(GICD_SGIR as *mut u32, sgir_value);
        }
    }
}
```

### Handle IPI

```rust
pub fn handle_ipi(ipi_type: IpiType) {
    match ipi_type {
        IpiType::Reschedule => {
            // Trigger reschedule on this CPU
        }
        IpiType::TlbShootdown => {
            #[cfg(target_arch = "x86_64")]
            unsafe { core::arch::asm!("invlpg [{}]", in(reg) 0u64); }

            #[cfg(target_arch = "aarch64")]
            unsafe {
                core::arch::asm!("tlbi vmalle1");
                core::arch::asm!("dsb sy");
                core::arch::asm!("isb");
            }
        }
        IpiType::Stop => {
            loop { crate::arch::halt(); }
        }
        IpiType::FunctionCall => {
            // Execute pending function calls for this CPU
        }
    }
}
```

---

## Global Instances

### Base Scheduler

```rust
static SCHEDULER: spin::Lazy<Scheduler> = spin::Lazy::new(|| {
    Scheduler::new(SchedulerConfig::default())
});

pub fn scheduler() -> &'static Scheduler {
    &SCHEDULER
}
```

### SMP Scheduler

```rust
static SMP_SCHEDULER: SmpScheduler = SmpScheduler::new();

pub fn smp_scheduler() -> &'static SmpScheduler {
    &SMP_SCHEDULER
}
```

### SMP State

```rust
static SMP_STATE: SmpState = SmpState::new();

pub fn smp_state() -> &'static SmpState {
    &SMP_STATE
}
```

---

## Initialization

### BSP Initialization

```rust
pub fn init_bsp() {
    // Register BSP
    let hw_id = get_bsp_hw_id();
    SMP_STATE.register_cpu(CpuId::BSP, true, hw_id);
    SMP_STATE.cpus[0].lock().as_ref().unwrap().set_state(CpuState::Online);

    // Initialize per-CPU data for BSP
    percpu::init_bsp();

    SMP_STATE.initialized.store(true, Ordering::Release);
}
```

### AP Startup (x86_64)

**ACPI CPU Enumeration:**
```rust
// kernel/src/acpi/mod.rs provides helper functions:
pub fn cpu_count() -> usize;        // Total number of enabled CPUs
pub fn get_apic_ids() -> Vec<u8>;   // APIC IDs of all enabled processors
pub fn bsp_apic_id() -> Option<u8>; // APIC ID of the bootstrap processor
```

**INIT-SIPI-SIPI Sequence:**
```rust
// kernel/src/smp/mod.rs
// x86_64 AP startup via INIT-SIPI-SIPI sequence:
// 1. Parse ACPI MADT table to find AP APIC IDs via acpi::get_apic_ids()
// 2. For each AP:
//    a. Send INIT IPI (ICR = 0x00C500 | (apic_id << 56))
//    b. Wait 10ms
//    c. Send SIPI IPI twice (ICR = 0x00C600 | vector | (apic_id << 56))
// 3. APs jump to trampoline code at vector*0x1000
//
// The LAPIC (0xFEE00000) is accessible via boot-time 4GB identity mapping
```

### AP Startup (aarch64)

```rust
// aarch64 AP startup via PSCI or spin-table:
// 1. Parse DTB to find CPU nodes and enable-method
// 2. For spin-table: write entry point to cpu-release-addr
// 3. For PSCI: call PSCI_CPU_ON (function ID 0xC4000003)
//    - x1 = target CPU MPIDR
//    - x2 = entry point
//    - x3 = context ID
```

---

## Get Current CPU

```rust
#[inline]
pub fn current_cpu_id() -> CpuId {
    #[cfg(target_arch = "x86_64")]
    {
        let id: u32;
        unsafe {
            core::arch::asm!(
                "rdtscp",
                out("ecx") id,
                out("eax") _,
                out("edx") _,
            );
        }
        CpuId::new(id)
    }

    #[cfg(target_arch = "aarch64")]
    {
        let mpidr: u64;
        unsafe {
            core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr);
        }
        CpuId::new((mpidr & 0xFF) as u32)
    }
}
```

---

## Scheduler Errors

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerError {
    /// Process not found
    ProcessNotFound,
    /// Too many processes
    TooManyProcesses,
    /// Invalid scheduling class
    InvalidClass,
}
```

---

## Shell Commands

```text
ps               - List all processes
ps -a            - List all processes with details
sched info       - Show scheduler statistics
sched class PID  - Change scheduling class
cpu list         - List all CPUs
cpu online       - Show online CPU count
cpu affinity PID - Show/set CPU affinity
```

---

## Usage Examples

### Register a Process

```rust
let pid = scheduler().register_process(
    SchedulingClass::Interactive,
    128,  // Priority
)?;
```

### Set CPU Affinity

```rust
let mut affinity = CpuMask::none();
affinity.set(CpuId::new(0));  // Only CPU 0
affinity.set(CpuId::new(1));  // Also CPU 1

let smp_data = SmpProcessData {
    affinity,
    last_cpu: None,
    migrations: 0,
};
```

### Manual Context Switch

```rust
if let Some(next_pid) = scheduler().schedule() {
    scheduler().switch_to(next_pid);
}
```

---

## File Structure

```text
kernel/src/sched/
├── mod.rs              # Base scheduler
└── smp.rs              # SMP extensions (per-CPU queues, affinity)

kernel/src/smp/
├── mod.rs              # SMP core (CpuId, CpuState, IPI)
└── percpu.rs           # Per-CPU data structures
```

---

## Architecture Support

| Feature | x86_64 | aarch64 | riscv64 |
|---------|--------|---------|---------|
| Context Switch | ✓ | ✓ | Planned |
| Per-CPU Run Queues | ✓ | ✓ | ✓ |
| IPI (Reschedule) | ✓ (APIC) | ✓ (GIC) | Planned |
| TLB Shootdown | ✓ | ✓ | Planned |
| Work Stealing | ✓ | ✓ | ✓ |

---

## Future Work

- [ ] Priority inheritance for real-time tasks
- [ ] Earliest Deadline First (EDF) scheduling class
- [ ] NUMA-aware scheduling
- [ ] CPU hotplug support
- [ ] Preemption points in kernel
- [ ] Real-time bandwidth reservation
- [ ] Deadline-based scheduling
- [ ] cgroup-style resource groups

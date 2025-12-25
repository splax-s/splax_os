//! # Per-CPU Data Structures
//!
//! This module provides CPU-local storage for data that should be
//! unique to each processor core.
//!
//! ## Design
//!
//! Each CPU has its own instance of `PerCpuData`, accessed via the
//! GS segment (x86_64) or TPIDR_EL1 register (aarch64).

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};

use super::{CpuId, CpuState, MAX_CPUS};
use crate::sched::ProcessId;

/// Per-CPU data structure.
///
/// This is stored at a fixed location for each CPU and accessed
/// via architecture-specific mechanisms.
#[repr(C)]
pub struct PerCpuData {
    /// Self-pointer for quick access.
    pub self_ptr: *mut PerCpuData,
    /// CPU identifier.
    pub cpu_id: CpuId,
    /// Current CPU state.
    pub state: CpuState,
    /// Currently running process on this CPU.
    pub current_process: Option<ProcessId>,
    /// Idle process for this CPU.
    pub idle_process: Option<ProcessId>,
    /// Interrupt nesting depth.
    pub interrupt_depth: u32,
    /// Preemption disable count.
    pub preempt_count: u32,
    /// Kernel stack pointer for this CPU.
    pub kernel_stack: u64,
    /// Number of ticks since boot on this CPU.
    pub tick_count: AtomicU64,
    /// Pending work flags.
    pub pending_work: AtomicU64,
    /// Scheduler run queue for this CPU (pointer to avoid circular deps).
    pub run_queue: AtomicPtr<()>,
}

// SAFETY: PerCpuData is only accessed by the owning CPU, so it's safe to share
// the array across threads (each CPU only touches its own entry).
unsafe impl Sync for PerCpuData {}
unsafe impl Send for PerCpuData {}

impl PerCpuData {
    /// Creates a new per-CPU data structure.
    pub const fn new(cpu_id: CpuId) -> Self {
        Self {
            self_ptr: core::ptr::null_mut(),
            cpu_id,
            state: CpuState::Offline,
            current_process: None,
            idle_process: None,
            interrupt_depth: 0,
            preempt_count: 0,
            kernel_stack: 0,
            tick_count: AtomicU64::new(0),
            pending_work: AtomicU64::new(0),
            run_queue: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// Returns true if preemption is disabled.
    pub fn preempt_disabled(&self) -> bool {
        self.preempt_count > 0 || self.interrupt_depth > 0
    }

    /// Disables preemption.
    pub fn preempt_disable(&mut self) {
        self.preempt_count += 1;
    }

    /// Enables preemption.
    pub fn preempt_enable(&mut self) {
        debug_assert!(self.preempt_count > 0);
        self.preempt_count -= 1;
    }

    /// Increments tick count.
    pub fn tick(&self) {
        self.tick_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns the current tick count.
    pub fn get_ticks(&self) -> u64 {
        self.tick_count.load(Ordering::Relaxed)
    }
}

/// Pending work flags.
pub mod work_flags {
    /// Need to run the scheduler.
    pub const NEED_RESCHED: u64 = 1 << 0;
    /// Pending signals to deliver.
    pub const PENDING_SIGNALS: u64 = 1 << 1;
    /// Pending softirqs.
    pub const PENDING_SOFTIRQ: u64 = 1 << 2;
}

/// Wrapper to make UnsafeCell Sync for per-CPU data.
/// 
/// SAFETY: Each CPU only accesses its own entry, so there are no races.
#[repr(transparent)]
struct SyncUnsafeCell<T>(UnsafeCell<T>);

// SAFETY: Per-CPU data is only accessed by the owning CPU
unsafe impl<T> Sync for SyncUnsafeCell<T> {}

impl<T> SyncUnsafeCell<T> {
    const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    fn get(&self) -> *mut T {
        self.0.get()
    }
}

/// Storage for per-CPU data.
///
/// This is a simple array indexed by CPU ID. For BSP, we use index 0.
static PER_CPU_ARRAY: [SyncUnsafeCell<PerCpuData>; MAX_CPUS] = {
    const INIT: SyncUnsafeCell<PerCpuData> = SyncUnsafeCell::new(PerCpuData::new(CpuId::BSP));
    [INIT; MAX_CPUS]
};

/// Whether per-CPU data is initialized.
static PERCPU_INITIALIZED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Initialize per-CPU data for the bootstrap processor.
pub fn init_bsp() {
    let data = unsafe { &mut *PER_CPU_ARRAY[0].get() };
    data.cpu_id = CpuId::BSP;
    data.state = CpuState::Online;
    data.self_ptr = data as *mut PerCpuData;

    // Set up architecture-specific per-CPU pointer
    #[cfg(target_arch = "x86_64")]
    {
        // Set GS base to point to per-CPU data
        // This requires the FSGSBASE feature or MSR writes
        unsafe {
            let ptr = data as *const PerCpuData as u64;
            // Write to IA32_GS_BASE MSR (0xC0000101)
            // For now, we'll use WRGSBASE if available
            core::arch::asm!(
                "wrgsbase {}",
                in(reg) ptr,
                options(nomem, nostack)
            );
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Set TPIDR_EL1 to point to per-CPU data
        unsafe {
            let ptr = data as *const PerCpuData as u64;
            core::arch::asm!(
                "msr tpidr_el1, {}",
                in(reg) ptr,
                options(nomem, nostack)
            );
        }
    }

    PERCPU_INITIALIZED.store(true, Ordering::Release);
}

/// Initialize per-CPU data for an application processor.
///
/// # Safety
///
/// Must be called on the target AP during its startup sequence.
pub unsafe fn init_ap(cpu_id: CpuId) {
    let idx = cpu_id.as_index();
    if idx >= MAX_CPUS {
        return;
    }

    let data = unsafe { &mut *PER_CPU_ARRAY[idx].get() };
    data.cpu_id = cpu_id;
    data.state = CpuState::Starting;
    data.self_ptr = data as *mut PerCpuData;

    #[cfg(target_arch = "x86_64")]
    {
        unsafe {
            let ptr = data as *const PerCpuData as u64;
            core::arch::asm!(
                "wrgsbase {}",
                in(reg) ptr,
                options(nomem, nostack)
            );
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        unsafe {
            let ptr = data as *const PerCpuData as u64;
            core::arch::asm!(
                "msr tpidr_el1, {}",
                in(reg) ptr,
                options(nomem, nostack)
            );
        }
    }
}

/// Get a reference to the current CPU's per-CPU data.
///
/// # Safety
///
/// This must only be called after per-CPU data is initialized.
#[inline(always)]
pub fn current() -> &'static PerCpuData {
    debug_assert!(PERCPU_INITIALIZED.load(Ordering::Acquire));

    #[cfg(target_arch = "x86_64")]
    {
        let ptr: u64;
        unsafe {
            core::arch::asm!(
                "rdgsbase {}",
                out(reg) ptr,
                options(nomem, nostack, pure)
            );
        }
        unsafe { &*(ptr as *const PerCpuData) }
    }

    #[cfg(target_arch = "aarch64")]
    {
        let ptr: u64;
        unsafe {
            core::arch::asm!(
                "mrs {}, tpidr_el1",
                out(reg) ptr,
                options(nomem, nostack)
            );
        }
        unsafe { &*(ptr as *const PerCpuData) }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        unsafe { &*PER_CPU_ARRAY[0].get() }
    }
}

/// Get a mutable reference to the current CPU's per-CPU data.
///
/// # Safety
///
/// Caller must ensure no other references exist.
#[inline(always)]
pub unsafe fn current_mut() -> &'static mut PerCpuData {
    debug_assert!(PERCPU_INITIALIZED.load(Ordering::Acquire));

    #[cfg(target_arch = "x86_64")]
    {
        let ptr: u64;
        unsafe {
            core::arch::asm!(
                "rdgsbase {}",
                out(reg) ptr,
                options(nomem, nostack, pure)
            );
        }
        unsafe { &mut *(ptr as *mut PerCpuData) }
    }

    #[cfg(target_arch = "aarch64")]
    {
        let ptr: u64;
        unsafe {
            core::arch::asm!(
                "mrs {}, tpidr_el1",
                out(reg) ptr,
                options(nomem, nostack)
            );
        }
        unsafe { &mut *(ptr as *mut PerCpuData) }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        unsafe { &mut *PER_CPU_ARRAY[0].get() }
    }
}

/// Get per-CPU data for a specific CPU.
///
/// # Safety
///
/// The returned reference is only valid if that CPU's per-CPU data is initialized.
pub unsafe fn get(cpu_id: CpuId) -> &'static PerCpuData {
    let idx = cpu_id.as_index();
    debug_assert!(idx < MAX_CPUS);
    unsafe { &*PER_CPU_ARRAY[idx].get() }
}

/// Disable preemption on the current CPU.
#[inline]
pub fn preempt_disable() {
    unsafe {
        current_mut().preempt_disable();
    }
}

/// Enable preemption on the current CPU.
#[inline]
pub fn preempt_enable() {
    unsafe {
        current_mut().preempt_enable();
    }
}

/// Check if preemption is disabled.
#[inline]
pub fn preempt_disabled() -> bool {
    current().preempt_disabled()
}

/// Get the current CPU ID quickly.
#[inline]
pub fn cpu_id() -> CpuId {
    if PERCPU_INITIALIZED.load(Ordering::Acquire) {
        current().cpu_id
    } else {
        CpuId::BSP
    }
}

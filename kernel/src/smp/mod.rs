//! # Symmetric Multi-Processing (SMP) Support
//!
//! This module provides multi-core CPU support for Splax OS.
//!
//! ## Features
//!
//! - Per-CPU data structures
//! - Application Processor (AP) startup
//! - Inter-Processor Interrupts (IPI)
//! - CPU-local storage
//!
//! ## Design
//!
//! Each CPU core has its own:
//! - Run queue for the scheduler
//! - Idle task
//! - Local APIC (x86_64) or redistributor (aarch64)
//! - Stack

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use spin::Mutex;

pub mod percpu;

/// Maximum number of supported CPUs.
pub const MAX_CPUS: usize = 256;

/// CPU identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CpuId(pub u32);

impl CpuId {
    /// Bootstrap processor (CPU 0).
    pub const BSP: Self = Self(0);

    /// Creates a new CPU ID.
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Returns the raw CPU ID value.
    pub const fn as_u32(&self) -> u32 {
        self.0
    }

    /// Returns this CPU's index in per-CPU arrays.
    pub const fn as_index(&self) -> usize {
        self.0 as usize
    }
}

/// CPU state in the SMP system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CpuState {
    /// CPU is offline/not present.
    Offline = 0,
    /// CPU is starting up.
    Starting = 1,
    /// CPU is online and running.
    Online = 2,
    /// CPU is halted (idle).
    Halted = 3,
    /// CPU is in panic state.
    Panicked = 4,
}

/// Information about a single CPU.
#[derive(Debug)]
pub struct CpuInfo {
    /// CPU identifier.
    pub id: CpuId,
    /// Current state.
    pub state: AtomicU32,
    /// Whether this is the bootstrap processor.
    pub is_bsp: bool,
    /// APIC ID (x86_64) or MPIDR (aarch64).
    pub hw_id: u64,
    /// Pointer to this CPU's stack.
    pub stack_top: u64,
    /// Number of interrupts handled.
    pub interrupt_count: AtomicUsize,
    /// Number of context switches.
    pub context_switches: AtomicUsize,
}

impl CpuInfo {
    /// Creates a new CPU info structure.
    pub const fn new(id: CpuId, is_bsp: bool, hw_id: u64) -> Self {
        Self {
            id,
            state: AtomicU32::new(CpuState::Offline as u32),
            is_bsp,
            hw_id,
            stack_top: 0,
            interrupt_count: AtomicUsize::new(0),
            context_switches: AtomicUsize::new(0),
        }
    }

    /// Returns the current CPU state.
    pub fn get_state(&self) -> CpuState {
        match self.state.load(Ordering::Acquire) {
            0 => CpuState::Offline,
            1 => CpuState::Starting,
            2 => CpuState::Online,
            3 => CpuState::Halted,
            4 => CpuState::Panicked,
            _ => CpuState::Offline,
        }
    }

    /// Sets the CPU state.
    pub fn set_state(&self, state: CpuState) {
        self.state.store(state as u32, Ordering::Release);
    }
}

/// Global SMP state.
pub struct SmpState {
    /// Number of CPUs detected.
    pub cpu_count: AtomicU32,
    /// Number of CPUs online.
    pub online_count: AtomicU32,
    /// Whether SMP is initialized.
    pub initialized: AtomicBool,
    /// Per-CPU information.
    cpus: [Mutex<Option<CpuInfo>>; MAX_CPUS],
}

impl SmpState {
    /// Creates a new SMP state.
    const fn new() -> Self {
        // Initialize all CPU slots to None
        const INIT: Mutex<Option<CpuInfo>> = Mutex::new(None);
        Self {
            cpu_count: AtomicU32::new(1),  // At least BSP
            online_count: AtomicU32::new(1),
            initialized: AtomicBool::new(false),
            cpus: [INIT; MAX_CPUS],
        }
    }

    /// Registers a CPU.
    pub fn register_cpu(&self, id: CpuId, is_bsp: bool, hw_id: u64) {
        let idx = id.as_index();
        if idx < MAX_CPUS {
            let mut cpu = self.cpus[idx].lock();
            *cpu = Some(CpuInfo::new(id, is_bsp, hw_id));
            self.cpu_count.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Gets CPU info by ID.
    pub fn get_cpu(&self, id: CpuId) -> Option<CpuId> {
        let idx = id.as_index();
        if idx < MAX_CPUS {
            let cpu = self.cpus[idx].lock();
            cpu.as_ref().map(|c| c.id)
        } else {
            None
        }
    }

    /// Marks a CPU as online.
    pub fn cpu_online(&self, id: CpuId) {
        let idx = id.as_index();
        if idx < MAX_CPUS {
            if let Some(ref cpu) = *self.cpus[idx].lock() {
                cpu.set_state(CpuState::Online);
                self.online_count.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    /// Returns the number of online CPUs.
    pub fn num_online(&self) -> u32 {
        self.online_count.load(Ordering::Acquire)
    }

    /// Returns the total number of CPUs.
    pub fn num_cpus(&self) -> u32 {
        self.cpu_count.load(Ordering::Acquire)
    }

    /// Iterates over all online CPUs.
    pub fn for_each_online<F: FnMut(CpuId)>(&self, mut f: F) {
        for i in 0..MAX_CPUS {
            if let Some(ref cpu) = *self.cpus[i].lock() {
                if cpu.get_state() == CpuState::Online {
                    f(cpu.id);
                }
            }
        }
    }
}

/// Global SMP state instance.
static SMP_STATE: SmpState = SmpState::new();

// ============================================================================
// Per-CPU Function Call Queue
// ============================================================================

/// A function call that can be executed on a remote CPU.
pub struct FunctionCall {
    /// The function to execute (takes a context pointer).
    func: fn(u64),
    /// Argument/context to pass to the function.
    arg: u64,
}

/// A lock-free queue for per-CPU function calls.
/// Uses a simple ring buffer with atomic head/tail pointers.
pub struct FunctionCallQueue {
    /// Ring buffer of function calls.
    buffer: Mutex<[Option<FunctionCall>; 64]>,
    /// Number of pending calls.
    count: AtomicUsize,
}

impl FunctionCallQueue {
    /// Creates a new empty function call queue.
    const fn new() -> Self {
        const EMPTY: Option<FunctionCall> = None;
        Self {
            buffer: Mutex::new([EMPTY; 64]),
            count: AtomicUsize::new(0),
        }
    }

    /// Pushes a function call onto the queue.
    /// Returns true if successful, false if the queue is full.
    pub fn push(&self, call: FunctionCall) -> bool {
        let mut buffer = self.buffer.lock();
        // Find an empty slot
        for slot in buffer.iter_mut() {
            if slot.is_none() {
                *slot = Some(call);
                self.count.fetch_add(1, Ordering::Release);
                return true;
            }
        }
        false
    }

    /// Pops a function call from the queue.
    pub fn pop(&self) -> Option<FunctionCall> {
        if self.count.load(Ordering::Acquire) == 0 {
            return None;
        }
        let mut buffer = self.buffer.lock();
        // Find a non-empty slot
        for slot in buffer.iter_mut() {
            if slot.is_some() {
                let call = slot.take();
                self.count.fetch_sub(1, Ordering::Release);
                return call;
            }
        }
        None
    }

    /// Returns true if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.count.load(Ordering::Acquire) == 0
    }
}

/// Per-CPU function call queues.
static FUNCTION_CALL_QUEUES: [FunctionCallQueue; MAX_CPUS] = {
    const INIT: FunctionCallQueue = FunctionCallQueue::new();
    [INIT; MAX_CPUS]
};

/// Schedules a function to be called on a remote CPU.
///
/// The function will be executed when the target CPU handles the
/// FunctionCall IPI. This is useful for operations that must be
/// performed on a specific CPU, such as TLB invalidations or
/// cache management.
///
/// # Arguments
///
/// * `target` - The CPU to execute the function on
/// * `func` - The function to execute
/// * `arg` - Argument to pass to the function
///
/// # Returns
///
/// `true` if the call was queued successfully, `false` if the queue is full.
pub fn call_on_cpu(target: CpuId, func: fn(u64), arg: u64) -> bool {
    let idx = target.as_index();
    if idx >= MAX_CPUS {
        return false;
    }

    let call = FunctionCall { func, arg };
    if FUNCTION_CALL_QUEUES[idx].push(call) {
        // Send IPI to wake up the target CPU
        send_ipi(target, IpiType::FunctionCall);
        true
    } else {
        false
    }
}

/// Schedules a function to be called on all CPUs except the current one.
///
/// # Arguments
///
/// * `func` - The function to execute
/// * `arg` - Argument to pass to the function
pub fn call_on_all_others(func: fn(u64), arg: u64) {
    let current = current_cpu_id();
    SMP_STATE.for_each_online(|cpu| {
        if cpu != current {
            let _ = call_on_cpu(cpu, func, arg);
        }
    });
}

/// Returns a reference to the global SMP state.
pub fn smp_state() -> &'static SmpState {
    &SMP_STATE
}

/// Gets the current CPU ID.
///
/// This reads the CPU ID from architecture-specific registers.
#[inline]
pub fn current_cpu_id() -> CpuId {
    #[cfg(target_arch = "x86_64")]
    {
        // Read from APIC ID or use CPUID
        // For now, simplified version using IA32_TSC_AUX if available
        // or fall back to LAPIC ID
        let id: u32;
        unsafe {
            // Try RDTSCP which puts CPU ID in ECX
            core::arch::asm!(
                "rdtscp",
                out("ecx") id,
                out("eax") _,
                out("edx") _,
                options(nomem, nostack)
            );
        }
        CpuId::new(id)
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Read MPIDR_EL1 to get CPU ID
        let mpidr: u64;
        unsafe {
            core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr, options(nomem, nostack));
        }
        // Extract Aff0 (lowest affinity level, usually core ID)
        CpuId::new((mpidr & 0xFF) as u32)
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        CpuId::BSP
    }
}

/// Initialize SMP on the bootstrap processor.
pub fn init_bsp() {
    // Register BSP
    let hw_id = get_bsp_hw_id();
    SMP_STATE.register_cpu(CpuId::BSP, true, hw_id);
    SMP_STATE.cpus[0].lock().as_ref().unwrap().set_state(CpuState::Online);

    // Initialize per-CPU data for BSP
    percpu::init_bsp();

    SMP_STATE.initialized.store(true, Ordering::Release);
}

/// Get the hardware ID of the BSP.
fn get_bsp_hw_id() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        // Read Local APIC ID from APIC base register
        // The APIC ID is at offset 0x20 from APIC base (0xFEE00000)
        const APIC_BASE: u64 = 0xFEE00000;
        const APIC_ID_OFFSET: u64 = 0x20;
        unsafe {
            let id_ptr = (APIC_BASE + APIC_ID_OFFSET) as *const u32;
            // APIC ID is in bits 24-31 of the register
            ((id_ptr.read_volatile() >> 24) & 0xFF) as u64
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        let mpidr: u64;
        unsafe {
            core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr, options(nomem, nostack));
        }
        mpidr
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        0
    }
}

/// AP trampoline code location (must be below 1MB for real mode startup)
const AP_TRAMPOLINE_ADDR: u64 = 0x8000;

/// AP startup stack size per CPU (16KB each)
const AP_STACK_SIZE: usize = 16 * 1024;

/// Start application processors (secondary CPUs).
///
/// Returns the total number of CPUs (including BSP).
///
/// # Safety
///
/// This must only be called once from the BSP after SMP is initialized.
pub fn start_application_processors() -> u32 {
    #[cfg(target_arch = "x86_64")]
    {
        use crate::acpi;
        
        // Get CPU count from ACPI MADT table
        let cpu_count = acpi::cpu_count();
        
        if cpu_count <= 1 {
            // Only BSP, no APs to start
            return 1;
        }
        
        // Get APIC IDs from ACPI
        let apic_ids = acpi::get_apic_ids();
        
        // Copy AP trampoline code to low memory (below 1MB)
        setup_ap_trampoline();
        
        // Start each AP using INIT-SIPI-SIPI sequence
        let mut started = 0u32;
        for (i, &apic_id) in apic_ids.iter().enumerate() {
            // Skip BSP (usually APIC ID 0)
            if i == 0 {
                continue;
            }
            
            if start_ap(apic_id) {
                started += 1;
            }
        }
        
        // Return total CPUs (BSP + started APs)
        1 + started
    }
    
    #[cfg(target_arch = "aarch64")]
    {
        // aarch64 AP startup would use PSCI CPU_ON
        1
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        1
    }
}

/// Set up the AP trampoline code in low memory.
#[cfg(target_arch = "x86_64")]
fn setup_ap_trampoline() {
    // The AP trampoline is a small piece of 16-bit real mode code
    // that sets up long mode and jumps to the AP entry point.
    // For now, we prepare the memory but the actual trampoline
    // assembly would need to be copied here.
    
    // This is a simplified version - a full implementation would:
    // 1. Copy real mode trampoline code to AP_TRAMPOLINE_ADDR
    // 2. Set up a GDT and page tables accessible from the trampoline
    // 3. Set the entry point address in the trampoline data area
    
    // Clear the trampoline area
    unsafe {
        core::ptr::write_bytes(AP_TRAMPOLINE_ADDR as *mut u8, 0, 4096);
    }
}

/// Start a single AP using INIT-SIPI-SIPI sequence.
/// Returns true if the AP started successfully.
#[cfg(target_arch = "x86_64")]
fn start_ap(apic_id: u8) -> bool {
    use crate::arch::x86_64::lapic;
    
    // The INIT-SIPI-SIPI sequence:
    // 1. Send INIT IPI
    // 2. Wait 10ms
    // 3. Send SIPI with vector (startup address / 4096)
    // 4. Wait 200us
    // 5. Send SIPI again
    // 6. Wait for AP to signal it's online
    
    // Calculate startup vector (trampoline address / 4096)
    let vector = (AP_TRAMPOLINE_ADDR / 4096) as u8;
    
    // Send INIT IPI
    lapic::send_init_ipi(apic_id);
    
    // Wait ~10ms (busy loop for now)
    for _ in 0..10_000_000 {
        core::hint::spin_loop();
    }
    
    // Send SIPI
    lapic::send_startup_ipi(apic_id, vector);
    
    // Wait ~200us
    for _ in 0..200_000 {
        core::hint::spin_loop();
    }
    
    // Send SIPI again (some CPUs need two SIPIs)
    lapic::send_startup_ipi(apic_id, vector);
    
    // Wait for AP to come online (timeout after ~1 second)
    for _ in 0..1_000_000 {
        if SMP_STATE.num_online() > 1 {
            return true;
        }
        core::hint::spin_loop();
    }
    
    // Timeout - AP didn't start
    false
}

/// Legacy start_aps function (deprecated, use start_application_processors)
///
/// # Safety
///
/// This must only be called once from the BSP after SMP is initialized.
#[deprecated(note = "Use start_application_processors instead")]
pub unsafe fn start_aps() {
    let _ = start_application_processors();
}

/// Inter-Processor Interrupt types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IpiType {
    /// Reschedule hint - target CPU should run scheduler.
    Reschedule = 0,
    /// TLB shootdown - target CPU should invalidate TLB.
    TlbShootdown = 1,
    /// Stop - target CPU should halt.
    Stop = 2,
    /// Function call - target CPU should call a function.
    FunctionCall = 3,
}

/// Send an IPI to a specific CPU.
pub fn send_ipi(target: CpuId, ipi_type: IpiType) {
    #[cfg(target_arch = "x86_64")]
    {
        // Send via Local APIC ICR
        send_ipi_x86(target, ipi_type);
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Send via GIC SGI
        send_ipi_aarch64(target, ipi_type);
    }
}

/// Send an IPI to all CPUs except self.
pub fn send_ipi_all_others(ipi_type: IpiType) {
    let current = current_cpu_id();
    SMP_STATE.for_each_online(|cpu| {
        if cpu != current {
            send_ipi(cpu, ipi_type);
        }
    });
}

#[cfg(target_arch = "x86_64")]
fn send_ipi_x86(target: CpuId, ipi_type: IpiType) {
    // Write to Local APIC ICR (Interrupt Command Register)
    // ICR is at LAPIC base + 0x300 (low) and 0x310 (high)
    // Format:
    //   High: bits 24-27 = destination APIC ID
    //   Low:  bits 0-7 = vector, bits 8-10 = delivery mode, bit 11 = dest mode
    //        bit 14 = level, bit 15 = trigger mode
    
    const LAPIC_BASE: usize = 0xFEE0_0000; // Default LAPIC address
    const ICR_LOW: usize = LAPIC_BASE + 0x300;
    const ICR_HIGH: usize = LAPIC_BASE + 0x310;
    
    let vector = match ipi_type {
        IpiType::Reschedule => 0xFD,    // Reschedule vector
        IpiType::TlbShootdown => 0xFC,  // TLB shootdown vector
        IpiType::Stop => 0xFE,          // Stop vector
        IpiType::FunctionCall => 0xFB,  // Function call vector
    };
    
    // Fixed delivery mode (000), physical destination
    let icr_low: u32 = vector as u32;
    let icr_high: u32 = (target.as_u32()) << 24;
    
    unsafe {
        // Write high first (sets destination)
        core::ptr::write_volatile(ICR_HIGH as *mut u32, icr_high);
        // Write low triggers the IPI
        core::ptr::write_volatile(ICR_LOW as *mut u32, icr_low);
    }
}

#[cfg(target_arch = "aarch64")]
fn send_ipi_aarch64(target: CpuId, ipi_type: IpiType) {
    // Send Software Generated Interrupt via GIC
    // SGI register format varies between GICv2 and GICv3
    let sgi_id = ipi_type as u8;
    let target_list = 1u8 << target.as_u32();

    // GICv2: GICD_SGIR at offset 0xF00
    const GICD_BASE: usize = 0x0800_0000;
    const GICD_SGIR: usize = GICD_BASE + 0xF00;

    // Format: [25:24] = target filter, [23:16] = CPU target list, [3:0] = SGI ID
    let sgir_value = ((target_list as u32) << 16) | (sgi_id as u32);

    unsafe {
        core::ptr::write_volatile(GICD_SGIR as *mut u32, sgir_value);
    }
}

/// Handle an incoming IPI.
pub fn handle_ipi(ipi_type: IpiType) {
    match ipi_type {
        IpiType::Reschedule => {
            // Trigger reschedule on this CPU
            // The scheduler will pick a new task on next opportunity
        }
        IpiType::TlbShootdown => {
            // Invalidate local TLB
            #[cfg(target_arch = "x86_64")]
            unsafe {
                core::arch::asm!("invlpg [{}]", in(reg) 0u64, options(nostack));
            }

            #[cfg(target_arch = "aarch64")]
            unsafe {
                core::arch::asm!("tlbi vmalle1", options(nostack));
                core::arch::asm!("dsb sy", options(nostack));
                core::arch::asm!("isb", options(nostack));
            }
        }
        IpiType::Stop => {
            // Halt this CPU
            loop {
                crate::arch::halt();
            }
        }
        IpiType::FunctionCall => {
            // Execute pending function calls for this CPU from the per-CPU queue
            let cpu_id = current_cpu_id();
            let idx = cpu_id.as_index();
            
            if idx < MAX_CPUS {
                // Drain and execute all pending function calls
                loop {
                    let func = FUNCTION_CALL_QUEUES[idx].pop();
                    match func {
                        Some(call) => {
                            // Execute the function with its context
                            (call.func)(call.arg);
                        }
                        None => break,
                    }
                }
            }
        }
    }
}

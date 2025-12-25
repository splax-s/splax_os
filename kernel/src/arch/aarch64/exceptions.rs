//! # AArch64 Exception Handling
//!
//! Exception vectors and handlers for ARM64.
//!
//! ## Exception Levels
//! - EL0: User mode
//! - EL1: Kernel mode (where we run)
//! - EL2: Hypervisor (not used)
//! - EL3: Secure monitor (not used)
//!
//! ## Vector Table
//! Each exception level has 4 types of exceptions:
//! - Synchronous: Instruction-caused (syscalls, faults)
//! - IRQ: Normal interrupts
//! - FIQ: Fast interrupts
//! - SError: System errors

use core::arch::asm;

/// Exception context saved on the stack.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct ExceptionContext {
    /// General purpose registers x0-x30
    pub gpr: [u64; 31],
    /// Stack pointer at exception
    pub sp: u64,
    /// Exception link register (return address)
    pub elr: u64,
    /// Saved program status register
    pub spsr: u64,
    /// Exception syndrome register
    pub esr: u64,
    /// Fault address register
    pub far: u64,
}

/// Exception types from ESR_EL1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExceptionClass {
    Unknown = 0x00,
    WFxTrap = 0x01,
    SVC64 = 0x15,
    HVC64 = 0x16,
    SMC64 = 0x17,
    MsrMrsTrap = 0x18,
    InstructionAbortLower = 0x20,
    InstructionAbortSame = 0x21,
    PcAlignment = 0x22,
    DataAbortLower = 0x24,
    DataAbortSame = 0x25,
    SpAlignment = 0x26,
    FpException = 0x28,
    SError = 0x2F,
    Breakpoint = 0x30,
    SoftwareStep = 0x32,
    Watchpoint = 0x34,
    Brk = 0x3C,
}

impl ExceptionClass {
    /// Parse exception class from ESR_EL1.
    pub fn from_esr(esr: u64) -> Self {
        match (esr >> 26) as u8 {
            0x00 => Self::Unknown,
            0x01 => Self::WFxTrap,
            0x15 => Self::SVC64,
            0x16 => Self::HVC64,
            0x17 => Self::SMC64,
            0x18 => Self::MsrMrsTrap,
            0x20 => Self::InstructionAbortLower,
            0x21 => Self::InstructionAbortSame,
            0x22 => Self::PcAlignment,
            0x24 => Self::DataAbortLower,
            0x25 => Self::DataAbortSame,
            0x26 => Self::SpAlignment,
            0x28 => Self::FpException,
            0x2F => Self::SError,
            0x30 => Self::Breakpoint,
            0x32 => Self::SoftwareStep,
            0x34 => Self::Watchpoint,
            0x3C => Self::Brk,
            _ => Self::Unknown,
        }
    }
}

/// Data abort fault status codes.
#[derive(Debug, Clone, Copy)]
pub enum DataFaultStatus {
    AddressSizeFault(u8),
    TranslationFault(u8),
    AccessFlagFault(u8),
    PermissionFault(u8),
    SyncExternal,
    SyncParityError,
    AlignmentFault,
    TlbConflict,
    Other(u8),
}

impl DataFaultStatus {
    /// Parse from ISS field of ESR_EL1.
    pub fn from_iss(iss: u64) -> Self {
        let dfsc = (iss & 0x3F) as u8;
        match dfsc {
            0b000000..=0b000011 => Self::AddressSizeFault(dfsc & 0x3),
            0b000100..=0b000111 => Self::TranslationFault(dfsc & 0x3),
            0b001000..=0b001011 => Self::AccessFlagFault(dfsc & 0x3),
            0b001100..=0b001111 => Self::PermissionFault(dfsc & 0x3),
            0b010000 => Self::SyncExternal,
            0b011000 => Self::SyncParityError,
            0b100001 => Self::AlignmentFault,
            0b110000 => Self::TlbConflict,
            _ => Self::Other(dfsc),
        }
    }
}

/// Initialize exception vector table.
///
/// # Safety
///
/// Must only be called once during boot.
pub unsafe fn init() {
    extern "C" {
        static __exception_vectors: u8;
    }
    
    // Set VBAR_EL1 to point to our exception vector table
    let vectors = unsafe { &__exception_vectors as *const u8 as u64 };
    unsafe {
        asm!(
            "msr vbar_el1, {}",
            "isb",
            in(reg) vectors,
            options(nomem, nostack)
        );
    }
}

/// Exception handler dispatcher (called from assembly).
#[no_mangle]
pub extern "C" fn exception_handler(ctx: &mut ExceptionContext, exception_type: u64) {
    let exc_class = ExceptionClass::from_esr(ctx.esr);
    
    match exception_type {
        0 => handle_sync_exception(ctx, exc_class),
        1 => handle_irq(ctx),
        2 => handle_fiq(ctx),
        3 => handle_serror(ctx),
        _ => panic!("Unknown exception type: {}", exception_type),
    }
}

/// Handle synchronous exceptions.
fn handle_sync_exception(ctx: &mut ExceptionContext, exc_class: ExceptionClass) {
    match exc_class {
        ExceptionClass::SVC64 => {
            // System call - x8 contains syscall number
            let syscall_num = ctx.gpr[8];
            handle_syscall(ctx, syscall_num);
        }
        ExceptionClass::DataAbortSame | ExceptionClass::DataAbortLower => {
            let fault_status = DataFaultStatus::from_iss(ctx.esr);
            handle_data_abort(ctx, fault_status);
        }
        ExceptionClass::InstructionAbortSame | ExceptionClass::InstructionAbortLower => {
            handle_instruction_abort(ctx);
        }
        ExceptionClass::Brk => {
            // Breakpoint instruction
            handle_breakpoint(ctx);
        }
        _ => {
            panic!(
                "Unhandled synchronous exception: {:?}\n\
                 ELR: {:#018x}, ESR: {:#018x}, FAR: {:#018x}",
                exc_class, ctx.elr, ctx.esr, ctx.far
            );
        }
    }
}

/// Handle system calls.
fn handle_syscall(ctx: &mut ExceptionContext, syscall_num: u64) {
    // Syscall arguments in x0-x5, result in x0
    let args = [
        ctx.gpr[0], ctx.gpr[1], ctx.gpr[2],
        ctx.gpr[3], ctx.gpr[4], ctx.gpr[5],
    ];
    
    // TODO: Implement syscall dispatch
    // For now, return -ENOSYS
    ctx.gpr[0] = (-38i64) as u64; // -ENOSYS
    
    // Advance past SVC instruction
    ctx.elr += 4;
}

/// Handle data abort.
fn handle_data_abort(ctx: &mut ExceptionContext, fault_status: DataFaultStatus) {
    let is_write = (ctx.esr >> 6) & 1 != 0;
    let fault_addr = ctx.far;
    
    // TODO: Handle page faults, demand paging
    panic!(
        "Data abort: {:?}, addr: {:#018x}, write: {}, ELR: {:#018x}",
        fault_status, fault_addr, is_write, ctx.elr
    );
}

/// Handle instruction abort.
fn handle_instruction_abort(ctx: &mut ExceptionContext) {
    panic!(
        "Instruction abort at {:#018x}, FAR: {:#018x}",
        ctx.elr, ctx.far
    );
}

/// Handle breakpoint.
fn handle_breakpoint(ctx: &mut ExceptionContext) {
    // Skip breakpoint and continue
    ctx.elr += 4;
}

/// Handle IRQ.
fn handle_irq(ctx: &mut ExceptionContext) {
    // Dispatch to GIC handler
    super::gic::handle_irq();
}

/// Handle FIQ.
fn handle_fiq(_ctx: &mut ExceptionContext) {
    // FIQ typically used for secure world, ignore for now
}

/// Handle SError (system error).
fn handle_serror(ctx: &mut ExceptionContext) {
    panic!(
        "SError: ESR: {:#018x}, ELR: {:#018x}",
        ctx.esr, ctx.elr
    );
}

/// Read ESR_EL1 register.
#[inline(always)]
pub fn read_esr() -> u64 {
    let val: u64;
    unsafe {
        asm!("mrs {}, esr_el1", out(reg) val, options(nomem, nostack));
    }
    val
}

/// Read FAR_EL1 register.
#[inline(always)]
pub fn read_far() -> u64 {
    let val: u64;
    unsafe {
        asm!("mrs {}, far_el1", out(reg) val, options(nomem, nostack));
    }
    val
}

/// Read ELR_EL1 register.
#[inline(always)]
pub fn read_elr() -> u64 {
    let val: u64;
    unsafe {
        asm!("mrs {}, elr_el1", out(reg) val, options(nomem, nostack));
    }
    val
}

/// Enable interrupts.
#[inline(always)]
pub fn enable_interrupts() {
    unsafe {
        asm!("msr daifclr, #0xf", options(nomem, nostack));
    }
}

/// Disable interrupts.
#[inline(always)]
pub fn disable_interrupts() {
    unsafe {
        asm!("msr daifset, #0xf", options(nomem, nostack));
    }
}

/// Check if interrupts are enabled.
#[inline(always)]
pub fn are_interrupts_enabled() -> bool {
    let daif: u64;
    unsafe {
        asm!("mrs {}, daif", out(reg) daif, options(nomem, nostack));
    }
    // If I bit (bit 7) is clear, IRQs are enabled
    (daif & (1 << 7)) == 0
}

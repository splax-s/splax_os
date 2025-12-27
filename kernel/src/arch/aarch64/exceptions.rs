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
    
    // Dispatch syscall based on number
    // Using Linux-compatible syscall numbers for AArch64
    ctx.gpr[0] = match syscall_num {
        // exit (93)
        93 => {
            let exit_code = args[0] as i32;
            let pid = crate::sched::scheduler().current_process()
                .unwrap_or(crate::sched::ProcessId::KERNEL);
            let _ = crate::process::wait::exit(pid, exit_code);
            0
        }
        // write (64)
        64 => {
            let fd = args[0];
            let buf = args[1] as *const u8;
            let count = args[2] as usize;
            if fd == 1 || fd == 2 {
                // stdout/stderr - write to UART
                for i in 0..count {
                    let c = unsafe { *buf.add(i) };
                    super::uart::putc(c);
                }
                count as u64
            } else {
                (-9i64) as u64 // -EBADF
            }
        }
        // read (63)
        63 => {
            let fd = args[0];
            if fd == 0 {
                // stdin - read from UART
                match super::uart::getc() {
                    Some(c) => {
                        let buf = args[1] as *mut u8;
                        unsafe { *buf = c; }
                        1
                    }
                    None => 0,
                }
            } else {
                (-9i64) as u64 // -EBADF
            }
        }
        // brk (214)
        214 => {
            let new_brk = args[0];
            let pid = crate::sched::scheduler().current_process()
                .unwrap_or(crate::sched::ProcessId::KERNEL);
            match crate::process::PROCESS_MANAGER.set_brk(pid, new_brk) {
                Ok(old_brk) => old_brk,
                Err(_) => (-12i64) as u64, // -ENOMEM
            }
        }
        // getpid (172)
        172 => {
            crate::sched::scheduler().current_process()
                .map(|p| p.0)
                .unwrap_or(0)
        }
        // clone (220) - simplified spawn (Linux clone3 is 435)
        220 => {
            // args[0] = path pointer, args[1] = path length
            let path_ptr = args[0] as *const u8;
            let path_len = args[1] as usize;
            if path_ptr.is_null() || path_len > 256 {
                (-22i64) as u64 // -EINVAL
            } else {
                // Read path from userspace
                let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, path_len) };
                if let Ok(path) = core::str::from_utf8(path_slice) {
                    // Spawn the process
                    match crate::process::exec::spawn(path) {
                        Ok(pid) => pid.0,
                        Err(_) => (-2i64) as u64, // -ENOENT
                    }
                } else {
                    (-22i64) as u64 // -EINVAL
                }
            }
        }
        // waitpid (260)
        260 => {
            let pid = args[0] as i64;
            let status_ptr = args[1] as *mut i32;
            let options = args[2] as i32;
            let target = if pid < 0 {
                crate::process::wait::WaitTarget::AnyChild
            } else if pid == 0 {
                crate::process::wait::WaitTarget::AnyInGroup
            } else {
                crate::process::wait::WaitTarget::Specific(crate::sched::ProcessId::new(pid as u64))
            };
            let opts = crate::process::wait::WaitOptions::from_bits(options);
            match crate::process::wait::waitpid(target, opts) {
                Ok(result) => {
                    if !status_ptr.is_null() {
                        unsafe { *status_ptr = result.status.to_wait_status(); }
                    }
                    result.pid.0 as u64
                }
                Err(e) => match e {
                    crate::process::wait::WaitError::NoChildren => (-10i64) as u64, // -ECHILD
                    crate::process::wait::WaitError::WouldBlock => 0,
                    _ => (-1i64) as u64,
                }
            }
        }
        // kill (129)
        129 => {
            let pid = crate::sched::ProcessId::new(args[0]);
            let sig = args[1] as i32;
            let signal = crate::process::signal::Signal::from_i32(sig);
            match crate::process::signal::send_signal(pid, signal) {
                Ok(()) => 0,
                Err(_) => (-3i64) as u64, // -ESRCH
            }
        }
        // Unknown syscall
        _ => (-38i64) as u64, // -ENOSYS
    };
    
    // Advance past SVC instruction
    ctx.elr += 4;
}

/// Handle data abort.
fn handle_data_abort(ctx: &mut ExceptionContext, fault_status: DataFaultStatus) {
    let is_write = (ctx.esr >> 6) & 1 != 0;
    let fault_addr = ctx.far;
    
    // Check if this is a translation fault (page not mapped)
    match fault_status {
        DataFaultStatus::TranslationL0 |
        DataFaultStatus::TranslationL1 |
        DataFaultStatus::TranslationL2 |
        DataFaultStatus::TranslationL3 => {
            // Page fault - could implement demand paging here
            // For now, check if it's in a valid range
            let pid = crate::sched::scheduler().current_process();
            
            // Check if the fault address is in user space (below kernel start)
            const KERNEL_START: u64 = 0xFFFF_0000_0000_0000;
            if fault_addr < KERNEL_START {
                // User-space fault - send SIGSEGV
                if let Some(pid) = pid {
                    let info = crate::process::signal::SignalInfo {
                        signo: 11, // SIGSEGV
                        errno: 0,
                        code: crate::process::signal::SignalCode::SegmentFault,
                        sender_pid: None,
                        sender_uid: 0,
                        value: 0,
                        fault_addr: Some(fault_addr),
                    };
                    let _ = crate::process::signal::SIGNAL_MANAGER.send(pid, 11, info);
                    return;
                }
            }
            
            // Kernel fault or no process - panic
            panic!(
                "Page fault: {:?}, addr: {:#018x}, write: {}, ELR: {:#018x}",
                fault_status, fault_addr, is_write, ctx.elr
            );
        }
        _ => {
            // Other fault types - always panic
            panic!(
                "Data abort: {:?}, addr: {:#018x}, write: {}, ELR: {:#018x}",
                fault_status, fault_addr, is_write, ctx.elr
            );
        }
    }
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

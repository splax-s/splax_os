//! # RISC-V Trap Handler
//!
//! Handles all traps (exceptions and interrupts) in supervisor mode.
//!
//! ## Trap Types
//!
//! - Interrupts: Timer, software, external (PLIC)
//! - Exceptions: Page faults, illegal instruction, syscall (ecall)

use super::csr::{self, scause};
use super::{plic, timer};

/// Trap context saved by assembly handler
#[repr(C)]
pub struct TrapContext {
    pub ra: u64,
    pub sp: u64,
    pub gp: u64,
    pub tp: u64,
    pub t0: u64,
    pub t1: u64,
    pub t2: u64,
    pub s0: u64,
    pub s1: u64,
    pub a0: u64,
    pub a1: u64,
    pub a2: u64,
    pub a3: u64,
    pub a4: u64,
    pub a5: u64,
    pub a6: u64,
    pub a7: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
    pub t3: u64,
    pub t4: u64,
    pub t5: u64,
    pub t6: u64,
}

/// Trap handler called from assembly
#[no_mangle]
pub extern "C" fn trap_handler(
    scause: u64,
    stval: u64,
    sepc: u64,
    context: &mut TrapContext,
) {
    let is_interrupt = (scause >> 63) != 0;
    let cause = scause & 0x7FFF_FFFF_FFFF_FFFF;
    
    if is_interrupt {
        handle_interrupt(cause);
    } else {
        handle_exception(cause, stval, sepc, context);
    }
}

/// Handle interrupt
fn handle_interrupt(cause: u64) {
    match cause {
        scause::SUPERVISOR_SOFTWARE => {
            // Software interrupt (IPI)
            // Clear the pending bit
            unsafe {
                let sip = csr::read_sip();
                csr::write_sip(sip & !csr::sip::SSIP);
            }
            // TODO: Handle IPI (e.g., TLB shootdown, reschedule)
        }
        scause::SUPERVISOR_TIMER => {
            // Timer interrupt
            timer::handle_interrupt();
        }
        scause::SUPERVISOR_EXTERNAL => {
            // External interrupt (PLIC)
            let hartid = super::hartid();
            plic::handle_interrupt(hartid);
        }
        _ => {
            // Unknown interrupt
            panic!("Unknown interrupt: cause={}", cause);
        }
    }
}

/// Handle exception
fn handle_exception(
    cause: u64,
    stval: u64,
    sepc: u64,
    context: &mut TrapContext,
) {
    match cause {
        scause::INSTRUCTION_MISALIGNED => {
            panic!(
                "Instruction address misaligned: sepc={:#x}, addr={:#x}",
                sepc, stval
            );
        }
        scause::INSTRUCTION_ACCESS_FAULT => {
            panic!(
                "Instruction access fault: sepc={:#x}, addr={:#x}",
                sepc, stval
            );
        }
        scause::ILLEGAL_INSTRUCTION => {
            panic!(
                "Illegal instruction at {:#x}: instruction={:#x}",
                sepc, stval
            );
        }
        scause::BREAKPOINT => {
            // Breakpoint - advance PC and continue
            unsafe {
                csr::write_sepc(sepc + 2);  // Compressed instruction
            }
        }
        scause::LOAD_MISALIGNED => {
            panic!(
                "Load address misaligned: sepc={:#x}, addr={:#x}",
                sepc, stval
            );
        }
        scause::LOAD_ACCESS_FAULT => {
            panic!(
                "Load access fault: sepc={:#x}, addr={:#x}",
                sepc, stval
            );
        }
        scause::STORE_MISALIGNED => {
            panic!(
                "Store address misaligned: sepc={:#x}, addr={:#x}",
                sepc, stval
            );
        }
        scause::STORE_ACCESS_FAULT => {
            panic!(
                "Store access fault: sepc={:#x}, addr={:#x}",
                sepc, stval
            );
        }
        scause::ECALL_FROM_U => {
            // System call from user mode
            handle_syscall(context);
            // Advance PC past ecall
            unsafe {
                csr::write_sepc(sepc + 4);
            }
        }
        scause::ECALL_FROM_S => {
            // System call from supervisor mode (shouldn't happen normally)
            panic!("Unexpected ecall from S-mode at {:#x}", sepc);
        }
        scause::INSTRUCTION_PAGE_FAULT => {
            handle_page_fault(stval, sepc, true, false, context);
        }
        scause::LOAD_PAGE_FAULT => {
            handle_page_fault(stval, sepc, false, false, context);
        }
        scause::STORE_PAGE_FAULT => {
            handle_page_fault(stval, sepc, false, true, context);
        }
        _ => {
            panic!(
                "Unknown exception: cause={}, sepc={:#x}, stval={:#x}",
                cause, sepc, stval
            );
        }
    }
}

/// Handle system call
fn handle_syscall(context: &mut TrapContext) {
    let syscall_num = context.a7;
    let arg0 = context.a0;
    let arg1 = context.a1;
    let arg2 = context.a2;
    let arg3 = context.a3;
    let arg4 = context.a4;
    let arg5 = context.a5;
    
    let result = match syscall_num {
        // write(fd, buf, len)
        64 => {
            // TODO: Implement write syscall
            arg2 as i64  // Return len for now
        }
        // exit(code)
        93 => {
            // TODO: Implement exit
            0
        }
        // getpid
        172 => {
            // TODO: Return actual PID
            1
        }
        _ => {
            // Unknown syscall
            -1
        }
    };
    
    context.a0 = result as u64;
}

/// Handle page fault
fn handle_page_fault(
    addr: u64,
    sepc: u64,
    is_instruction: bool,
    is_write: bool,
    _context: &mut TrapContext,
) {
    let fault_type = if is_instruction {
        "instruction fetch"
    } else if is_write {
        "store"
    } else {
        "load"
    };
    
    // TODO: Implement demand paging, CoW, etc.
    
    panic!(
        "Page fault ({}) at {:#x}, address={:#x}",
        fault_type, sepc, addr
    );
}

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
            
            // Handle IPI - trigger scheduler check
            #[cfg(feature = "smp")]
            {
                crate::smp::handle_ipi();
            }
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
    let _arg3 = context.a3;
    let _arg4 = context.a4;
    let _arg5 = context.a5;
    
    let result = match syscall_num {
        // write(fd, buf, len)
        64 => {
            let fd = arg0 as i32;
            let buf_ptr = arg1 as *const u8;
            let len = arg2 as usize;
            
            // Validate pointer is in user space
            if buf_ptr.is_null() || len == 0 {
                -1i64
            } else if fd == 1 || fd == 2 {
                // stdout/stderr - write to console
                for i in 0..len {
                    let byte = unsafe { *buf_ptr.add(i) };
                    super::uart::putc(byte as char);
                }
                len as i64
            } else {
                // Other file descriptors - use VFS
                #[cfg(feature = "vfs")]
                {
                    let slice = unsafe { core::slice::from_raw_parts(buf_ptr, len) };
                    match crate::fs::vfs::write(fd as u32, slice) {
                        Ok(written) => written as i64,
                        Err(_) => -1i64,
                    }
                }
                #[cfg(not(feature = "vfs"))]
                {
                    -1i64
                }
            }
        }
        // exit(code)
        93 => {
            let exit_code = arg0 as i32;
            crate::sched::exit_current(exit_code);
            // Never returns
            0
        }
        // getpid
        172 => {
            crate::sched::current_pid() as i64
        }
        // read(fd, buf, len)
        63 => {
            let fd = arg0 as i32;
            let buf_ptr = arg1 as *mut u8;
            let len = arg2 as usize;
            
            if buf_ptr.is_null() || len == 0 {
                -1i64
            } else if fd == 0 {
                // stdin - read from UART
                let mut count = 0usize;
                while count < len {
                    if let Some(c) = super::uart::read_input() {
                        unsafe { *buf_ptr.add(count) = c; }
                        count += 1;
                        // Return on newline for line-buffered input
                        if c == b'\n' {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                count as i64
            } else {
                #[cfg(feature = "vfs")]
                {
                    let slice = unsafe { core::slice::from_raw_parts_mut(buf_ptr, len) };
                    match crate::fs::vfs::read(fd as u32, slice) {
                        Ok(read) => read as i64,
                        Err(_) => -1i64,
                    }
                }
                #[cfg(not(feature = "vfs"))]
                {
                    -1i64
                }
            }
        }
        // brk(addr)
        214 => {
            let addr = arg0 as usize;
            match crate::sched::current_brk(addr) {
                Ok(new_brk) => new_brk as i64,
                Err(_) => -1i64,
            }
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
    
    // Try demand paging first
    let page_addr = addr & !0xFFF; // Page-align the address
    
    // Check if this is a valid virtual address that needs a page mapped
    if let Some(current_proc) = crate::sched::current_process() {
        let proc = current_proc.lock();
        
        // Check if address is within process's valid regions (heap, stack, mmap)
        if proc.is_valid_addr(page_addr as usize) {
            drop(proc); // Release lock before allocating
            
            // Allocate a new physical page
            if let Some(phys_page) = crate::mm::alloc_page() {
                // Zero the page for security
                unsafe {
                    core::ptr::write_bytes(phys_page as *mut u8, 0, 4096);
                }
                
                // Determine page permissions
                let mut flags = crate::mm::paging::PageFlags::USER | crate::mm::paging::PageFlags::VALID;
                if !is_instruction {
                    flags |= crate::mm::paging::PageFlags::READ;
                }
                if is_write {
                    flags |= crate::mm::paging::PageFlags::WRITE;
                }
                if is_instruction {
                    flags |= crate::mm::paging::PageFlags::EXECUTE;
                }
                
                // Map the page
                if crate::mm::paging::map_page(page_addr as usize, phys_page, flags).is_ok() {
                    // Flush TLB for this address
                    unsafe {
                        core::arch::asm!("sfence.vma {}, zero", in(reg) page_addr);
                    }
                    // Successfully handled the fault
                    return;
                } else {
                    // Failed to map, free the page
                    crate::mm::free_page(phys_page);
                }
            }
        }
        
        // Check for copy-on-write
        if is_write {
            if let Ok(()) = crate::mm::paging::handle_cow(page_addr as usize) {
                // COW handled successfully
                unsafe {
                    core::arch::asm!("sfence.vma {}, zero", in(reg) page_addr);
                }
                return;
            }
        }
    }
    
    // Could not handle the fault - kill the process or panic
    panic!(
        "Page fault ({}) at {:#x}, address={:#x}",
        fault_type, sepc, addr
    );
}

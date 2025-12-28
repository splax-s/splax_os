//! # RISC-V 64-bit Architecture Support
//!
//! This module provides RISC-V specific implementations for Splax OS.
//!
//! ## RISC-V Privilege Levels
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │  U-mode (User)          - Applications      │
//! ├─────────────────────────────────────────────┤
//! │  S-mode (Supervisor)    - Kernel (we are)   │
//! ├─────────────────────────────────────────────┤
//! │  M-mode (Machine)       - SBI Firmware      │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ## Key Differences from x86_64/AArch64
//!
//! - Uses SBI (Supervisor Binary Interface) for platform services
//! - PLIC for external interrupts, CLINT for timer/software interrupts
//! - Sv39/Sv48 paging modes (39-bit or 48-bit virtual addresses)
//! - CSRs (Control and Status Registers) for system configuration

pub mod csr;
pub mod plic;
pub mod timer;
pub mod uart;
pub mod mmu;
pub mod trap;
pub mod sbi;

use core::arch::asm;

/// CPU context for context switching
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CpuContext {
    /// Return address
    pub ra: u64,
    /// Stack pointer
    pub sp: u64,
    /// Global pointer
    pub gp: u64,
    /// Thread pointer
    pub tp: u64,
    /// Saved registers s0-s11
    pub s: [u64; 12],
    /// Supervisor Status register
    pub sstatus: u64,
    /// Supervisor Exception PC
    pub sepc: u64,
}

impl Default for CpuContext {
    fn default() -> Self {
        Self {
            ra: 0,
            sp: 0,
            gp: 0,
            tp: 0,
            s: [0; 12],
            sstatus: 0,
            sepc: 0,
        }
    }
}

/// Page table entry for Sv39/Sv48
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    pub const VALID: u64 = 1 << 0;
    pub const READ: u64 = 1 << 1;
    pub const WRITE: u64 = 1 << 2;
    pub const EXEC: u64 = 1 << 3;
    pub const USER: u64 = 1 << 4;
    pub const GLOBAL: u64 = 1 << 5;
    pub const ACCESSED: u64 = 1 << 6;
    pub const DIRTY: u64 = 1 << 7;

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn new(ppn: u64, flags: u64) -> Self {
        Self((ppn << 10) | flags)
    }

    pub fn is_valid(&self) -> bool {
        self.0 & Self::VALID != 0
    }

    pub fn ppn(&self) -> u64 {
        (self.0 >> 10) & 0xFFF_FFFF_FFFF
    }
}

/// Initialize RISC-V architecture
pub fn init() {
    // Initialize UART for early console
    uart::init();
    
    // Initialize timer
    timer::init();
    
    // Initialize PLIC (Platform Level Interrupt Controller)
    plic::init();
    
    // Enable interrupts
    unsafe {
        // Set sstatus.SIE to enable supervisor interrupts
        csr::set_sstatus_sie();
    }
}

/// Get current hart ID
#[inline(always)]
pub fn hartid() -> usize {
    let id: usize;
    unsafe {
        asm!("mv {}, tp", out(reg) id, options(nomem, nostack));
    }
    id
}

/// Wait for interrupt
#[inline(always)]
pub fn wfi() {
    unsafe {
        asm!("wfi", options(nomem, nostack));
    }
}

/// Memory fence
#[inline(always)]
pub fn fence() {
    unsafe {
        asm!("fence iorw, iorw", options(nomem, nostack));
    }
}

/// Instruction fence
#[inline(always)]
pub fn fence_i() {
    unsafe {
        asm!("fence.i", options(nomem, nostack));
    }
}

/// Supervisor fence for virtual memory
#[inline(always)]
pub fn sfence_vma() {
    unsafe {
        asm!("sfence.vma", options(nomem, nostack));
    }
}

/// Supervisor fence for specific ASID and address
#[inline(always)]
pub fn sfence_vma_addr(addr: usize) {
    unsafe {
        asm!("sfence.vma {}, zero", in(reg) addr, options(nomem, nostack));
    }
}

// Interrupt enable/disable

/// Disable all interrupts
#[inline(always)]
pub fn disable_interrupts() -> bool {
    let sstatus: u64;
    unsafe {
        asm!(
            "csrrci {}, sstatus, 0x2",
            out(reg) sstatus,
            options(nomem, nostack)
        );
    }
    (sstatus & 0x2) != 0
}

/// Enable interrupts
#[inline(always)]
pub fn enable_interrupts() {
    unsafe {
        asm!("csrsi sstatus, 0x2", options(nomem, nostack));
    }
}

/// Restore interrupt state
#[inline(always)]
pub fn restore_interrupts(enabled: bool) {
    if enabled {
        enable_interrupts();
    }
}

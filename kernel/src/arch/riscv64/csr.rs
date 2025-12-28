//! # RISC-V Control and Status Registers (CSRs)
//!
//! Provides safe wrappers for accessing RISC-V CSRs.
//!
//! ## Supervisor CSRs
//!
//! - `sstatus` - Supervisor Status
//! - `sie` - Supervisor Interrupt Enable
//! - `stvec` - Supervisor Trap Vector
//! - `sscratch` - Supervisor Scratch
//! - `sepc` - Supervisor Exception PC
//! - `scause` - Supervisor Cause
//! - `stval` - Supervisor Trap Value
//! - `sip` - Supervisor Interrupt Pending
//! - `satp` - Supervisor Address Translation and Protection

use core::arch::asm;

// ============================================================================
// SSTATUS - Supervisor Status Register
// ============================================================================

/// Read sstatus
#[inline(always)]
pub fn read_sstatus() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, sstatus", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Write sstatus
#[inline(always)]
pub unsafe fn write_sstatus(value: u64) {
    asm!("csrw sstatus, {}", in(reg) value, options(nomem, nostack));
}

/// Set SIE bit in sstatus (enable supervisor interrupts)
#[inline(always)]
pub unsafe fn set_sstatus_sie() {
    asm!("csrsi sstatus, 0x2", options(nomem, nostack));
}

/// Clear SIE bit in sstatus (disable supervisor interrupts)
#[inline(always)]
pub unsafe fn clear_sstatus_sie() {
    asm!("csrci sstatus, 0x2", options(nomem, nostack));
}

/// SSTATUS bit fields
pub mod sstatus {
    pub const SIE: u64 = 1 << 1;     // Supervisor Interrupt Enable
    pub const SPIE: u64 = 1 << 5;    // Previous Interrupt Enable
    pub const SPP: u64 = 1 << 8;     // Previous Privilege (0=User, 1=Supervisor)
    pub const FS: u64 = 3 << 13;     // Float status
    pub const XS: u64 = 3 << 15;     // Extension status
    pub const SUM: u64 = 1 << 18;    // Supervisor User Memory access
    pub const MXR: u64 = 1 << 19;    // Make eXecutable Readable
}

// ============================================================================
// SIE - Supervisor Interrupt Enable
// ============================================================================

/// Read sie
#[inline(always)]
pub fn read_sie() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, sie", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Write sie
#[inline(always)]
pub unsafe fn write_sie(value: u64) {
    asm!("csrw sie, {}", in(reg) value, options(nomem, nostack));
}

/// SIE bit fields
pub mod sie {
    pub const SSIE: u64 = 1 << 1;    // Supervisor Software Interrupt
    pub const STIE: u64 = 1 << 5;    // Supervisor Timer Interrupt
    pub const SEIE: u64 = 1 << 9;    // Supervisor External Interrupt
}

// ============================================================================
// SIP - Supervisor Interrupt Pending
// ============================================================================

/// Read sip
#[inline(always)]
pub fn read_sip() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, sip", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Write sip
#[inline(always)]
pub unsafe fn write_sip(value: u64) {
    asm!("csrw sip, {}", in(reg) value, options(nomem, nostack));
}

/// SIP bit fields (same as SIE)
pub mod sip {
    pub const SSIP: u64 = 1 << 1;
    pub const STIP: u64 = 1 << 5;
    pub const SEIP: u64 = 1 << 9;
}

// ============================================================================
// STVEC - Supervisor Trap Vector
// ============================================================================

/// Read stvec
#[inline(always)]
pub fn read_stvec() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, stvec", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Write stvec
#[inline(always)]
pub unsafe fn write_stvec(value: u64) {
    asm!("csrw stvec, {}", in(reg) value, options(nomem, nostack));
}

/// STVEC modes
pub mod stvec {
    pub const DIRECT: u64 = 0;       // All traps go to BASE
    pub const VECTORED: u64 = 1;     // Async interrupts go to BASE + 4*cause
}

// ============================================================================
// SSCRATCH - Supervisor Scratch
// ============================================================================

/// Read sscratch
#[inline(always)]
pub fn read_sscratch() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, sscratch", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Write sscratch
#[inline(always)]
pub unsafe fn write_sscratch(value: u64) {
    asm!("csrw sscratch, {}", in(reg) value, options(nomem, nostack));
}

// ============================================================================
// SEPC - Supervisor Exception Program Counter
// ============================================================================

/// Read sepc
#[inline(always)]
pub fn read_sepc() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, sepc", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Write sepc
#[inline(always)]
pub unsafe fn write_sepc(value: u64) {
    asm!("csrw sepc, {}", in(reg) value, options(nomem, nostack));
}

// ============================================================================
// SCAUSE - Supervisor Cause
// ============================================================================

/// Read scause
#[inline(always)]
pub fn read_scause() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, scause", out(reg) value, options(nomem, nostack));
    }
    value
}

/// SCAUSE codes
pub mod scause {
    // Interrupts (bit 63 set)
    pub const INTERRUPT: u64 = 1 << 63;
    pub const SUPERVISOR_SOFTWARE: u64 = 1;
    pub const SUPERVISOR_TIMER: u64 = 5;
    pub const SUPERVISOR_EXTERNAL: u64 = 9;
    
    // Exceptions
    pub const INSTRUCTION_MISALIGNED: u64 = 0;
    pub const INSTRUCTION_ACCESS_FAULT: u64 = 1;
    pub const ILLEGAL_INSTRUCTION: u64 = 2;
    pub const BREAKPOINT: u64 = 3;
    pub const LOAD_MISALIGNED: u64 = 4;
    pub const LOAD_ACCESS_FAULT: u64 = 5;
    pub const STORE_MISALIGNED: u64 = 6;
    pub const STORE_ACCESS_FAULT: u64 = 7;
    pub const ECALL_FROM_U: u64 = 8;
    pub const ECALL_FROM_S: u64 = 9;
    pub const INSTRUCTION_PAGE_FAULT: u64 = 12;
    pub const LOAD_PAGE_FAULT: u64 = 13;
    pub const STORE_PAGE_FAULT: u64 = 15;
}

// ============================================================================
// STVAL - Supervisor Trap Value
// ============================================================================

/// Read stval
#[inline(always)]
pub fn read_stval() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, stval", out(reg) value, options(nomem, nostack));
    }
    value
}

// ============================================================================
// SATP - Supervisor Address Translation and Protection
// ============================================================================

/// Read satp
#[inline(always)]
pub fn read_satp() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, satp", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Write satp
#[inline(always)]
pub unsafe fn write_satp(value: u64) {
    asm!("csrw satp, {}", in(reg) value, options(nomem, nostack));
}

/// Build SATP value
pub fn make_satp(mode: u64, asid: u64, ppn: u64) -> u64 {
    (mode << 60) | (asid << 44) | ppn
}

/// SATP modes
pub mod satp {
    pub const BARE: u64 = 0;         // No translation
    pub const SV39: u64 = 8;         // 39-bit virtual addressing
    pub const SV48: u64 = 9;         // 48-bit virtual addressing
    pub const SV57: u64 = 10;        // 57-bit virtual addressing (if supported)
}

// ============================================================================
// TIME - Cycle Counter
// ============================================================================

/// Read time CSR
#[inline(always)]
pub fn read_time() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, time", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Read cycle CSR
#[inline(always)]
pub fn read_cycle() -> u64 {
    let value: u64;
    unsafe {
        asm!("csrr {}, cycle", out(reg) value, options(nomem, nostack));
    }
    value
}

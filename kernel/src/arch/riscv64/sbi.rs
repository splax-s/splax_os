//! # SBI (Supervisor Binary Interface)
//!
//! The SBI provides a standard interface between S-mode (kernel) and
//! M-mode (firmware). It's like BIOS/UEFI for RISC-V.
//!
//! ## Common SBI Extensions
//!
//! - Legacy (v0.1): Timer, console, IPI, remote fence, shutdown
//! - Base (v0.2+): Get version, probe extensions
//! - Timer: Set timer
//! - IPI: Inter-processor interrupt
//! - RFENCE: Remote TLB flush
//! - HSM: Hart State Management
//! - SRST: System Reset

use core::arch::asm;

/// SBI return value
#[derive(Debug, Clone, Copy)]
pub struct SbiRet {
    pub error: i64,
    pub value: u64,
}

/// SBI error codes
pub mod error {
    pub const SUCCESS: i64 = 0;
    pub const FAILED: i64 = -1;
    pub const NOT_SUPPORTED: i64 = -2;
    pub const INVALID_PARAM: i64 = -3;
    pub const DENIED: i64 = -4;
    pub const INVALID_ADDRESS: i64 = -5;
    pub const ALREADY_AVAILABLE: i64 = -6;
    pub const ALREADY_STARTED: i64 = -7;
    pub const ALREADY_STOPPED: i64 = -8;
}

/// SBI extension IDs
pub mod extension {
    // Legacy extensions (v0.1)
    pub const SET_TIMER: u64 = 0x0;
    pub const CONSOLE_PUTCHAR: u64 = 0x1;
    pub const CONSOLE_GETCHAR: u64 = 0x2;
    pub const CLEAR_IPI: u64 = 0x3;
    pub const SEND_IPI: u64 = 0x4;
    pub const REMOTE_FENCE_I: u64 = 0x5;
    pub const REMOTE_SFENCE_VMA: u64 = 0x6;
    pub const REMOTE_SFENCE_VMA_ASID: u64 = 0x7;
    pub const SHUTDOWN: u64 = 0x8;
    
    // Modern extensions (v0.2+)
    pub const BASE: u64 = 0x10;
    pub const TIMER: u64 = 0x54494D45; // "TIME"
    pub const IPI: u64 = 0x735049;     // "sPI"
    pub const RFENCE: u64 = 0x52464E43; // "RFNC"
    pub const HSM: u64 = 0x48534D;     // "HSM"
    pub const SRST: u64 = 0x53525354;  // "SRST"
}

/// Base extension function IDs
pub mod base_func {
    pub const GET_SPEC_VERSION: u64 = 0;
    pub const GET_IMPL_ID: u64 = 1;
    pub const GET_IMPL_VERSION: u64 = 2;
    pub const PROBE_EXTENSION: u64 = 3;
    pub const GET_MVENDORID: u64 = 4;
    pub const GET_MARCHID: u64 = 5;
    pub const GET_MIMPID: u64 = 6;
}

/// HSM function IDs
pub mod hsm_func {
    pub const HART_START: u64 = 0;
    pub const HART_STOP: u64 = 1;
    pub const HART_GET_STATUS: u64 = 2;
    pub const HART_SUSPEND: u64 = 3;
}

/// SRST function IDs
pub mod srst_func {
    pub const SYSTEM_RESET: u64 = 0;
}

/// SRST reset types
pub mod srst_type {
    pub const SHUTDOWN: u32 = 0;
    pub const COLD_REBOOT: u32 = 1;
    pub const WARM_REBOOT: u32 = 2;
}

/// Make an SBI call
#[inline(always)]
fn sbi_call(extension: u64, function: u64, arg0: u64, arg1: u64, arg2: u64) -> SbiRet {
    let error: i64;
    let value: u64;
    
    unsafe {
        asm!(
            "ecall",
            in("a0") arg0,
            in("a1") arg1,
            in("a2") arg2,
            in("a6") function,
            in("a7") extension,
            lateout("a0") error,
            lateout("a1") value,
            options(nostack)
        );
    }
    
    SbiRet { error, value }
}

// ============================================================================
// Legacy Extensions (v0.1)
// ============================================================================

/// Set timer (legacy)
pub fn set_timer(stime_value: u64) {
    sbi_call(extension::SET_TIMER, 0, stime_value, 0, 0);
}

/// Console putchar (legacy)
pub fn console_putchar(ch: u8) {
    sbi_call(extension::CONSOLE_PUTCHAR, 0, ch as u64, 0, 0);
}

/// Console getchar (legacy)
pub fn console_getchar() -> Option<u8> {
    let ret = sbi_call(extension::CONSOLE_GETCHAR, 0, 0, 0, 0);
    if ret.error >= 0 {
        Some(ret.error as u8)
    } else {
        None
    }
}

/// Shutdown (legacy)
pub fn shutdown() -> ! {
    sbi_call(extension::SHUTDOWN, 0, 0, 0, 0);
    loop {
        unsafe { asm!("wfi", options(nomem, nostack)); }
    }
}

/// Send IPI (legacy)
pub fn send_ipi(hart_mask: u64) {
    sbi_call(extension::SEND_IPI, 0, &hart_mask as *const _ as u64, 0, 0);
}

// ============================================================================
// Base Extension (v0.2+)
// ============================================================================

/// Get SBI specification version
pub fn get_spec_version() -> (u32, u32) {
    let ret = sbi_call(extension::BASE, base_func::GET_SPEC_VERSION, 0, 0, 0);
    let major = ((ret.value >> 24) & 0x7F) as u32;
    let minor = (ret.value & 0xFFFFFF) as u32;
    (major, minor)
}

/// Get SBI implementation ID
pub fn get_impl_id() -> u64 {
    let ret = sbi_call(extension::BASE, base_func::GET_IMPL_ID, 0, 0, 0);
    ret.value
}

/// Probe if an extension is available
pub fn probe_extension(extension_id: u64) -> bool {
    let ret = sbi_call(extension::BASE, base_func::PROBE_EXTENSION, extension_id, 0, 0);
    ret.value != 0
}

// ============================================================================
// Timer Extension
// ============================================================================

/// Set timer (modern)
pub fn timer_set(stime_value: u64) -> SbiRet {
    sbi_call(extension::TIMER, 0, stime_value, 0, 0)
}

// ============================================================================
// IPI Extension
// ============================================================================

/// Send IPI to harts (modern)
pub fn ipi_send(hart_mask: u64, hart_mask_base: u64) -> SbiRet {
    sbi_call(extension::IPI, 0, hart_mask, hart_mask_base, 0)
}

// ============================================================================
// HSM (Hart State Management) Extension
// ============================================================================

/// Start a hart
pub fn hsm_hart_start(hartid: u64, start_addr: u64, opaque: u64) -> SbiRet {
    sbi_call(extension::HSM, hsm_func::HART_START, hartid, start_addr, opaque)
}

/// Stop the current hart
pub fn hsm_hart_stop() -> SbiRet {
    sbi_call(extension::HSM, hsm_func::HART_STOP, 0, 0, 0)
}

/// Get hart status
pub fn hsm_hart_get_status(hartid: u64) -> SbiRet {
    sbi_call(extension::HSM, hsm_func::HART_GET_STATUS, hartid, 0, 0)
}

// ============================================================================
// SRST (System Reset) Extension
// ============================================================================

/// System reset
pub fn system_reset(reset_type: u32, reset_reason: u32) -> ! {
    sbi_call(
        extension::SRST,
        srst_func::SYSTEM_RESET,
        reset_type as u64,
        reset_reason as u64,
        0,
    );
    // Should not return
    loop {
        unsafe { asm!("wfi", options(nomem, nostack)); }
    }
}

/// Reboot the system
pub fn reboot() -> ! {
    system_reset(srst_type::COLD_REBOOT, 0)
}

/// Power off the system
pub fn poweroff() -> ! {
    if probe_extension(extension::SRST) {
        system_reset(srst_type::SHUTDOWN, 0)
    } else {
        shutdown()
    }
}

//! # Architecture-Specific Code
//!
//! This module provides a unified interface to architecture-specific functionality.
//! The kernel uses this abstraction to remain portable across x86_64 and aarch64.

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;

// Re-export the current architecture's implementation
#[cfg(target_arch = "x86_64")]
pub use x86_64::{gdt, idt, CpuContext, PageTableEntry};

#[cfg(target_arch = "aarch64")]
pub use aarch64::*;

/// Initialize architecture-specific features.
///
/// This must be called early in kernel initialization.
/// It sets up:
/// - Interrupt handling
/// - CPU features
/// - Memory protection
pub fn init() {
    #[cfg(target_arch = "x86_64")]
    x86_64::init();

    #[cfg(target_arch = "aarch64")]
    unsafe { aarch64::init(); }
}

/// Halt the CPU until the next interrupt.
///
/// This is used when there are no runnable processes.
#[inline(always)]
pub fn halt() {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: HLT is a safe instruction that waits for interrupts
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack));
    }

    #[cfg(target_arch = "aarch64")]
    // SAFETY: WFE is a safe instruction that waits for events
    unsafe {
        core::arch::asm!("wfe", options(nomem, nostack));
    }
}

/// Disable interrupts and return the previous state.
///
/// # Returns
///
/// `true` if interrupts were previously enabled, `false` otherwise.
#[inline(always)]
pub fn disable_interrupts() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let flags: u64;
        // SAFETY: Reading RFLAGS and CLI are safe operations
        unsafe {
            core::arch::asm!(
                "pushfq",
                "pop {0}",
                "cli",
                out(reg) flags,
                options(nomem)
            );
        }
        (flags & (1 << 9)) != 0
    }

    #[cfg(target_arch = "aarch64")]
    {
        let daif: u64;
        // SAFETY: Reading and modifying DAIF is safe in kernel mode
        unsafe {
            core::arch::asm!("mrs {0}, daif", out(reg) daif);
            core::arch::asm!("msr daifset, #0xf");
        }
        (daif & 0x3C0) == 0
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    false
}

/// Restore interrupts to a previous state.
///
/// # Arguments
///
/// * `enabled` - The previous interrupt state from `disable_interrupts()`
#[inline(always)]
pub fn restore_interrupts(enabled: bool) {
    if enabled {
        #[cfg(target_arch = "x86_64")]
        // SAFETY: STI is safe when we previously had interrupts enabled
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }

        #[cfg(target_arch = "aarch64")]
        // SAFETY: Clearing DAIF is safe when we previously had interrupts enabled
        unsafe {
            core::arch::asm!("msr daifclr, #0xf");
        }
    }
}

/// Read the current CPU cycle counter.
///
/// Used for timing and deterministic scheduling.
#[inline(always)]
pub fn read_cycle_counter() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let low: u32;
        let high: u32;
        // SAFETY: RDTSC is a safe instruction
        unsafe {
            core::arch::asm!(
                "rdtsc",
                out("eax") low,
                out("edx") high,
                options(nomem, nostack)
            );
        }
        ((high as u64) << 32) | (low as u64)
    }

    #[cfg(target_arch = "aarch64")]
    {
        let counter: u64;
        // SAFETY: Reading CNTVCT_EL0 is safe
        unsafe {
            core::arch::asm!("mrs {0}, cntvct_el0", out(reg) counter);
        }
        counter
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    0
}

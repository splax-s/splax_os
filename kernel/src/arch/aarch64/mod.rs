//! # aarch64 Architecture Support
//!
//! This module contains all ARM64-specific code for the Splax kernel.
//!
//! ## Features
//! - Exception level handling (EL1 kernel mode)
//! - GIC (Generic Interrupt Controller)
//! - MMU with 4KB granule pages
//! - PL011 UART serial console
//! - ARM Generic Timer

use core::arch::asm;

pub mod exceptions;
pub mod gic;
pub mod mmu;
pub mod timer;
pub mod uart;

pub use mmu::PageTableEntry;

/// Initialize aarch64-specific features.
///
/// # Safety
///
/// Must only be called once during early boot.
pub unsafe fn init() {
    // Initialize UART for debug output
    uart::init();
    
    // Initialize exception vectors
    unsafe { exceptions::init(); }
    
    // Initialize GIC
    unsafe { gic::init(); }
    
    // Initialize timer
    unsafe { timer::init(); }
    
    // Enable interrupts
    exceptions::enable_interrupts();
}

/// CPU context saved during exceptions and context switches.
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct CpuContext {
    // General purpose registers x0-x30
    pub x: [u64; 31],
    // Stack pointer
    pub sp: u64,
    // Program counter (ELR_EL1)
    pub pc: u64,
    // Saved program status (SPSR_EL1)
    pub pstate: u64,
}

impl CpuContext {
    /// Creates a new context for a user process.
    ///
    /// # Arguments
    ///
    /// * `entry_point` - The instruction pointer to start execution
    /// * `stack_pointer` - The initial stack pointer
    pub fn new_user(entry_point: u64, stack_pointer: u64) -> Self {
        let mut ctx = Self::default();
        ctx.pc = entry_point;
        ctx.sp = stack_pointer;
        // PSTATE: EL0, interrupts enabled, no flags
        ctx.pstate = 0;
        ctx
    }

    /// Creates a new context for a kernel thread.
    ///
    /// # Arguments
    ///
    /// * `entry_point` - The instruction pointer to start execution
    /// * `stack_pointer` - The initial stack pointer
    pub fn new_kernel(entry_point: u64, stack_pointer: u64) -> Self {
        let mut ctx = Self::default();
        ctx.pc = entry_point;
        ctx.sp = stack_pointer;
        // PSTATE: EL1, interrupts enabled
        ctx.pstate = 0x04;  // EL1h
        ctx
    }
}

/// Page table descriptor for AArch64 4KB granule.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PageTableDescriptor(u64);

impl PageTableDescriptor {
    // Descriptor types
    pub const INVALID: u64 = 0b00;
    pub const BLOCK: u64 = 0b01;
    pub const TABLE: u64 = 0b11;
    pub const PAGE: u64 = 0b11;

    // Attribute fields
    pub const ATTR_INDEX_MASK: u64 = 0b111 << 2;
    pub const NS: u64 = 1 << 5;  // Non-secure
    pub const AP_RW_EL1: u64 = 0b00 << 6;
    pub const AP_RW_ALL: u64 = 0b01 << 6;
    pub const AP_RO_EL1: u64 = 0b10 << 6;
    pub const AP_RO_ALL: u64 = 0b11 << 6;
    pub const SH_NON: u64 = 0b00 << 8;
    pub const SH_OUTER: u64 = 0b10 << 8;
    pub const SH_INNER: u64 = 0b11 << 8;
    pub const AF: u64 = 1 << 10;  // Access flag
    pub const NG: u64 = 1 << 11;  // Not global
    pub const PXN: u64 = 1 << 53;  // Privileged execute never
    pub const UXN: u64 = 1 << 54;  // Unprivileged execute never

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn new_table(addr: u64) -> Self {
        Self((addr & 0x0000_FFFF_FFFF_F000) | Self::TABLE)
    }

    pub const fn new_page(addr: u64, flags: u64) -> Self {
        Self((addr & 0x0000_FFFF_FFFF_F000) | flags | Self::PAGE)
    }

    pub const fn is_valid(&self) -> bool {
        (self.0 & 0b11) != Self::INVALID
    }

    pub const fn is_table(&self) -> bool {
        (self.0 & 0b11) == Self::TABLE
    }

    pub const fn addr(&self) -> u64 {
        self.0 & 0x0000_FFFF_FFFF_F000
    }
}

/// Read the Translation Table Base Register 0 (TTBR0_EL1).
#[inline(always)]
pub fn read_ttbr0() -> u64 {
    let value: u64;
    // SAFETY: Reading TTBR0_EL1 is safe in EL1
    unsafe {
        asm!("mrs {}, ttbr0_el1", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Write to the Translation Table Base Register 0 (TTBR0_EL1).
///
/// # Safety
///
/// The caller must ensure that `value` points to a valid page table.
#[inline(always)]
pub unsafe fn write_ttbr0(value: u64) {
    // SAFETY: Caller guarantees value is a valid page table address
    unsafe {
        asm!("msr ttbr0_el1, {}", in(reg) value, options(nomem, nostack));
        asm!("isb");
    }
}

/// Invalidate TLB entries.
#[inline(always)]
pub fn invalidate_tlb() {
    // SAFETY: TLB invalidation is safe
    unsafe {
        asm!(
            "dsb ishst",
            "tlbi vmalle1is",
            "dsb ish",
            "isb",
            options(nomem, nostack)
        );
    }
}

/// Invalidate TLB entry for a specific address.
#[inline(always)]
pub fn invalidate_page(addr: u64) {
    // SAFETY: TLB invalidation is safe
    unsafe {
        asm!(
            "dsb ishst",
            "tlbi vale1is, {}",
            "dsb ish",
            "isb",
            in(reg) addr >> 12,
            options(nomem, nostack)
        );
    }
}

/// Read the current exception level.
#[inline(always)]
pub fn current_el() -> u8 {
    let el: u64;
    // SAFETY: Reading CurrentEL is always safe
    unsafe {
        asm!("mrs {}, CurrentEL", out(reg) el, options(nomem, nostack));
    }
    ((el >> 2) & 0b11) as u8
}

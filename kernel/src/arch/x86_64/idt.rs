//! # IDT (Interrupt Descriptor Table) for x86_64
//!
//! Handles interrupts, exceptions, and system calls.

use core::arch::asm;
use core::mem::size_of;

/// Number of IDT entries (256 for x86_64).
pub const IDT_ENTRIES: usize = 256;

/// Interrupt gate types.
pub mod gate_type {
    pub const INTERRUPT: u8 = 0x8E; // Present, DPL=0, Interrupt Gate
    pub const TRAP: u8 = 0x8F;      // Present, DPL=0, Trap Gate
    pub const USER_INTERRUPT: u8 = 0xEE; // Present, DPL=3, Interrupt Gate
}

/// IDT entry (16 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    /// Creates a null IDT entry.
    pub const fn null() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    /// Creates an IDT entry for the given handler.
    ///
    /// # Arguments
    ///
    /// * `handler` - Handler function address
    /// * `selector` - Code segment selector
    /// * `ist` - Interrupt Stack Table index (0 = don't switch stacks)
    /// * `type_attr` - Gate type and attributes
    pub const fn new(handler: u64, selector: u16, ist: u8, type_attr: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector,
            ist,
            type_attr,
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

/// IDT descriptor pointer for LIDT instruction.
#[repr(C, packed)]
pub struct IdtDescriptor {
    pub limit: u16,
    pub base: u64,
}

/// The Interrupt Descriptor Table.
#[repr(C, align(16))]
pub struct Idt {
    entries: [IdtEntry; IDT_ENTRIES],
}

impl Idt {
    /// Creates a new empty IDT.
    pub const fn new() -> Self {
        Self {
            entries: [IdtEntry::null(); IDT_ENTRIES],
        }
    }

    /// Sets an IDT entry.
    pub fn set_handler(&mut self, vector: u8, handler: u64, ist: u8, type_attr: u8) {
        self.entries[vector as usize] = IdtEntry::new(
            handler,
            super::gdt::selectors::KERNEL_CODE,
            ist,
            type_attr,
        );
    }

    /// Loads this IDT.
    ///
    /// # Safety
    ///
    /// This function is unsafe because loading an invalid IDT will crash.
    pub unsafe fn load(&'static self) {
        let descriptor = IdtDescriptor {
            limit: (size_of::<Idt>() - 1) as u16,
            base: self as *const _ as u64,
        };

        unsafe {
            asm!(
                "lidt [{}]",
                in(reg) &descriptor,
                options(readonly, nostack, preserves_flags)
            );
        }
    }
}

/// Interrupt stack frame pushed by CPU.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct InterruptStackFrame {
    pub instruction_pointer: u64,
    pub code_segment: u64,
    pub cpu_flags: u64,
    pub stack_pointer: u64,
    pub stack_segment: u64,
}

/// Exception vectors.
pub mod vector {
    pub const DIVIDE_ERROR: u8 = 0;
    pub const DEBUG: u8 = 1;
    pub const NMI: u8 = 2;
    pub const BREAKPOINT: u8 = 3;
    pub const OVERFLOW: u8 = 4;
    pub const BOUND_RANGE: u8 = 5;
    pub const INVALID_OPCODE: u8 = 6;
    pub const DEVICE_NOT_AVAILABLE: u8 = 7;
    pub const DOUBLE_FAULT: u8 = 8;
    pub const INVALID_TSS: u8 = 10;
    pub const SEGMENT_NOT_PRESENT: u8 = 11;
    pub const STACK_SEGMENT: u8 = 12;
    pub const GENERAL_PROTECTION: u8 = 13;
    pub const PAGE_FAULT: u8 = 14;
    pub const X87_FPU: u8 = 16;
    pub const ALIGNMENT_CHECK: u8 = 17;
    pub const MACHINE_CHECK: u8 = 18;
    pub const SIMD_FP: u8 = 19;
    pub const VIRTUALIZATION: u8 = 20;
    pub const SECURITY: u8 = 30;
    
    // IRQs (remapped via APIC)
    pub const TIMER: u8 = 32;
    pub const KEYBOARD: u8 = 33;
    pub const SPURIOUS: u8 = 255;
    
    // System call
    pub const SYSCALL: u8 = 0x80;
}

/// Exception names for debugging.
pub const EXCEPTION_NAMES: [&str; 32] = [
    "Division Error",
    "Debug",
    "Non-Maskable Interrupt",
    "Breakpoint",
    "Overflow",
    "Bound Range Exceeded",
    "Invalid Opcode",
    "Device Not Available",
    "Double Fault",
    "Coprocessor Segment Overrun",
    "Invalid TSS",
    "Segment Not Present",
    "Stack-Segment Fault",
    "General Protection Fault",
    "Page Fault",
    "Reserved",
    "x87 FPU Error",
    "Alignment Check",
    "Machine Check",
    "SIMD Floating-Point",
    "Virtualization Exception",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Reserved",
    "Security Exception",
    "Reserved",
];

/// Enables interrupts.
#[inline(always)]
pub fn enable() {
    unsafe {
        asm!("sti", options(nomem, nostack));
    }
}

/// Disables interrupts.
#[inline(always)]
pub fn disable() {
    unsafe {
        asm!("cli", options(nomem, nostack));
    }
}

/// Checks if interrupts are enabled.
#[inline(always)]
pub fn are_enabled() -> bool {
    let flags: u64;
    unsafe {
        asm!("pushfq; pop {}", out(reg) flags, options(nomem));
    }
    (flags & 0x200) != 0
}

/// Runs a closure with interrupts disabled, restoring previous state.
pub fn without_interrupts<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let was_enabled = are_enabled();
    disable();
    let result = f();
    if was_enabled {
        enable();
    }
    result
}

//! # x86_64 Architecture Support
//!
//! This module contains all x86_64-specific code for the Splax kernel.
//!
//! ## Features
//! - GDT/IDT setup
//! - Interrupt handling (PIC/APIC)
//! - Paging (4-level page tables)
//! - Context switching
//! - System calls (SYSCALL/SYSRET)
//! - SMP support (Local APIC, IPI)

use core::arch::asm;
use core::fmt::Write;

pub mod context;
pub mod gdt;
pub mod idt;
pub mod interrupts;
pub mod keyboard;
pub mod lapic;
pub mod paging;
pub mod serial;
pub mod vga;

/// Static GDT instance.
static mut GDT: gdt::Gdt = gdt::Gdt::new();

/// Static TSS instance.
static mut TSS: gdt::Tss = gdt::Tss::new();

/// Static IDT instance.
static mut IDT: idt::Idt = idt::Idt::new();

/// Initialize x86_64-specific features.
pub fn init() {
    // Initialize serial port for early debugging
    serial::init();
    
    {
        let mut s = serial::SERIAL.lock();
        let _ = writeln!(s, "[x86_64] Initializing architecture...");
    }
    
    // Initialize VGA text mode for screen output
    vga::init();
    
    // Set up GDT with TSS
    unsafe {
        let tss_addr = &raw const TSS as u64;
        GDT.set_tss(tss_addr);
        GDT.load();
        // Note: skip load_segments() - boot.S already set segments correctly
        gdt::load_tss();
    }
    {
        let mut s = serial::SERIAL.lock();
        let _ = writeln!(s, "[x86_64] GDT loaded");
    }
    
    // Set up IDT with exception handlers
    unsafe {
        setup_idt();
        IDT.load();
    }
    {
        let mut s = serial::SERIAL.lock();
        let _ = writeln!(s, "[x86_64] IDT loaded");
    }
    
    // Initialize PIC
    interrupts::init_pic();
    {
        let mut s = serial::SERIAL.lock();
        let _ = writeln!(s, "[x86_64] PIC initialized");
    }
    
    // Enable interrupts
    interrupts::enable_interrupts();
    
    {
        let mut s = serial::SERIAL.lock();
        let _ = writeln!(s, "[x86_64] Interrupts enabled");
        let _ = writeln!(s, "[x86_64] Architecture initialization complete");
    }
}

/// Set up the IDT with exception handlers.
unsafe fn setup_idt() {
    use interrupts::*;
    
    // SAFETY: We're in single-threaded initialization, IDT won't be accessed elsewhere
    unsafe {
        // Exception handlers (IST=0, type_attr=0x8E for 64-bit interrupt gate)
        IDT.set_handler(vector::DIVIDE_ERROR, divide_error_handler as *const () as u64, 0, 0x8E);
        IDT.set_handler(vector::DEBUG, debug_handler as *const () as u64, 0, 0x8E);
        IDT.set_handler(vector::BREAKPOINT, breakpoint_handler as *const () as u64, 0, 0x8E);
        IDT.set_handler(vector::INVALID_OPCODE, invalid_opcode_handler as *const () as u64, 0, 0x8E);
        IDT.set_handler(vector::DOUBLE_FAULT, double_fault_handler as *const () as u64, 1, 0x8E); // Use IST 1 for double fault
        IDT.set_handler(vector::GENERAL_PROTECTION, general_protection_handler as *const () as u64, 0, 0x8E);
        IDT.set_handler(vector::PAGE_FAULT, page_fault_handler as *const () as u64, 0, 0x8E);
        
        // Hardware interrupt handlers
        IDT.set_handler(vector::PIC_TIMER, timer_handler as *const () as u64, 0, 0x8E);
        IDT.set_handler(vector::PIC_KEYBOARD, keyboard_handler as *const () as u64, 0, 0x8E);
    }
}

/// CPU context saved during interrupts and context switches.
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct CpuContext {
    // General purpose registers (callee-saved first)
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,
    // Caller-saved registers
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    // Interrupt frame
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl CpuContext {
    /// Creates a new context for a user process.
    ///
    /// # Arguments
    ///
    /// * `entry_point` - The instruction pointer to start execution
    /// * `stack_pointer` - The initial stack pointer
    pub fn new_user(entry_point: u64, stack_pointer: u64) -> Self {
        Self {
            rip: entry_point,
            cs: 0x23,  // User code segment (ring 3)
            rflags: 0x202,  // Interrupts enabled
            rsp: stack_pointer,
            ss: 0x1b,  // User data segment (ring 3)
            ..Default::default()
        }
    }

    /// Creates a new context for a kernel thread.
    ///
    /// # Arguments
    ///
    /// * `entry_point` - The instruction pointer to start execution
    /// * `stack_pointer` - The initial stack pointer
    pub fn new_kernel(entry_point: u64, stack_pointer: u64) -> Self {
        Self {
            rip: entry_point,
            cs: 0x08,  // Kernel code segment (ring 0)
            rflags: 0x202,  // Interrupts enabled
            rsp: stack_pointer,
            ss: 0x10,  // Kernel data segment (ring 0)
            ..Default::default()
        }
    }
}

/// Page table entry for 4-level paging.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    pub const PRESENT: u64 = 1 << 0;
    pub const WRITABLE: u64 = 1 << 1;
    pub const USER_ACCESSIBLE: u64 = 1 << 2;
    pub const WRITE_THROUGH: u64 = 1 << 3;
    pub const NO_CACHE: u64 = 1 << 4;
    pub const ACCESSED: u64 = 1 << 5;
    pub const DIRTY: u64 = 1 << 6;
    pub const HUGE_PAGE: u64 = 1 << 7;
    pub const GLOBAL: u64 = 1 << 8;
    pub const NO_EXECUTE: u64 = 1 << 63;

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn new(addr: u64, flags: u64) -> Self {
        Self((addr & 0x000F_FFFF_FFFF_F000) | flags)
    }

    pub const fn is_present(&self) -> bool {
        (self.0 & Self::PRESENT) != 0
    }

    pub const fn addr(&self) -> u64 {
        self.0 & 0x000F_FFFF_FFFF_F000
    }

    pub const fn flags(&self) -> u64 {
        self.0 & 0xFFF0_0000_0000_0FFF
    }
}

/// Invalidate TLB entry for a specific address.
#[inline(always)]
pub fn invalidate_page(addr: u64) {
    // SAFETY: INVLPG is safe to call with any address
    unsafe {
        asm!("invlpg [{}]", in(reg) addr, options(nostack));
    }
}

/// Flush the entire TLB by reloading CR3.
#[inline(always)]
pub fn flush_tlb() {
    // SAFETY: Reading and writing CR3 is safe in kernel mode
    unsafe {
        let cr3: u64;
        asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack));
        asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack));
    }
}

/// Read the current page table base address (CR3).
#[inline(always)]
pub fn read_cr3() -> u64 {
    let value: u64;
    // SAFETY: Reading CR3 is safe in kernel mode
    unsafe {
        asm!("mov {}, cr3", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Write a new page table base address to CR3.
///
/// # Safety
///
/// The caller must ensure that `value` points to a valid page table.
#[inline(always)]
pub unsafe fn write_cr3(value: u64) {
    // SAFETY: Caller guarantees value is a valid page table address
    unsafe {
        asm!("mov cr3, {}", in(reg) value, options(nomem, nostack));
    }
}

//! # Context Switching
//!
//! Low-level context switching for x86_64.

use core::arch::{asm, naked_asm};

/// CPU context for context switching.
///
/// This struct must match the layout expected by the assembly code.
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct Context {
    // Callee-saved registers (must be preserved across function calls)
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
    // Instruction pointer (return address)
    pub rip: u64,
    // Stack pointer
    pub rsp: u64,
    // Flags
    pub rflags: u64,
    // CR3 (page table base)
    pub cr3: u64,
}

impl Context {
    /// Creates a new empty context.
    pub const fn new() -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: 0,
            rip: 0,
            rsp: 0,
            rflags: 0x202, // Interrupts enabled
            cr3: 0,
        }
    }

    /// Creates a new kernel thread context.
    ///
    /// # Arguments
    ///
    /// * `entry` - Entry point function
    /// * `stack_top` - Top of the stack (grows downward)
    /// * `cr3` - Page table base address
    pub fn new_kernel(entry: extern "C" fn() -> !, stack_top: u64, cr3: u64) -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: 0,
            rip: entry as u64,
            rsp: stack_top,
            rflags: 0x202, // Interrupts enabled
            cr3,
        }
    }

    /// Creates a new user process context.
    ///
    /// # Arguments
    ///
    /// * `entry` - Entry point address in user space
    /// * `stack_top` - Top of user stack
    /// * `cr3` - Page table base address
    pub fn new_user(entry: u64, stack_top: u64, cr3: u64) -> Self {
        Self {
            r15: 0,
            r14: 0,
            r13: 0,
            r12: 0,
            rbx: 0,
            rbp: 0,
            rip: entry,
            rsp: stack_top,
            rflags: 0x202, // Interrupts enabled
            cr3,
        }
    }
}

/// Performs a context switch from `old` to `new`.
///
/// # Safety
///
/// - Both contexts must be valid
/// - The stack pointers must point to valid memory
/// - This function must be called with interrupts disabled
#[unsafe(naked)]
pub unsafe extern "C" fn switch_context(_old: *mut Context, _new: *const Context) {
    // Save current context to `old`, load `new` context
    naked_asm!(
        // Save callee-saved registers to old context
        "mov [rdi + 0x00], r15",
        "mov [rdi + 0x08], r14",
        "mov [rdi + 0x10], r13",
        "mov [rdi + 0x18], r12",
        "mov [rdi + 0x20], rbx",
        "mov [rdi + 0x28], rbp",
        
        // Save return address (RIP) - it's on the stack from the call
        "mov rax, [rsp]",
        "mov [rdi + 0x30], rax",
        
        // Save stack pointer (after return address)
        "lea rax, [rsp + 8]",
        "mov [rdi + 0x38], rax",
        
        // Save flags
        "pushfq",
        "pop rax",
        "mov [rdi + 0x40], rax",
        
        // Save CR3
        "mov rax, cr3",
        "mov [rdi + 0x48], rax",
        
        // Load new context
        // First, check if we need to switch page tables
        "mov rax, [rsi + 0x48]",  // new CR3
        "mov rcx, cr3",
        "cmp rax, rcx",
        "je 2f",                   // Skip if same
        "mov cr3, rax",            // Switch page tables
        "2:",
        
        // Load flags
        "mov rax, [rsi + 0x40]",
        "push rax",
        "popfq",
        
        // Load callee-saved registers
        "mov r15, [rsi + 0x00]",
        "mov r14, [rsi + 0x08]",
        "mov r13, [rsi + 0x10]",
        "mov r12, [rsi + 0x18]",
        "mov rbx, [rsi + 0x20]",
        "mov rbp, [rsi + 0x28]",
        
        // Load stack pointer
        "mov rsp, [rsi + 0x38]",
        
        // Jump to new instruction pointer
        "mov rax, [rsi + 0x30]",
        "jmp rax",
    );
}

/// Initializes and jumps to a new context (doesn't save old).
///
/// # Safety
///
/// - The context must be valid
/// - This function never returns
#[unsafe(naked)]
pub unsafe extern "C" fn init_context(_ctx: *const Context) -> ! {
    naked_asm!(
        // Load CR3 if set
        "mov rax, [rdi + 0x48]",
        "test rax, rax",
        "jz 2f",
        "mov cr3, rax",
        "2:",
        
        // Load flags
        "mov rax, [rdi + 0x40]",
        "push rax",
        "popfq",
        
        // Load callee-saved registers
        "mov r15, [rdi + 0x00]",
        "mov r14, [rdi + 0x08]",
        "mov r13, [rdi + 0x10]",
        "mov r12, [rdi + 0x18]",
        "mov rbx, [rdi + 0x20]",
        "mov rbp, [rdi + 0x28]",
        
        // Load stack pointer
        "mov rsp, [rdi + 0x38]",
        
        // Jump to entry point
        "mov rax, [rdi + 0x30]",
        "jmp rax",
    );
}

/// Read the current stack pointer.
#[inline]
pub fn read_rsp() -> u64 {
    let rsp: u64;
    unsafe {
        asm!("mov {}, rsp", out(reg) rsp, options(nostack));
    }
    rsp
}

/// Read the current instruction pointer (return address).
#[inline]
pub fn read_rip() -> u64 {
    let rip: u64;
    unsafe {
        asm!(
            "lea {}, [rip]",
            out(reg) rip,
            options(nostack)
        );
    }
    rip
}

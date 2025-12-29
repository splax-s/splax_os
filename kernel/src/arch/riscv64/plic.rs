//! # PLIC (Platform-Level Interrupt Controller)
//!
//! The PLIC handles external interrupts from peripherals.
//!
//! ## Memory Map (typical QEMU virt)
//!
//! ```text
//! 0x0C000000 - Priority registers (per source)
//! 0x0C001000 - Pending bits
//! 0x0C002000 - Enable bits (per context)
//! 0x0C200000 - Threshold/Claim (per context)
//! ```
//!
//! ## Context
//!
//! Each hart has multiple contexts (M-mode and S-mode).
//! For hart N in S-mode: context = N * 2 + 1

use core::ptr::{read_volatile, write_volatile};

/// PLIC base address (QEMU virt machine)
const PLIC_BASE: usize = 0x0C00_0000;

/// Maximum number of interrupt sources
const MAX_SOURCES: usize = 1024;

/// Maximum number of contexts
const MAX_CONTEXTS: usize = 15872;

/// PLIC register offsets
mod regs {
    /// Priority: 4 bytes per source, sources 1-1023
    pub const PRIORITY: usize = 0x0000;
    
    /// Pending: 1 bit per source, 32 sources per word
    pub const PENDING: usize = 0x1000;
    
    /// Enable: per context, 1 bit per source
    pub const ENABLE: usize = 0x2000;
    
    /// Context registers (threshold + claim)
    pub const CONTEXT: usize = 0x20_0000;
    
    /// Bytes per context for enable
    pub const ENABLE_PER_CONTEXT: usize = 0x80;
    
    /// Bytes per context for threshold/claim
    pub const CONTEXT_STRIDE: usize = 0x1000;
}

/// PLIC interrupt sources (QEMU virt)
pub mod sources {
    pub const VIRTIO0: u32 = 1;
    pub const VIRTIO1: u32 = 2;
    pub const VIRTIO2: u32 = 3;
    pub const VIRTIO3: u32 = 4;
    pub const VIRTIO4: u32 = 5;
    pub const VIRTIO5: u32 = 6;
    pub const VIRTIO6: u32 = 7;
    pub const VIRTIO7: u32 = 8;
    pub const UART0: u32 = 10;
    pub const PCIE: u32 = 32;  // 32-35
}

/// Priority levels
pub mod priority {
    pub const DISABLED: u32 = 0;
    pub const LOW: u32 = 1;
    pub const MEDIUM: u32 = 4;
    pub const HIGH: u32 = 7;
}

/// Get context ID for hart in S-mode
#[inline]
fn s_context(hartid: usize) -> usize {
    hartid * 2 + 1
}

/// Initialize PLIC
pub fn init() {
    // Will be configured per-hart in hart_init
}

/// Initialize PLIC for current hart
pub fn hart_init(hartid: usize) {
    let ctx = s_context(hartid);
    
    // Set priority threshold to 0 (allow all priorities > 0)
    set_threshold(ctx, 0);
    
    // Enable UART interrupt
    enable_source(ctx, sources::UART0);
    set_priority(sources::UART0, priority::MEDIUM);
    
    // Enable VirtIO interrupts
    for i in 0..8 {
        let source = sources::VIRTIO0 + i;
        enable_source(ctx, source);
        set_priority(source, priority::MEDIUM);
    }
}

/// Set interrupt priority
pub fn set_priority(source: u32, priority: u32) {
    assert!(source > 0 && (source as usize) < MAX_SOURCES);
    
    let addr = PLIC_BASE + regs::PRIORITY + (source as usize) * 4;
    unsafe {
        write_volatile(addr as *mut u32, priority);
    }
}

/// Enable interrupt source for context
pub fn enable_source(ctx: usize, source: u32) {
    assert!(ctx < MAX_CONTEXTS);
    assert!(source > 0 && (source as usize) < MAX_SOURCES);
    
    let word = source as usize / 32;
    let bit = source as usize % 32;
    
    let addr = PLIC_BASE + regs::ENABLE + ctx * regs::ENABLE_PER_CONTEXT + word * 4;
    unsafe {
        let current = read_volatile(addr as *const u32);
        write_volatile(addr as *mut u32, current | (1 << bit));
    }
}

/// Disable interrupt source for context
pub fn disable_source(ctx: usize, source: u32) {
    assert!(ctx < MAX_CONTEXTS);
    assert!(source > 0 && (source as usize) < MAX_SOURCES);
    
    let word = source as usize / 32;
    let bit = source as usize % 32;
    
    let addr = PLIC_BASE + regs::ENABLE + ctx * regs::ENABLE_PER_CONTEXT + word * 4;
    unsafe {
        let current = read_volatile(addr as *const u32);
        write_volatile(addr as *mut u32, current & !(1 << bit));
    }
}

/// Set priority threshold for context
pub fn set_threshold(ctx: usize, threshold: u32) {
    assert!(ctx < MAX_CONTEXTS);
    
    let addr = PLIC_BASE + regs::CONTEXT + ctx * regs::CONTEXT_STRIDE;
    unsafe {
        write_volatile(addr as *mut u32, threshold);
    }
}

/// Claim an interrupt (returns source, 0 if none)
pub fn claim(ctx: usize) -> u32 {
    assert!(ctx < MAX_CONTEXTS);
    
    let addr = PLIC_BASE + regs::CONTEXT + ctx * regs::CONTEXT_STRIDE + 4;
    unsafe { read_volatile(addr as *const u32) }
}

/// Complete an interrupt
pub fn complete(ctx: usize, source: u32) {
    assert!(ctx < MAX_CONTEXTS);
    
    let addr = PLIC_BASE + regs::CONTEXT + ctx * regs::CONTEXT_STRIDE + 4;
    unsafe {
        write_volatile(addr as *mut u32, source);
    }
}

/// Handle external interrupt for current hart
pub fn handle_interrupt(hartid: usize) {
    let ctx = s_context(hartid);
    
    loop {
        let source = claim(ctx);
        if source == 0 {
            break;
        }
        
        // Handle based on source
        match source {
            s if s == sources::UART0 => {
                // UART interrupt
                super::uart::handle_interrupt();
            }
            s if s >= sources::VIRTIO0 && s <= sources::VIRTIO7 => {
                // VirtIO interrupt - dispatch to appropriate device
                let virtio_idx = (s - sources::VIRTIO0) as usize;
                #[cfg(feature = "virtio")]
                {
                    crate::net::virtio::handle_interrupt(virtio_idx);
                }
                #[cfg(not(feature = "virtio"))]
                {
                    let _ = virtio_idx;
                }
            }
            _ => {
                // Unknown source - log and ignore
                #[cfg(feature = "debug")]
                super::uart::puts(&alloc::format!("[plic] Unknown interrupt source: {}\n", source));
            }
        }
        
        complete(ctx, source);
    }
}

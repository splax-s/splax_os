//! # Splax OS UEFI Bootloader
//!
//! This bootloader is responsible for:
//! 1. Initializing UEFI services
//! 2. Loading the Splax kernel from disk
//! 3. Setting up the initial memory map
//! 4. Transferring control to the kernel
//!
//! ## Architecture Support
//! - x86_64: Primary target
//! - aarch64: Secondary target (Raspberry Pi 4/5)
//!
//! ## Security
//! The bootloader verifies kernel integrity before execution.
//! No capability tokens exist at this stage - the kernel will initialize S-CAP.

#![no_std]
#![no_main]

/// Boot information passed to the kernel.
/// This struct contains all information the kernel needs to initialize.
#[repr(C)]
pub struct BootInfo {
    /// Physical address of the framebuffer (if available)
    pub framebuffer_addr: u64,
    /// Framebuffer width in pixels
    pub framebuffer_width: u32,
    /// Framebuffer height in pixels
    pub framebuffer_height: u32,
    /// Bytes per pixel
    pub framebuffer_bpp: u32,
    /// Memory map pointer
    pub memory_map_addr: u64,
    /// Number of memory map entries
    pub memory_map_entries: u64,
    /// Size of each memory map entry
    pub memory_map_entry_size: u64,
    /// Physical address where kernel is loaded
    pub kernel_addr: u64,
    /// Size of the kernel in bytes
    pub kernel_size: u64,
    /// ACPI RSDP address (x86_64) or Device Tree address (aarch64)
    pub acpi_rsdp_addr: u64,
}

impl BootInfo {
    /// Creates a new empty BootInfo.
    pub const fn new() -> Self {
        Self {
            framebuffer_addr: 0,
            framebuffer_width: 0,
            framebuffer_height: 0,
            framebuffer_bpp: 0,
            memory_map_addr: 0,
            memory_map_entries: 0,
            memory_map_entry_size: 0,
            kernel_addr: 0,
            kernel_size: 0,
            acpi_rsdp_addr: 0,
        }
    }
}

/// Memory region types for the kernel
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplaxMemoryType {
    /// Usable RAM
    Usable = 0,
    /// Reserved by firmware
    Reserved = 1,
    /// ACPI reclaimable
    AcpiReclaimable = 2,
    /// ACPI NVS
    AcpiNvs = 3,
    /// Bad memory
    BadMemory = 4,
    /// Kernel code and data
    Kernel = 5,
    /// Bootloader code and data
    Bootloader = 6,
    /// Framebuffer
    Framebuffer = 7,
}

/// Memory map entry passed to kernel.
#[repr(C)]
pub struct MemoryMapEntry {
    /// Physical start address
    pub start: u64,
    /// Size in bytes
    pub size: u64,
    /// Memory type
    pub memory_type: SplaxMemoryType,
}

/// Entry point placeholder for UEFI bootloader.
///
/// In a real implementation, this would be the UEFI entry point
/// that initializes services, loads the kernel, and transfers control.
///
/// For now, this is a stub that demonstrates the structure.
#[no_mangle]
pub extern "C" fn efi_main() -> ! {
    // TODO: Phase 1 Week 1-2 Implementation
    // 1. Initialize UEFI services
    // 2. Get memory map from firmware
    // 3. Load kernel from filesystem
    // 4. Verify kernel signature
    // 5. Set up page tables for kernel
    // 6. Exit boot services
    // 7. Jump to kernel entry point with BootInfo
    
    // For now, halt
    loop {
        halt();
    }
}

/// Halts the CPU.
#[inline(always)]
fn halt() {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack));
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("wfe", options(nomem, nostack));
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        // Fallback: spin
        core::hint::spin_loop();
    }
}

/// Panic handler for the bootloader.
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        halt();
    }
}

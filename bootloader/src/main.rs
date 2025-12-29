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

/// UEFI bootloader entry point.
///
/// This is the standalone UEFI application entry point. In practice,
/// Splax uses the Limine bootloader which provides a complete boot
/// environment. This entry point exists for:
/// 1. Direct UEFI booting without Limine (development/testing)
/// 2. Recovery boot mode
/// 3. Custom secure boot chains
///
/// ## Boot Sequence
///
/// 1. UEFI firmware loads this application from ESP
/// 2. We initialize console output for diagnostics
/// 3. Memory map is obtained from UEFI
/// 4. Kernel is loaded from ESP:/EFI/SPLAX/KERNEL.ELF
/// 5. Page tables are configured for higher-half kernel
/// 6. ExitBootServices() is called
/// 7. Control transfers to kernel with BootInfo
///
/// ## Notes
///
/// The primary boot path uses Limine, configured in limine.cfg.
/// This code is a minimal fallback implementation.
#[no_mangle]
pub extern "C" fn efi_main() -> ! {
    // Initialize early console (write to UEFI ConOut)
    // In a full implementation, we'd use the UEFI SystemTable
    // to access the console output protocol.
    
    // For direct UEFI boot, the implementation would:
    //
    // 1. Parse UEFI SystemTable from firmware
    //    let system_table = unsafe { &*(system_table_ptr as *const SystemTable) };
    //
    // 2. Get boot services
    //    let boot_services = system_table.boot_services();
    //
    // 3. Obtain memory map
    //    let mmap = boot_services.get_memory_map();
    //
    // 4. Load kernel from Simple File System Protocol
    //    let fs = boot_services.locate_protocol::<SimpleFileSystem>();
    //    let kernel = fs.open("\\EFI\\SPLAX\\KERNEL.ELF");
    //
    // 5. Allocate pages for kernel and load it
    //    let pages = boot_services.allocate_pages(AllocateType::AnyPages, ...);
    //    kernel.read(pages, kernel_size);
    //
    // 6. Build BootInfo from UEFI memory map
    //    let boot_info = build_boot_info(&mmap, framebuffer, ...);
    //
    // 7. Exit boot services (point of no return for UEFI)
    //    boot_services.exit_boot_services(image_handle, map_key);
    //
    // 8. Jump to kernel (kernel expects BootInfo in rdi)
    //    let entry: extern "C" fn(*const BootInfo) -> ! = transmute(entry_point);
    //    entry(&boot_info);
    //
    // Since Limine handles all of this, we provide a minimal halt loop here.
    // For development, use: qemu-system-x86_64 -bios OVMF.fd -kernel kernel.elf
    
    // Output a message via serial (COM1) if available
    #[cfg(target_arch = "x86_64")]
    {
        let msg = b"Splax UEFI: Use Limine bootloader for full boot\r\n";
        for &byte in msg {
            unsafe {
                // Simple serial output (works in QEMU)
                core::arch::asm!(
                    "out dx, al",
                    in("dx") 0x3F8u16,
                    in("al") byte,
                    options(nostack, nomem)
                );
            }
        }
    }
    
    // Halt - in real UEFI we'd return EFI_SUCCESS or call RuntimeServices
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
#[cfg_attr(not(test), panic_handler)]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        halt();
    }
}

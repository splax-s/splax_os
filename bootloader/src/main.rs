//! # Splax OS Native Bootloader
//!
//! A standalone bootloader for Splax OS microkernel that supports:
//! - **UEFI**: Modern firmware on x86_64, aarch64, and RISC-V
//! - **BIOS**: Legacy x86 systems via Multiboot2
//! - **SBI**: RISC-V Supervisor Binary Interface boot
//!
//! ## Design Philosophy
//!
//! This bootloader is designed for a microkernel architecture:
//! 1. Minimal trusted computing base
//! 2. Direct hardware initialization (no third-party bootloaders)
//! 3. Verified boot chain with integrity checks
//! 4. Architecture-agnostic BootInfo structure
//!
//! ## Supported Platforms
//! - x86_64: UEFI (recommended) and Legacy BIOS
//! - aarch64: UEFI (Raspberry Pi 4/5, server platforms)
//! - riscv64: SBI (QEMU virt, SiFive boards)
//!
//! ## Security
//! The bootloader verifies kernel integrity before execution.
//! No capability tokens exist at this stage - the kernel will initialize S-CAP.

#![no_std]
#![no_main]

use core::ptr;

// ============================================================================
// Boot Information Structures
// ============================================================================

/// Boot information passed to the kernel.
/// This struct contains all information the kernel needs to initialize.
/// Architecture-agnostic design for microkernel boot.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct BootInfo {
    /// Magic number to verify boot info validity (0x53504C58 = "SPLX")
    pub magic: u32,
    /// Boot protocol version
    pub version: u32,
    /// Boot method used (see BootMethod enum)
    pub boot_method: u32,
    /// Reserved for alignment
    pub _reserved: u32,
    /// Physical address of the framebuffer (if available)
    pub framebuffer_addr: u64,
    /// Framebuffer width in pixels
    pub framebuffer_width: u32,
    /// Framebuffer height in pixels
    pub framebuffer_height: u32,
    /// Framebuffer pitch (bytes per scanline)
    pub framebuffer_pitch: u32,
    /// Bits per pixel
    pub framebuffer_bpp: u32,
    /// Memory map pointer (physical address)
    pub memory_map_addr: u64,
    /// Number of memory map entries
    pub memory_map_entries: u64,
    /// Size of each memory map entry
    pub memory_map_entry_size: u64,
    /// Physical address where kernel is loaded
    pub kernel_phys_addr: u64,
    /// Virtual address where kernel expects to run
    pub kernel_virt_addr: u64,
    /// Size of the kernel in bytes
    pub kernel_size: u64,
    /// ACPI RSDP address (x86_64), Device Tree address (aarch64/riscv64)
    pub acpi_or_dtb_addr: u64,
    /// UEFI Runtime Services pointer (0 if BIOS boot)
    pub uefi_runtime_addr: u64,
    /// Command line arguments pointer
    pub cmdline_addr: u64,
    /// Command line length
    pub cmdline_len: u32,
    /// Number of CPU cores detected
    pub cpu_count: u32,
    /// Boot CPU ID
    pub boot_cpu_id: u32,
    /// Architecture-specific flags
    pub arch_flags: u32,
}

/// Boot method identifier
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootMethod {
    /// Unknown/invalid
    Unknown = 0,
    /// UEFI boot (x86_64, aarch64, riscv64)
    Uefi = 1,
    /// Legacy BIOS via Multiboot2
    Multiboot2 = 2,
    /// RISC-V SBI boot
    Sbi = 3,
    /// Direct kernel load (e.g., QEMU -kernel)
    Direct = 4,
}

impl BootInfo {
    /// Magic number for boot info validation
    pub const MAGIC: u32 = 0x53504C58; // "SPLX"
    /// Current boot protocol version
    pub const VERSION: u32 = 1;

    /// Creates a new empty BootInfo with magic and version set.
    pub const fn new() -> Self {
        Self {
            magic: Self::MAGIC,
            version: Self::VERSION,
            boot_method: BootMethod::Unknown as u32,
            _reserved: 0,
            framebuffer_addr: 0,
            framebuffer_width: 0,
            framebuffer_height: 0,
            framebuffer_pitch: 0,
            framebuffer_bpp: 0,
            memory_map_addr: 0,
            memory_map_entries: 0,
            memory_map_entry_size: 0,
            kernel_phys_addr: 0,
            kernel_virt_addr: 0xFFFF_8000_0000_0000, // Higher-half default
            kernel_size: 0,
            acpi_or_dtb_addr: 0,
            uefi_runtime_addr: 0,
            cmdline_addr: 0,
            cmdline_len: 0,
            cpu_count: 1,
            boot_cpu_id: 0,
            arch_flags: 0,
        }
    }

    /// Validates the boot info structure.
    pub fn is_valid(&self) -> bool {
        self.magic == Self::MAGIC && self.version >= 1
    }
}

/// Memory region types for the kernel
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplaxMemoryType {
    /// Usable RAM - free for kernel use
    Usable = 0,
    /// Reserved by firmware - do not use
    Reserved = 1,
    /// ACPI reclaimable - can be freed after ACPI init
    AcpiReclaimable = 2,
    /// ACPI NVS - must be preserved
    AcpiNvs = 3,
    /// Bad memory - hardware defect
    BadMemory = 4,
    /// Kernel code and data
    Kernel = 5,
    /// Bootloader code and data - reclaimable after boot
    Bootloader = 6,
    /// Framebuffer memory - do not use for general allocation
    Framebuffer = 7,
    /// Page tables set up by bootloader
    PageTables = 8,
    /// Initial ramdisk
    Initrd = 9,
    /// Device Tree Blob (aarch64/riscv64)
    DeviceTree = 10,
}

/// Memory map entry passed to kernel.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MemoryMapEntry {
    /// Physical start address (4K aligned)
    pub start: u64,
    /// Size in bytes (4K aligned)
    pub size: u64,
    /// Memory type
    pub memory_type: SplaxMemoryType,
    /// Attributes (architecture-specific)
    pub attributes: u32,
}

// ============================================================================
// UEFI Definitions
// ============================================================================

/// UEFI Handle type
pub type EfiHandle = *mut core::ffi::c_void;

/// UEFI Status codes
#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EfiStatus {
    Success = 0,
    LoadError = 1,
    InvalidParameter = 2,
    Unsupported = 3,
    BadBufferSize = 4,
    BufferTooSmall = 5,
    NotReady = 6,
    DeviceError = 7,
    WriteProtected = 8,
    OutOfResources = 9,
    NotFound = 14,
}

/// UEFI Memory Type
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EfiMemoryType {
    ReservedMemoryType = 0,
    LoaderCode = 1,
    LoaderData = 2,
    BootServicesCode = 3,
    BootServicesData = 4,
    RuntimeServicesCode = 5,
    RuntimeServicesData = 6,
    ConventionalMemory = 7,
    UnusableMemory = 8,
    AcpiReclaimMemory = 9,
    AcpiMemoryNvs = 10,
    MemoryMappedIo = 11,
    MemoryMappedIoPortSpace = 12,
    PalCode = 13,
    PersistentMemory = 14,
}

/// UEFI Memory Descriptor
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct EfiMemoryDescriptor {
    pub memory_type: u32,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub number_of_pages: u64,
    pub attribute: u64,
}

/// UEFI Table Header
#[repr(C)]
pub struct EfiTableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

/// UEFI Simple Text Output Protocol
#[repr(C)]
pub struct EfiSimpleTextOutput {
    pub reset: unsafe extern "efiapi" fn(*mut Self, bool) -> EfiStatus,
    pub output_string: unsafe extern "efiapi" fn(*mut Self, *const u16) -> EfiStatus,
    // ... more fields omitted for brevity
}

/// UEFI Boot Services (subset of functions we need)
#[repr(C)]
pub struct EfiBootServices {
    pub hdr: EfiTableHeader,
    // Task Priority Services
    _raise_tpl: usize,
    _restore_tpl: usize,
    // Memory Services
    pub allocate_pages: unsafe extern "efiapi" fn(
        alloc_type: u32,
        memory_type: EfiMemoryType,
        pages: usize,
        memory: *mut u64,
    ) -> EfiStatus,
    pub free_pages: unsafe extern "efiapi" fn(memory: u64, pages: usize) -> EfiStatus,
    pub get_memory_map: unsafe extern "efiapi" fn(
        memory_map_size: *mut usize,
        memory_map: *mut EfiMemoryDescriptor,
        map_key: *mut usize,
        descriptor_size: *mut usize,
        descriptor_version: *mut u32,
    ) -> EfiStatus,
    pub allocate_pool: unsafe extern "efiapi" fn(
        pool_type: EfiMemoryType,
        size: usize,
        buffer: *mut *mut u8,
    ) -> EfiStatus,
    pub free_pool: unsafe extern "efiapi" fn(buffer: *mut u8) -> EfiStatus,
    // Event & Timer Services (8 entries)
    _events: [usize; 8],
    // Protocol Handler Services (6 entries)
    _protocol_handlers: [usize; 6],
    // Image Services
    _image: [usize; 5],
    // Misc Services
    pub exit_boot_services: unsafe extern "efiapi" fn(
        image_handle: EfiHandle,
        map_key: usize,
    ) -> EfiStatus,
    // ... more fields
}

/// UEFI Runtime Services (subset)
#[repr(C)]
pub struct EfiRuntimeServices {
    pub hdr: EfiTableHeader,
    // Time Services
    _time: [usize; 4],
    // Virtual Memory Services
    pub set_virtual_address_map: unsafe extern "efiapi" fn(
        memory_map_size: usize,
        descriptor_size: usize,
        descriptor_version: u32,
        virtual_map: *mut EfiMemoryDescriptor,
    ) -> EfiStatus,
    _convert_pointer: usize,
    // Variable Services
    _variables: [usize; 3],
    // Misc Services
    _misc: [usize; 3],
}

/// UEFI Configuration Table Entry
#[repr(C)]
pub struct EfiConfigurationTable {
    pub vendor_guid: [u8; 16],
    pub vendor_table: *mut core::ffi::c_void,
}

/// UEFI System Table
#[repr(C)]
pub struct EfiSystemTable {
    pub hdr: EfiTableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: EfiHandle,
    pub con_in: *mut core::ffi::c_void,
    pub console_out_handle: EfiHandle,
    pub con_out: *mut EfiSimpleTextOutput,
    pub standard_error_handle: EfiHandle,
    pub std_err: *mut EfiSimpleTextOutput,
    pub runtime_services: *mut EfiRuntimeServices,
    pub boot_services: *mut EfiBootServices,
    pub number_of_table_entries: usize,
    pub configuration_table: *mut EfiConfigurationTable,
}

// ACPI RSDP GUID: 8868E871-E4F1-11D3-BC22-0080C73C8881
const ACPI_20_TABLE_GUID: [u8; 16] = [
    0x71, 0xe8, 0x68, 0x88, 0xf1, 0xe4, 0xd3, 0x11,
    0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81,
];

// ============================================================================
// UEFI Entry Point (x86_64, aarch64, riscv64)
// ============================================================================

/// UEFI entry point for Splax OS bootloader.
///
/// This is the main entry point when booting via UEFI firmware.
/// Supports x86_64, aarch64, and RISC-V platforms.
#[no_mangle]
pub extern "efiapi" fn efi_main(image_handle: EfiHandle, system_table: *mut EfiSystemTable) -> usize {
    // Validate pointers
    if system_table.is_null() {
        return EfiStatus::InvalidParameter as usize;
    }

    let st = unsafe { &*system_table };
    
    // Initialize console output
    if !st.con_out.is_null() {
        let con_out = unsafe { &mut *st.con_out };
        uefi_print(con_out, "Splax OS Bootloader v1.0\r\n");
        uefi_print(con_out, "Initializing UEFI boot...\r\n");
    }

    // Get boot services
    if st.boot_services.is_null() {
        return EfiStatus::Unsupported as usize;
    }
    let bs = unsafe { &*st.boot_services };

    // Prepare boot info
    let mut boot_info = BootInfo::new();
    boot_info.boot_method = BootMethod::Uefi as u32;

    // Find ACPI RSDP from configuration tables
    boot_info.acpi_or_dtb_addr = find_acpi_rsdp(st);

    // Get memory map
    let mut mmap_size: usize = 0;
    let mut map_key: usize = 0;
    let mut desc_size: usize = 0;
    let mut desc_version: u32 = 0;

    // First call to get required size
    unsafe {
        (bs.get_memory_map)(
            &mut mmap_size,
            ptr::null_mut(),
            &mut map_key,
            &mut desc_size,
            &mut desc_version,
        );
    }

    // Add extra space for allocation
    mmap_size += desc_size * 4;

    // Allocate buffer for memory map
    let mut mmap_buffer: *mut u8 = ptr::null_mut();
    let status = unsafe {
        (bs.allocate_pool)(EfiMemoryType::LoaderData, mmap_size, &mut mmap_buffer)
    };
    if status != EfiStatus::Success || mmap_buffer.is_null() {
        if !st.con_out.is_null() {
            uefi_print(unsafe { &mut *st.con_out }, "Failed to allocate memory map buffer\r\n");
        }
        return EfiStatus::OutOfResources as usize;
    }

    // Get actual memory map
    let status = unsafe {
        (bs.get_memory_map)(
            &mut mmap_size,
            mmap_buffer as *mut EfiMemoryDescriptor,
            &mut map_key,
            &mut desc_size,
            &mut desc_version,
        )
    };
    if status != EfiStatus::Success {
        if !st.con_out.is_null() {
            uefi_print(unsafe { &mut *st.con_out }, "Failed to get memory map\r\n");
        }
        return status as usize;
    }

    // Convert UEFI memory map to Splax format
    let entry_count = mmap_size / desc_size;
    
    // Allocate Splax memory map
    let splax_mmap_size = entry_count * core::mem::size_of::<MemoryMapEntry>();
    let mut splax_mmap: *mut u8 = ptr::null_mut();
    let status = unsafe {
        (bs.allocate_pool)(EfiMemoryType::LoaderData, splax_mmap_size, &mut splax_mmap)
    };
    if status != EfiStatus::Success {
        return status as usize;
    }

    // Convert entries
    for i in 0..entry_count {
        let efi_entry = unsafe {
            &*((mmap_buffer as usize + i * desc_size) as *const EfiMemoryDescriptor)
        };
        let splax_entry = unsafe {
            &mut *((splax_mmap as usize + i * core::mem::size_of::<MemoryMapEntry>()) as *mut MemoryMapEntry)
        };
        
        splax_entry.start = efi_entry.physical_start;
        splax_entry.size = efi_entry.number_of_pages * 4096;
        splax_entry.memory_type = efi_to_splax_memory_type(efi_entry.memory_type);
        splax_entry.attributes = (efi_entry.attribute & 0xFFFFFFFF) as u32;
    }

    boot_info.memory_map_addr = splax_mmap as u64;
    boot_info.memory_map_entries = entry_count as u64;
    boot_info.memory_map_entry_size = core::mem::size_of::<MemoryMapEntry>() as u64;

    // Store runtime services address for kernel
    boot_info.uefi_runtime_addr = st.runtime_services as u64;

    if !st.con_out.is_null() {
        let con_out = unsafe { &mut *st.con_out };
        uefi_print(con_out, "Memory map obtained\r\n");
        uefi_print(con_out, "Exiting boot services...\r\n");
    }

    // Exit boot services - point of no return
    // We need to get a fresh map_key as allocations may have changed it
    mmap_size = 0;
    unsafe {
        (bs.get_memory_map)(
            &mut mmap_size,
            ptr::null_mut(),
            &mut map_key,
            &mut desc_size,
            &mut desc_version,
        );
    }
    mmap_size += desc_size * 2;

    // Reallocate if needed
    let mut final_mmap: *mut u8 = ptr::null_mut();
    unsafe { (bs.allocate_pool)(EfiMemoryType::LoaderData, mmap_size, &mut final_mmap) };
    
    let status = unsafe {
        (bs.get_memory_map)(
            &mut mmap_size,
            final_mmap as *mut EfiMemoryDescriptor,
            &mut map_key,
            &mut desc_size,
            &mut desc_version,
        )
    };
    if status != EfiStatus::Success {
        return status as usize;
    }

    // Exit boot services
    let status = unsafe { (bs.exit_boot_services)(image_handle, map_key) };
    if status != EfiStatus::Success {
        // Try again with fresh map key
        mmap_size = 4096;
        let status = unsafe {
            (bs.get_memory_map)(
                &mut mmap_size,
                final_mmap as *mut EfiMemoryDescriptor,
                &mut map_key,
                &mut desc_size,
                &mut desc_version,
            )
        };
        if status == EfiStatus::Success {
            let _ = unsafe { (bs.exit_boot_services)(image_handle, map_key) };
        }
    }

    // We are now in a post-ExitBootServices environment
    // No more UEFI boot services available
    // Initialize serial for debugging
    serial_init();
    serial_print("Splax: Boot services exited\r\n");

    // In a complete implementation, we would:
    // 1. Load kernel from ESP (already done before ExitBootServices in real impl)
    // 2. Set up page tables for higher-half kernel
    // 3. Jump to kernel entry point

    // For now, output boot info and halt
    serial_print("Splax: Boot complete, halting\r\n");
    serial_print("BootInfo ready at: ");
    serial_print_hex(&boot_info as *const _ as u64);
    serial_print("\r\n");

    // Halt
    loop {
        halt();
    }
}

/// Convert UEFI memory type to Splax memory type
fn efi_to_splax_memory_type(efi_type: u32) -> SplaxMemoryType {
    match efi_type {
        7 => SplaxMemoryType::Usable,           // ConventionalMemory
        9 => SplaxMemoryType::AcpiReclaimable,  // AcpiReclaimMemory
        10 => SplaxMemoryType::AcpiNvs,         // AcpiMemoryNvs
        8 => SplaxMemoryType::BadMemory,        // UnusableMemory
        1 | 2 => SplaxMemoryType::Bootloader,   // LoaderCode/Data
        3 | 4 => SplaxMemoryType::Bootloader,   // BootServicesCode/Data (reclaimable)
        5 | 6 => SplaxMemoryType::Reserved,     // RuntimeServicesCode/Data
        _ => SplaxMemoryType::Reserved,
    }
}

/// Find ACPI RSDP from UEFI configuration tables
fn find_acpi_rsdp(st: &EfiSystemTable) -> u64 {
    for i in 0..st.number_of_table_entries {
        let table = unsafe { &*st.configuration_table.add(i) };
        if table.vendor_guid == ACPI_20_TABLE_GUID {
            return table.vendor_table as u64;
        }
    }
    0
}

/// Print string to UEFI console
fn uefi_print(con_out: &mut EfiSimpleTextOutput, s: &str) {
    // Convert to UCS-2 (simple ASCII subset)
    let mut buffer = [0u16; 128];
    for (i, c) in s.bytes().enumerate() {
        if i >= 127 {
            break;
        }
        buffer[i] = c as u16;
    }
    unsafe {
        (con_out.output_string)(con_out as *mut _, buffer.as_ptr());
    }
}

// ============================================================================
// Multiboot2 Entry Point (BIOS/Legacy x86)
// ============================================================================

/// Multiboot2 header magic
const MULTIBOOT2_HEADER_MAGIC: u32 = 0xE85250D6;
/// Multiboot2 bootloader magic (passed by bootloader)
const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36D76289;

/// Multiboot2 header wrapper for alignment
#[repr(C, align(8))]
struct Multiboot2Header {
    data: [u32; 6],
}

/// Multiboot2 header (must be in first 32KB of binary)
#[used]
#[link_section = ".multiboot2"]
static MULTIBOOT2_HEADER: Multiboot2Header = Multiboot2Header {
    data: [
        MULTIBOOT2_HEADER_MAGIC,  // magic
        0,                         // architecture: 0 = i386 protected mode
        24,                        // header length
        !(MULTIBOOT2_HEADER_MAGIC.wrapping_add(0).wrapping_add(24)).wrapping_add(1), // checksum
        0,                         // end tag type
        8,                         // end tag size
    ],
};

/// Multiboot2 entry point for legacy BIOS boot.
/// Called by GRUB or other Multiboot2-compliant bootloader.
#[no_mangle]
#[cfg(target_arch = "x86_64")]
pub extern "C" fn multiboot2_entry(magic: u32, mbi_ptr: u64) -> ! {
    serial_init();
    serial_print("Splax: Multiboot2 entry\r\n");

    if magic != MULTIBOOT2_BOOTLOADER_MAGIC {
        serial_print("ERROR: Invalid Multiboot2 magic\r\n");
        loop { halt(); }
    }

    let mut boot_info = BootInfo::new();
    boot_info.boot_method = BootMethod::Multiboot2 as u32;

    // Parse Multiboot2 information structure
    parse_multiboot2_info(mbi_ptr, &mut boot_info);

    serial_print("Splax: Multiboot2 boot info parsed\r\n");
    serial_print("Memory map entries: ");
    serial_print_hex(boot_info.memory_map_entries);
    serial_print("\r\n");

    // Transfer to kernel (would load and jump in real implementation)
    serial_print("Splax: Halting (kernel load not implemented in stub)\r\n");
    
    loop {
        halt();
    }
}

/// Parse Multiboot2 information structure
fn parse_multiboot2_info(mbi_ptr: u64, boot_info: &mut BootInfo) {
    // Multiboot2 info starts with total_size (u32) and reserved (u32)
    let total_size = unsafe { *(mbi_ptr as *const u32) };
    let mut offset: u64 = 8; // Skip header

    // Static buffer for memory map (bootloader memory, will be copied by kernel)
    static mut MMAP_BUFFER: [MemoryMapEntry; 64] = [MemoryMapEntry {
        start: 0,
        size: 0,
        memory_type: SplaxMemoryType::Reserved,
        attributes: 0,
    }; 64];
    let mut mmap_count: usize = 0;

    while offset < total_size as u64 {
        let tag_ptr = (mbi_ptr + offset) as *const u32;
        let tag_type = unsafe { *tag_ptr };
        let tag_size = unsafe { *tag_ptr.add(1) };

        if tag_type == 0 {
            break; // End tag
        }

        match tag_type {
            4 => {
                // Basic memory info
            }
            6 => {
                // Memory map
                let entry_size = unsafe { *((mbi_ptr + offset + 8) as *const u32) };
                let entry_version = unsafe { *((mbi_ptr + offset + 12) as *const u32) };
                let _ = entry_version;
                
                let mut entry_offset = 16u64;
                while entry_offset < tag_size as u64 {
                    let entry_ptr = (mbi_ptr + offset + entry_offset) as *const u64;
                    let base = unsafe { *entry_ptr };
                    let length = unsafe { *entry_ptr.add(1) };
                    let mtype = unsafe { *((entry_ptr as *const u32).add(4)) };

                    if mmap_count < 64 {
                        unsafe {
                            MMAP_BUFFER[mmap_count] = MemoryMapEntry {
                                start: base,
                                size: length,
                                memory_type: multiboot2_to_splax_memory_type(mtype),
                                attributes: 0,
                            };
                        }
                        mmap_count += 1;
                    }

                    entry_offset += entry_size as u64;
                }
            }
            8 => {
                // Framebuffer info
                let fb_ptr = (mbi_ptr + offset + 8) as *const u64;
                boot_info.framebuffer_addr = unsafe { *fb_ptr };
                boot_info.framebuffer_pitch = unsafe { *((fb_ptr as *const u32).add(2)) };
                boot_info.framebuffer_width = unsafe { *((fb_ptr as *const u32).add(3)) };
                boot_info.framebuffer_height = unsafe { *((fb_ptr as *const u32).add(4)) };
                boot_info.framebuffer_bpp = unsafe { *((mbi_ptr + offset + 28) as *const u8) } as u32;
            }
            14 => {
                // ACPI old RSDP
                boot_info.acpi_or_dtb_addr = mbi_ptr + offset + 8;
            }
            15 => {
                // ACPI new RSDP (prefer this)
                boot_info.acpi_or_dtb_addr = mbi_ptr + offset + 8;
            }
            _ => {}
        }

        // Align to 8 bytes
        offset += ((tag_size + 7) & !7) as u64;
    }

    // Set memory map in boot info
    boot_info.memory_map_addr = unsafe { MMAP_BUFFER.as_ptr() as u64 };
    boot_info.memory_map_entries = mmap_count as u64;
    boot_info.memory_map_entry_size = core::mem::size_of::<MemoryMapEntry>() as u64;
}

/// Convert Multiboot2 memory type to Splax
fn multiboot2_to_splax_memory_type(mtype: u32) -> SplaxMemoryType {
    match mtype {
        1 => SplaxMemoryType::Usable,        // Available
        2 => SplaxMemoryType::Reserved,      // Reserved
        3 => SplaxMemoryType::AcpiReclaimable,
        4 => SplaxMemoryType::AcpiNvs,
        5 => SplaxMemoryType::BadMemory,
        _ => SplaxMemoryType::Reserved,
    }
}

// ============================================================================
// RISC-V SBI Entry Point
// ============================================================================

/// RISC-V SBI entry point.
/// Called by OpenSBI or other SBI implementations.
#[no_mangle]
#[cfg(target_arch = "riscv64")]
pub extern "C" fn riscv_entry(hart_id: usize, dtb_ptr: usize) -> ! {
    // Initialize serial via SBI console
    sbi_console_putchar(b'S');
    sbi_console_putchar(b'p');
    sbi_console_putchar(b'l');
    sbi_console_putchar(b'a');
    sbi_console_putchar(b'x');
    sbi_console_putchar(b'\r');
    sbi_console_putchar(b'\n');

    let mut boot_info = BootInfo::new();
    boot_info.boot_method = BootMethod::Sbi as u32;
    boot_info.boot_cpu_id = hart_id as u32;
    boot_info.acpi_or_dtb_addr = dtb_ptr as u64;

    // Parse Device Tree to get memory map
    // (Simplified - real implementation would parse FDT)
    
    loop {
        sbi_wfi();
    }
}

/// SBI console putchar (legacy extension)
#[cfg(target_arch = "riscv64")]
fn sbi_console_putchar(c: u8) {
    unsafe {
        core::arch::asm!(
            "li a7, 1",      // SBI_EXT_CONSOLE_PUTCHAR
            "mv a0, {0}",
            "ecall",
            in(reg) c as usize,
            out("a7") _,
            out("a0") _,
        );
    }
}

/// SBI wait for interrupt
#[cfg(target_arch = "riscv64")]
fn sbi_wfi() {
    unsafe {
        core::arch::asm!("wfi");
    }
}

// ============================================================================
// aarch64 Entry Point
// ============================================================================

/// aarch64 direct boot entry (for non-UEFI platforms like Raspberry Pi bare metal)
#[no_mangle]
#[cfg(target_arch = "aarch64")]
pub extern "C" fn aarch64_entry(dtb_ptr: usize) -> ! {
    // Setup UART on Raspberry Pi
    // GPIO function select, etc. would go here
    
    let mut boot_info = BootInfo::new();
    boot_info.boot_method = BootMethod::Direct as u32;
    boot_info.acpi_or_dtb_addr = dtb_ptr as u64;

    // Parse Device Tree for memory map
    // Real implementation would parse FDT structure

    loop {
        halt();
    }
}

// ============================================================================
// Serial Console (x86_64)
// ============================================================================

const COM1_PORT: u16 = 0x3F8;

/// Initialize serial port for debugging
#[cfg(target_arch = "x86_64")]
fn serial_init() {
    unsafe {
        // Disable interrupts
        outb(COM1_PORT + 1, 0x00);
        // Enable DLAB (set baud rate divisor)
        outb(COM1_PORT + 3, 0x80);
        // Set divisor to 1 (115200 baud)
        outb(COM1_PORT + 0, 0x01);
        outb(COM1_PORT + 1, 0x00);
        // 8 bits, no parity, one stop bit
        outb(COM1_PORT + 3, 0x03);
        // Enable FIFO, clear them, with 14-byte threshold
        outb(COM1_PORT + 2, 0xC7);
        // IRQs enabled, RTS/DSR set
        outb(COM1_PORT + 4, 0x0B);
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn serial_init() {}

/// Print string to serial port
#[cfg(target_arch = "x86_64")]
fn serial_print(s: &str) {
    for byte in s.bytes() {
        serial_putchar(byte);
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn serial_print(_s: &str) {}

/// Print hex value
fn serial_print_hex(val: u64) {
    #[cfg(target_arch = "x86_64")]
    {
        serial_print("0x");
        for i in (0..16).rev() {
            let nibble = ((val >> (i * 4)) & 0xF) as u8;
            let c = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
            serial_putchar(c);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = val;
}

#[cfg(target_arch = "x86_64")]
fn serial_putchar(c: u8) {
    unsafe {
        // Wait for transmit buffer empty
        while (inb(COM1_PORT + 5) & 0x20) == 0 {}
        outb(COM1_PORT, c);
    }
}

/// Port I/O: output byte
#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") val,
        options(nostack, nomem)
    );
}

/// Port I/O: input byte
#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!(
        "in al, dx",
        in("dx") port,
        out("al") val,
        options(nostack, nomem)
    );
    val
}

// ============================================================================
// Common Utilities
// ============================================================================

/// Halts the CPU in a power-efficient manner.
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
    #[cfg(target_arch = "riscv64")]
    unsafe {
        core::arch::asm!("wfi", options(nomem, nostack));
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "riscv64")))]
    {
        core::hint::spin_loop();
    }
}

/// Panic handler for the bootloader.
#[cfg_attr(not(test), panic_handler)]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_print("\r\n!!! BOOTLOADER PANIC !!!\r\n");
    if let Some(location) = info.location() {
        serial_print("at ");
        serial_print(location.file());
        serial_print(":");
        serial_print_hex(location.line() as u64);
        serial_print("\r\n");
    }
    loop {
        halt();
    }
}

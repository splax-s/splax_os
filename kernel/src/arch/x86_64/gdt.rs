//! # GDT (Global Descriptor Table) for x86_64
//!
//! In 64-bit long mode, the GDT is mostly vestigial but still required
//! for segment selectors and TSS (Task State Segment).

use core::mem::size_of;

/// GDT entry - 8 bytes each.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
}

impl GdtEntry {
    /// Creates a null GDT entry.
    pub const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: 0,
            granularity: 0,
            base_high: 0,
        }
    }

    /// Creates a kernel code segment (64-bit).
    pub const fn kernel_code() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            access: 0x9A, // Present, Ring 0, Code, Execute/Read
            granularity: 0xAF, // 64-bit, 4KB granularity
            base_high: 0,
        }
    }

    /// Creates a kernel data segment.
    pub const fn kernel_data() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            access: 0x92, // Present, Ring 0, Data, Read/Write
            granularity: 0xCF, // 32-bit, 4KB granularity
            base_high: 0,
        }
    }

    /// Creates a user code segment (64-bit).
    pub const fn user_code() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            access: 0xFA, // Present, Ring 3, Code, Execute/Read
            granularity: 0xAF, // 64-bit, 4KB granularity
            base_high: 0,
        }
    }

    /// Creates a user data segment.
    pub const fn user_data() -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_middle: 0,
            access: 0xF2, // Present, Ring 3, Data, Read/Write
            granularity: 0xCF, // 32-bit, 4KB granularity
            base_high: 0,
        }
    }
}

/// TSS entry in GDT (16 bytes - spans two GDT entries).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct TssEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
    base_upper: u32,
    reserved: u32,
}

impl TssEntry {
    /// Creates a TSS entry for the given TSS address.
    pub const fn new(tss_addr: u64, tss_size: u16) -> Self {
        Self {
            limit_low: tss_size,
            base_low: tss_addr as u16,
            base_middle: (tss_addr >> 16) as u8,
            access: 0x89, // Present, 64-bit TSS (Available)
            granularity: 0,
            base_high: (tss_addr >> 24) as u8,
            base_upper: (tss_addr >> 32) as u32,
            reserved: 0,
        }
    }
}

/// Task State Segment (TSS).
#[repr(C, packed)]
pub struct Tss {
    reserved0: u32,
    /// Stack pointers for privilege levels 0-2
    pub rsp: [u64; 3],
    reserved1: u64,
    /// Interrupt stack table
    pub ist: [u64; 7],
    reserved2: u64,
    reserved3: u16,
    /// I/O map base address
    pub iopb_offset: u16,
}

impl Tss {
    /// Creates a new TSS with default values.
    pub const fn new() -> Self {
        Self {
            reserved0: 0,
            rsp: [0; 3],
            reserved1: 0,
            ist: [0; 7],
            reserved2: 0,
            reserved3: 0,
            iopb_offset: size_of::<Tss>() as u16,
        }
    }
}

/// GDT descriptor pointer for LGDT instruction.
#[repr(C, packed)]
pub struct GdtDescriptor {
    pub limit: u16,
    pub base: u64,
}

/// Segment selectors.
pub mod selectors {
    pub const NULL: u16 = 0x00;
    pub const KERNEL_CODE: u16 = 0x08;
    pub const KERNEL_DATA: u16 = 0x10;
    pub const USER_DATA: u16 = 0x18 | 3; // Ring 3
    pub const USER_CODE: u16 = 0x20 | 3; // Ring 3
    pub const TSS: u16 = 0x28;
}

/// The Global Descriptor Table.
#[repr(C, align(16))]
pub struct Gdt {
    pub null: GdtEntry,
    pub kernel_code: GdtEntry,
    pub kernel_data: GdtEntry,
    pub user_data: GdtEntry,
    pub user_code: GdtEntry,
    pub tss: TssEntry,
}

impl Gdt {
    /// Creates a new GDT with standard entries.
    pub const fn new() -> Self {
        Self {
            null: GdtEntry::null(),
            kernel_code: GdtEntry::kernel_code(),
            kernel_data: GdtEntry::kernel_data(),
            user_data: GdtEntry::user_data(),
            user_code: GdtEntry::user_code(),
            tss: TssEntry::new(0, 0), // Will be updated with actual TSS address
        }
    }

    /// Sets the TSS entry.
    pub fn set_tss(&mut self, tss_addr: u64) {
        self.tss = TssEntry::new(tss_addr, (size_of::<Tss>() - 1) as u16);
    }

    /// Loads this GDT.
    ///
    /// # Safety
    ///
    /// This function is unsafe because loading an invalid GDT will crash.
    pub unsafe fn load(&'static self) {
        let descriptor = GdtDescriptor {
            limit: (size_of::<Gdt>() - 1) as u16,
            base: self as *const _ as u64,
        };

        unsafe {
            core::arch::asm!(
                "lgdt [{}]",
                in(reg) &descriptor,
                options(readonly, nostack, preserves_flags)
            );
        }
    }
}

/// Loads segment registers after GDT is loaded.
///
/// # Safety
///
/// Must be called after GDT is loaded with valid selectors.
pub unsafe fn load_segments() {
    unsafe {
        core::arch::asm!(
            // Reload CS via far return
            "push {kcs}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            // Reload data segments
            "mov ds, {kds:x}",
            "mov es, {kds:x}",
            "mov fs, {kds:x}",
            "mov gs, {kds:x}",
            "mov ss, {kds:x}",
            kcs = const selectors::KERNEL_CODE as u64,
            kds = in(reg) selectors::KERNEL_DATA as u64,
            tmp = lateout(reg) _,
            options(preserves_flags)
        );
    }
}

/// Loads the TSS.
///
/// # Safety
///
/// Must be called after GDT with TSS entry is loaded.
pub unsafe fn load_tss() {
    unsafe {
        core::arch::asm!(
            "ltr {0:x}",
            in(reg) selectors::TSS,
            options(nomem, nostack, preserves_flags)
        );
    }
}

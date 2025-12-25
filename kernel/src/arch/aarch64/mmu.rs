//! # AArch64 MMU (Memory Management Unit)
//!
//! Page table management for ARM64 with 4KB granule.
//!
//! ## Translation Levels
//! - Level 0: 512GB regions (PGD)
//! - Level 1: 1GB regions (PUD)
//! - Level 2: 2MB regions (PMD)
//! - Level 3: 4KB pages (PTE)
//!
//! ## Memory Attributes (MAIR)
//! - 0: Device-nGnRnE (strongly ordered)
//! - 1: Normal, outer/inner write-back, non-transient
//! - 2: Normal, outer/inner non-cacheable

use core::arch::asm;
use super::PageTableDescriptor;

/// Page size (4KB).
pub const PAGE_SIZE: usize = 4096;
pub const PAGE_SHIFT: usize = 12;

/// Number of entries per page table.
pub const ENTRIES_PER_TABLE: usize = 512;

/// Virtual address space sizes.
pub const VA_BITS: usize = 48;
pub const VA_MASK: u64 = (1 << VA_BITS) - 1;

/// Physical address mask.
pub const PA_MASK: u64 = 0x0000_FFFF_FFFF_F000;

/// MAIR (Memory Attribute Indirection Register) values.
pub mod mair {
    /// Device-nGnRnE memory
    pub const DEVICE_NGNRNE: u64 = 0x00;
    /// Normal memory, outer/inner write-back
    pub const NORMAL_WB: u64 = 0xFF;
    /// Normal memory, outer/inner non-cacheable
    pub const NORMAL_NC: u64 = 0x44;
    
    /// Default MAIR value
    pub const DEFAULT: u64 = 
        (DEVICE_NGNRNE << 0) |  // Index 0
        (NORMAL_WB << 8) |      // Index 1
        (NORMAL_NC << 16);      // Index 2
}

/// Memory attribute indices for page table entries.
pub mod attr_idx {
    pub const DEVICE: u64 = 0 << 2;
    pub const NORMAL: u64 = 1 << 2;
    pub const NORMAL_NC: u64 = 2 << 2;
}

/// TCR (Translation Control Register) values.
pub mod tcr {
    /// T0SZ: 48-bit VA for TTBR0
    pub const T0SZ_48: u64 = 16;
    /// T1SZ: 48-bit VA for TTBR1
    pub const T1SZ_48: u64 = 16 << 16;
    /// 4KB granule for TTBR0
    pub const TG0_4KB: u64 = 0b00 << 14;
    /// 4KB granule for TTBR1
    pub const TG1_4KB: u64 = 0b10 << 30;
    /// Inner shareable
    pub const SH0_INNER: u64 = 0b11 << 12;
    pub const SH1_INNER: u64 = 0b11 << 28;
    /// Outer write-back cacheable
    pub const ORGN0_WB: u64 = 0b01 << 10;
    pub const ORGN1_WB: u64 = 0b01 << 26;
    /// Inner write-back cacheable
    pub const IRGN0_WB: u64 = 0b01 << 8;
    pub const IRGN1_WB: u64 = 0b01 << 24;
    /// Intermediate physical address size (48 bits)
    pub const IPS_48: u64 = 0b101 << 32;
    
    /// Default TCR value
    pub const DEFAULT: u64 = 
        T0SZ_48 | T1SZ_48 |
        TG0_4KB | TG1_4KB |
        SH0_INNER | SH1_INNER |
        ORGN0_WB | ORGN1_WB |
        IRGN0_WB | IRGN1_WB |
        IPS_48;
}

/// SCTLR (System Control Register) bits.
pub mod sctlr {
    /// MMU enable
    pub const M: u64 = 1 << 0;
    /// Alignment check enable
    pub const A: u64 = 1 << 1;
    /// Data cache enable
    pub const C: u64 = 1 << 2;
    /// Stack alignment check enable
    pub const SA: u64 = 1 << 3;
    /// SP EL0 stack alignment check enable
    pub const SA0: u64 = 1 << 4;
    /// Instruction cache enable
    pub const I: u64 = 1 << 12;
    /// Write permission implies XN
    pub const WXN: u64 = 1 << 19;
}

/// Page table entry for 4KB granule.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    /// Invalid entry.
    pub const INVALID: Self = Self(0);
    
    /// Create a new table entry pointing to next-level table.
    pub const fn table(phys_addr: u64) -> Self {
        Self((phys_addr & PA_MASK) | PageTableDescriptor::TABLE)
    }
    
    /// Create a new page entry.
    pub const fn page(phys_addr: u64, flags: u64) -> Self {
        Self((phys_addr & PA_MASK) | flags | PageTableDescriptor::PAGE)
    }
    
    /// Create a block entry (1GB or 2MB).
    pub const fn block(phys_addr: u64, flags: u64) -> Self {
        Self((phys_addr & PA_MASK) | flags | PageTableDescriptor::BLOCK)
    }
    
    /// Check if entry is valid.
    pub const fn is_valid(&self) -> bool {
        (self.0 & 0b11) != 0
    }
    
    /// Check if entry is a table reference.
    pub const fn is_table(&self) -> bool {
        (self.0 & 0b11) == 0b11 && ((self.0 >> 1) & 1) == 1
    }
    
    /// Get physical address.
    pub const fn addr(&self) -> u64 {
        self.0 & PA_MASK
    }
    
    /// Get raw value.
    pub const fn raw(&self) -> u64 {
        self.0
    }
}

/// Kernel page flags.
pub fn kernel_page_flags() -> u64 {
    PageTableDescriptor::AF |           // Access flag
    PageTableDescriptor::SH_INNER |     // Inner shareable
    attr_idx::NORMAL |                  // Normal memory
    PageTableDescriptor::AP_RW_EL1      // Read-write EL1 only
}

/// Device page flags.
pub fn device_page_flags() -> u64 {
    PageTableDescriptor::AF |
    PageTableDescriptor::SH_NON |
    attr_idx::DEVICE |
    PageTableDescriptor::AP_RW_EL1 |
    PageTableDescriptor::PXN |          // No execute
    PageTableDescriptor::UXN
}

/// User page flags.
pub fn user_page_flags(writable: bool, executable: bool) -> u64 {
    let mut flags = PageTableDescriptor::AF |
        PageTableDescriptor::SH_INNER |
        attr_idx::NORMAL |
        PageTableDescriptor::NG;        // Not global
    
    if writable {
        flags |= PageTableDescriptor::AP_RW_ALL;
    } else {
        flags |= PageTableDescriptor::AP_RO_ALL;
    }
    
    if !executable {
        flags |= PageTableDescriptor::UXN;
    }
    
    flags
}

/// Initialize the MMU.
///
/// # Safety
///
/// Must only be called once during boot with valid page tables.
pub unsafe fn init(kernel_pgd: u64) {
    unsafe {
        // Set MAIR
        asm!("msr mair_el1, {}", in(reg) mair::DEFAULT, options(nomem, nostack));
        
        // Set TCR
        asm!("msr tcr_el1, {}", in(reg) tcr::DEFAULT, options(nomem, nostack));
        
        // Set TTBR0 and TTBR1 (both point to kernel tables initially)
        asm!("msr ttbr0_el1, {}", in(reg) kernel_pgd, options(nomem, nostack));
        asm!("msr ttbr1_el1, {}", in(reg) kernel_pgd, options(nomem, nostack));
        
        // Barrier
        asm!("isb");
        
        // Invalidate TLB
        super::invalidate_tlb();
        
        // Enable MMU
        let mut sctlr: u64;
        asm!("mrs {}, sctlr_el1", out(reg) sctlr, options(nomem, nostack));
        sctlr |= sctlr::M | sctlr::C | sctlr::I;
        asm!("msr sctlr_el1, {}", in(reg) sctlr, options(nomem, nostack));
        asm!("isb");
    }
}

/// Map a single page.
///
/// # Safety
///
/// Caller must ensure addresses are valid and properly aligned.
pub unsafe fn map_page(
    pgd: *mut [PageTableEntry; ENTRIES_PER_TABLE],
    virt: u64,
    phys: u64,
    flags: u64,
    allocate_table: fn() -> *mut [PageTableEntry; ENTRIES_PER_TABLE],
) {
    let indices = [
        ((virt >> 39) & 0x1FF) as usize,  // L0
        ((virt >> 30) & 0x1FF) as usize,  // L1
        ((virt >> 21) & 0x1FF) as usize,  // L2
        ((virt >> 12) & 0x1FF) as usize,  // L3
    ];
    
    let mut table = pgd;
    
    // Walk levels 0-2, allocating tables as needed
    for level in 0..3 {
        let entry = unsafe { &mut (*table)[indices[level]] };
        
        if !entry.is_valid() {
            let new_table = allocate_table();
            *entry = PageTableEntry::table(new_table as u64);
        }
        
        table = entry.addr() as *mut [PageTableEntry; ENTRIES_PER_TABLE];
    }
    
    // Set level 3 entry
    unsafe { (*table)[indices[3]] = PageTableEntry::page(phys, flags); }
}

/// Translate virtual address to physical.
pub fn translate(virt: u64) -> Option<u64> {
    let par: u64;
    
    unsafe {
        // Use AT S1E1R to translate address
        asm!(
            "at s1e1r, {}",
            "isb",
            "mrs {}, par_el1",
            in(reg) virt,
            out(reg) par,
            options(nomem, nostack)
        );
    }
    
    // Check for translation fault (bit 0)
    if par & 1 != 0 {
        None
    } else {
        // Extract physical address
        let phys = (par & PA_MASK) | (virt & 0xFFF);
        Some(phys)
    }
}

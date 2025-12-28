//! # RISC-V MMU (Sv39/Sv48)
//!
//! Memory Management Unit support for RISC-V.
//!
//! ## Sv39 Page Table
//!
//! 3-level page table with 512 entries per level.
//!
//! ```text
//! Virtual Address (39 bits):
//! ┌─────────┬─────────┬─────────┬─────────────┐
//! │  VPN[2] │  VPN[1] │  VPN[0] │   Offset    │
//! │  9 bits │  9 bits │  9 bits │   12 bits   │
//! └─────────┴─────────┴─────────┴─────────────┘
//!
//! Physical Address (56 bits):
//! ┌─────────────────────────────┬─────────────┐
//! │            PPN              │   Offset    │
//! │          44 bits            │   12 bits   │
//! └─────────────────────────────┴─────────────┘
//! ```
//!
//! ## Page Table Entry
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────┐
//! │ Reserved │    PPN    │ RSW │ D │ A │ G │ U │ X │ W │ R │ V │
//! │  10 bits │  44 bits  │ 2b  │ 1 │ 1 │ 1 │ 1 │ 1 │ 1 │ 1 │ 1 │
//! └────────────────────────────────────────────────────────────────┘
//! ```

use core::ptr::{read_volatile, write_volatile};
use super::csr::{self, satp};

/// Page size (4KB)
pub const PAGE_SIZE: usize = 4096;

/// Page shift
pub const PAGE_SHIFT: usize = 12;

/// Number of entries per page table
pub const ENTRIES_PER_TABLE: usize = 512;

/// Page table entry
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    // Flags
    pub const VALID: u64 = 1 << 0;
    pub const READ: u64 = 1 << 1;
    pub const WRITE: u64 = 1 << 2;
    pub const EXEC: u64 = 1 << 3;
    pub const USER: u64 = 1 << 4;
    pub const GLOBAL: u64 = 1 << 5;
    pub const ACCESSED: u64 = 1 << 6;
    pub const DIRTY: u64 = 1 << 7;

    /// Create empty entry
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Create entry with PPN and flags
    pub const fn new(ppn: u64, flags: u64) -> Self {
        Self((ppn << 10) | flags)
    }

    /// Check if valid
    pub fn is_valid(&self) -> bool {
        self.0 & Self::VALID != 0
    }

    /// Check if leaf (has R/W/X permissions)
    pub fn is_leaf(&self) -> bool {
        self.is_valid() && (self.0 & (Self::READ | Self::WRITE | Self::EXEC)) != 0
    }

    /// Get PPN
    pub fn ppn(&self) -> u64 {
        (self.0 >> 10) & 0xFFF_FFFF_FFFF
    }

    /// Get physical address of next level or mapped page
    pub fn phys_addr(&self) -> u64 {
        self.ppn() << PAGE_SHIFT
    }

    /// Get flags
    pub fn flags(&self) -> u64 {
        self.0 & 0x3FF
    }

    /// Set accessed bit
    pub fn set_accessed(&mut self) {
        self.0 |= Self::ACCESSED;
    }

    /// Set dirty bit
    pub fn set_dirty(&mut self) {
        self.0 |= Self::DIRTY;
    }
}

/// Page table (512 entries)
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; ENTRIES_PER_TABLE],
}

impl PageTable {
    /// Create empty page table
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::empty(); ENTRIES_PER_TABLE],
        }
    }

    /// Get entry by index
    pub fn get(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }

    /// Get mutable entry by index
    pub fn get_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }
}

/// Virtual address utilities
pub struct VirtAddr(pub u64);

impl VirtAddr {
    /// Get VPN[level] (0, 1, or 2)
    pub fn vpn(&self, level: usize) -> usize {
        ((self.0 >> (PAGE_SHIFT + level * 9)) & 0x1FF) as usize
    }

    /// Get page offset
    pub fn offset(&self) -> usize {
        (self.0 & 0xFFF) as usize
    }
}

/// Physical address utilities
pub struct PhysAddr(pub u64);

impl PhysAddr {
    /// Get PPN
    pub fn ppn(&self) -> u64 {
        self.0 >> PAGE_SHIFT
    }

    /// Create from page number
    pub fn from_ppn(ppn: u64) -> Self {
        Self(ppn << PAGE_SHIFT)
    }
}

/// Enable Sv39 paging
pub unsafe fn enable_sv39(root_table_phys: u64) {
    let ppn = root_table_phys >> PAGE_SHIFT;
    let satp_val = csr::make_satp(satp::SV39, 0, ppn);
    
    // Write satp and flush TLB
    csr::write_satp(satp_val);
    super::sfence_vma();
}

/// Enable Sv48 paging
pub unsafe fn enable_sv48(root_table_phys: u64) {
    let ppn = root_table_phys >> PAGE_SHIFT;
    let satp_val = csr::make_satp(satp::SV48, 0, ppn);
    
    csr::write_satp(satp_val);
    super::sfence_vma();
}

/// Disable paging (bare mode)
pub unsafe fn disable_paging() {
    csr::write_satp(0);
    super::sfence_vma();
}

/// Walk page table and return PTE for virtual address
pub fn walk(
    root: &PageTable,
    vaddr: VirtAddr,
    create: bool,
    alloc_page: impl Fn() -> Option<u64>,
) -> Option<&'static mut PageTableEntry> {
    let mut table = root as *const PageTable as *mut PageTable;
    
    for level in (1..=2).rev() {
        let vpn = vaddr.vpn(level);
        let entry = unsafe { &mut (*table).entries[vpn] };
        
        if entry.is_valid() {
            if entry.is_leaf() {
                // Superpage mapping
                return None;  // Or handle superpages
            }
            table = entry.phys_addr() as *mut PageTable;
        } else if create {
            // Allocate new table
            let new_table = alloc_page()?;
            
            // Zero the new table
            unsafe {
                core::ptr::write_bytes(new_table as *mut u8, 0, PAGE_SIZE);
            }
            
            // Set entry to point to new table
            *entry = PageTableEntry::new(new_table >> PAGE_SHIFT, PageTableEntry::VALID);
            table = new_table as *mut PageTable;
        } else {
            return None;
        }
    }
    
    // Return leaf entry
    let vpn = vaddr.vpn(0);
    Some(unsafe { &mut (*table).entries[vpn] })
}

/// Map a page
pub fn map_page(
    root: &mut PageTable,
    vaddr: u64,
    paddr: u64,
    flags: u64,
    alloc_page: impl Fn() -> Option<u64>,
) -> Result<(), &'static str> {
    let pte = walk(root, VirtAddr(vaddr), true, alloc_page)
        .ok_or("Failed to walk page table")?;
    
    if pte.is_valid() {
        return Err("Page already mapped");
    }
    
    *pte = PageTableEntry::new(
        paddr >> PAGE_SHIFT,
        flags | PageTableEntry::VALID,
    );
    
    Ok(())
}

/// Unmap a page
pub fn unmap_page(root: &mut PageTable, vaddr: u64) -> Result<u64, &'static str> {
    let pte = walk(root, VirtAddr(vaddr), false, || None)
        .ok_or("Page not mapped")?;
    
    if !pte.is_valid() {
        return Err("Page not mapped");
    }
    
    let paddr = pte.phys_addr();
    *pte = PageTableEntry::empty();
    
    // Flush TLB for this address
    super::sfence_vma_addr(vaddr as usize);
    
    Ok(paddr)
}

/// Translate virtual address to physical
pub fn translate(root: &PageTable, vaddr: u64) -> Option<u64> {
    let pte = walk(root, VirtAddr(vaddr), false, || None)?;
    
    if !pte.is_valid() {
        return None;
    }
    
    Some(pte.phys_addr() | (vaddr & 0xFFF))
}

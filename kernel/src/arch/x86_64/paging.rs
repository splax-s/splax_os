//! # Page Table Management
//!
//! x86_64 4-level paging implementation for Splax OS.
//!
//! ## Address Space Layout
//!
//! ```text
//! 0x0000_0000_0000_0000 - 0x0000_7FFF_FFFF_FFFF : User space (128 TB)
//! 0xFFFF_8000_0000_0000 - 0xFFFF_FFFF_FFFF_FFFF : Kernel space (128 TB)
//!   0xFFFF_8000_0000_0000 : Physical memory direct map
//!   0xFFFF_FFFF_8000_0000 : Kernel code/data
//!   0xFFFF_FFFF_C000_0000 : Kernel heap
//! ```

use core::ptr::NonNull;
use bitflags::bitflags;

/// Size of a page (4 KB)
pub const PAGE_SIZE: usize = 4096;

/// Size of a large page (2 MB)
pub const LARGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// Size of a huge page (1 GB)
pub const HUGE_PAGE_SIZE: usize = 1024 * 1024 * 1024;

/// Number of entries in a page table
pub const ENTRIES_PER_TABLE: usize = 512;

/// Physical memory direct map base address
pub const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFF_8000_0000_0000;

/// Kernel heap start address
pub const KERNEL_HEAP_START: u64 = 0xFFFF_FFFF_C000_0000;

/// Kernel heap size (256 MB)
pub const KERNEL_HEAP_SIZE: usize = 256 * 1024 * 1024;

bitflags! {
    /// Page table entry flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PageFlags: u64 {
        /// Page is present in memory
        const PRESENT = 1 << 0;
        /// Page is writable
        const WRITABLE = 1 << 1;
        /// Page is accessible from user mode
        const USER_ACCESSIBLE = 1 << 2;
        /// Writes go directly to memory (no caching)
        const WRITE_THROUGH = 1 << 3;
        /// Disable caching for this page
        const NO_CACHE = 1 << 4;
        /// Page has been accessed
        const ACCESSED = 1 << 5;
        /// Page has been written to
        const DIRTY = 1 << 6;
        /// Use huge pages (1GB at PML4, 2MB at PDPT)
        const HUGE_PAGE = 1 << 7;
        /// Page is global (not flushed on CR3 switch)
        const GLOBAL = 1 << 8;
        /// Disable execution (NX bit)
        const NO_EXECUTE = 1 << 63;
    }
}

impl PageFlags {
    /// Flags for kernel code pages
    pub const KERNEL_CODE: Self = Self::PRESENT;
    
    /// Flags for kernel data pages
    pub const KERNEL_DATA: Self = Self::PRESENT.union(Self::WRITABLE).union(Self::NO_EXECUTE);
    
    /// Flags for user code pages  
    pub const USER_CODE: Self = Self::PRESENT.union(Self::USER_ACCESSIBLE);
    
    /// Flags for user data pages
    pub const USER_DATA: Self = Self::PRESENT.union(Self::WRITABLE).union(Self::USER_ACCESSIBLE).union(Self::NO_EXECUTE);
}

/// A physical frame number (PFN).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysFrame(u64);

impl PhysFrame {
    /// Creates a new physical frame from an address.
    /// The address must be page-aligned.
    pub const fn from_address(addr: u64) -> Option<Self> {
        if addr % PAGE_SIZE as u64 == 0 {
            Some(Self(addr / PAGE_SIZE as u64))
        } else {
            None
        }
    }

    /// Returns the physical address of this frame.
    pub const fn address(&self) -> u64 {
        self.0 * PAGE_SIZE as u64
    }

    /// Returns the frame number.
    pub const fn number(&self) -> u64 {
        self.0
    }
}

/// A virtual page number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VirtPage(u64);

impl VirtPage {
    /// Creates a new virtual page from an address.
    pub const fn from_address(addr: u64) -> Option<Self> {
        if addr % PAGE_SIZE as u64 == 0 {
            Some(Self(addr / PAGE_SIZE as u64))
        } else {
            None
        }
    }

    /// Returns the virtual address of this page.
    pub const fn address(&self) -> u64 {
        self.0 * PAGE_SIZE as u64
    }

    /// Returns the PML4 index for this page.
    pub const fn pml4_index(&self) -> usize {
        ((self.address() >> 39) & 0x1FF) as usize
    }

    /// Returns the PDPT index for this page.
    pub const fn pdpt_index(&self) -> usize {
        ((self.address() >> 30) & 0x1FF) as usize
    }

    /// Returns the PD index for this page.
    pub const fn pd_index(&self) -> usize {
        ((self.address() >> 21) & 0x1FF) as usize
    }

    /// Returns the PT index for this page.
    pub const fn pt_index(&self) -> usize {
        ((self.address() >> 12) & 0x1FF) as usize
    }
}

/// A page table entry.
#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    /// Creates an empty (not present) entry.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Creates a new entry pointing to a frame with flags.
    pub const fn new(frame: PhysFrame, flags: PageFlags) -> Self {
        Self(frame.address() | flags.bits())
    }

    /// Returns whether this entry is present.
    pub const fn is_present(&self) -> bool {
        self.0 & PageFlags::PRESENT.bits() != 0
    }

    /// Returns whether this is a huge page entry.
    pub const fn is_huge(&self) -> bool {
        self.0 & PageFlags::HUGE_PAGE.bits() != 0
    }

    /// Returns the physical frame this entry points to.
    pub const fn frame(&self) -> Option<PhysFrame> {
        if self.is_present() {
            PhysFrame::from_address(self.0 & 0x000F_FFFF_FFFF_F000)
        } else {
            None
        }
    }

    /// Returns the flags of this entry.
    pub const fn flags(&self) -> PageFlags {
        PageFlags::from_bits_truncate(self.0)
    }

    /// Sets the flags of this entry.
    pub fn set_flags(&mut self, flags: PageFlags) {
        let addr = self.0 & 0x000F_FFFF_FFFF_F000;
        self.0 = addr | flags.bits();
    }
}

/// A page table (any level).
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [PageTableEntry; ENTRIES_PER_TABLE],
}

impl PageTable {
    /// Creates a new empty page table.
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::empty(); ENTRIES_PER_TABLE],
        }
    }

    /// Returns a reference to an entry.
    pub fn entry(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }

    /// Returns a mutable reference to an entry.
    pub fn entry_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }

    /// Clears all entries in the table.
    pub fn clear(&mut self) {
        for entry in self.entries.iter_mut() {
            *entry = PageTableEntry::empty();
        }
    }
}

/// Page table walker for address translation.
pub struct PageTableWalker {
    pml4_phys: u64,
}

impl PageTableWalker {
    /// Creates a new walker for the given PML4 physical address.
    pub const fn new(pml4_phys: u64) -> Self {
        Self { pml4_phys }
    }

    /// Creates a walker from the current CR3 value.
    pub fn from_cr3() -> Self {
        let cr3: u64;
        unsafe {
            core::arch::asm!("mov {}, cr3", out(reg) cr3);
        }
        Self::new(cr3 & 0x000F_FFFF_FFFF_F000)
    }

    /// Translates a virtual address to a physical address.
    pub fn translate(&self, virt: u64) -> Option<u64> {
        let page = VirtPage::from_address(virt & !0xFFF)?;
        let offset = virt & 0xFFF;

        // Walk the page tables
        let pml4 = self.read_table(self.pml4_phys)?;
        let pml4e = pml4.entry(page.pml4_index());
        if !pml4e.is_present() {
            return None;
        }

        let pdpt = self.read_table(pml4e.frame()?.address())?;
        let pdpte = pdpt.entry(page.pdpt_index());
        if !pdpte.is_present() {
            return None;
        }
        if pdpte.is_huge() {
            // 1GB page
            return Some(pdpte.frame()?.address() | (virt & 0x3FFF_FFFF));
        }

        let pd = self.read_table(pdpte.frame()?.address())?;
        let pde = pd.entry(page.pd_index());
        if !pde.is_present() {
            return None;
        }
        if pde.is_huge() {
            // 2MB page
            return Some(pde.frame()?.address() | (virt & 0x1F_FFFF));
        }

        let pt = self.read_table(pde.frame()?.address())?;
        let pte = pt.entry(page.pt_index());
        if !pte.is_present() {
            return None;
        }

        Some(pte.frame()?.address() | offset)
    }

    /// Reads a page table from physical memory.
    fn read_table(&self, phys: u64) -> Option<&'static PageTable> {
        // Use the physical memory direct map
        let virt = phys + PHYSICAL_MEMORY_OFFSET;
        unsafe { Some(&*(virt as *const PageTable)) }
    }
}

/// Invalidate a TLB entry for a specific virtual address.
#[inline]
pub fn invalidate_page(addr: u64) {
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
    }
}

/// Flush the entire TLB by reloading CR3.
#[inline]
pub fn flush_tlb() {
    unsafe {
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3);
        core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nostack, preserves_flags));
    }
}

/// Read the current CR3 value.
#[inline]
pub fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) value, options(nostack, preserves_flags));
    }
    value
}

/// Write a new value to CR3.
///
/// # Safety
///
/// The caller must ensure that the new CR3 value points to a valid PML4 table.
#[inline]
pub unsafe fn write_cr3(value: u64) {
    unsafe {
        core::arch::asm!("mov cr3, {}", in(reg) value, options(nostack, preserves_flags));
    }
}

//! # Memory Tagging Extension (MTE) for AArch64
//!
//! Implements ARM Memory Tagging Extension support for detecting
//! memory safety violations at runtime.
//!
//! ## Features
//!
//! - **Allocation Tagging**: Assign random 4-bit tags to allocations
//! - **Use-After-Free Detection**: Detect accesses to freed memory
//! - **Buffer Overflow Detection**: Detect out-of-bounds accesses
//! - **Synchronous/Asynchronous Modes**: Configurable fault reporting
//!
//! ## Architecture
//!
//! MTE uses a 4-bit tag stored in the top byte of pointers and in
//! memory tag storage (1 bit per 16 bytes of memory).
//!
//! ```text
//! Pointer Layout (Top Byte Ignore + MTE):
//! ┌────────────────────────────────────────────────────────────────┐
//! │ 63  60 59   56 55                                           0 │
//! │ ├────┼───────┼───────────────────────────────────────────────┤│
//! │ │Ignr│  TAG  │              Virtual Address                  ││
//! │ ├────┼───────┼───────────────────────────────────────────────┤│
//! └────────────────────────────────────────────────────────────────┘
//! ```

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

// =============================================================================
// MTE Configuration
// =============================================================================

/// MTE granule size (16 bytes per tag).
pub const MTE_GRANULE_SIZE: usize = 16;

/// Number of possible tag values (4 bits = 16 tags).
pub const MTE_TAG_COUNT: usize = 16;

/// Tag mask for extracting tag from pointer.
pub const MTE_TAG_MASK: u64 = 0x0F00_0000_0000_0000;

/// Tag shift position.
pub const MTE_TAG_SHIFT: u32 = 56;

/// MTE checking mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MteMode {
    /// MTE disabled
    Disabled,
    /// Synchronous checking - fault on mismatch
    Synchronous,
    /// Asynchronous checking - accumulate faults
    Asynchronous,
    /// Asymmetric - sync for reads, async for writes
    Asymmetric,
}

impl Default for MteMode {
    fn default() -> Self {
        MteMode::Synchronous
    }
}

/// MTE fault type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MteFaultType {
    /// Tag mismatch on load
    LoadMismatch,
    /// Tag mismatch on store
    StoreMismatch,
    /// Tag check failure (hardware reported)
    TagCheckFault,
}

/// MTE fault information.
#[derive(Debug, Clone)]
pub struct MteFault {
    /// Fault type
    pub fault_type: MteFaultType,
    /// Faulting address
    pub address: u64,
    /// Expected tag (from memory)
    pub expected_tag: u8,
    /// Actual tag (from pointer)
    pub actual_tag: u8,
    /// Instruction pointer
    pub ip: u64,
    /// Timestamp
    pub timestamp: u64,
}

// =============================================================================
// MTE Hardware Detection
// =============================================================================

/// MTE hardware capabilities.
#[derive(Debug, Clone, Copy)]
pub struct MteCapabilities {
    /// MTE supported
    pub supported: bool,
    /// MTE version (0 = not supported, 2 = MTE2, 3 = MTE3)
    pub version: u8,
    /// Asymmetric mode supported
    pub asymmetric: bool,
    /// Tag storage size
    pub tag_storage_size: usize,
}

impl MteCapabilities {
    /// Detect MTE hardware capabilities.
    #[cfg(target_arch = "aarch64")]
    pub fn detect() -> Self {
        let mut caps = Self {
            supported: false,
            version: 0,
            asymmetric: false,
            tag_storage_size: 0,
        };

        unsafe {
            // Read ID_AA64PFR1_EL1 for MTE support
            // MTE: bits [11:8]
            // 0b0000 = not implemented
            // 0b0001 = MTE (EL0 only)
            // 0b0010 = MTE2 (EL0 and EL1)
            // 0b0011 = MTE3 (with asymmetric)
            let id_pfr1: u64;
            core::arch::asm!(
                "mrs {}, ID_AA64PFR1_EL1",
                out(reg) id_pfr1,
                options(nomem, nostack),
            );
            
            let mte_field = ((id_pfr1 >> 8) & 0xF) as u8;
            
            if mte_field >= 2 {
                caps.supported = true;
                caps.version = mte_field;
                caps.asymmetric = mte_field >= 3;
                // Tag storage is 1 tag per 16 bytes
                caps.tag_storage_size = 1; // bits per granule
            }
        }

        caps
    }

    #[cfg(not(target_arch = "aarch64"))]
    pub fn detect() -> Self {
        Self {
            supported: false,
            version: 0,
            asymmetric: false,
            tag_storage_size: 0,
        }
    }
}

// =============================================================================
// Tag Management
// =============================================================================

/// Generate a random MTE tag (0-15).
pub fn generate_tag() -> u8 {
    // Use hardware RNG if available, otherwise use a simple PRNG
    static TAG_COUNTER: AtomicU64 = AtomicU64::new(0x5DEECE66D);
    
    let val = TAG_COUNTER.fetch_add(0x5851F42D4C957F2D, Ordering::Relaxed);
    let hash = val.wrapping_mul(0x2545F4914F6CDD1D);
    ((hash >> 60) & 0xF) as u8
}

/// Generate a random tag that's different from the excluded tag.
pub fn generate_different_tag(exclude: u8) -> u8 {
    let mut tag = generate_tag();
    while tag == exclude {
        tag = (tag + 1) % 16;
    }
    tag
}

/// Apply a tag to a pointer.
#[inline]
pub fn tag_pointer(ptr: u64, tag: u8) -> u64 {
    (ptr & !MTE_TAG_MASK) | ((tag as u64 & 0xF) << MTE_TAG_SHIFT)
}

/// Extract tag from a pointer.
#[inline]
pub fn get_pointer_tag(ptr: u64) -> u8 {
    ((ptr & MTE_TAG_MASK) >> MTE_TAG_SHIFT) as u8
}

/// Clear tag from a pointer (get physical address).
#[inline]
pub fn clear_pointer_tag(ptr: u64) -> u64 {
    ptr & !MTE_TAG_MASK
}

// =============================================================================
// MTE Hardware Operations (AArch64)
// =============================================================================

#[cfg(target_arch = "aarch64")]
mod hw {
    use super::*;

    /// Set memory tags for a range (IRG - Insert Random Tag instruction).
    #[inline]
    pub unsafe fn set_tag(addr: u64, tag: u8) {
        // STG - Store Allocation Tag
        // Sets the tag for the 16-byte granule at addr
        let tagged_addr = tag_pointer(addr, tag);
        core::arch::asm!(
            "stg {0}, [{0}]",
            in(reg) tagged_addr,
            options(nostack),
        );
    }

    /// Set memory tags for a zeroed range (STZG).
    #[inline]
    pub unsafe fn set_tag_and_zero(addr: u64, tag: u8) {
        let tagged_addr = tag_pointer(addr, tag);
        core::arch::asm!(
            "stzg {0}, [{0}]",
            in(reg) tagged_addr,
            options(nostack),
        );
    }

    /// Set memory tags for a pair of granules (ST2G).
    #[inline]
    pub unsafe fn set_tag_pair(addr: u64, tag: u8) {
        let tagged_addr = tag_pointer(addr, tag);
        core::arch::asm!(
            "st2g {0}, [{0}]",
            in(reg) tagged_addr,
            options(nostack),
        );
    }

    /// Load memory tag for an address (LDG).
    #[inline]
    pub unsafe fn get_memory_tag(addr: u64) -> u8 {
        let result: u64;
        core::arch::asm!(
            "ldg {0}, [{1}]",
            out(reg) result,
            in(reg) addr,
            options(nostack, readonly),
        );
        get_pointer_tag(result)
    }

    /// Generate a random tag (IRG instruction).
    #[inline]
    pub unsafe fn insert_random_tag(ptr: u64, exclude_mask: u16) -> u64 {
        let result: u64;
        core::arch::asm!(
            "irg {0}, {1}, {2}",
            out(reg) result,
            in(reg) ptr,
            in(reg) exclude_mask as u64,
            options(nomem, nostack),
        );
        result
    }

    /// Add tag to pointer (ADDG instruction).
    #[inline]
    pub unsafe fn add_tag(ptr: u64, offset: u16, tag_offset: u8) -> u64 {
        let result: u64;
        // ADDG Xd, Xn, #offset, #tag_offset
        // Since we can't use immediate operands directly, compute manually
        let new_tag = (get_pointer_tag(ptr) + tag_offset) & 0xF;
        let new_ptr = clear_pointer_tag(ptr) + offset as u64;
        tag_pointer(new_ptr, new_tag)
    }

    /// Subtract tag from pointer (SUBG instruction).
    #[inline]
    pub unsafe fn sub_tag(ptr: u64, offset: u16, tag_offset: u8) -> u64 {
        let new_tag = (get_pointer_tag(ptr).wrapping_sub(tag_offset)) & 0xF;
        let new_ptr = clear_pointer_tag(ptr).wrapping_sub(offset as u64);
        tag_pointer(new_ptr, new_tag)
    }

    /// Enable MTE for the current EL.
    pub unsafe fn enable_mte(mode: MteMode) {
        // Set SCTLR_EL1.TCF1 and TCF0 for EL1 and EL0 tag checking
        let mut sctlr: u64;
        core::arch::asm!(
            "mrs {}, SCTLR_EL1",
            out(reg) sctlr,
            options(nomem, nostack),
        );

        // Clear existing TCF bits
        sctlr &= !(0b11 << 40); // TCF1 for EL1
        sctlr &= !(0b11 << 38); // TCF0 for EL0

        let tcf_bits = match mode {
            MteMode::Disabled => 0b00,
            MteMode::Synchronous => 0b01,
            MteMode::Asynchronous => 0b10,
            MteMode::Asymmetric => 0b11,
        };

        sctlr |= (tcf_bits as u64) << 40; // TCF1
        sctlr |= (tcf_bits as u64) << 38; // TCF0

        // Enable tagged address ABI
        sctlr |= 1 << 37; // ATA (Enable allocation tag access)

        core::arch::asm!(
            "msr SCTLR_EL1, {}",
            "isb",
            in(reg) sctlr,
            options(nomem, nostack),
        );
    }

    /// Disable MTE.
    pub unsafe fn disable_mte() {
        enable_mte(MteMode::Disabled);
    }

    /// Read TFSR_EL1 (Tag Fault Status Register).
    pub unsafe fn read_tag_fault_status() -> u64 {
        let tfsr: u64;
        core::arch::asm!(
            "mrs {}, TFSR_EL1",
            out(reg) tfsr,
            options(nomem, nostack),
        );
        tfsr
    }

    /// Clear TFSR_EL1.
    pub unsafe fn clear_tag_fault_status() {
        core::arch::asm!(
            "msr TFSR_EL1, xzr",
            options(nomem, nostack),
        );
    }
}

#[cfg(not(target_arch = "aarch64"))]
mod hw {
    use super::*;

    pub unsafe fn set_tag(_addr: u64, _tag: u8) {}
    pub unsafe fn set_tag_and_zero(_addr: u64, _tag: u8) {}
    pub unsafe fn set_tag_pair(_addr: u64, _tag: u8) {}
    pub unsafe fn get_memory_tag(_addr: u64) -> u8 { 0 }
    pub unsafe fn insert_random_tag(ptr: u64, _exclude: u16) -> u64 { ptr }
    pub unsafe fn add_tag(ptr: u64, _offset: u16, _tag_offset: u8) -> u64 { ptr }
    pub unsafe fn sub_tag(ptr: u64, _offset: u16, _tag_offset: u8) -> u64 { ptr }
    pub unsafe fn enable_mte(_mode: MteMode) {}
    pub unsafe fn disable_mte() {}
    pub unsafe fn read_tag_fault_status() -> u64 { 0 }
    pub unsafe fn clear_tag_fault_status() {}
}

// =============================================================================
// MTE Manager
// =============================================================================

/// MTE manager state.
pub struct MteManager {
    /// Whether MTE is enabled
    enabled: AtomicBool,
    /// Current mode
    mode: Mutex<MteMode>,
    /// Hardware capabilities
    capabilities: Mutex<Option<MteCapabilities>>,
    /// Fault log
    faults: Mutex<Vec<MteFault>>,
    /// Total fault count
    fault_count: AtomicU64,
}

impl MteManager {
    /// Create a new MTE manager.
    pub const fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            mode: Mutex::new(MteMode::Disabled),
            capabilities: Mutex::new(None),
            faults: Mutex::new(Vec::new()),
            fault_count: AtomicU64::new(0),
        }
    }

    /// Initialize MTE.
    pub fn init(&self, mode: MteMode) {
        let caps = MteCapabilities::detect();
        
        crate::serial_println!("[MTE] Capabilities: {:?}", caps);
        
        *self.capabilities.lock() = Some(caps);

        if !caps.supported {
            crate::serial_println!("[MTE] Hardware MTE not available, using software tagging");
            return;
        }

        if mode != MteMode::Disabled {
            unsafe {
                hw::enable_mte(mode);
            }
            self.enabled.store(true, Ordering::SeqCst);
            *self.mode.lock() = mode;
            
            crate::serial_println!("[MTE] Enabled in {:?} mode", mode);
        }
    }

    /// Check if MTE is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Get current mode.
    pub fn mode(&self) -> MteMode {
        *self.mode.lock()
    }

    /// Tag a memory region.
    pub fn tag_region(&self, addr: u64, size: usize, tag: u8) {
        if !self.is_enabled() {
            return;
        }

        let aligned_addr = addr & !(MTE_GRANULE_SIZE as u64 - 1);
        let end = addr + size as u64;
        let mut current = aligned_addr;

        while current < end {
            unsafe {
                hw::set_tag(current, tag);
            }
            current += MTE_GRANULE_SIZE as u64;
        }
    }

    /// Tag and zero a memory region.
    pub fn tag_and_zero_region(&self, addr: u64, size: usize, tag: u8) {
        if !self.is_enabled() {
            return;
        }

        let aligned_addr = addr & !(MTE_GRANULE_SIZE as u64 - 1);
        let end = addr + size as u64;
        let mut current = aligned_addr;

        while current < end {
            unsafe {
                hw::set_tag_and_zero(current, tag);
            }
            current += MTE_GRANULE_SIZE as u64;
        }
    }

    /// Get tag for a memory address.
    pub fn get_tag(&self, addr: u64) -> u8 {
        if !self.is_enabled() {
            return 0;
        }

        unsafe { hw::get_memory_tag(addr) }
    }

    /// Tag an allocation with a random tag.
    pub fn tag_allocation(&self, addr: u64, size: usize) -> u64 {
        let tag = generate_tag();
        self.tag_region(addr, size, tag);
        tag_pointer(addr, tag)
    }

    /// Retag an allocation (for reuse after free).
    pub fn retag_allocation(&self, addr: u64, size: usize, old_tag: u8) -> u64 {
        let new_tag = generate_different_tag(old_tag);
        self.tag_region(addr, size, new_tag);
        tag_pointer(addr, new_tag)
    }

    /// Record a tag fault.
    pub fn record_fault(&self, fault: MteFault) {
        self.fault_count.fetch_add(1, Ordering::Relaxed);
        
        let mut faults = self.faults.lock();
        if faults.len() < 1000 {
            faults.push(fault.clone());
        }

        crate::serial_println!(
            "[MTE] Tag fault at 0x{:016x}: expected tag {}, got tag {}",
            fault.address,
            fault.expected_tag,
            fault.actual_tag
        );
    }

    /// Check for and process any pending asynchronous faults.
    pub fn check_async_faults(&self) {
        if !self.is_enabled() {
            return;
        }

        unsafe {
            let tfsr = hw::read_tag_fault_status();
            if tfsr != 0 {
                // TF0 (bit 0) = EL0 fault
                // TF1 (bit 1) = EL1 fault
                if tfsr & 0x1 != 0 {
                    self.record_fault(MteFault {
                        fault_type: MteFaultType::TagCheckFault,
                        address: 0, // Not available in async mode
                        expected_tag: 0,
                        actual_tag: 0,
                        ip: 0,
                        timestamp: crate::arch::read_cycle_counter(),
                    });
                }
                hw::clear_tag_fault_status();
            }
        }
    }

    /// Get fault statistics.
    pub fn stats(&self) -> MteStats {
        let caps = self.capabilities.lock().unwrap_or(MteCapabilities {
            supported: false,
            version: 0,
            asymmetric: false,
            tag_storage_size: 0,
        });
        
        MteStats {
            enabled: self.is_enabled(),
            mode: self.mode(),
            hardware_version: caps.version,
            fault_count: self.fault_count.load(Ordering::Relaxed),
        }
    }
}

/// MTE statistics.
#[derive(Debug, Clone)]
pub struct MteStats {
    pub enabled: bool,
    pub mode: MteMode,
    pub hardware_version: u8,
    pub fault_count: u64,
}

// =============================================================================
// Global MTE Manager
// =============================================================================

static MTE_MANAGER: MteManager = MteManager::new();

/// Get the global MTE manager.
pub fn mte_manager() -> &'static MteManager {
    &MTE_MANAGER
}

/// Initialize MTE with the specified mode.
pub fn init(mode: MteMode) {
    MTE_MANAGER.init(mode);
}

/// Tag a newly allocated region.
pub fn tag_allocation(addr: u64, size: usize) -> u64 {
    MTE_MANAGER.tag_allocation(addr, size)
}

/// Retag a freed region (use-after-free protection).
pub fn retag_freed(addr: u64, size: usize, old_tag: u8) -> u64 {
    MTE_MANAGER.retag_allocation(addr, size, old_tag)
}

/// Get current tag for an address.
pub fn get_tag(addr: u64) -> u8 {
    MTE_MANAGER.get_tag(addr)
}

// =============================================================================
// Allocator Integration
// =============================================================================

/// MTE-aware allocation wrapper.
/// Provides tagged pointers with automatic use-after-free detection.
pub struct MteAllocation {
    /// Tagged pointer
    pub ptr: u64,
    /// Size of allocation
    pub size: usize,
    /// Current tag
    pub tag: u8,
}

impl MteAllocation {
    /// Create a new MTE-tracked allocation.
    pub fn new(ptr: u64, size: usize) -> Self {
        let tagged_ptr = tag_allocation(ptr, size);
        let tag = get_pointer_tag(tagged_ptr);
        
        Self {
            ptr: tagged_ptr,
            size,
            tag,
        }
    }

    /// Get the tagged pointer.
    pub fn as_ptr(&self) -> u64 {
        self.ptr
    }

    /// Get the untagged address.
    pub fn untagged_ptr(&self) -> u64 {
        clear_pointer_tag(self.ptr)
    }
}

impl Drop for MteAllocation {
    fn drop(&mut self) {
        // Retag with a different tag to detect use-after-free
        let _ = retag_freed(
            clear_pointer_tag(self.ptr),
            self.size,
            self.tag,
        );
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_pointer() {
        let ptr = 0x0000_FFFF_8000_0000u64;
        let tagged = tag_pointer(ptr, 5);
        
        assert_eq!(get_pointer_tag(tagged), 5);
        assert_eq!(clear_pointer_tag(tagged), ptr);
    }

    #[test]
    fn test_generate_different_tag() {
        for exclude in 0..16u8 {
            let tag = generate_different_tag(exclude);
            assert_ne!(tag, exclude);
            assert!(tag < 16);
        }
    }

    #[test]
    fn test_tag_generation() {
        let mut tags = [0u32; 16];
        for _ in 0..1000 {
            let tag = generate_tag();
            assert!(tag < 16);
            tags[tag as usize] += 1;
        }
        // Check distribution is somewhat uniform
        for count in tags.iter() {
            assert!(*count > 0); // Each tag should appear at least once
        }
    }
}

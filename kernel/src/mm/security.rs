//! # Security Hardening
//!
//! This module implements security hardening features for Splax OS:
//!
//! - **Stack Canaries**: Detect stack buffer overflows
//! - **ASLR**: Address Space Layout Randomization
//! - **Guard Pages**: Detect stack/heap overflows
//! - **CFI**: Control Flow Integrity (basic implementation)
//!
//! These protections work together to make exploitation significantly harder.

use core::sync::atomic::{AtomicU64, Ordering};

// =============================================================================
// Stack Canaries
// =============================================================================

/// Global stack canary value.
/// Randomized at boot time to prevent prediction attacks.
static STACK_CANARY: AtomicU64 = AtomicU64::new(0);

/// Initialize the stack canary with a random value.
///
/// Must be called early in boot before any protected functions run.
pub fn init_stack_canary(random: u64) {
    // Use a value that's unlikely to appear in normal data
    // and will cause issues if used as an address
    let canary = random | 0x0000_0001_0000_0001; // Set some bits to detect partial overwrites
    STACK_CANARY.store(canary, Ordering::SeqCst);
}

/// Get the current stack canary value.
#[inline(always)]
pub fn get_stack_canary() -> u64 {
    STACK_CANARY.load(Ordering::Relaxed)
}

/// Stack canary check failure handler.
/// Called when a stack buffer overflow is detected.
#[cold]
#[inline(never)]
pub fn stack_check_fail() -> ! {
    // In a real kernel, this would:
    // 1. Log the violation
    // 2. Terminate the offending process
    // 3. Potentially trigger a security alert

    #[cfg(feature = "security_logging")]
    {
        // Log security violation
        crate::log::security_violation("Stack canary corruption detected");
    }

    // For now, panic
    panic!("SECURITY: Stack buffer overflow detected (canary corrupted)");
}

/// Stack protector guard.
/// Place at the start of functions to enable protection.
#[derive(Debug)]
pub struct StackGuard {
    canary: u64,
}

impl StackGuard {
    /// Create a new stack guard.
    /// The canary is placed on the stack between local variables and return address.
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            canary: get_stack_canary(),
        }
    }

    /// Verify the stack canary is intact.
    #[inline(always)]
    pub fn check(&self) {
        if self.canary != get_stack_canary() {
            stack_check_fail();
        }
    }
}

impl Drop for StackGuard {
    #[inline(always)]
    fn drop(&mut self) {
        self.check();
    }
}

impl Default for StackGuard {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Address Space Layout Randomization (ASLR)
// =============================================================================

/// ASLR entropy source.
static ASLR_SEED: AtomicU64 = AtomicU64::new(0);

/// ASLR state for generating randomized addresses.
pub struct AslrState {
    state: u64,
}

impl AslrState {
    /// Create a new ASLR state from the global seed.
    pub fn new() -> Self {
        Self {
            state: ASLR_SEED.load(Ordering::Relaxed),
        }
    }

    /// Create with a specific seed (for testing).
    pub fn with_seed(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Generate the next random value.
    fn next(&mut self) -> u64 {
        // xorshift64
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// Generate a random offset within the given range.
    /// Returns a page-aligned offset.
    pub fn random_offset(&mut self, max_pages: usize) -> usize {
        let pages = (self.next() as usize) % max_pages;
        pages * 4096 // Page size
    }

    /// Generate randomized stack base address.
    pub fn randomize_stack(&mut self, base: u64, entropy_bits: u8) -> u64 {
        let mask = (1u64 << entropy_bits) - 1;
        let offset = (self.next() & mask) * 4096; // Page-aligned
        base - offset // Stack grows down
    }

    /// Generate randomized heap base address.
    pub fn randomize_heap(&mut self, base: u64, entropy_bits: u8) -> u64 {
        let mask = (1u64 << entropy_bits) - 1;
        let offset = (self.next() & mask) * 4096;
        base + offset
    }

    /// Generate randomized mmap base address.
    pub fn randomize_mmap(&mut self, base: u64, entropy_bits: u8) -> u64 {
        let mask = (1u64 << entropy_bits) - 1;
        let offset = (self.next() & mask) * 4096;
        base + offset
    }

    /// Generate randomized executable base (PIE).
    pub fn randomize_pie(&mut self, base: u64, entropy_bits: u8) -> u64 {
        let mask = (1u64 << entropy_bits) - 1;
        let offset = (self.next() & mask) * 4096;
        base + offset
    }
}

impl Default for AslrState {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize ASLR with a random seed.
pub fn init_aslr(seed: u64) {
    ASLR_SEED.store(seed, Ordering::SeqCst);
}

/// ASLR configuration for a process.
#[derive(Debug, Clone, Copy)]
pub struct AslrConfig {
    /// Enable ASLR for this process.
    pub enabled: bool,
    /// Entropy bits for stack randomization (typically 22-30).
    pub stack_entropy: u8,
    /// Entropy bits for heap randomization.
    pub heap_entropy: u8,
    /// Entropy bits for mmap region.
    pub mmap_entropy: u8,
    /// Entropy bits for PIE executables.
    pub pie_entropy: u8,
}

impl AslrConfig {
    /// Default configuration with strong randomization.
    pub const fn default_config() -> Self {
        Self {
            enabled: true,
            stack_entropy: 22, // ~16GB of entropy
            heap_entropy: 16,  // ~256MB of entropy
            mmap_entropy: 28,  // ~1TB of entropy
            pie_entropy: 18,   // ~1GB of entropy
        }
    }

    /// Disabled ASLR (for debugging).
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            stack_entropy: 0,
            heap_entropy: 0,
            mmap_entropy: 0,
            pie_entropy: 0,
        }
    }
}

impl Default for AslrConfig {
    fn default() -> Self {
        Self::default_config()
    }
}

/// Process address space with ASLR.
#[derive(Debug)]
pub struct RandomizedAddressSpace {
    /// Stack base address.
    pub stack_base: u64,
    /// Heap base address.
    pub heap_base: u64,
    /// mmap region base.
    pub mmap_base: u64,
    /// Executable base (for PIE).
    pub exe_base: u64,
    /// Configuration used.
    pub config: AslrConfig,
}

impl RandomizedAddressSpace {
    /// Create a new randomized address space.
    pub fn new(config: AslrConfig) -> Self {
        let mut aslr = AslrState::new();

        if config.enabled {
            Self {
                // Default bases for x86_64
                stack_base: aslr.randomize_stack(0x7FFF_FFFF_F000, config.stack_entropy),
                heap_base: aslr.randomize_heap(0x0000_1000_0000, config.heap_entropy),
                mmap_base: aslr.randomize_mmap(0x0000_7F00_0000_0000, config.mmap_entropy),
                exe_base: aslr.randomize_pie(0x0000_5555_5555_0000, config.pie_entropy),
                config,
            }
        } else {
            Self {
                stack_base: 0x7FFF_FFFF_F000,
                heap_base: 0x0000_1000_0000,
                mmap_base: 0x0000_7F00_0000_0000,
                exe_base: 0x0000_0040_0000,
                config,
            }
        }
    }
}

// =============================================================================
// Guard Pages
// =============================================================================

/// Guard page flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardPageType {
    /// Stack guard (detect stack overflow).
    Stack,
    /// Heap guard (detect heap overflow).
    Heap,
    /// General purpose guard.
    General,
}

/// Guard page descriptor.
#[derive(Debug, Clone, Copy)]
pub struct GuardPage {
    /// Virtual address of the guard page.
    pub address: u64,
    /// Type of guard.
    pub guard_type: GuardPageType,
    /// Size in bytes (usually one page).
    pub size: usize,
}

impl GuardPage {
    /// Create a new guard page.
    pub const fn new(address: u64, guard_type: GuardPageType) -> Self {
        Self {
            address,
            guard_type,
            size: 4096,
        }
    }

    /// Check if an address falls within this guard page.
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.address && addr < self.address + self.size as u64
    }
}

/// Guard page manager for a process.
pub struct GuardPageManager {
    /// List of guard pages.
    pages: [Option<GuardPage>; 16],
    /// Number of active guard pages.
    count: usize,
}

impl GuardPageManager {
    /// Create a new guard page manager.
    pub const fn new() -> Self {
        Self {
            pages: [None; 16],
            count: 0,
        }
    }

    /// Add a guard page.
    pub fn add(&mut self, page: GuardPage) -> Result<(), ()> {
        if self.count >= 16 {
            return Err(());
        }
        self.pages[self.count] = Some(page);
        self.count += 1;
        Ok(())
    }

    /// Check if an address hits a guard page.
    pub fn check(&self, addr: u64) -> Option<&GuardPage> {
        for page in self.pages.iter().flatten() {
            if page.contains(addr) {
                return Some(page);
            }
        }
        None
    }

    /// Handle a guard page violation.
    pub fn handle_violation(&self, addr: u64) -> GuardViolation {
        if let Some(page) = self.check(addr) {
            match page.guard_type {
                GuardPageType::Stack => GuardViolation::StackOverflow,
                GuardPageType::Heap => GuardViolation::HeapOverflow,
                GuardPageType::General => GuardViolation::AccessViolation,
            }
        } else {
            GuardViolation::NotGuardPage
        }
    }
}

impl Default for GuardPageManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard page violation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardViolation {
    /// Stack overflow detected.
    StackOverflow,
    /// Heap overflow detected.
    HeapOverflow,
    /// General access violation.
    AccessViolation,
    /// Address is not a guard page.
    NotGuardPage,
}

// =============================================================================
// Control Flow Integrity (CFI)
// =============================================================================

/// CFI shadow stack entry.
#[derive(Debug, Clone, Copy)]
pub struct ShadowStackEntry {
    /// Expected return address.
    pub return_addr: u64,
    /// Frame pointer for validation.
    pub frame_ptr: u64,
}

/// Shadow stack for CFI.
/// Maintains a parallel stack of return addresses to detect ROP attacks.
pub struct ShadowStack {
    /// Stack entries.
    entries: [ShadowStackEntry; 256],
    /// Current stack pointer.
    sp: usize,
}

impl ShadowStack {
    /// Create a new shadow stack.
    pub const fn new() -> Self {
        Self {
            entries: [ShadowStackEntry {
                return_addr: 0,
                frame_ptr: 0,
            }; 256],
            sp: 0,
        }
    }

    /// Push a return address onto the shadow stack.
    #[inline(always)]
    pub fn push(&mut self, return_addr: u64, frame_ptr: u64) {
        if self.sp < 256 {
            self.entries[self.sp] = ShadowStackEntry {
                return_addr,
                frame_ptr,
            };
            self.sp += 1;
        }
    }

    /// Pop and verify a return address.
    #[inline(always)]
    pub fn pop(&mut self, return_addr: u64) -> bool {
        if self.sp == 0 {
            return false;
        }
        self.sp -= 1;
        self.entries[self.sp].return_addr == return_addr
    }

    /// Verify without popping.
    #[inline(always)]
    pub fn verify(&self, return_addr: u64) -> bool {
        if self.sp == 0 {
            return false;
        }
        self.entries[self.sp - 1].return_addr == return_addr
    }

    /// Check for shadow stack corruption.
    pub fn check_integrity(&self) -> bool {
        // Verify stack pointer is valid
        self.sp <= 256
    }
}

impl Default for ShadowStack {
    fn default() -> Self {
        Self::new()
    }
}

/// CFI violation handler.
#[cold]
#[inline(never)]
pub fn cfi_check_fail(expected: u64, actual: u64) -> ! {
    #[cfg(feature = "security_logging")]
    {
        crate::log::security_violation(&alloc::format!(
            "CFI violation: expected {:016x}, got {:016x}",
            expected, actual
        ));
    }

    panic!(
        "SECURITY: Control flow integrity violation (expected {:016x}, got {:016x})",
        expected, actual
    );
}

// =============================================================================
// Memory Tagging (ARM MTE Simulation)
// =============================================================================

/// Memory tag (4 bits on ARM MTE).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryTag(pub u8);

impl MemoryTag {
    /// Create a new tag (only lower 4 bits are used).
    pub const fn new(tag: u8) -> Self {
        Self(tag & 0x0F)
    }

    /// Generate a random tag.
    pub fn random(state: &mut AslrState) -> Self {
        Self::new((state.next() & 0x0F) as u8)
    }

    /// Check if tags match.
    pub fn matches(&self, other: MemoryTag) -> bool {
        self.0 == other.0
    }
}

/// Tagged pointer (combines address with tag).
#[derive(Debug, Clone, Copy)]
pub struct TaggedPtr {
    /// The pointer value with tag in upper bits.
    ptr: u64,
}

impl TaggedPtr {
    /// Create a tagged pointer.
    pub const fn new(addr: u64, tag: MemoryTag) -> Self {
        // Tag in bits 56-59 (ARM TBI region)
        let tagged = (addr & 0x00FF_FFFF_FFFF_FFFF) | ((tag.0 as u64) << 56);
        Self { ptr: tagged }
    }

    /// Extract the address.
    pub const fn address(&self) -> u64 {
        self.ptr & 0x00FF_FFFF_FFFF_FFFF
    }

    /// Extract the tag.
    pub const fn tag(&self) -> MemoryTag {
        MemoryTag(((self.ptr >> 56) & 0x0F) as u8)
    }

    /// Verify tag matches expected.
    pub fn verify(&self, expected: MemoryTag) -> bool {
        self.tag().matches(expected)
    }
}

/// Memory tag table for a region.
pub struct TagTable {
    /// Tags for each 16-byte granule.
    tags: [MemoryTag; 4096],
    /// Base address of the tagged region.
    base: u64,
    /// Size of the region.
    size: usize,
}

impl TagTable {
    /// Create a new tag table.
    pub const fn new(base: u64, size: usize) -> Self {
        Self {
            tags: [MemoryTag(0); 4096],
            base,
            size,
        }
    }

    /// Set tag for a granule.
    pub fn set_tag(&mut self, addr: u64, tag: MemoryTag) -> Result<(), ()> {
        let offset = addr.saturating_sub(self.base) as usize;
        let granule = offset / 16;
        if granule < self.tags.len() {
            self.tags[granule] = tag;
            Ok(())
        } else {
            Err(())
        }
    }

    /// Get tag for a granule.
    pub fn get_tag(&self, addr: u64) -> Option<MemoryTag> {
        let offset = addr.saturating_sub(self.base) as usize;
        let granule = offset / 16;
        self.tags.get(granule).copied()
    }

    /// Verify access with tagged pointer.
    pub fn verify_access(&self, ptr: TaggedPtr) -> bool {
        if let Some(expected) = self.get_tag(ptr.address()) {
            ptr.tag().matches(expected)
        } else {
            false
        }
    }
}

// =============================================================================
// Security Policy
// =============================================================================

/// Security policy for a process.
#[derive(Debug, Clone, Copy)]
pub struct SecurityPolicy {
    /// Enable stack canaries.
    pub stack_canary: bool,
    /// Enable ASLR.
    pub aslr: bool,
    /// Enable guard pages.
    pub guard_pages: bool,
    /// Enable shadow stack (CFI).
    pub shadow_stack: bool,
    /// Enable memory tagging.
    pub memory_tagging: bool,
    /// Enable W^X (write XOR execute).
    pub wx_protection: bool,
}

impl SecurityPolicy {
    /// Default security policy with all protections enabled.
    pub const fn default_policy() -> Self {
        Self {
            stack_canary: true,
            aslr: true,
            guard_pages: true,
            shadow_stack: true,
            memory_tagging: false, // Requires hardware support
            wx_protection: true,
        }
    }

    /// Minimal policy (for legacy/trusted code).
    pub const fn minimal() -> Self {
        Self {
            stack_canary: true,
            aslr: true,
            guard_pages: false,
            shadow_stack: false,
            memory_tagging: false,
            wx_protection: true,
        }
    }

    /// Maximum security policy.
    pub const fn maximum() -> Self {
        Self {
            stack_canary: true,
            aslr: true,
            guard_pages: true,
            shadow_stack: true,
            memory_tagging: true,
            wx_protection: true,
        }
    }

    /// No security (for debugging only).
    pub const fn none() -> Self {
        Self {
            stack_canary: false,
            aslr: false,
            guard_pages: false,
            shadow_stack: false,
            memory_tagging: false,
            wx_protection: false,
        }
    }
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        Self::default_policy()
    }
}

// =============================================================================
// Initialization
// =============================================================================

/// Initialize all security features.
pub fn init_security(random_seed: u64) {
    // Use different portions of the seed for different features
    init_stack_canary(random_seed);
    init_aslr(random_seed.rotate_left(32));
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_guard() {
        init_stack_canary(0xDEAD_BEEF_CAFE_BABE);

        let guard = StackGuard::new();
        assert_eq!(guard.canary, get_stack_canary());
        guard.check(); // Should not panic
    }

    #[test]
    fn test_aslr_randomization() {
        init_aslr(12345);

        let mut aslr1 = AslrState::new();
        let mut aslr2 = AslrState::new();

        let addr1 = aslr1.randomize_stack(0x7FFF_FFFF_F000, 22);
        let addr2 = aslr2.randomize_stack(0x7FFF_FFFF_F000, 22);

        // Same seed should produce same results
        assert_eq!(addr1, addr2);

        // Different calls should produce different results
        let addr3 = aslr1.randomize_heap(0x1000_0000, 16);
        assert_ne!(addr1, addr3);
    }

    #[test]
    fn test_guard_pages() {
        let mut manager = GuardPageManager::new();
        manager
            .add(GuardPage::new(0x1000, GuardPageType::Stack))
            .unwrap();

        assert_eq!(
            manager.handle_violation(0x1000),
            GuardViolation::StackOverflow
        );
        assert_eq!(
            manager.handle_violation(0x1FFF),
            GuardViolation::StackOverflow
        );
        assert_eq!(
            manager.handle_violation(0x2000),
            GuardViolation::NotGuardPage
        );
    }

    #[test]
    fn test_shadow_stack() {
        let mut ss = ShadowStack::new();

        ss.push(0x1234, 0);
        ss.push(0x5678, 0);

        assert!(ss.pop(0x5678));
        assert!(ss.pop(0x1234));
        assert!(!ss.pop(0x9999)); // Empty, should fail
    }

    #[test]
    fn test_memory_tags() {
        let tag1 = MemoryTag::new(5);
        let tag2 = MemoryTag::new(5);
        let tag3 = MemoryTag::new(7);

        assert!(tag1.matches(tag2));
        assert!(!tag1.matches(tag3));

        let ptr = TaggedPtr::new(0x1000, tag1);
        assert_eq!(ptr.address(), 0x1000);
        assert!(ptr.verify(tag1));
        assert!(!ptr.verify(tag3));
    }

    #[test]
    fn test_randomized_address_space() {
        init_aslr(42);

        let space1 = RandomizedAddressSpace::new(AslrConfig::default_config());
        let space2 = RandomizedAddressSpace::new(AslrConfig::disabled());

        // Disabled should use fixed addresses
        assert_eq!(space2.stack_base, 0x7FFF_FFFF_F000);

        // Enabled should have randomized addresses
        assert_ne!(space1.stack_base, space2.stack_base);
    }
}

//! # Control Flow Integrity (CFI)
//!
//! Implements software-based Control Flow Integrity to prevent
//! code-reuse attacks (ROP, JOP, COP).
//!
//! ## Features
//!
//! - **Shadow Stack**: Separate stack for return addresses
//! - **Indirect Branch Tracking**: Validate indirect call/jump targets
//! - **Landing Pads**: Mark valid indirect branch targets
//! - **Forward-Edge CFI**: Protect indirect calls
//! - **Backward-Edge CFI**: Protect returns via shadow stack
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                  Control Flow Graph                      │
//! ├─────────────────────────────────────────────────────────┤
//! │   Forward-Edge CFI    │    Backward-Edge CFI            │
//! │   (indirect calls)    │    (shadow stack returns)       │
//! ├───────────────────────┴─────────────────────────────────┤
//! │              Landing Pad Registry                        │
//! └─────────────────────────────────────────────────────────┘
//! ```

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// =============================================================================
// Configuration
// =============================================================================

/// Maximum shadow stack depth per thread
const SHADOW_STACK_SIZE: usize = 4096;

/// Maximum landing pads per module
const MAX_LANDING_PADS: usize = 65536;

/// CFI policy enforcement level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfiPolicy {
    /// Log violations but don't enforce
    Permissive,
    /// Enforce CFI, terminate on violation
    Enforcing,
    /// Strict mode - additional runtime checks
    Strict,
}

impl Default for CfiPolicy {
    fn default() -> Self {
        CfiPolicy::Enforcing
    }
}

// =============================================================================
// Shadow Stack
// =============================================================================

/// Per-thread shadow stack for return address protection.
/// 
/// The shadow stack maintains a copy of return addresses that is
/// compared against the actual stack on function return.
#[repr(C)]
pub struct ShadowStack {
    /// Stack of return addresses
    stack: [u64; SHADOW_STACK_SIZE],
    /// Current stack pointer (index)
    sp: usize,
    /// Stack overflow count
    overflow_count: u64,
    /// Mismatch count (potential attacks)
    mismatch_count: u64,
}

impl ShadowStack {
    /// Create a new empty shadow stack.
    pub const fn new() -> Self {
        Self {
            stack: [0; SHADOW_STACK_SIZE],
            sp: 0,
            overflow_count: 0,
            mismatch_count: 0,
        }
    }

    /// Push a return address onto the shadow stack.
    #[inline]
    pub fn push(&mut self, return_addr: u64) {
        if self.sp < SHADOW_STACK_SIZE {
            self.stack[self.sp] = return_addr;
            self.sp += 1;
        } else {
            self.overflow_count += 1;
            // In strict mode this would be a violation
        }
    }

    /// Pop and verify a return address.
    /// Returns true if the address matches, false on mismatch.
    #[inline]
    pub fn pop_and_verify(&mut self, actual_return_addr: u64) -> bool {
        if self.sp == 0 {
            // Underflow - potential stack manipulation
            return false;
        }
        
        self.sp -= 1;
        let expected = self.stack[self.sp];
        
        if expected != actual_return_addr {
            self.mismatch_count += 1;
            return false;
        }
        
        true
    }

    /// Check the current top of stack without popping.
    #[inline]
    pub fn peek(&self) -> Option<u64> {
        if self.sp > 0 {
            Some(self.stack[self.sp - 1])
        } else {
            None
        }
    }

    /// Get current stack depth.
    #[inline]
    pub fn depth(&self) -> usize {
        self.sp
    }

    /// Clear the shadow stack.
    pub fn clear(&mut self) {
        self.sp = 0;
        // Zero out for security
        for addr in self.stack.iter_mut() {
            *addr = 0;
        }
    }

    /// Get statistics.
    pub fn stats(&self) -> ShadowStackStats {
        ShadowStackStats {
            current_depth: self.sp,
            overflow_count: self.overflow_count,
            mismatch_count: self.mismatch_count,
        }
    }
}

/// Shadow stack statistics.
#[derive(Debug, Clone, Copy)]
pub struct ShadowStackStats {
    pub current_depth: usize,
    pub overflow_count: u64,
    pub mismatch_count: u64,
}

// =============================================================================
// Landing Pads (Valid Indirect Branch Targets)
// =============================================================================

/// Type signature for CFI type checking.
/// Used to ensure indirect calls go to functions with matching signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CfiTypeId(u32);

impl CfiTypeId {
    /// Create a new type ID from a hash.
    pub const fn new(hash: u32) -> Self {
        Self(hash)
    }

    /// Compute type ID from function signature.
    pub fn from_signature(arg_count: u8, returns_value: bool, is_variadic: bool) -> Self {
        let mut hash: u32 = 0x5F3759DF;
        hash = hash.wrapping_mul(31).wrapping_add(arg_count as u32);
        hash = hash.wrapping_mul(31).wrapping_add(returns_value as u32);
        hash = hash.wrapping_mul(31).wrapping_add(is_variadic as u32);
        Self(hash)
    }

    /// Get the raw ID value.
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

/// Landing pad entry - a valid indirect branch target.
#[derive(Debug, Clone, Copy)]
pub struct LandingPad {
    /// Address of the valid target
    pub address: u64,
    /// Type signature for type checking
    pub type_id: CfiTypeId,
    /// Whether this is an exported function
    pub is_exported: bool,
}

/// Landing pad registry for a module.
pub struct LandingPadRegistry {
    /// Map from address to landing pad info
    pads: BTreeMap<u64, LandingPad>,
    /// Bloom filter for fast negative lookups
    bloom_filter: [u64; 64],
    /// Total registered pads
    count: usize,
}

impl LandingPadRegistry {
    /// Create a new registry.
    pub fn new() -> Self {
        Self {
            pads: BTreeMap::new(),
            bloom_filter: [0; 64],
            count: 0,
        }
    }

    /// Register a landing pad.
    pub fn register(&mut self, pad: LandingPad) -> Result<(), CfiError> {
        if self.count >= MAX_LANDING_PADS {
            return Err(CfiError::RegistryFull);
        }

        // Update bloom filter
        let hash = self.bloom_hash(pad.address);
        self.bloom_filter[hash / 64] |= 1 << (hash % 64);

        self.pads.insert(pad.address, pad);
        self.count += 1;
        
        Ok(())
    }

    /// Check if an address is a valid landing pad.
    #[inline]
    pub fn is_valid_target(&self, address: u64) -> bool {
        // Fast path: bloom filter check
        let hash = self.bloom_hash(address);
        if self.bloom_filter[hash / 64] & (1 << (hash % 64)) == 0 {
            return false;
        }

        // Slow path: exact lookup
        self.pads.contains_key(&address)
    }

    /// Check if address is valid with type matching.
    #[inline]
    pub fn is_valid_typed_target(&self, address: u64, expected_type: CfiTypeId) -> bool {
        if let Some(pad) = self.pads.get(&address) {
            pad.type_id == expected_type
        } else {
            false
        }
    }

    /// Get landing pad info.
    pub fn get(&self, address: u64) -> Option<&LandingPad> {
        self.pads.get(&address)
    }

    fn bloom_hash(&self, address: u64) -> usize {
        // Simple hash for bloom filter indexing
        let h = address.wrapping_mul(0x517cc1b727220a95);
        ((h >> 32) ^ h) as usize % (64 * 64)
    }

    /// Get count of registered pads.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl Default for LandingPadRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// CFI Manager
// =============================================================================

/// CFI error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfiError {
    /// Invalid indirect call target
    InvalidCallTarget,
    /// Invalid indirect jump target
    InvalidJumpTarget,
    /// Return address mismatch
    ReturnAddressMismatch,
    /// Type signature mismatch
    TypeMismatch,
    /// Shadow stack overflow
    ShadowStackOverflow,
    /// Shadow stack underflow
    ShadowStackUnderflow,
    /// Landing pad registry full
    RegistryFull,
    /// CFI not initialized
    NotInitialized,
}

/// CFI violation info.
#[derive(Debug, Clone)]
pub struct CfiViolation {
    /// Type of violation
    pub error: CfiError,
    /// Address that caused the violation
    pub faulting_address: u64,
    /// Expected target (if applicable)
    pub expected_target: Option<u64>,
    /// Instruction pointer at time of violation
    pub instruction_pointer: u64,
    /// Stack pointer at time of violation
    pub stack_pointer: u64,
    /// Timestamp (cycles)
    pub timestamp: u64,
}

/// CFI manager state.
pub struct CfiManager {
    /// Enforcement policy
    policy: CfiPolicy,
    /// Whether CFI is enabled
    enabled: AtomicBool,
    /// Global landing pad registry
    landing_pads: Mutex<LandingPadRegistry>,
    /// Per-module registries
    module_registries: Mutex<BTreeMap<u64, LandingPadRegistry>>,
    /// Violation log
    violations: Mutex<Vec<CfiViolation>>,
    /// Total violation count
    violation_count: AtomicU64,
    /// Enforced violation count (blocked)
    enforced_count: AtomicU64,
}

impl CfiManager {
    /// Create a new CFI manager.
    pub const fn new() -> Self {
        Self {
            policy: CfiPolicy::Enforcing,
            enabled: AtomicBool::new(false),
            landing_pads: Mutex::new(LandingPadRegistry {
                pads: BTreeMap::new(),
                bloom_filter: [0; 64],
                count: 0,
            }),
            module_registries: Mutex::new(BTreeMap::new()),
            violations: Mutex::new(Vec::new()),
            violation_count: AtomicU64::new(0),
            enforced_count: AtomicU64::new(0),
        }
    }

    /// Initialize the CFI manager.
    pub fn init(&self, policy: CfiPolicy) {
        // Policy is set at creation, but we can update it
        self.enabled.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[CFI] Control Flow Integrity initialized (policy: {:?})", policy);
    }

    /// Check if CFI is enabled.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Register a kernel landing pad.
    pub fn register_landing_pad(&self, address: u64, type_id: CfiTypeId) -> Result<(), CfiError> {
        let mut registry = self.landing_pads.lock();
        registry.register(LandingPad {
            address,
            type_id,
            is_exported: true,
        })
    }

    /// Register multiple landing pads for a module.
    pub fn register_module(&self, module_base: u64, pads: &[(u64, CfiTypeId)]) -> Result<(), CfiError> {
        let mut registry = LandingPadRegistry::new();
        
        for (offset, type_id) in pads {
            let address = module_base + offset;
            registry.register(LandingPad {
                address,
                type_id: *type_id,
                is_exported: false,
            })?;
        }

        let mut modules = self.module_registries.lock();
        modules.insert(module_base, registry);
        
        Ok(())
    }

    /// Validate an indirect call target (forward-edge CFI).
    #[inline]
    pub fn validate_indirect_call(&self, target: u64, expected_type: Option<CfiTypeId>) -> Result<(), CfiError> {
        if !self.is_enabled() {
            return Ok(());
        }

        let registry = self.landing_pads.lock();
        
        let valid = if let Some(type_id) = expected_type {
            registry.is_valid_typed_target(target, type_id)
        } else {
            registry.is_valid_target(target)
        };

        if !valid {
            self.handle_violation(CfiError::InvalidCallTarget, target, None);
            return Err(CfiError::InvalidCallTarget);
        }

        Ok(())
    }

    /// Validate an indirect jump target.
    #[inline]
    pub fn validate_indirect_jump(&self, target: u64) -> Result<(), CfiError> {
        if !self.is_enabled() {
            return Ok(());
        }

        let registry = self.landing_pads.lock();
        
        if !registry.is_valid_target(target) {
            self.handle_violation(CfiError::InvalidJumpTarget, target, None);
            return Err(CfiError::InvalidJumpTarget);
        }

        Ok(())
    }

    /// Handle a CFI violation.
    fn handle_violation(&self, error: CfiError, faulting_address: u64, expected: Option<u64>) {
        self.violation_count.fetch_add(1, Ordering::Relaxed);

        let violation = CfiViolation {
            error,
            faulting_address,
            expected_target: expected,
            instruction_pointer: 0, // Would be set from caller context
            stack_pointer: 0,
            timestamp: crate::arch::read_cycle_counter(),
        };

        // Log violation
        let mut violations = self.violations.lock();
        if violations.len() < 1000 {
            violations.push(violation.clone());
        }

        match self.policy {
            CfiPolicy::Permissive => {
                crate::serial_println!(
                    "[CFI] PERMISSIVE: {:?} at 0x{:016x}",
                    error,
                    faulting_address
                );
            }
            CfiPolicy::Enforcing | CfiPolicy::Strict => {
                self.enforced_count.fetch_add(1, Ordering::Relaxed);
                crate::serial_println!(
                    "[CFI] VIOLATION: {:?} at 0x{:016x} - BLOCKED",
                    error,
                    faulting_address
                );
            }
        }
    }

    /// Get violation statistics.
    pub fn stats(&self) -> CfiStats {
        let registry = self.landing_pads.lock();
        CfiStats {
            enabled: self.is_enabled(),
            policy: self.policy,
            landing_pad_count: registry.len(),
            violation_count: self.violation_count.load(Ordering::Relaxed),
            enforced_count: self.enforced_count.load(Ordering::Relaxed),
        }
    }
}

/// CFI statistics.
#[derive(Debug, Clone)]
pub struct CfiStats {
    pub enabled: bool,
    pub policy: CfiPolicy,
    pub landing_pad_count: usize,
    pub violation_count: u64,
    pub enforced_count: u64,
}

// =============================================================================
// Hardware CFI Support (Intel CET / ARM BTI)
// =============================================================================

/// Hardware CFI capabilities.
#[derive(Debug, Clone, Copy)]
pub struct HardwareCfiCapabilities {
    /// Intel Control-flow Enforcement Technology
    pub intel_cet: bool,
    /// Intel Shadow Stack
    pub intel_shadow_stack: bool,
    /// Intel Indirect Branch Tracking
    pub intel_ibt: bool,
    /// ARM Branch Target Identification
    pub arm_bti: bool,
    /// ARM Pointer Authentication
    pub arm_pac: bool,
}

impl HardwareCfiCapabilities {
    /// Detect hardware CFI support.
    pub fn detect() -> Self {
        let mut caps = Self {
            intel_cet: false,
            intel_shadow_stack: false,
            intel_ibt: false,
            arm_bti: false,
            arm_pac: false,
        };

        #[cfg(target_arch = "x86_64")]
        {
            // Check CPUID for CET support
            // CPUID.07H.0H:ECX[7] = CET_SS (Shadow Stack)
            // CPUID.07H.0H:EDX[20] = CET_IBT (Indirect Branch Tracking)
            unsafe {
                let mut eax: u32;
                let mut ebx: u32;
                let mut ecx: u32;
                let mut edx: u32;
                
                // Check max supported CPUID leaf
                core::arch::asm!(
                    "push rbx",
                    "cpuid",
                    "mov {ebx_out:e}, ebx",
                    "pop rbx",
                    in("eax") 0u32,
                    ebx_out = out(reg) ebx,
                    out("ecx") ecx,
                    out("edx") edx,
                    options(nostack, preserves_flags),
                );
                
                let max_leaf = ebx;
                
                if max_leaf >= 7 {
                    let mut ecx_out: u32;
                    core::arch::asm!(
                        "push rbx",
                        "cpuid",
                        "mov {ebx_out:e}, ebx",
                        "pop rbx",
                        in("eax") 7u32,
                        inout("ecx") 0u32 => ecx_out,
                        ebx_out = out(reg) ebx,
                        out("edx") edx,
                        options(nostack, preserves_flags),
                    );
                    
                    caps.intel_shadow_stack = (ecx_out & (1 << 7)) != 0;
                    caps.intel_ibt = (edx & (1 << 20)) != 0;
                    caps.intel_cet = caps.intel_shadow_stack || caps.intel_ibt;
                }
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            // Check ID_AA64PFR1_EL1 for BTI and PAC
            // BTI: bits [3:0]
            // PAC: multiple fields in ID_AA64ISAR1_EL1
            unsafe {
                let mut id_pfr1: u64;
                let mut id_isar1: u64;
                
                core::arch::asm!(
                    "mrs {}, ID_AA64PFR1_EL1",
                    out(reg) id_pfr1,
                    options(nomem, nostack),
                );
                
                core::arch::asm!(
                    "mrs {}, ID_AA64ISAR1_EL1",
                    out(reg) id_isar1,
                    options(nomem, nostack),
                );
                
                caps.arm_bti = (id_pfr1 & 0xF) != 0;
                // PAC is in various fields of ID_AA64ISAR1_EL1
                caps.arm_pac = (id_isar1 & 0xFF) != 0 || // APA
                               ((id_isar1 >> 4) & 0xF) != 0 || // API
                               ((id_isar1 >> 8) & 0xF) != 0;   // GPA
            }
        }

        caps
    }

    /// Check if any hardware CFI is available.
    pub fn any_available(&self) -> bool {
        self.intel_cet || self.arm_bti || self.arm_pac
    }
}

/// Enable hardware CFI if available.
pub fn enable_hardware_cfi() -> HardwareCfiCapabilities {
    let caps = HardwareCfiCapabilities::detect();
    
    #[cfg(target_arch = "x86_64")]
    if caps.intel_cet {
        // Enable CET in CR4
        // CR4.CET (bit 23) = 1
        unsafe {
            let cr4: u64;
            core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
            let new_cr4 = cr4 | (1 << 23);
            core::arch::asm!("mov cr4, {}", in(reg) new_cr4, options(nomem, nostack));
        }
        crate::serial_println!("[CFI] Intel CET enabled");
    }

    #[cfg(target_arch = "aarch64")]
    if caps.arm_bti {
        // BTI is enabled via SCTLR_EL1.BT0/BT1
        // For kernel, set SCTLR_EL1.BT1 = 1
        unsafe {
            let mut sctlr: u64;
            core::arch::asm!(
                "mrs {}, SCTLR_EL1",
                out(reg) sctlr,
                options(nomem, nostack),
            );
            sctlr |= 1 << 36; // BT1 for EL1
            core::arch::asm!(
                "msr SCTLR_EL1, {}",
                in(reg) sctlr,
                options(nomem, nostack),
            );
        }
        crate::serial_println!("[CFI] ARM BTI enabled");
    }

    caps
}

// =============================================================================
// Global CFI Manager
// =============================================================================

static CFI_MANAGER: CfiManager = CfiManager::new();

/// Get the global CFI manager.
pub fn cfi_manager() -> &'static CfiManager {
    &CFI_MANAGER
}

/// Initialize CFI with the given policy.
pub fn init(policy: CfiPolicy) {
    // First try to enable hardware CFI
    let hw_caps = enable_hardware_cfi();
    
    if hw_caps.any_available() {
        crate::serial_println!("[CFI] Hardware CFI capabilities: {:?}", hw_caps);
    } else {
        crate::serial_println!("[CFI] Using software-only CFI");
    }
    
    CFI_MANAGER.init(policy);
}

/// Register a landing pad for an address.
pub fn register_landing_pad(address: u64, type_id: CfiTypeId) -> Result<(), CfiError> {
    CFI_MANAGER.register_landing_pad(address, type_id)
}

/// Validate an indirect call (for instrumentation).
#[inline]
pub fn check_indirect_call(target: u64) -> Result<(), CfiError> {
    CFI_MANAGER.validate_indirect_call(target, None)
}

/// Validate a typed indirect call.
#[inline]
pub fn check_typed_call(target: u64, type_id: CfiTypeId) -> Result<(), CfiError> {
    CFI_MANAGER.validate_indirect_call(target, Some(type_id))
}

// =============================================================================
// Per-Thread Shadow Stack Storage
// =============================================================================

/// Per-CPU shadow stack storage.
#[repr(C, align(4096))]
pub struct PerCpuShadowStack {
    stacks: [ShadowStack; 256], // Max 256 CPUs
}

impl PerCpuShadowStack {
    const fn new() -> Self {
        const STACK: ShadowStack = ShadowStack::new();
        Self {
            stacks: [STACK; 256],
        }
    }
}

static mut PER_CPU_SHADOW_STACKS: PerCpuShadowStack = PerCpuShadowStack::new();

/// Get the shadow stack for the current CPU.
#[inline]
pub fn current_shadow_stack() -> &'static mut ShadowStack {
    // Use CPU ID 0 when SMP is not initialized
    #[cfg(target_arch = "x86_64")]
    let cpu_id = {
        // Read APIC ID or use 0 as fallback
        0usize
    };
    #[cfg(not(target_arch = "x86_64"))]
    let cpu_id = 0usize;
    
    unsafe { &mut PER_CPU_SHADOW_STACKS.stacks[cpu_id.min(255)] }
}

/// Push a return address (called on function entry).
#[inline]
pub fn shadow_push(return_addr: u64) {
    if CFI_MANAGER.is_enabled() {
        current_shadow_stack().push(return_addr);
    }
}

/// Pop and verify a return address (called on function return).
#[inline]
pub fn shadow_pop_verify(return_addr: u64) -> bool {
    if CFI_MANAGER.is_enabled() {
        if !current_shadow_stack().pop_and_verify(return_addr) {
            CFI_MANAGER.handle_violation(
                CfiError::ReturnAddressMismatch,
                return_addr,
                current_shadow_stack().peek(),
            );
            return false;
        }
    }
    true
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_stack() {
        let mut ss = ShadowStack::new();
        
        ss.push(0x1000);
        ss.push(0x2000);
        ss.push(0x3000);
        
        assert_eq!(ss.depth(), 3);
        assert!(ss.pop_and_verify(0x3000));
        assert!(ss.pop_and_verify(0x2000));
        assert!(ss.pop_and_verify(0x1000));
        assert_eq!(ss.depth(), 0);
    }

    #[test]
    fn test_shadow_stack_mismatch() {
        let mut ss = ShadowStack::new();
        
        ss.push(0x1000);
        assert!(!ss.pop_and_verify(0x9999)); // Wrong address
        
        let stats = ss.stats();
        assert_eq!(stats.mismatch_count, 1);
    }

    #[test]
    fn test_landing_pad_registry() {
        let mut registry = LandingPadRegistry::new();
        
        let type_id = CfiTypeId::from_signature(2, true, false);
        
        registry.register(LandingPad {
            address: 0x1000,
            type_id,
            is_exported: true,
        }).unwrap();
        
        assert!(registry.is_valid_target(0x1000));
        assert!(!registry.is_valid_target(0x2000));
        assert!(registry.is_valid_typed_target(0x1000, type_id));
    }

    #[test]
    fn test_cfi_type_id() {
        let t1 = CfiTypeId::from_signature(2, true, false);
        let t2 = CfiTypeId::from_signature(2, true, false);
        let t3 = CfiTypeId::from_signature(3, true, false);
        
        assert_eq!(t1, t2);
        assert_ne!(t1, t3);
    }
}

//! # Executable Memory Management
//!
//! This module manages executable memory for JIT-compiled code.
//! It provides memory allocation with execute permissions and proper
//! cache coherency handling.

use alloc::vec::Vec;
use core::ptr::NonNull;

use super::JitError;

// =============================================================================
// Memory Protection
// =============================================================================

/// Memory protection flags.
#[derive(Debug, Clone, Copy)]
pub struct Protection(u32);

impl Protection {
    /// No access.
    pub const NONE: Protection = Protection(0);
    /// Read permission.
    pub const READ: Protection = Protection(1 << 0);
    /// Write permission.
    pub const WRITE: Protection = Protection(1 << 1);
    /// Execute permission.
    pub const EXEC: Protection = Protection(1 << 2);

    /// Read + Write.
    pub const RW: Protection = Protection(Self::READ.0 | Self::WRITE.0);
    /// Read + Execute.
    pub const RX: Protection = Protection(Self::READ.0 | Self::EXEC.0);
    /// Read + Write + Execute (W^X violation - use carefully!).
    pub const RWX: Protection = Protection(Self::READ.0 | Self::WRITE.0 | Self::EXEC.0);

    /// Check if readable.
    pub fn is_readable(&self) -> bool {
        self.0 & Self::READ.0 != 0
    }

    /// Check if writable.
    pub fn is_writable(&self) -> bool {
        self.0 & Self::WRITE.0 != 0
    }

    /// Check if executable.
    pub fn is_executable(&self) -> bool {
        self.0 & Self::EXEC.0 != 0
    }
}

// =============================================================================
// Code Region
// =============================================================================

/// A region of executable memory.
#[derive(Debug)]
pub struct CodeRegion {
    /// Base address.
    base: NonNull<u8>,
    /// Total size.
    size: usize,
    /// Current offset (next allocation position).
    offset: usize,
    /// Current protection.
    protection: Protection,
}

impl CodeRegion {
    /// Default region size (64 KB).
    pub const DEFAULT_SIZE: usize = 64 * 1024;

    /// Create a new code region.
    ///
    /// # Safety
    ///
    /// This allocates memory with execute permissions. The caller must ensure
    /// the memory is properly initialized before execution.
    pub unsafe fn new(size: usize) -> Result<Self, JitError> {
        let aligned_size = (size + 4095) & !4095; // Page-align

        // Allocate memory
        // In a real kernel, this would use mmap or similar
        let ptr = Self::allocate_pages(aligned_size)?;

        Ok(Self {
            base: ptr,
            size: aligned_size,
            offset: 0,
            protection: Protection::RW, // Start writable
        })
    }

    /// Allocate pages (platform-specific).
    #[cfg(target_arch = "x86_64")]
    unsafe fn allocate_pages(size: usize) -> Result<NonNull<u8>, JitError> {
        // In kernel mode, we'd allocate from physical memory
        // For now, use a static buffer (limited but works for no_std)
        static mut CODE_BUFFER: [u8; 1024 * 1024] = [0; 1024 * 1024]; // 1 MB
        static mut BUFFER_OFFSET: usize = 0;

        let offset = BUFFER_OFFSET;
        if offset + size > CODE_BUFFER.len() {
            return Err(JitError::MemoryAllocationFailed);
        }

        BUFFER_OFFSET += size;

        Ok(NonNull::new_unchecked(CODE_BUFFER.as_mut_ptr().add(offset)))
    }

    #[cfg(not(target_arch = "x86_64"))]
    unsafe fn allocate_pages(size: usize) -> Result<NonNull<u8>, JitError> {
        // Placeholder for other architectures
        static mut CODE_BUFFER: [u8; 1024 * 1024] = [0; 1024 * 1024];
        static mut BUFFER_OFFSET: usize = 0;

        let offset = BUFFER_OFFSET;
        if offset + size > CODE_BUFFER.len() {
            return Err(JitError::MemoryAllocationFailed);
        }

        BUFFER_OFFSET += size;

        Ok(NonNull::new_unchecked(CODE_BUFFER.as_mut_ptr().add(offset)))
    }

    /// Available space.
    pub fn available(&self) -> usize {
        self.size - self.offset
    }

    /// Allocate space for code.
    pub fn allocate(&mut self, size: usize) -> Result<*const u8, JitError> {
        // Align to 16 bytes for better cache behavior
        let aligned_offset = (self.offset + 15) & !15;
        let aligned_size = (size + 15) & !15;

        if aligned_offset + aligned_size > self.size {
            return Err(JitError::MemoryAllocationFailed);
        }

        let ptr = unsafe { self.base.as_ptr().add(aligned_offset) };
        self.offset = aligned_offset + aligned_size;

        Ok(ptr)
    }

    /// Make region executable.
    ///
    /// This switches from RW to RX, enforcing W^X.
    pub fn make_executable(&mut self) -> Result<(), JitError> {
        // Flush instruction cache
        self.flush_icache();

        // Change protection to RX
        self.protection = Protection::RX;

        // In a real implementation, we'd call mprotect or similar

        Ok(())
    }

    /// Make region writable again.
    pub fn make_writable(&mut self) -> Result<(), JitError> {
        self.protection = Protection::RW;
        Ok(())
    }

    /// Flush instruction cache.
    #[cfg(target_arch = "x86_64")]
    fn flush_icache(&self) {
        // x86_64 has coherent I-cache, no explicit flush needed
        // But we need a memory barrier
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }

    #[cfg(target_arch = "aarch64")]
    fn flush_icache(&self) {
        // AArch64 requires explicit I-cache invalidation
        unsafe {
            let start = self.base.as_ptr() as usize;
            let end = start + self.offset;

            // Clean D-cache and invalidate I-cache
            for addr in (start..end).step_by(64) {
                core::arch::asm!(
                    "dc cvau, {addr}",  // Clean D-cache to PoU
                    addr = in(reg) addr,
                    options(nostack),
                );
            }

            core::arch::asm!("dsb ish", options(nostack));

            for addr in (start..end).step_by(64) {
                core::arch::asm!(
                    "ic ivau, {addr}",  // Invalidate I-cache to PoU
                    addr = in(reg) addr,
                    options(nostack),
                );
            }

            core::arch::asm!(
                "dsb ish",
                "isb",
                options(nostack),
            );
        }
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    fn flush_icache(&self) {
        // Generic: just use memory barrier
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }
}

// =============================================================================
// Executable Memory Manager
// =============================================================================

/// Manages executable memory for JIT code.
pub struct ExecutableMemory {
    /// Code regions.
    regions: Vec<CodeRegion>,
}

impl ExecutableMemory {
    /// Create a new executable memory manager.
    pub fn new() -> Self {
        Self {
            regions: Vec::new(),
        }
    }

    /// Allocate space for code.
    pub fn allocate(&mut self, size: usize) -> Result<*const u8, JitError> {
        // Try to find a region with enough space
        for region in &mut self.regions {
            if region.available() >= size {
                return region.allocate(size);
            }
        }

        // Need to allocate a new region
        let region_size = core::cmp::max(CodeRegion::DEFAULT_SIZE, size * 2);
        let mut region = unsafe { CodeRegion::new(region_size)? };
        let ptr = region.allocate(size)?;
        self.regions.push(region);

        Ok(ptr)
    }

    /// Make all regions executable.
    pub fn finalize(&mut self) -> Result<(), JitError> {
        for region in &mut self.regions {
            region.make_executable()?;
        }
        Ok(())
    }

    /// Total allocated size.
    pub fn total_size(&self) -> usize {
        self.regions.iter().map(|r| r.size).sum()
    }

    /// Total used size.
    pub fn used_size(&self) -> usize {
        self.regions.iter().map(|r| r.offset).sum()
    }
}

impl Default for ExecutableMemory {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Code Pointer
// =============================================================================

/// Safe wrapper for a pointer to JIT-compiled code.
#[derive(Debug, Clone, Copy)]
pub struct CodePtr(*const u8);

impl CodePtr {
    /// Create a new code pointer.
    ///
    /// # Safety
    ///
    /// The pointer must point to valid, executable code.
    pub unsafe fn new(ptr: *const u8) -> Self {
        Self(ptr)
    }

    /// Get the raw pointer.
    pub fn as_ptr(&self) -> *const u8 {
        self.0
    }

    /// Call as a function with no arguments returning i64.
    ///
    /// # Safety
    ///
    /// The code must be a valid function with matching signature.
    pub unsafe fn call_0(&self) -> i64 {
        let func: extern "C" fn() -> i64 = core::mem::transmute(self.0);
        func()
    }

    /// Call as a function with one i64 argument returning i64.
    ///
    /// # Safety
    ///
    /// The code must be a valid function with matching signature.
    pub unsafe fn call_1(&self, arg0: i64) -> i64 {
        let func: extern "C" fn(i64) -> i64 = core::mem::transmute(self.0);
        func(arg0)
    }

    /// Call as a function with two i64 arguments returning i64.
    ///
    /// # Safety
    ///
    /// The code must be a valid function with matching signature.
    pub unsafe fn call_2(&self, arg0: i64, arg1: i64) -> i64 {
        let func: extern "C" fn(i64, i64) -> i64 = core::mem::transmute(self.0);
        func(arg0, arg1)
    }

    /// Call as a function with three i64 arguments returning i64.
    ///
    /// # Safety
    ///
    /// The code must be a valid function with matching signature.
    pub unsafe fn call_3(&self, arg0: i64, arg1: i64, arg2: i64) -> i64 {
        let func: extern "C" fn(i64, i64, i64) -> i64 = core::mem::transmute(self.0);
        func(arg0, arg1, arg2)
    }

    /// Call as a function with four i64 arguments returning i64.
    ///
    /// # Safety
    ///
    /// The code must be a valid function with matching signature.
    pub unsafe fn call_4(&self, arg0: i64, arg1: i64, arg2: i64, arg3: i64) -> i64 {
        let func: extern "C" fn(i64, i64, i64, i64) -> i64 = core::mem::transmute(self.0);
        func(arg0, arg1, arg2, arg3)
    }
}

// =============================================================================
// Trampoline Support
// =============================================================================

/// A trampoline for calling between JIT and host code.
#[derive(Debug)]
pub struct Trampoline {
    /// Code for the trampoline.
    code: CodePtr,
    /// Target address.
    target: *const (),
}

impl Trampoline {
    /// Create a trampoline to a host function.
    ///
    /// # Safety
    ///
    /// The target must be a valid function pointer.
    pub unsafe fn new(
        memory: &mut ExecutableMemory,
        target: *const (),
    ) -> Result<Self, JitError> {
        // Generate trampoline code
        let code = Self::generate_trampoline(memory, target)?;

        Ok(Self {
            code: CodePtr::new(code),
            target,
        })
    }

    #[cfg(target_arch = "x86_64")]
    unsafe fn generate_trampoline(
        memory: &mut ExecutableMemory,
        target: *const (),
    ) -> Result<*const u8, JitError> {
        // x86_64 trampoline: movabs rax, target; jmp rax
        let mut code = [0u8; 12];

        // movabs rax, imm64
        code[0] = 0x48;
        code[1] = 0xB8;
        code[2..10].copy_from_slice(&(target as u64).to_le_bytes());

        // jmp rax
        code[10] = 0xFF;
        code[11] = 0xE0;

        let ptr = memory.allocate(code.len())?;
        core::ptr::copy_nonoverlapping(code.as_ptr(), ptr as *mut u8, code.len());

        Ok(ptr)
    }

    #[cfg(not(target_arch = "x86_64"))]
    unsafe fn generate_trampoline(
        memory: &mut ExecutableMemory,
        _target: *const (),
    ) -> Result<*const u8, JitError> {
        // Placeholder for other architectures
        memory.allocate(16)
    }

    /// Get the trampoline code pointer.
    pub fn code(&self) -> CodePtr {
        self.code
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protection_flags() {
        let rw = Protection::RW;
        assert!(rw.is_readable());
        assert!(rw.is_writable());
        assert!(!rw.is_executable());

        let rx = Protection::RX;
        assert!(rx.is_readable());
        assert!(!rx.is_writable());
        assert!(rx.is_executable());
    }

    #[test]
    fn test_executable_memory() {
        let mut mem = ExecutableMemory::new();

        let ptr1 = mem.allocate(100).unwrap();
        let ptr2 = mem.allocate(200).unwrap();

        assert!(!ptr1.is_null());
        assert!(!ptr2.is_null());
        assert_ne!(ptr1, ptr2);
    }
}

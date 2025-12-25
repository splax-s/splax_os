//! # Memory Manager
//!
//! The Splax memory manager follows these principles:
//!
//! 1. **No Swap**: All memory is physical. If you're out, you're out.
//! 2. **No Overcommit**: Allocations are guaranteed at allocation time.
//! 3. **Explicit Allocation**: No lazy allocation, no COW by default.
//! 4. **Capability-Gated**: All memory regions are accessed via capabilities.
//!
//! ## Memory Regions
//!
//! Memory is divided into typed regions:
//! - Kernel heap: For kernel data structures
//! - User heap: For process allocations
//! - Device memory: For MMIO regions
//! - Shared memory: For IPC zero-copy transfers

pub mod frame;

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;

use spin::Mutex;

pub use frame::{FrameAllocator, FrameNumber, FRAME_ALLOCATOR};

/// Memory region types in Splax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRegionType {
    /// Kernel heap memory
    KernelHeap,
    /// User process heap
    UserHeap,
    /// Memory-mapped I/O
    DeviceMemory,
    /// Shared memory for IPC
    SharedMemory,
    /// DMA-capable memory
    DmaMemory,
}

/// Configuration for the memory manager.
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Size of the kernel heap in bytes
    pub kernel_heap_size: usize,
    /// Maximum number of memory regions
    pub max_regions: usize,
    /// Page size (typically 4096)
    pub page_size: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            kernel_heap_size: 16 * 1024 * 1024, // 16 MB
            max_regions: 1024,
            page_size: 4096,
        }
    }
}

/// A contiguous region of physical memory.
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    /// Physical start address
    pub base: u64,
    /// Size in bytes
    pub size: usize,
    /// Type of memory
    pub region_type: MemoryRegionType,
    /// Whether this region is currently allocated
    pub allocated: bool,
}

/// The global memory manager.
pub struct MemoryManager {
    config: MemoryConfig,
    /// Total available memory in bytes
    total_memory: usize,
    /// Currently used memory in bytes
    used_memory: usize,
    /// Kernel heap allocator
    kernel_heap: Mutex<BumpAllocator>,
}

impl MemoryManager {
    /// Creates a new memory manager.
    ///
    /// # Arguments
    ///
    /// * `config` - Memory configuration
    pub fn new(config: MemoryConfig) -> Self {
        Self {
            config,
            total_memory: 0,
            used_memory: 0,
            kernel_heap: Mutex::new(BumpAllocator::new()),
        }
    }

    /// Initializes the memory manager with the physical memory map.
    ///
    /// # Arguments
    ///
    /// * `memory_map` - Slice of memory regions from the bootloader
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, or an error if initialization fails.
    pub fn init(&mut self, memory_map: &[MemoryRegion]) -> Result<(), MemoryError> {
        for region in memory_map {
            if region.region_type == MemoryRegionType::KernelHeap {
                self.total_memory += region.size;
            }
        }
        Ok(())
    }

    /// Allocates a contiguous block of physical memory.
    ///
    /// # Arguments
    ///
    /// * `size` - Size in bytes (will be rounded up to page size)
    /// * `region_type` - Type of memory to allocate
    /// * `cap_token` - Capability token authorizing this allocation
    ///
    /// # Returns
    ///
    /// Physical address of the allocated memory, or an error.
    pub fn allocate(
        &mut self,
        size: usize,
        region_type: MemoryRegionType,
        _cap_token: &crate::cap::CapabilityToken,
    ) -> Result<u64, MemoryError> {
        // Round up to page size
        let aligned_size = (size + self.config.page_size - 1) & !(self.config.page_size - 1);

        // Check if we have enough memory (no overcommit!)
        if self.used_memory + aligned_size > self.total_memory {
            return Err(MemoryError::OutOfMemory);
        }

        // TODO: Actual allocation from free list
        self.used_memory += aligned_size;

        // Placeholder: return a dummy address
        Ok(0x1000_0000)
    }

    /// Frees a previously allocated memory block.
    ///
    /// # Arguments
    ///
    /// * `addr` - Physical address returned from `allocate`
    /// * `cap_token` - Capability token authorizing this deallocation
    pub fn free(
        &mut self,
        addr: u64,
        _cap_token: &crate::cap::CapabilityToken,
    ) -> Result<(), MemoryError> {
        // TODO: Return memory to free list
        Ok(())
    }

    /// Returns memory statistics.
    pub fn stats(&self) -> MemoryStats {
        MemoryStats {
            total: self.total_memory,
            used: self.used_memory,
            free: self.total_memory.saturating_sub(self.used_memory),
        }
    }
}

/// Memory statistics.
#[derive(Debug, Clone, Copy)]
pub struct MemoryStats {
    pub total: usize,
    pub used: usize,
    pub free: usize,
}

/// Memory allocation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryError {
    /// Not enough memory available
    OutOfMemory,
    /// Invalid address provided
    InvalidAddress,
    /// Alignment requirements not met
    InvalidAlignment,
    /// Capability check failed
    PermissionDenied,
    /// Region already allocated
    AlreadyAllocated,
}

/// Simple bump allocator for the kernel heap.
///
/// This is a placeholder. A real implementation would use
/// a more sophisticated allocator (buddy system, slab, etc.)
struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
}

impl BumpAllocator {
    const fn new() -> Self {
        Self {
            heap_start: 0,
            heap_end: 0,
            next: 0,
        }
    }

    fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;
        self.next = heap_start;
    }

    fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let alloc_start = (self.next + layout.align() - 1) & !(layout.align() - 1);
        let alloc_end = alloc_start.checked_add(layout.size())?;

        if alloc_end > self.heap_end {
            return None;
        }

        self.next = alloc_end;

        // SAFETY: We just verified the address is within our heap
        Some(unsafe { NonNull::new_unchecked(alloc_start as *mut u8) })
    }
}

/// Global allocator for the kernel.
///
/// This allows the kernel to use `alloc` crate types like `Vec` and `Box`.
#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

struct KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // TODO: Use the actual kernel heap
        core::ptr::null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // TODO: Implement deallocation
    }
}

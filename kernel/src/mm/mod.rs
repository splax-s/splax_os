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
pub mod security;
pub mod cfi;
pub mod mte;

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use spin::Mutex;

pub use frame::{FrameAllocator, FrameNumber, FRAME_ALLOCATOR, PAGE_SIZE};

/// Kernel heap size: 8 MB (enough for virtio buffers, verification, and data structures)
const KERNEL_HEAP_SIZE: usize = 8 * 1024 * 1024;

/// Static kernel heap memory region
/// This is placed in BSS and will be zero-initialized
#[repr(C, align(4096))]
struct HeapMemory {
    data: UnsafeCell<[u8; KERNEL_HEAP_SIZE]>,
}

unsafe impl Sync for HeapMemory {}

static HEAP_MEMORY: HeapMemory = HeapMemory {
    data: UnsafeCell::new([0u8; KERNEL_HEAP_SIZE]),
};

/// Flag to track if the allocator has been initialized
static ALLOCATOR_INITIALIZED: AtomicBool = AtomicBool::new(false);

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
    /// Memory configuration
    _config: MemoryConfig,
    /// Total available memory in bytes
    total_memory: usize,
    /// Currently used memory in bytes
    used_memory: usize,
}

impl MemoryManager {
    /// Creates a new memory manager.
    ///
    /// # Arguments
    ///
    /// * `config` - Memory configuration
    pub fn new(config: MemoryConfig) -> Self {
        // Initialize the kernel heap
        init_heap();
        
        Self {
            _config: config,
            total_memory: KERNEL_HEAP_SIZE,
            used_memory: 0,
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
        _region_type: MemoryRegionType,
        _cap_token: &crate::cap::CapabilityToken,
    ) -> Result<u64, MemoryError> {
        // Round up to page size (4KB)
        const PAGE_SIZE: usize = 4096;
        let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // Check if we have enough memory (no overcommit!)
        if self.used_memory + aligned_size > self.total_memory {
            return Err(MemoryError::OutOfMemory);
        }

        // Allocate frames from the global frame allocator
        let num_frames = aligned_size / PAGE_SIZE;
        let frame = FRAME_ALLOCATOR.allocate_contiguous(num_frames)
            .map_err(|_| MemoryError::OutOfMemory)?;
        
        self.used_memory += aligned_size;

        Ok(frame.address())
    }

    /// Frees a previously allocated memory block.
    ///
    /// # Arguments
    ///
    /// * `addr` - Physical address returned from `allocate`
    /// * `size` - Size of the allocation in bytes
    /// * `cap_token` - Capability token authorizing this deallocation
    pub fn free(
        &mut self,
        addr: u64,
        size: usize,
        _cap_token: &crate::cap::CapabilityToken,
    ) -> Result<(), MemoryError> {
        if addr == 0 {
            return Err(MemoryError::InvalidAddress);
        }
        
        // Calculate number of frames to free
        let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let num_frames = aligned_size / PAGE_SIZE;
        let frame = FrameNumber::from_address(addr);
        
        // Return frames to the allocator
        FRAME_ALLOCATOR.free_contiguous(frame, num_frames);
        
        // Update accounting
        self.used_memory = self.used_memory.saturating_sub(aligned_size);
        
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
/// This is a simple bump allocator that allocates memory linearly.
/// It cannot free individual allocations, but is fast and simple.
/// For a production kernel, we would use a more sophisticated allocator
/// (buddy system, slab, etc.)
#[allow(dead_code)]
pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: AtomicUsize,
    allocations: AtomicUsize,
}

#[allow(dead_code)]
impl BumpAllocator {
    /// Creates a new uninitialized bump allocator.
    pub const fn new() -> Self {
        Self {
            heap_start: 0,
            heap_end: 0,
            next: AtomicUsize::new(0),
            allocations: AtomicUsize::new(0),
        }
    }

    /// Initializes the allocator with a heap region.
    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;
        self.next.store(heap_start, Ordering::SeqCst);
    }

    /// Allocates memory with the given layout.
    pub fn allocate(&self, layout: Layout) -> Option<NonNull<u8>> {
        let size = layout.size();
        let align = layout.align();

        // Handle zero-sized allocations - return aligned dangling pointer
        // This is the standard pattern for zero-sized types
        if size == 0 {
            // Use alignment as the pointer value - this is always non-null
            // since alignment is always >= 1
            return Some(unsafe { NonNull::new_unchecked(align as *mut u8) });
        }

        // Make sure the heap is initialized
        if self.heap_start == 0 {
            return None;
        }

        loop {
            let current = self.next.load(Ordering::Relaxed);
            
            // Align up
            let alloc_start = (current + align - 1) & !(align - 1);
            let alloc_end = match alloc_start.checked_add(size) {
                Some(end) => end,
                None => return None,
            };

            if alloc_end > self.heap_end {
                return None; // Out of memory
            }

            // Try to atomically update the next pointer
            match self.next.compare_exchange_weak(
                current,
                alloc_end,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.allocations.fetch_add(1, Ordering::Relaxed);
                    // SAFETY: We verified the address is within our heap bounds
                    return NonNull::new(alloc_start as *mut u8);
                }
                Err(_) => continue, // Another thread allocated, retry
            }
        }
    }

    /// Returns memory usage statistics.
    pub fn stats(&self) -> (usize, usize, usize) {
        let used = self.next.load(Ordering::Relaxed).saturating_sub(self.heap_start);
        let total = self.heap_end.saturating_sub(self.heap_start);
        let free = total.saturating_sub(used);
        (used, free, self.allocations.load(Ordering::Relaxed))
    }
}

// ============================================================================
// Free-List Allocator with Coalescing
// ============================================================================

/// Block header for the free-list allocator.
/// 
/// Each block (free or allocated) has this header.
/// For allocated blocks, user data follows immediately after.
/// For free blocks, next/prev pointers are stored in the user data area.
#[repr(C)]
struct BlockHeader {
    /// Size of the block (including header), low bit indicates if allocated
    size_and_flags: usize,
}

impl BlockHeader {
    /// Minimum block size (header + enough space for free list pointers)
    const MIN_BLOCK_SIZE: usize = core::mem::size_of::<BlockHeader>() + 2 * core::mem::size_of::<usize>();
    
    /// Flag indicating block is allocated (stored in low bit of size)
    const ALLOCATED_FLAG: usize = 1;
    
    /// Creates a new block header.
    fn new(size: usize, allocated: bool) -> Self {
        Self {
            size_and_flags: if allocated { size | Self::ALLOCATED_FLAG } else { size & !Self::ALLOCATED_FLAG },
        }
    }
    
    /// Returns the total size of this block (including header).
    fn size(&self) -> usize {
        self.size_and_flags & !Self::ALLOCATED_FLAG
    }
    
    /// Returns true if this block is allocated.
    fn is_allocated(&self) -> bool {
        self.size_and_flags & Self::ALLOCATED_FLAG != 0
    }
    
    /// Sets the allocated flag.
    fn set_allocated(&mut self, allocated: bool) {
        if allocated {
            self.size_and_flags |= Self::ALLOCATED_FLAG;
        } else {
            self.size_and_flags &= !Self::ALLOCATED_FLAG;
        }
    }
    
    /// Sets the size (preserving flags).
    fn set_size(&mut self, size: usize) {
        let allocated = self.is_allocated();
        self.size_and_flags = if allocated { size | Self::ALLOCATED_FLAG } else { size };
    }
    
    /// Returns pointer to the usable data area.
    fn data_ptr(&self) -> *mut u8 {
        unsafe {
            (self as *const Self as *mut u8).add(core::mem::size_of::<BlockHeader>())
        }
    }
    
    /// Returns pointer to the next block in memory.
    fn next_block(&self) -> *mut BlockHeader {
        unsafe {
            (self as *const Self as *mut u8).add(self.size()) as *mut BlockHeader
        }
    }
}

/// Free block node - stored in the data area of free blocks.
#[repr(C)]
struct FreeNode {
    next: *mut FreeNode,
    prev: *mut FreeNode,
}

/// Free-list allocator with first-fit strategy and coalescing.
/// 
/// This allocator maintains a doubly-linked list of free blocks.
/// On deallocation, it coalesces adjacent free blocks to reduce fragmentation.
pub struct FreeListAllocator {
    /// Start of heap memory
    heap_start: usize,
    /// End of heap memory
    heap_end: usize,
    /// Head of free list
    free_list: *mut FreeNode,
    /// Statistics
    total_allocated: usize,
    allocation_count: usize,
    deallocation_count: usize,
}

// SAFETY: FreeListAllocator is only accessed through a Mutex, ensuring exclusive access
unsafe impl Send for FreeListAllocator {}
unsafe impl Sync for FreeListAllocator {}

impl FreeListAllocator {
    /// Creates a new uninitialized allocator.
    pub const fn new() -> Self {
        Self {
            heap_start: 0,
            heap_end: 0,
            free_list: core::ptr::null_mut(),
            total_allocated: 0,
            allocation_count: 0,
            deallocation_count: 0,
        }
    }
    
    /// Initializes the allocator with a heap region.
    pub fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;
        
        // Create one large free block spanning the entire heap
        let header = heap_start as *mut BlockHeader;
        unsafe {
            (*header) = BlockHeader::new(heap_size, false);
            
            // Set up free node in the data area
            let node = (*header).data_ptr() as *mut FreeNode;
            (*node).next = core::ptr::null_mut();
            (*node).prev = core::ptr::null_mut();
            
            self.free_list = node;
        }
    }
    
    /// Allocates memory with the given layout.
    pub fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let size = layout.size();
        let align = layout.align();
        
        // Handle zero-sized allocations
        if size == 0 {
            return Some(unsafe { NonNull::new_unchecked(align as *mut u8) });
        }
        
        // Calculate required block size (header + aligned data)
        let header_size = core::mem::size_of::<BlockHeader>();
        let required_size = header_size + size;
        let required_size = required_size.max(BlockHeader::MIN_BLOCK_SIZE);
        
        // First-fit search through free list
        let mut current = self.free_list;
        while !current.is_null() {
            let header = self.node_to_header(current);
            let block_size = unsafe { (*header).size() };
            
            // Check if this block is large enough
            // Account for alignment padding
            let data_ptr = unsafe { (*header).data_ptr() };
            let aligned_ptr = align_up(data_ptr as usize, align);
            let padding = aligned_ptr - data_ptr as usize;
            let total_needed = required_size + padding;
            
            if block_size >= total_needed {
                // Found a suitable block
                unsafe {
                    // Remove from free list
                    self.remove_from_free_list(current);
                    
                    // Split block if there's enough remaining space
                    let remaining = block_size - total_needed;
                    if remaining >= BlockHeader::MIN_BLOCK_SIZE {
                        // Create new free block from remainder
                        let new_block = (header as *mut u8).add(total_needed) as *mut BlockHeader;
                        (*new_block) = BlockHeader::new(remaining, false);
                        self.add_to_free_list(new_block);
                        
                        // Shrink current block
                        (*header).set_size(total_needed);
                    }
                    
                    // Mark as allocated
                    (*header).set_allocated(true);
                    
                    // Update statistics
                    self.total_allocated += (*header).size();
                    self.allocation_count += 1;
                    
                    return NonNull::new(aligned_ptr as *mut u8);
                }
            }
            
            current = unsafe { (*current).next };
        }
        
        None // Out of memory
    }
    
    /// Deallocates memory.
    pub fn deallocate(&mut self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() || layout.size() == 0 {
            return;
        }
        
        // Find the block header
        let header_size = core::mem::size_of::<BlockHeader>();
        let header = unsafe { ptr.sub(header_size) as *mut BlockHeader };
        
        // Validate the pointer is within our heap
        if (header as usize) < self.heap_start || (header as usize) >= self.heap_end {
            return; // Invalid pointer
        }
        
        let is_allocated = unsafe { (*header).is_allocated() };
        if !is_allocated {
            return; // Double free
        }
        
        let block_size = unsafe { (*header).size() };
        self.total_allocated = self.total_allocated.saturating_sub(block_size);
        self.deallocation_count += 1;
        
        // Mark as free
        unsafe { (*header).set_allocated(false) };
        
        // Try to coalesce with adjacent blocks
        self.coalesce(header);
    }
    
    /// Coalesces a freed block with adjacent free blocks.
    fn coalesce(&mut self, header: *mut BlockHeader) {
        unsafe {
            let mut current_header = header;
            let mut current_size = (*current_header).size();
            
            // Try to coalesce with next block
            let next_header = (*current_header).next_block();
            if (next_header as usize) < self.heap_end && !(*next_header).is_allocated() {
                // Remove next block from free list
                let next_node = (*next_header).data_ptr() as *mut FreeNode;
                self.remove_from_free_list(next_node);
                
                // Merge sizes
                current_size += (*next_header).size();
                (*current_header).set_size(current_size);
            }
            
            // Try to coalesce with previous block
            let prev_header = self.find_prev_block(current_header);
            if !prev_header.is_null() && !(*prev_header).is_allocated() {
                // Remove previous block from free list
                let prev_node = (*prev_header).data_ptr() as *mut FreeNode;
                self.remove_from_free_list(prev_node);
                
                // Merge into previous block
                let merged_size = (*prev_header).size() + current_size;
                (*prev_header).set_size(merged_size);
                current_header = prev_header;
            }
            
            // Add the (potentially merged) block to free list
            self.add_to_free_list(current_header);
        }
    }
    
    /// Finds the block immediately before the given block.
    fn find_prev_block(&self, target: *mut BlockHeader) -> *mut BlockHeader {
        unsafe {
            let mut current = self.heap_start as *mut BlockHeader;
            let mut prev: *mut BlockHeader = core::ptr::null_mut();
            
            while (current as usize) < self.heap_end && current != target {
                prev = current;
                current = (*current).next_block();
            }
            
            if current == target {
                prev
            } else {
                core::ptr::null_mut()
            }
        }
    }
    
    /// Converts a free node pointer to its block header.
    fn node_to_header(&self, node: *mut FreeNode) -> *mut BlockHeader {
        let header_size = core::mem::size_of::<BlockHeader>();
        unsafe { (node as *mut u8).sub(header_size) as *mut BlockHeader }
    }
    
    /// Adds a block to the free list.
    fn add_to_free_list(&mut self, header: *mut BlockHeader) {
        unsafe {
            let node = (*header).data_ptr() as *mut FreeNode;
            (*node).next = self.free_list;
            (*node).prev = core::ptr::null_mut();
            
            if !self.free_list.is_null() {
                (*self.free_list).prev = node;
            }
            
            self.free_list = node;
        }
    }
    
    /// Removes a node from the free list.
    fn remove_from_free_list(&mut self, node: *mut FreeNode) {
        unsafe {
            let prev = (*node).prev;
            let next = (*node).next;
            
            if prev.is_null() {
                self.free_list = next;
            } else {
                (*prev).next = next;
            }
            
            if !next.is_null() {
                (*next).prev = prev;
            }
        }
    }
    
    /// Returns memory usage statistics.
    pub fn stats(&self) -> AllocatorStats {
        AllocatorStats {
            heap_size: self.heap_end.saturating_sub(self.heap_start),
            total_allocated: self.total_allocated,
            allocation_count: self.allocation_count,
            deallocation_count: self.deallocation_count,
            free_blocks: self.count_free_blocks(),
        }
    }
    
    /// Counts the number of free blocks.
    fn count_free_blocks(&self) -> usize {
        let mut count = 0;
        let mut current = self.free_list;
        while !current.is_null() {
            count += 1;
            current = unsafe { (*current).next };
        }
        count
    }
}

/// Allocator statistics.
#[derive(Debug, Clone, Copy)]
pub struct AllocatorStats {
    pub heap_size: usize,
    pub total_allocated: usize,
    pub allocation_count: usize,
    pub deallocation_count: usize,
    pub free_blocks: usize,
}

/// Aligns a value up to the given alignment.
fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// Global free-list allocator instance
static FREE_LIST_ALLOCATOR: Mutex<FreeListAllocator> = Mutex::new(FreeListAllocator::new());

/// Initializes the kernel heap allocator.
/// This must be called early in kernel initialization.
pub fn init_heap() {
    if ALLOCATOR_INITIALIZED.swap(true, Ordering::SeqCst) {
        return; // Already initialized
    }

    let heap_start = HEAP_MEMORY.data.get() as usize;
    let heap_size = KERNEL_HEAP_SIZE;

    let mut allocator = FREE_LIST_ALLOCATOR.lock();
    allocator.init(heap_start, heap_size);
    
    // Log heap initialization if serial is available
    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(
                serial,
                "[mm] Kernel heap initialized: {} KB at {:#x} (free-list allocator)",
                heap_size / 1024,
                heap_start
            );
        }
    }
}

/// Returns current allocator statistics.
pub fn heap_stats() -> AllocatorStats {
    FREE_LIST_ALLOCATOR.lock().stats()
}

/// Global allocator for the kernel.
///
/// This allows the kernel to use `alloc` crate types like `Vec` and `Box`.
#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

struct KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Handle zero-sized allocations immediately without touching the heap
        // This is the standard pattern - return the alignment as a dangling pointer
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }
        
        // Auto-initialize heap on first allocation if not already done
        if !ALLOCATOR_INITIALIZED.load(Ordering::SeqCst) {
            init_heap();
        }

        let mut allocator = FREE_LIST_ALLOCATOR.lock();
        let result = allocator.allocate(layout);
        
        // Debug output for failed allocations
        #[cfg(target_arch = "x86_64")]
        if result.is_none() {
            use core::fmt::Write;
            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                let stats = allocator.stats();
                let _ = writeln!(
                    serial,
                    "[alloc] FAILED: size={}, align={}, allocated={}, free_blocks={}",
                    layout.size(), layout.align(), stats.total_allocated, stats.free_blocks
                );
            }
        }
        
        match result {
            Some(ptr) => ptr.as_ptr(),
            None => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() || layout.size() == 0 {
            return;
        }
        
        let mut allocator = FREE_LIST_ALLOCATOR.lock();
        allocator.deallocate(ptr, layout);
    }
}

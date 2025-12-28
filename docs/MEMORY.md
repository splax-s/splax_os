# Memory Management Documentation

## Overview

Splax OS implements a straightforward, explicit memory management system with no surprises:

- **No Swap**: All memory is physical. If you're out, you're out.
- **No Overcommit**: Allocations are guaranteed at allocation time.
- **Explicit Allocation**: No lazy allocation, no COW by default.
- **Capability-Gated**: All memory regions are accessed via capabilities.

```text
┌─────────────────────────────────────────────────────────────────┐
│                     User Space                                  │
│  (Process heap, stacks, shared memory)                         │
├─────────────────────────────────────────────────────────────────┤
│                     Kernel Heap                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              Free-List Allocator                         │   │
│  │  - First-fit allocation                                 │   │
│  │  - Block coalescing on free                             │   │
│  │  - GlobalAlloc implementation                           │   │
│  └─────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│                   Frame Allocator                               │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                Bitmap Allocator                          │   │
│  │  - 1 bit per 4KB frame                                  │   │
│  │  - Contiguous allocation support                        │   │
│  │  - O(n) search with hints                               │   │
│  └─────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────┤
│                   Physical Memory                               │
│  0x0000_0000 ────────────────────────────── 0xFFFF_FFFF        │
└─────────────────────────────────────────────────────────────────┘
```

---

## Design Principles

### No Swap

Physical memory only. If you allocate 1GB, you get 1GB of RAM, not disk-backed virtual memory. This provides:

- Predictable latency (no page faults to disk)
- Simpler debugging (memory is where you expect it)
- Explicit resource management (you know exactly what you have)

### No Overcommit

Every allocation either succeeds with real memory or fails immediately:

```rust
pub fn allocate(&mut self, size: usize, ...) -> Result<u64, MemoryError> {
    // Check if we have enough memory (no overcommit!)
    if self.used_memory + aligned_size > self.total_memory {
        return Err(MemoryError::OutOfMemory);
    }
    // ...
}
```

### Capability-Gated

All memory operations require capability tokens:

```rust
pub fn allocate(
    &mut self,
    size: usize,
    region_type: MemoryRegionType,
    cap_token: &CapabilityToken,  // Required!
) -> Result<u64, MemoryError>
```

---

## Constants

```rust
/// Page size (4KB) - same across all architectures
pub const PAGE_SIZE: usize = 4096;

/// Maximum physical memory supported (16 GB)
pub const MAX_PHYSICAL_MEMORY: usize = 16 * 1024 * 1024 * 1024;

/// Maximum frames in system
pub const MAX_FRAMES: usize = MAX_PHYSICAL_MEMORY / PAGE_SIZE;  // 4,194,304 frames

/// Kernel heap size (1 MB)
const KERNEL_HEAP_SIZE: usize = 1024 * 1024;
```

---

## Memory Region Types

```rust
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
```

---

## Physical Frame Allocator

The frame allocator manages physical memory at page granularity (4KB frames).

### Frame Number

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameNumber(pub usize);

impl FrameNumber {
    pub const fn new(n: usize) -> Self {
        Self(n)
    }

    /// Returns the physical address of this frame
    pub fn address(&self) -> u64 {
        (self.0 * PAGE_SIZE) as u64
    }

    /// Creates a frame number from a physical address
    pub fn from_address(addr: u64) -> Self {
        Self((addr as usize) / PAGE_SIZE)
    }
}
```

### Bitmap Allocator

```rust
pub struct FrameAllocator {
    /// Bitmap: 1 = free, 0 = used
    bitmap: Mutex<[u64; BITMAP_WORDS]>,
    /// Total frames in the system
    total_frames: AtomicUsize,
    /// Free frames count
    free_frames: AtomicUsize,
    /// Hint for next allocation search start
    next_hint: AtomicUsize,
}
```

The bitmap uses 64-bit words for efficiency:
- Each bit represents one 4KB frame
- `BITMAP_WORDS = MAX_FRAMES / 64 = 65,536` words for 16GB

### Frame Allocator Operations

#### Add Free Region

Called during boot to mark usable memory:

```rust
impl FrameAllocator {
    /// Adds a free memory region
    pub fn add_region(&self, start: u64, size: usize) {
        let start_frame = (start as usize + PAGE_SIZE - 1) / PAGE_SIZE;
        let end_frame = (start as usize + size) / PAGE_SIZE;

        let mut bitmap = self.bitmap.lock();

        for frame in start_frame..end_frame {
            if frame >= MAX_FRAMES { break; }
            let word = frame / 64;
            let bit = frame % 64;
            bitmap[word] |= 1u64 << bit;  // Mark as free
        }

        self.total_frames.fetch_add(count, Ordering::SeqCst);
        self.free_frames.fetch_add(count, Ordering::SeqCst);
    }
}
```

#### Reserve Region

Mark memory as used (for MMIO, kernel, etc.):

```rust
impl FrameAllocator {
    /// Marks a region as used (reserved)
    pub fn reserve_region(&self, start: u64, size: usize) {
        let start_frame = start as usize / PAGE_SIZE;
        let end_frame = (start as usize + size + PAGE_SIZE - 1) / PAGE_SIZE;

        let mut bitmap = self.bitmap.lock();

        for frame in start_frame..end_frame {
            let word = frame / 64;
            let bit = frame % 64;
            bitmap[word] &= !(1u64 << bit);  // Mark as used
        }
    }
}
```

#### Allocate Frames

```rust
impl FrameAllocator {
    /// Allocates a single frame
    pub fn allocate(&self) -> Result<FrameNumber, FrameAllocError> {
        self.allocate_contiguous(1)
    }

    /// Allocates multiple contiguous frames
    pub fn allocate_contiguous(&self, count: usize) -> Result<FrameNumber, FrameAllocError> {
        if count == 0 {
            return Err(FrameAllocError::ZeroFrames);
        }

        let mut bitmap = self.bitmap.lock();
        let hint = self.next_hint.load(Ordering::Relaxed);

        // Search from hint to end
        if let Some(frame) = self.find_contiguous(&bitmap, hint, MAX_FRAMES, count) {
            self.mark_used(&mut bitmap, frame, count);
            self.next_hint.store(frame + count, Ordering::Relaxed);
            self.free_frames.fetch_sub(count, Ordering::SeqCst);
            return Ok(FrameNumber::new(frame));
        }

        // Wrap around and search from start
        if let Some(frame) = self.find_contiguous(&bitmap, 0, hint, count) {
            self.mark_used(&mut bitmap, frame, count);
            self.next_hint.store(frame + count, Ordering::Relaxed);
            self.free_frames.fetch_sub(count, Ordering::SeqCst);
            return Ok(FrameNumber::new(frame));
        }

        Err(FrameAllocError::OutOfMemory)
    }
}
```

#### Free Frames

```rust
impl FrameAllocator {
    /// Frees a single frame
    pub fn free(&self, frame: FrameNumber) {
        self.free_contiguous(frame, 1);
    }

    /// Frees multiple contiguous frames
    pub fn free_contiguous(&self, start: FrameNumber, count: usize) {
        let mut bitmap = self.bitmap.lock();

        for i in 0..count {
            let frame = start.0 + i;
            let word = frame / 64;
            let bit = frame % 64;
            bitmap[word] |= 1u64 << bit;  // Mark as free
        }

        self.free_frames.fetch_add(count, Ordering::SeqCst);
    }
}
```

### Frame Allocation Errors

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAllocError {
    /// No free frames available
    OutOfMemory,
    /// Requested count is zero
    ZeroFrames,
    /// Cannot find contiguous region
    FragmentedMemory,
}
```

### Global Frame Allocator

```rust
pub static FRAME_ALLOCATOR: FrameAllocator = FrameAllocator::new();
```

---

## Kernel Heap

The kernel uses a 1MB static heap with a free-list allocator.

### Heap Memory

```rust
/// Kernel heap size: 1 MB
const KERNEL_HEAP_SIZE: usize = 1024 * 1024;

/// Static kernel heap memory (placed in BSS)
#[repr(C, align(4096))]
struct HeapMemory {
    data: UnsafeCell<[u8; KERNEL_HEAP_SIZE]>,
}

static HEAP_MEMORY: HeapMemory = HeapMemory {
    data: UnsafeCell::new([0u8; KERNEL_HEAP_SIZE]),
};
```

### Block Header

Each allocation has a header:

```rust
#[repr(C)]
struct BlockHeader {
    /// Size of the block (including header)
    /// Low bit indicates if allocated
    size_and_flags: usize,
}

impl BlockHeader {
    /// Minimum block size (header + free list pointers)
    const MIN_BLOCK_SIZE: usize = 
        core::mem::size_of::<BlockHeader>() + 
        2 * core::mem::size_of::<usize>();

    const ALLOCATED_FLAG: usize = 1;

    fn size(&self) -> usize {
        self.size_and_flags & !Self::ALLOCATED_FLAG
    }

    fn is_allocated(&self) -> bool {
        self.size_and_flags & Self::ALLOCATED_FLAG != 0
    }
}
```

### Free-List Allocator

```rust
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
```

#### Allocation (First-Fit)

```rust
impl FreeListAllocator {
    pub fn allocate(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let size = layout.size();
        let align = layout.align();

        // Handle zero-sized allocations
        if size == 0 {
            return Some(unsafe { NonNull::new_unchecked(align as *mut u8) });
        }

        // First-fit search through free list
        let mut current = self.free_list;
        while !current.is_null() {
            let header = self.node_to_header(current);
            let block_size = unsafe { (*header).size() };

            // Account for alignment padding
            let data_ptr = unsafe { (*header).data_ptr() };
            let aligned_ptr = align_up(data_ptr as usize, align);
            let padding = aligned_ptr - data_ptr as usize;
            let total_needed = required_size + padding;

            if block_size >= total_needed {
                // Found a suitable block
                unsafe {
                    self.remove_from_free_list(current);

                    // Split block if there's enough remaining space
                    let remaining = block_size - total_needed;
                    if remaining >= BlockHeader::MIN_BLOCK_SIZE {
                        let new_block = ...;
                        self.add_to_free_list(new_block);
                        (*header).set_size(total_needed);
                    }

                    (*header).set_allocated(true);
                    return NonNull::new(aligned_ptr as *mut u8);
                }
            }

            current = unsafe { (*current).next };
        }

        None // Out of memory
    }
}
```

#### Deallocation with Coalescing

```rust
impl FreeListAllocator {
    pub fn deallocate(&mut self, ptr: *mut u8, layout: Layout) {
        // Find the block header
        let header = unsafe { ptr.sub(header_size) as *mut BlockHeader };

        // Validate pointer is within heap
        // ...

        // Mark as free
        unsafe { (*header).set_allocated(false) };

        // Coalesce with adjacent free blocks
        self.coalesce(header);
    }

    fn coalesce(&mut self, header: *mut BlockHeader) {
        unsafe {
            // Try to coalesce with next block
            let next_header = (*header).next_block();
            if !(*next_header).is_allocated() {
                self.remove_from_free_list(next_node);
                // Merge sizes
            }

            // Try to coalesce with previous block
            let prev_header = self.find_prev_block(header);
            if !prev_header.is_null() && !(*prev_header).is_allocated() {
                self.remove_from_free_list(prev_node);
                // Merge into previous block
            }

            // Add merged block to free list
            self.add_to_free_list(current_header);
        }
    }
}
```

### Global Allocator

The kernel uses `#[global_allocator]` to enable `alloc` crate types:

```rust
#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator;

struct KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Handle zero-sized allocations
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        // Auto-initialize heap on first allocation
        if !ALLOCATOR_INITIALIZED.load(Ordering::SeqCst) {
            init_heap();
        }

        let mut allocator = FREE_LIST_ALLOCATOR.lock();
        match allocator.allocate(layout) {
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
```

---

## Memory Manager

High-level interface for capability-gated allocations:

### Configuration

```rust
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Size of the kernel heap in bytes
    pub kernel_heap_size: usize,     // Default: 16 MB
    /// Maximum number of memory regions
    pub max_regions: usize,           // Default: 1024
    /// Page size (typically 4096)
    pub page_size: usize,             // Default: 4096
}
```

### Memory Manager Structure

```rust
pub struct MemoryManager {
    /// Memory configuration
    config: MemoryConfig,
    /// Total available memory in bytes
    total_memory: usize,
    /// Currently used memory in bytes
    used_memory: usize,
}
```

### Operations

#### Allocate

```rust
impl MemoryManager {
    pub fn allocate(
        &mut self,
        size: usize,
        region_type: MemoryRegionType,
        cap_token: &CapabilityToken,
    ) -> Result<u64, MemoryError> {
        // Round up to page size
        let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

        // No overcommit!
        if self.used_memory + aligned_size > self.total_memory {
            return Err(MemoryError::OutOfMemory);
        }

        // Allocate from frame allocator
        let num_frames = aligned_size / PAGE_SIZE;
        let frame = FRAME_ALLOCATOR.allocate_contiguous(num_frames)?;

        self.used_memory += aligned_size;
        Ok(frame.address())
    }
}
```

#### Free

```rust
impl MemoryManager {
    pub fn free(
        &mut self,
        addr: u64,
        size: usize,
        cap_token: &CapabilityToken,
    ) -> Result<(), MemoryError> {
        if addr == 0 {
            return Err(MemoryError::InvalidAddress);
        }

        let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let num_frames = aligned_size / PAGE_SIZE;
        let frame = FrameNumber::from_address(addr);

        FRAME_ALLOCATOR.free_contiguous(frame, num_frames);
        self.used_memory = self.used_memory.saturating_sub(aligned_size);

        Ok(())
    }
}
```

### Memory Statistics

```rust
#[derive(Debug, Clone, Copy)]
pub struct MemoryStats {
    pub total: usize,
    pub used: usize,
    pub free: usize,
}

impl MemoryManager {
    pub fn stats(&self) -> MemoryStats {
        MemoryStats {
            total: self.total_memory,
            used: self.used_memory,
            free: self.total_memory.saturating_sub(self.used_memory),
        }
    }
}
```

### Memory Errors

```rust
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
```

---

## Allocator Statistics

```rust
#[derive(Debug, Clone, Copy)]
pub struct AllocatorStats {
    pub heap_size: usize,
    pub total_allocated: usize,
    pub allocation_count: usize,
    pub deallocation_count: usize,
    pub free_blocks: usize,
}

/// Returns current heap statistics
pub fn heap_stats() -> AllocatorStats {
    FREE_LIST_ALLOCATOR.lock().stats()
}
```

---

## Initialization

### Heap Initialization

```rust
pub fn init_heap() {
    if ALLOCATOR_INITIALIZED.swap(true, Ordering::SeqCst) {
        return; // Already initialized
    }

    let heap_start = HEAP_MEMORY.data.get() as usize;
    let heap_size = KERNEL_HEAP_SIZE;

    let mut allocator = FREE_LIST_ALLOCATOR.lock();
    allocator.init(heap_start, heap_size);
}
```

### Boot Sequence

```rust
// During kernel init:
// 1. Frame allocator marks usable regions from bootloader memory map
for entry in memory_map {
    if entry.kind == MemoryKind::Usable {
        FRAME_ALLOCATOR.add_region(entry.base, entry.length);
    }
}

// 2. Reserve kernel, MMIO, etc.
FRAME_ALLOCATOR.reserve_region(kernel_start, kernel_size);

// 3. Heap auto-initializes on first allocation
// Or call init_heap() explicitly
```

---

## Shell Commands

```text
mem              - Show memory statistics
mem stats        - Detailed memory breakdown
mem frames       - Frame allocator statistics
mem heap         - Kernel heap statistics
```

---

## Usage Examples

### Allocate Physical Memory

```rust
let cap = CapabilityToken::new([1, 2, 3, 4]);
let addr = memory_manager.allocate(
    4096 * 10,  // 10 pages
    MemoryRegionType::DmaMemory,
    &cap
)?;
```

### Allocate Frames Directly

```rust
// Single frame
let frame = FRAME_ALLOCATOR.allocate()?;
let phys_addr = frame.address();

// Contiguous frames (for DMA)
let frames = FRAME_ALLOCATOR.allocate_contiguous(256)?;  // 1MB
```

### Use Heap (Vec, Box, etc.)

```rust
use alloc::vec::Vec;
use alloc::boxed::Box;

// These use the kernel heap automatically
let mut data: Vec<u8> = Vec::with_capacity(1024);
let boxed = Box::new(SomeStruct::new());
```

---

## File Structure

```text
kernel/src/mm/
├── mod.rs              # MemoryManager, heap allocators, GlobalAlloc
└── frame.rs            # Physical frame allocator (bitmap)
```

---

## Architecture Support

| Feature | x86_64 | aarch64 | riscv64 |
|---------|--------|---------|---------|
| Frame Allocator | ✓ | ✓ | ✓ |
| Kernel Heap | ✓ | ✓ | ✓ |
| Capability-Gated | ✓ | ✓ | ✓ |
| Paging/VMM | Planned | Planned | Planned |

---

## Memory Layout (x86_64)

```text
0x0000_0000_0000_0000 ┬─────────────────────────────────
                      │ Reserved (NULL guard)
0x0000_0000_0010_0000 ├─────────────────────────────────
                      │ Kernel Code/Data
0x0000_0000_0020_0000 ├─────────────────────────────────
                      │ Kernel Heap (1 MB)
0x0000_0000_0030_0000 ├─────────────────────────────────
                      │ Frame Allocator Managed Memory
                      │ (Physical frames)
                      │
0x0000_000X_XXXX_XXXX ├─────────────────────────────────
                      │ Device MMIO Regions
0xFEE0_0000           │ - Local APIC
0xFEC0_0000           │ - I/O APIC
                      │ - PCI BARs
                      │
0xFFFF_FFFF_FFFF_FFFF ┴─────────────────────────────────
```

---

## Future Work

- [ ] Virtual Memory Manager (VMM) with paging
- [ ] Per-process address spaces
- [ ] Slab allocator for kernel objects
- [ ] Buddy allocator for efficient power-of-2 allocations
- [ ] NUMA-aware allocation
- [ ] Memory pressure callbacks
- [ ] Huge page support (2MB, 1GB)
- [ ] Guard pages for stack overflow detection

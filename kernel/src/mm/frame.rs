//! # Physical Frame Allocator
//!
//! Manages physical memory frames. Uses a bitmap to track which frames are
//! free and which are allocated.
//!
//! ## Design
//!
//! - Each frame is PAGE_SIZE (4096) bytes
//! - A bitmap tracks allocation status (1 bit per frame)
//! - Allocation is O(n) worst case, but typically faster due to hints
//! - Supports contiguous multi-frame allocation

use core::sync::atomic::{AtomicUsize, Ordering};

use spin::Mutex;

/// Page size constant (4KB) - same across all architectures we support
pub const PAGE_SIZE: usize = 4096;

/// Maximum physical memory supported (16 GB for now).
pub const MAX_PHYSICAL_MEMORY: usize = 16 * 1024 * 1024 * 1024;

/// Number of frames in maximum memory.
pub const MAX_FRAMES: usize = MAX_PHYSICAL_MEMORY / PAGE_SIZE;

/// Bitmap size in u64 words.
const BITMAP_WORDS: usize = MAX_FRAMES / 64;

/// Physical frame number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameNumber(pub usize);

impl FrameNumber {
    /// Creates a new frame number.
    pub const fn new(n: usize) -> Self {
        Self(n)
    }

    /// Returns the physical address of this frame.
    pub fn address(&self) -> u64 {
        (self.0 * PAGE_SIZE) as u64
    }

    /// Creates a frame number from a physical address.
    pub fn from_address(addr: u64) -> Self {
        Self((addr as usize) / PAGE_SIZE)
    }
}

/// Allocation error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAllocError {
    /// No free frames available
    OutOfMemory,
    /// Requested count is zero
    ZeroFrames,
    /// Cannot find contiguous region
    FragmentedMemory,
}

/// A physical frame allocator using a bitmap.
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

impl FrameAllocator {
    /// Creates a new frame allocator.
    ///
    /// All frames start as used. Call `add_region` to mark free regions.
    pub const fn new() -> Self {
        Self {
            bitmap: Mutex::new([0u64; BITMAP_WORDS]),
            total_frames: AtomicUsize::new(0),
            free_frames: AtomicUsize::new(0),
            next_hint: AtomicUsize::new(0),
        }
    }

    /// Adds a free memory region.
    ///
    /// # Arguments
    ///
    /// * `start` - Start physical address (aligned to PAGE_SIZE)
    /// * `size` - Size in bytes
    pub fn add_region(&self, start: u64, size: usize) {
        let start_frame = (start as usize + PAGE_SIZE - 1) / PAGE_SIZE;
        let end_frame = (start as usize + size) / PAGE_SIZE;

        if end_frame <= start_frame {
            return;
        }

        let count = end_frame - start_frame;
        let mut bitmap = self.bitmap.lock();

        for frame in start_frame..end_frame {
            if frame >= MAX_FRAMES {
                break;
            }
            let word = frame / 64;
            let bit = frame % 64;
            bitmap[word] |= 1u64 << bit;
        }

        self.total_frames.fetch_add(count, Ordering::SeqCst);
        self.free_frames.fetch_add(count, Ordering::SeqCst);
    }

    /// Marks a region as used (reserved).
    ///
    /// # Arguments
    ///
    /// * `start` - Start physical address
    /// * `size` - Size in bytes
    pub fn reserve_region(&self, start: u64, size: usize) {
        let start_frame = start as usize / PAGE_SIZE;
        let end_frame = (start as usize + size + PAGE_SIZE - 1) / PAGE_SIZE;

        let mut bitmap = self.bitmap.lock();
        let mut freed = 0;

        for frame in start_frame..end_frame {
            if frame >= MAX_FRAMES {
                break;
            }
            let word = frame / 64;
            let bit = frame % 64;
            if bitmap[word] & (1u64 << bit) != 0 {
                bitmap[word] &= !(1u64 << bit);
                freed += 1;
            }
        }

        self.free_frames.fetch_sub(freed, Ordering::SeqCst);
    }

    /// Allocates a single frame.
    pub fn allocate(&self) -> Result<FrameNumber, FrameAllocError> {
        self.allocate_contiguous(1).map(|f| f)
    }

    /// Allocates multiple contiguous frames.
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

        // Wrap around and search from start to hint
        if let Some(frame) = self.find_contiguous(&bitmap, 0, hint, count) {
            self.mark_used(&mut bitmap, frame, count);
            self.next_hint.store(frame + count, Ordering::Relaxed);
            self.free_frames.fetch_sub(count, Ordering::SeqCst);
            return Ok(FrameNumber::new(frame));
        }

        Err(FrameAllocError::OutOfMemory)
    }

    /// Finds contiguous free frames.
    fn find_contiguous(
        &self,
        bitmap: &[u64; BITMAP_WORDS],
        start: usize,
        end: usize,
        count: usize,
    ) -> Option<usize> {
        let mut run_start = None;
        let mut run_len = 0;

        for frame in start..end {
            if frame >= MAX_FRAMES {
                break;
            }

            let word = frame / 64;
            let bit = frame % 64;

            if bitmap[word] & (1u64 << bit) != 0 {
                // Frame is free
                if run_start.is_none() {
                    run_start = Some(frame);
                    run_len = 1;
                } else {
                    run_len += 1;
                }

                if run_len >= count {
                    return run_start;
                }
            } else {
                // Frame is used, reset run
                run_start = None;
                run_len = 0;
            }
        }

        None
    }

    /// Marks frames as used.
    fn mark_used(&self, bitmap: &mut [u64; BITMAP_WORDS], start: usize, count: usize) {
        for frame in start..start + count {
            if frame >= MAX_FRAMES {
                break;
            }
            let word = frame / 64;
            let bit = frame % 64;
            bitmap[word] &= !(1u64 << bit);
        }
    }

    /// Frees a single frame.
    pub fn free(&self, frame: FrameNumber) {
        self.free_contiguous(frame, 1);
    }

    /// Frees multiple contiguous frames.
    pub fn free_contiguous(&self, start: FrameNumber, count: usize) {
        let mut bitmap = self.bitmap.lock();

        for i in 0..count {
            let frame = start.0 + i;
            if frame >= MAX_FRAMES {
                break;
            }
            let word = frame / 64;
            let bit = frame % 64;
            bitmap[word] |= 1u64 << bit;
        }

        self.free_frames.fetch_add(count, Ordering::SeqCst);
    }

    /// Returns the number of free frames.
    pub fn free_count(&self) -> usize {
        self.free_frames.load(Ordering::SeqCst)
    }

    /// Returns the total number of frames.
    pub fn total_count(&self) -> usize {
        self.total_frames.load(Ordering::SeqCst)
    }

    /// Returns free memory in bytes.
    pub fn free_memory(&self) -> usize {
        self.free_count() * PAGE_SIZE
    }

    /// Returns total memory in bytes.
    pub fn total_memory(&self) -> usize {
        self.total_count() * PAGE_SIZE
    }
}

/// Global frame allocator.
pub static FRAME_ALLOCATOR: FrameAllocator = FrameAllocator::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_number() {
        let frame = FrameNumber::new(10);
        assert_eq!(frame.address(), 10 * PAGE_SIZE as u64);

        let from_addr = FrameNumber::from_address(0x5000);
        assert_eq!(from_addr.0, 5);
    }
}

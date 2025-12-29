//! # Block I/O (Bio) Layer
//!
//! The Bio layer provides the fundamental abstraction for block I/O operations,
//! similar to Linux's `struct bio`. It handles request splitting, merging,
//! and scatter-gather I/O.
//!
//! ## Design
//!
//! A Bio represents a single I/O operation that may span multiple
//! non-contiguous memory regions (scatter-gather). The bio layer:
//!
//! - Splits large requests into device-sized chunks
//! - Merges adjacent requests when possible
//! - Handles partial completions
//! - Provides async completion callbacks

use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use super::{BlockError, SECTOR_SIZE};

/// Unique bio ID counter
static BIO_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Bio operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BioOp {
    /// Read from device
    Read,
    /// Write to device
    Write,
    /// Flush device cache
    Flush,
    /// Discard/trim sectors
    Discard,
    /// Write zeros (secure erase)
    WriteZeroes,
}

/// Bio status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BioStatus {
    /// Bio is queued for processing
    Pending,
    /// Bio is currently being processed
    Active,
    /// Bio completed successfully
    Complete,
    /// Bio failed with an error
    Error(BlockError),
}

/// A memory segment for scatter-gather I/O
#[derive(Debug, Clone)]
pub struct BioVec {
    /// Physical page address
    pub page: usize,
    /// Offset within the page
    pub offset: usize,
    /// Length in bytes
    pub len: usize,
}

impl BioVec {
    /// Creates a new bio vector
    pub fn new(page: usize, offset: usize, len: usize) -> Self {
        Self { page, offset, len }
    }

    /// Creates a bio vector from a contiguous buffer
    pub fn from_buffer(buffer: &[u8]) -> Self {
        Self {
            page: buffer.as_ptr() as usize & !0xFFF,
            offset: buffer.as_ptr() as usize & 0xFFF,
            len: buffer.len(),
        }
    }

    /// Returns the virtual address
    pub fn addr(&self) -> usize {
        self.page + self.offset
    }
}

/// Completion callback type
pub type BioEndIo = Box<dyn FnOnce(&Bio, BioStatus) + Send + 'static>;

/// Block I/O request
pub struct Bio {
    /// Unique bio ID
    pub id: u64,
    /// Operation type
    pub op: BioOp,
    /// Starting sector on device
    pub sector: u64,
    /// Total size in bytes
    pub size: usize,
    /// Memory segments (scatter-gather list)
    pub bvecs: Vec<BioVec>,
    /// Current segment index
    bvec_idx: usize,
    /// Current offset within segment
    bvec_offset: usize,
    /// Completion callback
    end_io: Option<BioEndIo>,
    /// Bio status
    pub status: BioStatus,
    /// Device name
    pub device: Option<&'static str>,
    /// Private data for driver use
    pub private: usize,
}

impl Bio {
    /// Creates a new bio
    pub fn new(op: BioOp, sector: u64) -> Self {
        Self {
            id: BIO_ID_COUNTER.fetch_add(1, Ordering::Relaxed),
            op,
            sector,
            size: 0,
            bvecs: Vec::new(),
            bvec_idx: 0,
            bvec_offset: 0,
            end_io: None,
            status: BioStatus::Pending,
            device: None,
            private: 0,
        }
    }

    /// Creates a read bio
    pub fn read(sector: u64) -> Self {
        Self::new(BioOp::Read, sector)
    }

    /// Creates a write bio
    pub fn write(sector: u64) -> Self {
        Self::new(BioOp::Write, sector)
    }

    /// Creates a flush bio
    pub fn flush() -> Self {
        Self::new(BioOp::Flush, 0)
    }

    /// Creates a discard bio
    pub fn discard(sector: u64, count: usize) -> Self {
        let mut bio = Self::new(BioOp::Discard, sector);
        bio.size = count * SECTOR_SIZE;
        bio
    }

    /// Adds a memory segment
    pub fn add_page(&mut self, page: usize, offset: usize, len: usize) -> &mut Self {
        self.bvecs.push(BioVec::new(page, offset, len));
        self.size += len;
        self
    }

    /// Adds a contiguous buffer
    pub fn add_buffer(&mut self, buffer: &[u8]) -> &mut Self {
        self.bvecs.push(BioVec::from_buffer(buffer));
        self.size += buffer.len();
        self
    }

    /// Sets the completion callback
    pub fn set_end_io<F>(&mut self, callback: F)
    where
        F: FnOnce(&Bio, BioStatus) + Send + 'static,
    {
        self.end_io = Some(Box::new(callback));
    }

    /// Returns the number of sectors
    pub fn sector_count(&self) -> usize {
        (self.size + SECTOR_SIZE - 1) / SECTOR_SIZE
    }

    /// Returns the ending sector (exclusive)
    pub fn end_sector(&self) -> u64 {
        self.sector + self.sector_count() as u64
    }

    /// Checks if this bio can merge with another
    pub fn can_merge(&self, other: &Bio) -> bool {
        // Must be same operation
        if self.op != other.op {
            return false;
        }

        // Must be adjacent sectors
        if self.end_sector() != other.sector {
            return false;
        }

        // Same device
        if self.device != other.device {
            return false;
        }

        true
    }

    /// Merges another bio into this one
    pub fn merge(&mut self, other: Bio) {
        self.bvecs.extend(other.bvecs);
        self.size += other.size;
    }

    /// Advances the bio by the given number of bytes
    pub fn advance(&mut self, bytes: usize) {
        let mut remaining = bytes;
        
        while remaining > 0 && self.bvec_idx < self.bvecs.len() {
            let bvec = &self.bvecs[self.bvec_idx];
            let available = bvec.len - self.bvec_offset;
            
            if remaining >= available {
                remaining -= available;
                self.bvec_idx += 1;
                self.bvec_offset = 0;
            } else {
                self.bvec_offset += remaining;
                remaining = 0;
            }
        }
    }

    /// Returns the current memory segment
    pub fn current_bvec(&self) -> Option<&BioVec> {
        if self.bvec_idx < self.bvecs.len() {
            Some(&self.bvecs[self.bvec_idx])
        } else {
            None
        }
    }

    /// Returns remaining bytes in current segment
    pub fn current_remaining(&self) -> usize {
        if let Some(bvec) = self.current_bvec() {
            bvec.len - self.bvec_offset
        } else {
            0
        }
    }

    /// Resets iteration to start
    pub fn reset(&mut self) {
        self.bvec_idx = 0;
        self.bvec_offset = 0;
    }

    /// Completes the bio with the given status
    pub fn complete(mut self, status: BioStatus) {
        self.status = status;
        if let Some(callback) = self.end_io.take() {
            callback(&self, status);
        }
    }

    /// Completes successfully
    pub fn complete_ok(self) {
        self.complete(BioStatus::Complete);
    }

    /// Completes with an error
    pub fn complete_error(self, error: BlockError) {
        self.complete(BioStatus::Error(error));
    }
}

impl core::fmt::Debug for Bio {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Bio")
            .field("id", &self.id)
            .field("op", &self.op)
            .field("sector", &self.sector)
            .field("size", &self.size)
            .field("bvecs", &self.bvecs.len())
            .field("status", &self.status)
            .finish()
    }
}

// ============================================================================
// Bio Builder (Fluent API)
// ============================================================================

/// Builder for constructing bios
pub struct BioBuilder {
    bio: Bio,
}

impl BioBuilder {
    /// Creates a new builder for a read operation
    pub fn read(sector: u64) -> Self {
        Self {
            bio: Bio::read(sector),
        }
    }

    /// Creates a new builder for a write operation
    pub fn write(sector: u64) -> Self {
        Self {
            bio: Bio::write(sector),
        }
    }

    /// Adds a page to the bio
    pub fn page(mut self, page: usize, offset: usize, len: usize) -> Self {
        self.bio.add_page(page, offset, len);
        self
    }

    /// Adds a buffer to the bio
    pub fn buffer(mut self, buffer: &[u8]) -> Self {
        self.bio.add_buffer(buffer);
        self
    }

    /// Sets the device
    pub fn device(mut self, device: &'static str) -> Self {
        self.bio.device = Some(device);
        self
    }

    /// Sets the completion callback
    pub fn on_complete<F>(mut self, callback: F) -> Self
    where
        F: FnOnce(&Bio, BioStatus) + Send + 'static,
    {
        self.bio.set_end_io(callback);
        self
    }

    /// Builds the bio
    pub fn build(self) -> Bio {
        self.bio
    }
}

// ============================================================================
// Bio Pool (Memory Management)
// ============================================================================

/// Pre-allocated bio pool for fast allocation
pub struct BioPool {
    pool: Mutex<VecDeque<Bio>>,
    max_size: usize,
}

impl BioPool {
    /// Creates a new bio pool
    pub const fn new(max_size: usize) -> Self {
        Self {
            pool: Mutex::new(VecDeque::new()),
            max_size,
        }
    }

    /// Allocates a bio from the pool
    pub fn alloc(&self, op: BioOp, sector: u64) -> Bio {
        let mut pool = self.pool.lock();
        if let Some(mut bio) = pool.pop_front() {
            // Reuse existing bio
            bio.id = BIO_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
            bio.op = op;
            bio.sector = sector;
            bio.size = 0;
            bio.bvecs.clear();
            bio.bvec_idx = 0;
            bio.bvec_offset = 0;
            bio.end_io = None;
            bio.status = BioStatus::Pending;
            bio.device = None;
            bio.private = 0;
            bio
        } else {
            // Allocate new
            Bio::new(op, sector)
        }
    }

    /// Returns a bio to the pool
    pub fn free(&self, mut bio: Bio) {
        let mut pool = self.pool.lock();
        if pool.len() < self.max_size {
            // Clear for reuse
            bio.bvecs.clear();
            bio.end_io = None;
            pool.push_back(bio);
        }
        // Otherwise just drop
    }
}

/// Global bio pool
static BIO_POOL: BioPool = BioPool::new(256);

/// Allocates a bio from the global pool
pub fn bio_alloc(op: BioOp, sector: u64) -> Bio {
    BIO_POOL.alloc(op, sector)
}

/// Returns a bio to the global pool
pub fn bio_free(bio: Bio) {
    BIO_POOL.free(bio);
}

// ============================================================================
// Bio Splitting
// ============================================================================

/// Splits a bio at the given sector boundary
pub fn bio_split(bio: &mut Bio, sectors: u64) -> Option<Bio> {
    let split_bytes = (sectors as usize) * SECTOR_SIZE;
    
    if split_bytes >= bio.size || split_bytes == 0 {
        return None;
    }

    let mut new_bio = Bio::new(bio.op, bio.sector + sectors);
    new_bio.device = bio.device;
    
    // Find split point in bvecs
    let mut accumulated = 0;
    let mut split_idx = 0;
    let mut split_offset = 0;
    
    for (i, bvec) in bio.bvecs.iter().enumerate() {
        if accumulated + bvec.len > split_bytes {
            split_idx = i;
            split_offset = split_bytes - accumulated;
            break;
        }
        accumulated += bvec.len;
        if accumulated == split_bytes {
            split_idx = i + 1;
            split_offset = 0;
            break;
        }
    }
    
    // Move bvecs after split point to new bio
    if split_offset > 0 {
        // Split within a bvec
        let bvec = &bio.bvecs[split_idx];
        new_bio.bvecs.push(BioVec::new(
            bvec.page,
            bvec.offset + split_offset,
            bvec.len - split_offset,
        ));
        bio.bvecs[split_idx] = BioVec::new(bvec.page, bvec.offset, split_offset);
        split_idx += 1;
    }
    
    // Move remaining bvecs
    while split_idx < bio.bvecs.len() {
        new_bio.bvecs.push(bio.bvecs.remove(split_idx));
    }
    
    // Update sizes
    new_bio.size = bio.size - split_bytes;
    bio.size = split_bytes;
    
    Some(new_bio)
}

// ============================================================================
// Bio Chain (Multiple Bios)
// ============================================================================

/// A chain of bios that complete together
pub struct BioChain {
    bios: Vec<Bio>,
    pending: usize,
}

impl BioChain {
    /// Creates a new bio chain
    pub fn new() -> Self {
        Self {
            bios: Vec::new(),
            pending: 0,
        }
    }

    /// Adds a bio to the chain
    pub fn add(&mut self, bio: Bio) {
        self.pending += 1;
        self.bios.push(bio);
    }

    /// Marks a bio as complete
    pub fn complete_one(&mut self) -> bool {
        if self.pending > 0 {
            self.pending -= 1;
        }
        self.pending == 0
    }

    /// Checks if all bios are complete
    pub fn is_complete(&self) -> bool {
        self.pending == 0
    }

    /// Returns the number of pending bios
    pub fn pending(&self) -> usize {
        self.pending
    }
}

impl Default for BioChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bio_creation() {
        let bio = Bio::read(100);
        assert_eq!(bio.op, BioOp::Read);
        assert_eq!(bio.sector, 100);
        assert_eq!(bio.size, 0);
    }

    #[test]
    fn test_bio_builder() {
        let data = [0u8; 512];
        let bio = BioBuilder::write(50)
            .buffer(&data)
            .device("vda")
            .build();
        
        assert_eq!(bio.op, BioOp::Write);
        assert_eq!(bio.sector, 50);
        assert_eq!(bio.size, 512);
        assert_eq!(bio.device, Some("vda"));
    }

    #[test]
    fn test_bio_merge() {
        let mut bio1 = Bio::read(0);
        bio1.size = 512;
        
        let mut bio2 = Bio::read(1);
        bio2.size = 512;
        
        assert!(bio1.can_merge(&bio2));
    }
}

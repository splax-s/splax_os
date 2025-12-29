//! # Block Layer
//!
//! Splax OS block device abstraction layer (inspired by Linux block/).
//!
//! ## Design
//!
//! The block layer provides:
//! - Abstract BlockDevice trait for all block devices
//! - Block I/O request queue
//! - Sector-based read/write operations
//! - Device registration and discovery
//! - I/O scheduling (NoOp, Deadline, CFQ)
//! - Bio layer for scatter-gather I/O
//! - Partition table support (MBR/GPT)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │            Filesystem (VFS)             │
//! ├─────────────────────────────────────────┤
//! │              Block Layer                │
//! │  - Bio layer (scatter-gather)           │
//! │  - Request queue                        │
//! │  - I/O scheduling                       │
//! │  - Device abstraction                   │
//! │  - Partition handling                   │
//! ├─────────────────────────────────────────┤
//! │         Block Device Drivers            │
//! │  - VirtIO-blk                           │
//! │  - NVMe                                 │
//! │  - AHCI                                 │
//! └─────────────────────────────────────────┘
//! ```

pub mod ahci;
pub mod bio;
pub mod nvme;
pub mod partitions;
pub mod scheduler;
pub mod virtio_blk;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// Standard sector size (512 bytes)
pub const SECTOR_SIZE: usize = 512;

/// Block device errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// Device not found
    NotFound,
    /// Device busy
    Busy,
    /// I/O error
    IoError,
    /// Invalid sector number
    InvalidSector,
    /// Invalid buffer size
    InvalidSize,
    /// Device not ready
    NotReady,
    /// Write protected
    WriteProtected,
    /// Device timeout
    Timeout,
    /// Out of memory
    OutOfMemory,
    /// Unsupported operation
    Unsupported,
}

/// Block device information
#[derive(Debug, Clone)]
pub struct BlockDeviceInfo {
    /// Device name (e.g., "vda", "nvme0n1")
    pub name: String,
    /// Total number of sectors
    pub total_sectors: u64,
    /// Sector size in bytes
    pub sector_size: usize,
    /// Whether the device is read-only
    pub read_only: bool,
    /// Device model/description
    pub model: String,
}

/// Block I/O request type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockRequestType {
    Read,
    Write,
    Flush,
    Discard,
}

/// A block I/O request
#[derive(Debug)]
pub struct BlockRequest {
    /// Request type
    pub request_type: BlockRequestType,
    /// Starting sector
    pub sector: u64,
    /// Number of sectors
    pub count: usize,
    /// Data buffer (for read/write)
    pub buffer: Option<Vec<u8>>,
}

impl BlockRequest {
    /// Creates a read request
    pub fn read(sector: u64, count: usize) -> Self {
        Self {
            request_type: BlockRequestType::Read,
            sector,
            count,
            buffer: None,
        }
    }

    /// Creates a write request
    pub fn write(sector: u64, data: Vec<u8>) -> Self {
        let count = (data.len() + SECTOR_SIZE - 1) / SECTOR_SIZE;
        Self {
            request_type: BlockRequestType::Write,
            sector,
            count,
            buffer: Some(data),
        }
    }

    /// Creates a flush request
    pub fn flush() -> Self {
        Self {
            request_type: BlockRequestType::Flush,
            sector: 0,
            count: 0,
            buffer: None,
        }
    }
}

/// Block device trait - all block devices implement this
pub trait BlockDevice: Send + Sync {
    /// Returns device information
    fn info(&self) -> BlockDeviceInfo;

    /// Reads sectors from the device
    fn read_sectors(&self, start_sector: u64, buffer: &mut [u8]) -> Result<(), BlockError>;

    /// Writes sectors to the device
    fn write_sectors(&self, start_sector: u64, buffer: &[u8]) -> Result<(), BlockError>;

    /// Flushes pending writes to disk
    fn flush(&self) -> Result<(), BlockError>;

    /// Returns true if the device is ready
    fn is_ready(&self) -> bool;
}

/// Global block device registry
static BLOCK_DEVICES: Mutex<BTreeMap<String, Box<dyn BlockDevice>>> = Mutex::new(BTreeMap::new());

/// Device counter for auto-naming
static DEVICE_COUNTER: Mutex<usize> = Mutex::new(0);

/// Registers a block device
pub fn register_device(device: Box<dyn BlockDevice>) -> Result<String, BlockError> {
    // Get info before taking ownership
    let info = device.info();
    let name = info.name;
    let total_sectors = info.total_sectors;
    let sector_size = info.sector_size;

    {
        let mut devices = BLOCK_DEVICES.lock();
        if devices.contains_key(&name) {
            return Err(BlockError::Busy);
        }
        devices.insert(name.clone(), device);
    }
    
    // Print after releasing the lock - use static strings where possible
    crate::serial_println!("[BLOCK] Registered device: {} ({} sectors, {} bytes/sector)",
        name, total_sectors, sector_size);
    crate::serial_println!("[BLOCK]   Model: VirtIO Block Device");
    crate::serial_println!("[BLOCK]   Capacity: {} MB", 
        (total_sectors * sector_size as u64) / (1024 * 1024));

    Ok(name)
}

/// Unregisters a block device
pub fn unregister_device(name: &str) -> Result<(), BlockError> {
    let mut devices = BLOCK_DEVICES.lock();
    if devices.remove(name).is_some() {
        crate::serial_println!("[BLOCK] Unregistered device: {}", name);
        Ok(())
    } else {
        Err(BlockError::NotFound)
    }
}

/// Lists all registered block devices
pub fn list_devices() -> Vec<BlockDeviceInfo> {
    let devices = BLOCK_DEVICES.lock();
    devices.values().map(|d| d.info()).collect()
}

/// Gets a reference to a block device (borrows the lock)
pub fn with_device<F, R>(name: &str, f: F) -> Result<R, BlockError>
where
    F: FnOnce(&dyn BlockDevice) -> R,
{
    let devices = BLOCK_DEVICES.lock();
    match devices.get(name) {
        Some(device) => Ok(f(device.as_ref())),
        None => Err(BlockError::NotFound),
    }
}

/// Reads from a block device by name
pub fn read(name: &str, sector: u64, count: usize) -> Result<Vec<u8>, BlockError> {
    let devices = BLOCK_DEVICES.lock();
    let device = devices.get(name).ok_or(BlockError::NotFound)?;
    
    let size = count * device.info().sector_size;
    let mut buffer = alloc::vec![0u8; size];
    device.read_sectors(sector, &mut buffer)?;
    Ok(buffer)
}

/// Writes to a block device by name
pub fn write(name: &str, sector: u64, data: &[u8]) -> Result<(), BlockError> {
    let devices = BLOCK_DEVICES.lock();
    let device = devices.get(name).ok_or(BlockError::NotFound)?;
    device.write_sectors(sector, data)
}

/// Flushes a block device by name
pub fn flush(name: &str) -> Result<(), BlockError> {
    let devices = BLOCK_DEVICES.lock();
    let device = devices.get(name).ok_or(BlockError::NotFound)?;
    device.flush()
}

/// Generates the next device name (e.g., "vda", "vdb", ...)
pub fn next_virtio_name() -> String {
    let mut counter = DEVICE_COUNTER.lock();
    let name = alloc::format!("vd{}", (b'a' + *counter as u8) as char);
    *counter += 1;
    name
}

/// Initializes the block subsystem
pub fn init() {
    crate::serial_println!("[BLOCK] Initializing block subsystem...");
    
    // Probe for VirtIO block devices (virtual machines)
    virtio_blk::probe_devices();
    
    // Probe for NVMe devices (modern SSDs)
    nvme::probe_devices();
    
    // Probe for AHCI/SATA devices (HDDs and legacy SSDs)
    ahci::probe_devices();
    
    crate::serial_println!("[BLOCK] Block subsystem initialized");
}

/// Block device statistics
#[derive(Debug, Default, Clone)]
pub struct BlockStats {
    /// Number of read operations
    pub reads: u64,
    /// Number of write operations
    pub writes: u64,
    /// Total bytes read
    pub bytes_read: u64,
    /// Total bytes written
    pub bytes_written: u64,
}

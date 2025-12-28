# Block Device Subsystem Documentation

## Overview

Splax OS implements a comprehensive block device subsystem providing an abstraction layer for all block storage devices. The design is inspired by Linux's block layer, offering a unified interface for filesystems while supporting multiple storage technologies.

```text
┌─────────────────────────────────────────┐
│            Filesystem (VFS)             │
├─────────────────────────────────────────┤
│              Block Layer                │
│  - Request queue                        │
│  - I/O scheduling                       │
│  - Device abstraction                   │
├─────────────────────────────────────────┤
│         Block Device Drivers            │
│  - VirtIO-blk (virtual machines)        │
│  - NVMe (modern SSDs)                   │
│  - AHCI (SATA HDDs/SSDs)               │
└─────────────────────────────────────────┘
```

---

## Core Types

### Sector Size

```rust
/// Standard sector size (512 bytes)
pub const SECTOR_SIZE: usize = 512;
```

### Block Errors

```rust
/// Block device errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    NotFound,       // Device not found
    Busy,           // Device busy
    IoError,        // I/O error
    InvalidSector,  // Invalid sector number
    InvalidSize,    // Invalid buffer size
    NotReady,       // Device not ready
    WriteProtected, // Write protected
    Timeout,        // Device timeout
    OutOfMemory,    // Out of memory
    Unsupported,    // Unsupported operation
}
```

### Device Information

```rust
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
```

### Request Types

```rust
/// Block I/O request type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockRequestType {
    Read,    // Read data from device
    Write,   // Write data to device
    Flush,   // Flush pending writes
    Discard, // Discard/TRIM sectors
}
```

### Block Request

```rust
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
```

### Device Statistics

```rust
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
```

---

## BlockDevice Trait

All block devices implement this common interface:

```rust
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
```

---

## Device Registry

### Global Registry

```rust
/// Global block device registry
static BLOCK_DEVICES: Mutex<BTreeMap<String, Box<dyn BlockDevice>>> = 
    Mutex::new(BTreeMap::new());

/// Device counter for auto-naming
static DEVICE_COUNTER: Mutex<usize> = Mutex::new(0);
```

### Registration Functions

```rust
/// Registers a block device
pub fn register_device(device: Box<dyn BlockDevice>) -> Result<String, BlockError> {
    let info = device.info();
    let name = info.name;
    
    {
        let mut devices = BLOCK_DEVICES.lock();
        if devices.contains_key(&name) {
            return Err(BlockError::Busy);
        }
        devices.insert(name.clone(), device);
    }
    
    serial_println!("[BLOCK] Registered device: {} ({} sectors, {} bytes/sector)",
        name, info.total_sectors, info.sector_size);
    serial_println!("[BLOCK]   Capacity: {} MB", 
        (info.total_sectors * info.sector_size as u64) / (1024 * 1024));
    
    Ok(name)
}

/// Unregisters a block device
pub fn unregister_device(name: &str) -> Result<(), BlockError> {
    let mut devices = BLOCK_DEVICES.lock();
    if devices.remove(name).is_some() {
        serial_println!("[BLOCK] Unregistered device: {}", name);
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
```

### Device Access

```rust
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
    let mut buffer = vec![0u8; size];
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
```

### Device Naming

```rust
/// Generates the next device name (e.g., "vda", "vdb", ...)
pub fn next_virtio_name() -> String {
    let mut counter = DEVICE_COUNTER.lock();
    let name = format!("vd{}", (b'a' + *counter as u8) as char);
    *counter += 1;
    name
}
```

---

## VirtIO Block Device Driver

### Overview

VirtIO-blk is the standard block device driver for virtual machines running under QEMU/KVM. It provides high-performance block I/O through virtqueues.

### Device IDs

```rust
/// VirtIO block device ID (transitional)
pub const VIRTIO_BLK_DEVICE_ID_TRANSITIONAL: u16 = 0x1001;

/// VirtIO block device ID (modern)  
pub const VIRTIO_BLK_DEVICE_ID: u16 = 0x1042;

/// VirtIO vendor ID
const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
```

### Request Types

```rust
pub mod request_type {
    pub const VIRTIO_BLK_T_IN: u32 = 0;      // Read
    pub const VIRTIO_BLK_T_OUT: u32 = 1;     // Write
    pub const VIRTIO_BLK_T_FLUSH: u32 = 4;   // Flush
    pub const VIRTIO_BLK_T_DISCARD: u32 = 11; // Discard
}
```

### Status Codes

```rust
pub mod status {
    pub const VIRTIO_BLK_S_OK: u8 = 0;      // Success
    pub const VIRTIO_BLK_S_IOERR: u8 = 1;   // I/O error
    pub const VIRTIO_BLK_S_UNSUPP: u8 = 2;  // Unsupported
}
```

### Device Status Flags

```rust
pub mod device_status {
    pub const RESET: u8 = 0;
    pub const ACKNOWLEDGE: u8 = 1;
    pub const DRIVER: u8 = 2;
    pub const DRIVER_OK: u8 = 4;
    pub const FEATURES_OK: u8 = 8;
    pub const FAILED: u8 = 128;
}
```

### Feature Bits

```rust
pub mod features {
    pub const VIRTIO_BLK_F_SIZE_MAX: u32 = 1 << 1;   // Max segment size
    pub const VIRTIO_BLK_F_SEG_MAX: u32 = 1 << 2;    // Max segments
    pub const VIRTIO_BLK_F_GEOMETRY: u32 = 1 << 4;   // Disk geometry
    pub const VIRTIO_BLK_F_RO: u32 = 1 << 5;         // Read-only
    pub const VIRTIO_BLK_F_BLK_SIZE: u32 = 1 << 6;   // Block size
    pub const VIRTIO_BLK_F_FLUSH: u32 = 1 << 9;      // Flush support
    pub const VIRTIO_BLK_F_TOPOLOGY: u32 = 1 << 10;  // Topology info
    pub const VIRTIO_BLK_F_CONFIG_WCE: u32 = 1 << 11; // Write cache
    pub const VIRTIO_BLK_F_DISCARD: u32 = 1 << 13;   // Discard/TRIM
}
```

### Register Offsets (Legacy PIO)

```rust
mod regs {
    pub const DEVICE_FEATURES: u16 = 0x00;
    pub const GUEST_FEATURES: u16 = 0x04;
    pub const QUEUE_ADDRESS: u16 = 0x08;
    pub const QUEUE_SIZE: u16 = 0x0C;
    pub const QUEUE_SELECT: u16 = 0x0E;
    pub const QUEUE_NOTIFY: u16 = 0x10;
    pub const DEVICE_STATUS: u16 = 0x12;
    pub const ISR_STATUS: u16 = 0x13;
    // Block device config starts at offset 0x14
    pub const CONFIG_CAPACITY: u16 = 0x14;      // 8 bytes (u64)
    pub const CONFIG_SIZE_MAX: u16 = 0x1C;      // 4 bytes (u32)
    pub const CONFIG_SEG_MAX: u16 = 0x20;       // 4 bytes (u32)
    pub const CONFIG_BLK_SIZE: u16 = 0x28;      // 4 bytes (u32)
}
```

### Request Header

```rust
/// VirtIO block request header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioBlkReqHeader {
    pub request_type: u32,  // Read/Write/Flush
    pub reserved: u32,
    pub sector: u64,        // Starting sector
}
```

### Virtqueue Structures

```rust
/// VirtIO ring descriptor
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqDesc {
    pub addr: u64,   // Physical address of buffer
    pub len: u32,    // Length of buffer
    pub flags: u16,  // NEXT, WRITE flags
    pub next: u16,   // Next descriptor index
}

/// Available ring
#[repr(C)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; QUEUE_SIZE],
    pub used_event: u16,
}

/// Used ring element
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqUsedElem {
    pub id: u32,   // Descriptor chain head
    pub len: u32,  // Bytes written
}

/// Used ring
#[repr(C)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtqUsedElem; QUEUE_SIZE],
    pub avail_event: u16,
}
```

### VirtIO Device Structure

```rust
/// VirtIO block device
pub struct VirtioBlkDevice {
    name: String,
    io_base: u16,
    capacity: u64,
    sector_size: usize,
    read_only: bool,
    queue: Mutex<Option<Virtqueue>>,
    ready: AtomicBool,
    request_buffers: Mutex<Vec<Vec<u8>>>,
}
```

### Initialization Sequence

```rust
impl VirtioBlkDevice {
    pub fn init(&mut self) -> Result<(), BlockError> {
        // 1. Reset device
        self.write_status(device_status::RESET);

        // 2. Acknowledge device
        self.write_status(device_status::ACKNOWLEDGE);
        self.write_status(device_status::ACKNOWLEDGE | device_status::DRIVER);

        // 3. Read and negotiate features
        let features = self.read_features();
        self.read_only = (features & features::VIRTIO_BLK_F_RO) != 0;
        let accepted = features & (features::VIRTIO_BLK_F_FLUSH | 
                                   features::VIRTIO_BLK_F_BLK_SIZE);
        self.write_features(accepted);

        // 4. Set features OK
        self.write_status(device_status::ACKNOWLEDGE | 
                         device_status::DRIVER | 
                         device_status::FEATURES_OK);

        // 5. Verify features accepted
        let status = self.read_status();
        if (status & device_status::FEATURES_OK) == 0 {
            self.write_status(device_status::FAILED);
            return Err(BlockError::NotReady);
        }

        // 6. Read capacity
        self.capacity = self.read_capacity();

        // 7. Setup virtqueue
        self.setup_queue()?;

        // 8. Mark driver ready
        self.write_status(
            device_status::ACKNOWLEDGE | 
            device_status::DRIVER | 
            device_status::FEATURES_OK | 
            device_status::DRIVER_OK
        );

        self.ready.store(true, Ordering::SeqCst);
        Ok(())
    }
}
```

### I/O Operation

```rust
fn do_request(&self, request_type: u32, sector: u64, data: Option<&mut [u8]>) 
    -> Result<(), BlockError> 
{
    // 1. Allocate descriptors (header, data, status)
    // 2. Setup header descriptor
    // 3. Setup data descriptor with appropriate flags
    // 4. Setup status descriptor
    // 5. Add to available ring
    // 6. Notify device
    // 7. Poll for completion
    // 8. Check status and return
}
```

---

## NVMe Driver

### Overview

NVMe (Non-Volatile Memory Express) is designed for high-performance SSDs connected via PCIe, offering low latency and high IOPS.

### Architecture

```text
┌─────────────────────────────────────────┐
│           NVMe Controller               │
├───────────────┬─────────────────────────┤
│  Admin Queue  │     I/O Queues          │
│   (SQ + CQ)   │  (SQ0+CQ0, SQ1+CQ1...)  │
├───────────────┴─────────────────────────┤
│         Controller Registers            │
│  (CAP, VS, CC, CSTS, AQA, ASQ, ACQ)    │
└─────────────────────────────────────────┘
```

### Constants

```rust
const MAX_QUEUES: usize = 64;
const SQE_SIZE: usize = 64;
const CQE_SIZE: usize = 16;
const MAX_QUEUE_DEPTH: u16 = 1024;
const DEFAULT_QUEUE_DEPTH: u16 = 64;
const ADMIN_QUEUE_ID: u16 = 0;
const PAGE_SIZE: usize = 4096;
```

### Register Offsets

```rust
const REG_CAP: usize = 0x00;    // Controller Capabilities
const REG_VS: usize = 0x08;     // Version
const REG_INTMS: usize = 0x0C;  // Interrupt Mask Set
const REG_INTMC: usize = 0x10;  // Interrupt Mask Clear
const REG_CC: usize = 0x14;     // Controller Configuration
const REG_CSTS: usize = 0x1C;   // Controller Status
const REG_AQA: usize = 0x24;    // Admin Queue Attributes
const REG_ASQ: usize = 0x28;    // Admin Submission Queue Base
const REG_ACQ: usize = 0x30;    // Admin Completion Queue Base
const REG_SQ0TDBL: usize = 0x1000; // Submission Queue 0 Tail Doorbell
```

### Admin Command Opcodes

```rust
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum AdminOpcode {
    DeleteIOSQ = 0x00,
    CreateIOSQ = 0x01,
    GetLogPage = 0x02,
    DeleteIOCQ = 0x04,
    CreateIOCQ = 0x05,
    Identify = 0x06,
    Abort = 0x08,
    SetFeatures = 0x09,
    GetFeatures = 0x0A,
    AsyncEventReq = 0x0C,
    NamespaceMgmt = 0x0D,
    FirmwareCommit = 0x10,
    FirmwareDownload = 0x11,
    FormatNVM = 0x80,
    SecuritySend = 0x81,
    SecurityReceive = 0x82,
}
```

### I/O Command Opcodes

```rust
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum IoOpcode {
    Flush = 0x00,
    Write = 0x01,
    Read = 0x02,
    WriteUncorrectable = 0x04,
    Compare = 0x05,
    WriteZeroes = 0x08,
    DatasetMgmt = 0x09,
    ReservationRegister = 0x0D,
    ReservationReport = 0x0E,
    ReservationAcquire = 0x11,
    ReservationRelease = 0x15,
}
```

### Submission Queue Entry (64 bytes)

```rust
/// NVMe Submission Queue Entry (64 bytes)
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Default)]
pub struct SubmissionQueueEntry {
    pub cdw0: u32,       // Opcode, Fused, PSDT, CID
    pub nsid: u32,       // Namespace ID
    pub cdw2: u32,       // Reserved
    pub cdw3: u32,       // Reserved
    pub mptr: u64,       // Metadata pointer
    pub dptr_prp1: u64,  // Data pointer (PRP1)
    pub dptr_prp2: u64,  // Data pointer (PRP2 or SGL)
    pub cdw10: u32,      // Command Dword 10
    pub cdw11: u32,      // Command Dword 11
    pub cdw12: u32,      // Command Dword 12
    pub cdw13: u32,      // Command Dword 13
    pub cdw14: u32,      // Command Dword 14
    pub cdw15: u32,      // Command Dword 15
}

impl SubmissionQueueEntry {
    /// Sets up a read command
    pub fn setup_read(&mut self, cid: u16, nsid: u32, lba: u64, blocks: u16, 
                      prp1: u64, prp2: u64) {
        self.cdw0 = (IoOpcode::Read as u32) | ((cid as u32) << 16);
        self.nsid = nsid;
        self.dptr_prp1 = prp1;
        self.dptr_prp2 = prp2;
        self.cdw10 = lba as u32;
        self.cdw11 = (lba >> 32) as u32;
        self.cdw12 = (blocks as u32) - 1;  // 0-based
    }

    /// Sets up a write command
    pub fn setup_write(&mut self, cid: u16, nsid: u32, lba: u64, blocks: u16,
                       prp1: u64, prp2: u64) {
        self.cdw0 = (IoOpcode::Write as u32) | ((cid as u32) << 16);
        self.nsid = nsid;
        self.dptr_prp1 = prp1;
        self.dptr_prp2 = prp2;
        self.cdw10 = lba as u32;
        self.cdw11 = (lba >> 32) as u32;
        self.cdw12 = (blocks as u32) - 1;
    }

    /// Sets up an identify command
    pub fn setup_identify(&mut self, cid: u16, cns: u8, nsid: u32, prp1: u64) {
        self.cdw0 = (AdminOpcode::Identify as u32) | ((cid as u32) << 16);
        self.nsid = nsid;
        self.dptr_prp1 = prp1;
        self.cdw10 = cns as u32;
    }
}
```

### Completion Queue Entry (16 bytes)

```rust
/// NVMe Completion Queue Entry (16 bytes)
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default)]
pub struct CompletionQueueEntry {
    pub dw0: u32,    // Command-specific result
    pub dw1: u32,    // Reserved
    pub sqhd: u16,   // SQ Head Pointer
    pub sqid: u16,   // SQ Identifier
    pub cid: u16,    // Command Identifier
    pub status: u16, // Status (includes Phase bit)
}

impl CompletionQueueEntry {
    /// Returns true if phase bit matches expected
    pub fn phase_matches(&self, expected: bool) -> bool {
        let phase = (self.status & 0x0001) != 0;
        phase == expected
    }

    /// Returns the status code
    pub fn status_code(&self) -> u16 {
        (self.status >> 1) & 0x7FFF
    }

    /// Returns true if successful
    pub fn success(&self) -> bool {
        self.status_code() == 0
    }
}
```

### NVMe Queue

```rust
pub struct NvmeQueue {
    id: u16,
    depth: u16,
    sq_entries: Vec<SubmissionQueueEntry>,
    cq_entries: Vec<CompletionQueueEntry>,
    sq_tail: u16,
    cq_head: u16,
    cq_phase: bool,
    doorbell_stride: usize,
}

impl NvmeQueue {
    pub fn submit(&mut self, entry: SubmissionQueueEntry) -> u16 {
        let tail = self.sq_tail;
        self.sq_entries[tail as usize] = entry;
        self.sq_tail = (self.sq_tail + 1) % self.depth;
        tail
    }

    pub fn poll_completion(&mut self) -> Option<CompletionQueueEntry> {
        let entry = &self.cq_entries[self.cq_head as usize];
        if entry.phase_matches(self.cq_phase) {
            let result = *entry;
            self.cq_head = (self.cq_head + 1) % self.depth;
            if self.cq_head == 0 {
                self.cq_phase = !self.cq_phase;
            }
            Some(result)
        } else {
            None
        }
    }
}
```

### NVMe Namespace

```rust
#[derive(Debug, Clone)]
pub struct NvmeNamespace {
    pub nsid: u32,         // Namespace ID
    pub nsze: u64,         // Number of logical blocks
    pub ncap: u64,         // Number of capacity blocks
    pub block_size: u32,   // Block size in bytes
    pub metadata_size: u16, // Metadata size
    pub flbas: u8,         // Formatted LBA size index
}
```

### Controller Initialization

```rust
impl NvmeController {
    fn init(&mut self) -> Result<(), BlockError> {
        // 1. Disable controller
        self.disable()?;
        
        // 2. Configure admin queues
        self.configure_admin_queues()?;
        
        // 3. Enable controller
        self.enable()?;
        
        // 4. Identify controller
        self.identify_controller()?;
        
        // 5. Create I/O queue pair
        self.create_io_queue_pair(1, DEFAULT_QUEUE_DEPTH)?;
        
        // 6. Identify namespaces
        self.identify_namespaces()?;
        
        self.ready = true;
        Ok(())
    }
}
```

---

## AHCI Driver (SATA)

### Overview

AHCI (Advanced Host Controller Interface) is the standard interface for SATA storage devices (HDDs and SSDs).

### Architecture

```text
┌─────────────────────────────────────────┐
│        Host Bus Adapter (HBA)           │
├─────────────────────────────────────────┤
│    Generic Host Control Registers       │
│  (CAP, GHC, IS, PI, VS, etc.)          │
├─────────────────────────────────────────┤
│           Port 0..31                    │
│  ┌─────────────────────────────────┐   │
│  │  Command List (32 slots)        │   │
│  │  Received FIS Area             │   │
│  │  Port Registers                 │   │
│  └─────────────────────────────────┘   │
└─────────────────────────────────────────┘
```

### Device Signatures

```rust
const SATA_SIG_ATA: u32 = 0x00000101;    // SATA drive (HDD/SSD)
const SATA_SIG_ATAPI: u32 = 0xEB140101;  // ATAPI device (CD/DVD)
const SATA_SIG_SEMB: u32 = 0xC33C0101;   // SEMB device
const SATA_SIG_PM: u32 = 0x96690101;     // Port Multiplier
```

### Constants

```rust
const MAX_PORTS: usize = 32;
const MAX_COMMANDS: usize = 32;
const PRDT_ENTRIES: usize = 8;
const ATA_SECTOR_SIZE: usize = 512;
```

### Generic Host Control Registers

```rust
const REG_CAP: usize = 0x00;       // Host Capabilities
const REG_GHC: usize = 0x04;       // Global Host Control
const REG_IS: usize = 0x08;        // Interrupt Status
const REG_PI: usize = 0x0C;        // Ports Implemented
const REG_VS: usize = 0x10;        // AHCI Version
const REG_CCC_CTL: usize = 0x14;   // Command Completion Coalescing Control
const REG_EM_LOC: usize = 0x1C;    // Enclosure Management Location
const REG_EM_CTL: usize = 0x20;    // Enclosure Management Control
const REG_CAP2: usize = 0x24;      // Host Capabilities Extended
const REG_BOHC: usize = 0x28;      // BIOS/OS Handoff Control
```

### Port Registers (per port)

```rust
const PX_CLB: usize = 0x00;   // Command List Base Address (Low)
const PX_CLBU: usize = 0x04;  // Command List Base Address (High)
const PX_FB: usize = 0x08;    // FIS Base Address (Low)
const PX_FBU: usize = 0x0C;   // FIS Base Address (High)
const PX_IS: usize = 0x10;    // Interrupt Status
const PX_IE: usize = 0x14;    // Interrupt Enable
const PX_CMD: usize = 0x18;   // Command and Status
const PX_TFD: usize = 0x20;   // Task File Data
const PX_SIG: usize = 0x24;   // Signature
const PX_SSTS: usize = 0x28;  // SATA Status (SStatus)
const PX_SCTL: usize = 0x2C;  // SATA Control (SControl)
const PX_SERR: usize = 0x30;  // SATA Error (SError)
const PX_SACT: usize = 0x34;  // SATA Active
const PX_CI: usize = 0x38;    // Command Issue
```

### ATA Commands

```rust
const ATA_CMD_IDENTIFY: u8 = 0xEC;        // Identify Device
const ATA_CMD_READ_DMA_EXT: u8 = 0x25;    // Read DMA Extended
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;   // Write DMA Extended
const ATA_CMD_FLUSH_CACHE_EXT: u8 = 0xEA; // Flush Cache Extended
const ATA_CMD_READ_FPDMA: u8 = 0x60;      // Read FPDMA (NCQ)
const ATA_CMD_WRITE_FPDMA: u8 = 0x61;     // Write FPDMA (NCQ)
```

### FIS Types

```rust
const FIS_TYPE_REG_H2D: u8 = 0x27;    // Register - Host to Device
const FIS_TYPE_REG_D2H: u8 = 0x34;    // Register - Device to Host
const FIS_TYPE_DMA_ACT: u8 = 0x39;    // DMA Activate
const FIS_TYPE_DMA_SETUP: u8 = 0x41;  // DMA Setup
const FIS_TYPE_DATA: u8 = 0x46;       // Data
const FIS_TYPE_BIST: u8 = 0x58;       // BIST Activate
const FIS_TYPE_PIO_SETUP: u8 = 0x5F;  // PIO Setup
const FIS_TYPE_DEV_BITS: u8 = 0xA1;   // Set Device Bits
```

### Host to Device FIS (Register FIS)

```rust
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FisRegH2D {
    pub fis_type: u8,   // 0x27
    pub pmport_c: u8,   // Port multiplier, Command bit
    pub command: u8,    // Command register
    pub featurel: u8,   // Feature register (low)
    pub lba0: u8,       // LBA low
    pub lba1: u8,       // LBA mid
    pub lba2: u8,       // LBA high
    pub device: u8,     // Device register
    pub lba3: u8,       // LBA 47:40
    pub lba4: u8,       // LBA 39:32
    pub lba5: u8,       // LBA 31:24
    pub featureh: u8,   // Feature register (high)
    pub countl: u8,     // Count register (low)
    pub counth: u8,     // Count register (high)
    pub icc: u8,        // Isochronous command completion
    pub control: u8,    // Control register
    pub rsv1: [u8; 4],  // Reserved
}

impl FisRegH2D {
    pub fn setup_read_dma(&mut self, lba: u64, count: u16) {
        self.fis_type = FIS_TYPE_REG_H2D;
        self.pmport_c = 0x80;  // Command bit set
        self.command = ATA_CMD_READ_DMA_EXT;
        self.device = 0x40;    // LBA mode
        self.lba0 = (lba & 0xFF) as u8;
        self.lba1 = ((lba >> 8) & 0xFF) as u8;
        self.lba2 = ((lba >> 16) & 0xFF) as u8;
        self.lba3 = ((lba >> 24) & 0xFF) as u8;
        self.lba4 = ((lba >> 32) & 0xFF) as u8;
        self.lba5 = ((lba >> 40) & 0xFF) as u8;
        self.countl = (count & 0xFF) as u8;
        self.counth = ((count >> 8) & 0xFF) as u8;
    }
}
```

### Physical Region Descriptor Table Entry

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PrdtEntry {
    pub dba: u32,   // Data base address (low)
    pub dbau: u32,  // Data base address (high)
    pub rsv0: u32,  // Reserved
    pub dbc: u32,   // Byte count (bit 31 = interrupt on completion)
}

impl PrdtEntry {
    pub fn new(addr: u64, byte_count: u32, interrupt: bool) -> Self {
        Self {
            dba: addr as u32,
            dbau: (addr >> 32) as u32,
            rsv0: 0,
            dbc: ((byte_count - 1) & 0x3FFFFF) | 
                 if interrupt { 0x80000000 } else { 0 },
        }
    }
}
```

### Command Header (32 bytes)

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandHeader {
    pub dw0: u32,     // Description information
    pub prdbc: u32,   // PRDT Byte Count
    pub ctba: u32,    // Command Table Base Address (low)
    pub ctbau: u32,   // Command Table Base Address (high)
    pub rsv: [u32; 4], // Reserved
}

impl CommandHeader {
    pub fn new(ctba: u64, prdtl: u16, write: bool, atapi: bool, cfl: u8) -> Self {
        let dw0 = (cfl as u32 & 0x1F) 
            | if atapi { 0x20 } else { 0 }
            | if write { 0x40 } else { 0 }
            | ((prdtl as u32) << 16);
        
        Self {
            dw0,
            prdbc: 0,
            ctba: ctba as u32,
            ctbau: (ctba >> 32) as u32,
            rsv: [0; 4],
        }
    }
}
```

### Command Table

```rust
#[repr(C, align(128))]
pub struct CommandTable {
    pub cfis: [u8; 64],            // Command FIS (64 bytes)
    pub acmd: [u8; 16],            // ATAPI Command (16 bytes)
    pub rsv: [u8; 48],             // Reserved (48 bytes)
    pub prdt: [PrdtEntry; PRDT_ENTRIES], // PRDT
}
```

### Device Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AhciDeviceType {
    None,           // No device
    Sata,           // SATA drive (HDD/SSD)
    Atapi,          // ATAPI drive (CD/DVD)
    Semb,           // SEMB device
    PortMultiplier, // Port multiplier
}
```

---

## Usage Examples

### Initialize Block Subsystem

```rust
use crate::block;

pub fn init_storage() {
    block::init();
}
```

### List Available Devices

```rust
fn list_block_devices() {
    for dev in block::list_devices() {
        println!("{}: {} sectors ({} MB), model: {}",
            dev.name,
            dev.total_sectors,
            (dev.total_sectors * dev.sector_size as u64) / (1024 * 1024),
            dev.model);
    }
}
```

### Read from Device

```rust
fn read_boot_sector(device: &str) -> Result<Vec<u8>, BlockError> {
    block::read(device, 0, 1)  // Read sector 0
}
```

### Write to Device

```rust
fn write_data(device: &str, sector: u64, data: &[u8]) -> Result<(), BlockError> {
    block::write(device, sector, data)
}
```

### Access Device Directly

```rust
fn get_device_info(name: &str) -> Result<BlockDeviceInfo, BlockError> {
    block::with_device(name, |dev| dev.info())
}
```

---

## Shell Commands

```text
block list       - List all block devices
block info <dev> - Show device details
block read <dev> <sector> [count] - Read sectors
block write <dev> <sector> <hex>  - Write data
block flush <dev> - Flush device cache
```

---

## File Structure

```text
kernel/src/block/
├── mod.rs           # Core types, BlockDevice trait, registry
├── virtio_blk.rs    # VirtIO block device driver
├── nvme.rs          # NVMe SSD driver
└── ahci.rs          # AHCI/SATA driver
```

---

## Initialization Flow

```rust
pub fn init() {
    serial_println!("[BLOCK] Initializing block subsystem...");
    
    // 1. Probe for VirtIO block devices (virtual machines)
    virtio_blk::probe_devices();
    
    // 2. Probe for NVMe devices (modern SSDs)
    nvme::probe_devices();
    
    // 3. Probe for AHCI/SATA devices (HDDs and legacy SSDs)
    ahci::probe_devices();
    
    serial_println!("[BLOCK] Block subsystem initialized");
}
```

---

## Future Work

- [ ] I/O request queue and scheduling (CFQ, deadline, NOOP)
- [ ] Async I/O support
- [ ] Partition table parsing (GPT, MBR)
- [ ] RAID support (software RAID 0/1/5)
- [ ] S.M.A.R.T. monitoring
- [ ] Hot-plug detection
- [ ] Power management (device suspend/resume)
- [ ] Block layer caching
- [ ] Device mapper for logical volumes
- [ ] NVMe multipath support

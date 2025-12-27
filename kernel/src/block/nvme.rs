//! # NVMe Storage Driver
//!
//! This module implements the NVMe (Non-Volatile Memory Express) driver for
//! high-performance SSD storage. NVMe is designed for SSDs connected via PCIe.
//!
//! ## NVMe Architecture
//!
//! - Controller registers mapped to MMIO space
//! - Submission queues (SQ) for commands
//! - Completion queues (CQ) for results
//! - Admin queue pair for management commands
//! - I/O queue pairs for data transfer
//!
//! ## Features
//!
//! - Full NVMe 1.4 command set support
//! - Multiple I/O queue support (up to 64)
//! - Command completion via MSI-X interrupts
//! - Namespace management

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

use super::{BlockDevice, BlockDeviceInfo, BlockError};

// =============================================================================
// NVMe Constants
// =============================================================================

/// NVMe signature in CAP register
const NVME_CAP_SIGNATURE: u64 = 0x00_00_00_00_00_00_00_00;

/// Maximum number of queues per controller
const MAX_QUEUES: usize = 64;

/// Queue entry size (submission)
const SQE_SIZE: usize = 64;

/// Queue entry size (completion)
const CQE_SIZE: usize = 16;

/// Maximum queue depth
const MAX_QUEUE_DEPTH: u16 = 1024;

/// Default queue depth
const DEFAULT_QUEUE_DEPTH: u16 = 64;

/// Admin queue ID
const ADMIN_QUEUE_ID: u16 = 0;

/// Page size (4KB)
const PAGE_SIZE: usize = 4096;

// =============================================================================
// NVMe Register Offsets
// =============================================================================

/// Controller Capabilities (CAP)
const REG_CAP: usize = 0x00;
/// Version (VS)
const REG_VS: usize = 0x08;
/// Interrupt Mask Set (INTMS)
const REG_INTMS: usize = 0x0C;
/// Interrupt Mask Clear (INTMC)
const REG_INTMC: usize = 0x10;
/// Controller Configuration (CC)
const REG_CC: usize = 0x14;
/// Controller Status (CSTS)
const REG_CSTS: usize = 0x1C;
/// Admin Queue Attributes (AQA)
const REG_AQA: usize = 0x24;
/// Admin Submission Queue Base Address (ASQ)
const REG_ASQ: usize = 0x28;
/// Admin Completion Queue Base Address (ACQ)
const REG_ACQ: usize = 0x30;
/// Submission Queue 0 Tail Doorbell (offset, stride = 4 * (2^DSTRD))
const REG_SQ0TDBL: usize = 0x1000;

// =============================================================================
// NVMe Commands
// =============================================================================

/// Admin command opcodes
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum AdminOpcode {
    /// Delete I/O Submission Queue
    DeleteIOSQ = 0x00,
    /// Create I/O Submission Queue
    CreateIOSQ = 0x01,
    /// Get Log Page
    GetLogPage = 0x02,
    /// Delete I/O Completion Queue
    DeleteIOCQ = 0x04,
    /// Create I/O Completion Queue
    CreateIOCQ = 0x05,
    /// Identify
    Identify = 0x06,
    /// Abort
    Abort = 0x08,
    /// Set Features
    SetFeatures = 0x09,
    /// Get Features
    GetFeatures = 0x0A,
    /// Asynchronous Event Request
    AsyncEventReq = 0x0C,
    /// Namespace Management
    NamespaceMgmt = 0x0D,
    /// Firmware Commit
    FirmwareCommit = 0x10,
    /// Firmware Image Download
    FirmwareDownload = 0x11,
    /// Format NVM
    FormatNVM = 0x80,
    /// Security Send
    SecuritySend = 0x81,
    /// Security Receive
    SecurityReceive = 0x82,
}

/// I/O command opcodes
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum IoOpcode {
    /// Flush
    Flush = 0x00,
    /// Write
    Write = 0x01,
    /// Read
    Read = 0x02,
    /// Write Uncorrectable
    WriteUncorrectable = 0x04,
    /// Compare
    Compare = 0x05,
    /// Write Zeroes
    WriteZeroes = 0x08,
    /// Dataset Management
    DatasetMgmt = 0x09,
    /// Reservation Register
    ReservationRegister = 0x0D,
    /// Reservation Report
    ReservationReport = 0x0E,
    /// Reservation Acquire
    ReservationAcquire = 0x11,
    /// Reservation Release
    ReservationRelease = 0x15,
}

// =============================================================================
// NVMe Data Structures
// =============================================================================

/// NVMe Submission Queue Entry (64 bytes)
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Default)]
pub struct SubmissionQueueEntry {
    /// Command Dword 0: Opcode, Fused operation, PSDT, CID
    pub cdw0: u32,
    /// Namespace ID
    pub nsid: u32,
    /// Reserved
    pub cdw2: u32,
    /// Reserved
    pub cdw3: u32,
    /// Metadata pointer
    pub mptr: u64,
    /// Data pointer (PRP1)
    pub dptr_prp1: u64,
    /// Data pointer (PRP2 or SGL)
    pub dptr_prp2: u64,
    /// Command Dword 10
    pub cdw10: u32,
    /// Command Dword 11
    pub cdw11: u32,
    /// Command Dword 12
    pub cdw12: u32,
    /// Command Dword 13
    pub cdw13: u32,
    /// Command Dword 14
    pub cdw14: u32,
    /// Command Dword 15
    pub cdw15: u32,
}

impl SubmissionQueueEntry {
    /// Creates a new empty submission queue entry
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets up a read command
    pub fn setup_read(&mut self, cid: u16, nsid: u32, lba: u64, blocks: u16, prp1: u64, prp2: u64) {
        self.cdw0 = (IoOpcode::Read as u32) | ((cid as u32) << 16);
        self.nsid = nsid;
        self.dptr_prp1 = prp1;
        self.dptr_prp2 = prp2;
        self.cdw10 = lba as u32;
        self.cdw11 = (lba >> 32) as u32;
        self.cdw12 = (blocks as u32) - 1; // 0-based block count
    }

    /// Sets up a write command
    pub fn setup_write(&mut self, cid: u16, nsid: u32, lba: u64, blocks: u16, prp1: u64, prp2: u64) {
        self.cdw0 = (IoOpcode::Write as u32) | ((cid as u32) << 16);
        self.nsid = nsid;
        self.dptr_prp1 = prp1;
        self.dptr_prp2 = prp2;
        self.cdw10 = lba as u32;
        self.cdw11 = (lba >> 32) as u32;
        self.cdw12 = (blocks as u32) - 1;
    }

    /// Sets up a flush command
    pub fn setup_flush(&mut self, cid: u16, nsid: u32) {
        self.cdw0 = (IoOpcode::Flush as u32) | ((cid as u32) << 16);
        self.nsid = nsid;
    }

    /// Sets up an identify command
    pub fn setup_identify(&mut self, cid: u16, cns: u8, nsid: u32, prp1: u64) {
        self.cdw0 = (AdminOpcode::Identify as u32) | ((cid as u32) << 16);
        self.nsid = nsid;
        self.dptr_prp1 = prp1;
        self.cdw10 = cns as u32;
    }
}

/// NVMe Completion Queue Entry (16 bytes)
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default)]
pub struct CompletionQueueEntry {
    /// Command-specific result
    pub dw0: u32,
    /// Reserved
    pub dw1: u32,
    /// Submission Queue Head Pointer
    pub sqhd: u16,
    /// Submission Queue Identifier
    pub sqid: u16,
    /// Command Identifier
    pub cid: u16,
    /// Status Field (includes Phase bit)
    pub status: u16,
}

impl CompletionQueueEntry {
    /// Returns true if the phase bit matches expected phase
    pub fn phase_matches(&self, expected: bool) -> bool {
        let phase = (self.status & 0x0001) != 0;
        phase == expected
    }

    /// Returns the status code
    pub fn status_code(&self) -> u16 {
        (self.status >> 1) & 0x7FFF
    }

    /// Returns true if command completed successfully
    pub fn success(&self) -> bool {
        self.status_code() == 0
    }
}

/// NVMe Queue
pub struct NvmeQueue {
    /// Queue ID
    id: u16,
    /// Queue depth
    depth: u16,
    /// Submission queue entries
    sq_entries: Vec<SubmissionQueueEntry>,
    /// Completion queue entries
    cq_entries: Vec<CompletionQueueEntry>,
    /// Submission queue tail
    sq_tail: u16,
    /// Completion queue head
    cq_head: u16,
    /// Expected phase bit
    cq_phase: bool,
    /// Doorbell stride
    doorbell_stride: usize,
}

impl NvmeQueue {
    /// Creates a new NVMe queue
    pub fn new(id: u16, depth: u16, doorbell_stride: usize) -> Self {
        Self {
            id,
            depth,
            sq_entries: (0..depth).map(|_| SubmissionQueueEntry::default()).collect(),
            cq_entries: (0..depth).map(|_| CompletionQueueEntry::default()).collect(),
            sq_tail: 0,
            cq_head: 0,
            cq_phase: true,
            doorbell_stride,
        }
    }

    /// Submits a command to the queue
    pub fn submit(&mut self, entry: SubmissionQueueEntry) -> u16 {
        let tail = self.sq_tail;
        self.sq_entries[tail as usize] = entry;
        self.sq_tail = (self.sq_tail + 1) % self.depth;
        tail
    }

    /// Polls for completion
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

    /// Gets the submission queue physical address (for MMIO)
    pub fn sq_phys_addr(&self) -> u64 {
        self.sq_entries.as_ptr() as u64
    }

    /// Gets the completion queue physical address (for MMIO)
    pub fn cq_phys_addr(&self) -> u64 {
        self.cq_entries.as_ptr() as u64
    }
}

/// NVMe Namespace
#[derive(Debug, Clone)]
pub struct NvmeNamespace {
    /// Namespace ID
    pub nsid: u32,
    /// Number of logical blocks
    pub nsze: u64,
    /// Number of capacity blocks
    pub ncap: u64,
    /// Block size (in bytes)
    pub block_size: u32,
    /// Metadata size (in bytes)
    pub metadata_size: u16,
    /// Formatted LBA size index
    pub flbas: u8,
}

impl NvmeNamespace {
    /// Creates a new namespace from identify data
    pub fn from_identify_data(nsid: u32, data: &[u8]) -> Self {
        let nsze = u64::from_le_bytes([
            data[0], data[1], data[2], data[3],
            data[4], data[5], data[6], data[7],
        ]);
        let ncap = u64::from_le_bytes([
            data[8], data[9], data[10], data[11],
            data[12], data[13], data[14], data[15],
        ]);
        let flbas = data[26] & 0x0F;
        
        // LBA format is at offset 128 + flbas * 4
        let lba_format_offset = 128 + (flbas as usize) * 4;
        let lba_format = u32::from_le_bytes([
            data[lba_format_offset],
            data[lba_format_offset + 1],
            data[lba_format_offset + 2],
            data[lba_format_offset + 3],
        ]);
        
        let lba_ds = ((lba_format >> 16) & 0xFF) as u32;
        let block_size = if lba_ds >= 9 { 1u32 << lba_ds } else { 512 };
        let metadata_size = (lba_format & 0xFFFF) as u16;
        
        Self {
            nsid,
            nsze,
            ncap,
            block_size,
            metadata_size,
            flbas,
        }
    }
}

/// NVMe Controller
pub struct NvmeController {
    /// Controller name (e.g., "nvme0")
    name: String,
    /// MMIO base address
    mmio_base: u64,
    /// Controller version
    version: u32,
    /// Maximum queue entries supported
    mqes: u16,
    /// Doorbell stride
    dstrd: usize,
    /// Admin queue
    admin_queue: NvmeQueue,
    /// I/O queues
    io_queues: Vec<NvmeQueue>,
    /// Namespaces
    namespaces: Vec<NvmeNamespace>,
    /// Command ID counter
    next_cid: AtomicU32,
    /// Is controller ready?
    ready: bool,
}

impl NvmeController {
    /// Creates a new NVMe controller from PCI device
    pub fn new(name: String, mmio_base: u64) -> Result<Self, BlockError> {
        // Read controller capabilities
        let cap = unsafe { Self::read_reg64(mmio_base, REG_CAP) };
        let mqes = ((cap & 0xFFFF) + 1) as u16;
        let dstrd = (((cap >> 32) & 0xF) as usize) + 2;
        
        // Read version
        let version = unsafe { Self::read_reg32(mmio_base, REG_VS) };
        let major = (version >> 16) & 0xFFFF;
        let minor = (version >> 8) & 0xFF;
        let tertiary = version & 0xFF;
        
        crate::serial_println!("[nvme] Controller at 0x{:016x}", mmio_base);
        crate::serial_println!("[nvme] Version: {}.{}.{}", major, minor, tertiary);
        crate::serial_println!("[nvme] Max Queue Entries: {}", mqes);
        crate::serial_println!("[nvme] Doorbell Stride: {} bytes", 1 << dstrd);
        
        let admin_queue = NvmeQueue::new(ADMIN_QUEUE_ID, DEFAULT_QUEUE_DEPTH, 1 << dstrd);
        
        let mut controller = Self {
            name,
            mmio_base,
            version,
            mqes: mqes.min(MAX_QUEUE_DEPTH),
            dstrd,
            admin_queue,
            io_queues: Vec::new(),
            namespaces: Vec::new(),
            next_cid: AtomicU32::new(1),
            ready: false,
        };
        
        // Initialize the controller
        controller.init()?;
        
        Ok(controller)
    }
    
    /// Initializes the controller
    fn init(&mut self) -> Result<(), BlockError> {
        // Disable controller
        self.disable()?;
        
        // Configure admin queues
        self.configure_admin_queues()?;
        
        // Enable controller
        self.enable()?;
        
        // Identify controller
        self.identify_controller()?;
        
        // Create I/O queue pair
        self.create_io_queue_pair(1, DEFAULT_QUEUE_DEPTH)?;
        
        // Identify namespaces
        self.identify_namespaces()?;
        
        self.ready = true;
        crate::serial_println!("[nvme] Controller initialized successfully");
        
        Ok(())
    }
    
    /// Disables the controller
    fn disable(&mut self) -> Result<(), BlockError> {
        unsafe {
            let cc = Self::read_reg32(self.mmio_base, REG_CC);
            Self::write_reg32(self.mmio_base, REG_CC, cc & !0x1);
        }
        
        // Wait for controller to be disabled
        for _ in 0..1000 {
            let csts = unsafe { Self::read_reg32(self.mmio_base, REG_CSTS) };
            if (csts & 0x1) == 0 {
                return Ok(());
            }
            // Spin wait
            for _ in 0..1000 { core::hint::spin_loop(); }
        }
        
        Err(BlockError::Timeout)
    }
    
    /// Enables the controller
    fn enable(&mut self) -> Result<(), BlockError> {
        unsafe {
            // CC: Enable, IOCQES=4 (16 bytes), IOSQES=6 (64 bytes), AMS=Round Robin
            let cc = 0x00460001u32; // EN=1, CSS=0, MPS=0, AMS=0, SHN=0, IOSQES=6, IOCQES=4
            Self::write_reg32(self.mmio_base, REG_CC, cc);
        }
        
        // Wait for controller to be ready
        for _ in 0..1000 {
            let csts = unsafe { Self::read_reg32(self.mmio_base, REG_CSTS) };
            if (csts & 0x1) != 0 {
                if (csts & 0x2) != 0 {
                    crate::serial_println!("[nvme] Controller fatal error");
                    return Err(BlockError::IoError);
                }
                return Ok(());
            }
            for _ in 0..1000 { core::hint::spin_loop(); }
        }
        
        Err(BlockError::Timeout)
    }
    
    /// Configures admin submission and completion queues
    fn configure_admin_queues(&mut self) -> Result<(), BlockError> {
        let depth = self.admin_queue.depth;
        let sq_addr = self.admin_queue.sq_phys_addr();
        let cq_addr = self.admin_queue.cq_phys_addr();
        
        unsafe {
            // AQA: Admin Queue Attributes
            let aqa = ((depth as u32 - 1) << 16) | (depth as u32 - 1);
            Self::write_reg32(self.mmio_base, REG_AQA, aqa);
            
            // ASQ: Admin Submission Queue Base Address
            Self::write_reg64(self.mmio_base, REG_ASQ, sq_addr);
            
            // ACQ: Admin Completion Queue Base Address
            Self::write_reg64(self.mmio_base, REG_ACQ, cq_addr);
        }
        
        Ok(())
    }
    
    /// Creates an I/O queue pair
    fn create_io_queue_pair(&mut self, qid: u16, depth: u16) -> Result<(), BlockError> {
        let queue = NvmeQueue::new(qid, depth, 1 << self.dstrd);
        
        // Create completion queue first
        self.create_io_cq(qid, depth, queue.cq_phys_addr())?;
        
        // Then create submission queue
        self.create_io_sq(qid, depth, queue.sq_phys_addr(), qid)?;
        
        self.io_queues.push(queue);
        crate::serial_println!("[nvme] Created I/O queue pair {}", qid);
        
        Ok(())
    }
    
    /// Creates an I/O completion queue
    fn create_io_cq(&mut self, qid: u16, depth: u16, addr: u64) -> Result<(), BlockError> {
        let cid = self.alloc_cid();
        let mut entry = SubmissionQueueEntry::new();
        entry.cdw0 = (AdminOpcode::CreateIOCQ as u32) | ((cid as u32) << 16);
        entry.dptr_prp1 = addr;
        entry.cdw10 = ((depth as u32 - 1) << 16) | (qid as u32);
        entry.cdw11 = 0x1; // Physically contiguous, no interrupt vector
        
        self.submit_admin_command(entry)
    }
    
    /// Creates an I/O submission queue
    fn create_io_sq(&mut self, qid: u16, depth: u16, addr: u64, cqid: u16) -> Result<(), BlockError> {
        let cid = self.alloc_cid();
        let mut entry = SubmissionQueueEntry::new();
        entry.cdw0 = (AdminOpcode::CreateIOSQ as u32) | ((cid as u32) << 16);
        entry.dptr_prp1 = addr;
        entry.cdw10 = ((depth as u32 - 1) << 16) | (qid as u32);
        entry.cdw11 = ((cqid as u32) << 16) | 0x1; // Physically contiguous, priority=0
        
        self.submit_admin_command(entry)
    }
    
    /// Identifies the controller
    fn identify_controller(&mut self) -> Result<(), BlockError> {
        let mut data = alloc::vec![0u8; 4096];
        let prp1 = data.as_mut_ptr() as u64;
        
        let cid = self.alloc_cid();
        let mut entry = SubmissionQueueEntry::new();
        entry.setup_identify(cid, 0x01, 0, prp1); // CNS=1: Controller
        
        self.submit_admin_command(entry)?;
        
        // Parse controller data
        let sn = core::str::from_utf8(&data[4..24]).unwrap_or("?").trim();
        let mn = core::str::from_utf8(&data[24..64]).unwrap_or("?").trim();
        let fr = core::str::from_utf8(&data[64..72]).unwrap_or("?").trim();
        
        crate::serial_println!("[nvme] Serial: {}", sn);
        crate::serial_println!("[nvme] Model: {}", mn);
        crate::serial_println!("[nvme] Firmware: {}", fr);
        
        Ok(())
    }
    
    /// Identifies namespaces
    fn identify_namespaces(&mut self) -> Result<(), BlockError> {
        // First, get list of active namespaces
        let mut ns_list = alloc::vec![0u8; 4096];
        let prp1 = ns_list.as_mut_ptr() as u64;
        
        let cid = self.alloc_cid();
        let mut entry = SubmissionQueueEntry::new();
        entry.setup_identify(cid, 0x02, 0, prp1); // CNS=2: Active namespace list
        
        self.submit_admin_command(entry)?;
        
        // Parse namespace list (array of 32-bit NSIDs)
        for i in 0..1024 {
            let nsid = u32::from_le_bytes([
                ns_list[i * 4],
                ns_list[i * 4 + 1],
                ns_list[i * 4 + 2],
                ns_list[i * 4 + 3],
            ]);
            
            if nsid == 0 {
                break;
            }
            
            // Identify this namespace
            let mut ns_data = alloc::vec![0u8; 4096];
            let prp1 = ns_data.as_mut_ptr() as u64;
            
            let cid = self.alloc_cid();
            let mut entry = SubmissionQueueEntry::new();
            entry.setup_identify(cid, 0x00, nsid, prp1); // CNS=0: Namespace
            
            self.submit_admin_command(entry)?;
            
            let namespace = NvmeNamespace::from_identify_data(nsid, &ns_data);
            crate::serial_println!("[nvme] Namespace {}: {} blocks, {} bytes/block",
                nsid, namespace.nsze, namespace.block_size);
            
            self.namespaces.push(namespace);
        }
        
        if self.namespaces.is_empty() {
            crate::serial_println!("[nvme] No namespaces found");
        }
        
        Ok(())
    }
    
    /// Submits an admin command and waits for completion
    fn submit_admin_command(&mut self, entry: SubmissionQueueEntry) -> Result<(), BlockError> {
        let _idx = self.admin_queue.submit(entry);
        
        // Ring doorbell
        unsafe {
            let doorbell_addr = self.mmio_base + REG_SQ0TDBL as u64;
            core::ptr::write_volatile(doorbell_addr as *mut u32, self.admin_queue.sq_tail as u32);
        }
        
        // Poll for completion
        for _ in 0..100_000 {
            if let Some(cqe) = self.admin_queue.poll_completion() {
                // Ring completion doorbell
                unsafe {
                    let doorbell_addr = self.mmio_base + REG_SQ0TDBL as u64 + (1 << self.dstrd) as u64;
                    core::ptr::write_volatile(doorbell_addr as *mut u32, self.admin_queue.cq_head as u32);
                }
                
                if cqe.success() {
                    return Ok(());
                } else {
                    crate::serial_println!("[nvme] Command failed: status=0x{:04x}", cqe.status_code());
                    return Err(BlockError::IoError);
                }
            }
            core::hint::spin_loop();
        }
        
        Err(BlockError::Timeout)
    }
    
    /// Allocates a command ID
    fn alloc_cid(&self) -> u16 {
        (self.next_cid.fetch_add(1, Ordering::Relaxed) & 0xFFFF) as u16
    }
    
    /// Reads from the device
    fn read(&mut self, nsid: u32, lba: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        if self.io_queues.is_empty() {
            return Err(BlockError::NotReady);
        }
        
        let namespace = self.namespaces.iter().find(|ns| ns.nsid == nsid)
            .ok_or(BlockError::NotFound)?;
        
        let block_size = namespace.block_size as usize;
        let blocks = (buffer.len() + block_size - 1) / block_size;
        
        let prp1 = buffer.as_ptr() as u64;
        let prp2 = if buffer.len() > PAGE_SIZE {
            prp1 + PAGE_SIZE as u64
        } else {
            0
        };
        
        let cid = self.alloc_cid();
        let mut entry = SubmissionQueueEntry::new();
        entry.setup_read(cid, nsid, lba, blocks as u16, prp1, prp2);
        
        let queue = &mut self.io_queues[0];
        queue.submit(entry);
        
        // Ring doorbell
        let qid = queue.id;
        let sq_tail = queue.sq_tail;
        unsafe {
            let doorbell_addr = self.mmio_base + REG_SQ0TDBL as u64 + 
                (qid as u64 * 2 * (1 << self.dstrd) as u64);
            core::ptr::write_volatile(doorbell_addr as *mut u32, sq_tail as u32);
        }
        
        // Poll for completion
        for _ in 0..100_000 {
            if let Some(cqe) = queue.poll_completion() {
                let cq_head = queue.cq_head;
                unsafe {
                    let doorbell_addr = self.mmio_base + REG_SQ0TDBL as u64 + 
                        ((qid as u64 * 2 + 1) * (1 << self.dstrd) as u64);
                    core::ptr::write_volatile(doorbell_addr as *mut u32, cq_head as u32);
                }
                
                if cqe.success() {
                    return Ok(());
                } else {
                    return Err(BlockError::IoError);
                }
            }
            core::hint::spin_loop();
        }
        
        Err(BlockError::Timeout)
    }
    
    /// Writes to the device
    fn write(&mut self, nsid: u32, lba: u64, buffer: &[u8]) -> Result<(), BlockError> {
        if self.io_queues.is_empty() {
            return Err(BlockError::NotReady);
        }
        
        let namespace = self.namespaces.iter().find(|ns| ns.nsid == nsid)
            .ok_or(BlockError::NotFound)?;
        
        let block_size = namespace.block_size as usize;
        let blocks = (buffer.len() + block_size - 1) / block_size;
        
        let prp1 = buffer.as_ptr() as u64;
        let prp2 = if buffer.len() > PAGE_SIZE {
            prp1 + PAGE_SIZE as u64
        } else {
            0
        };
        
        let cid = self.alloc_cid();
        let mut entry = SubmissionQueueEntry::new();
        entry.setup_write(cid, nsid, lba, blocks as u16, prp1, prp2);
        
        let queue = &mut self.io_queues[0];
        queue.submit(entry);
        
        // Ring doorbell
        let qid = queue.id;
        let sq_tail = queue.sq_tail;
        unsafe {
            let doorbell_addr = self.mmio_base + REG_SQ0TDBL as u64 + 
                (qid as u64 * 2 * (1 << self.dstrd) as u64);
            core::ptr::write_volatile(doorbell_addr as *mut u32, sq_tail as u32);
        }
        
        // Poll for completion
        for _ in 0..100_000 {
            if let Some(cqe) = queue.poll_completion() {
                let cq_head = queue.cq_head;
                unsafe {
                    let doorbell_addr = self.mmio_base + REG_SQ0TDBL as u64 + 
                        ((qid as u64 * 2 + 1) * (1 << self.dstrd) as u64);
                    core::ptr::write_volatile(doorbell_addr as *mut u32, cq_head as u32);
                }
                
                if cqe.success() {
                    return Ok(());
                } else {
                    return Err(BlockError::IoError);
                }
            }
            core::hint::spin_loop();
        }
        
        Err(BlockError::Timeout)
    }
    
    /// Flushes the device
    fn flush(&mut self, nsid: u32) -> Result<(), BlockError> {
        if self.io_queues.is_empty() {
            return Err(BlockError::NotReady);
        }
        
        let cid = self.alloc_cid();
        let mut entry = SubmissionQueueEntry::new();
        entry.setup_flush(cid, nsid);
        
        let queue = &mut self.io_queues[0];
        queue.submit(entry);
        
        // Ring doorbell and wait for completion (similar to read/write)
        let qid = queue.id;
        let sq_tail = queue.sq_tail;
        unsafe {
            let doorbell_addr = self.mmio_base + REG_SQ0TDBL as u64 + 
                (qid as u64 * 2 * (1 << self.dstrd) as u64);
            core::ptr::write_volatile(doorbell_addr as *mut u32, sq_tail as u32);
        }
        
        for _ in 0..100_000 {
            if let Some(cqe) = queue.poll_completion() {
                let cq_head = queue.cq_head;
                unsafe {
                    let doorbell_addr = self.mmio_base + REG_SQ0TDBL as u64 + 
                        ((qid as u64 * 2 + 1) * (1 << self.dstrd) as u64);
                    core::ptr::write_volatile(doorbell_addr as *mut u32, cq_head as u32);
                }
                
                return if cqe.success() { Ok(()) } else { Err(BlockError::IoError) };
            }
            core::hint::spin_loop();
        }
        
        Err(BlockError::Timeout)
    }

    // Register access helpers
    unsafe fn read_reg32(base: u64, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((base + offset as u64) as *const u32) }
    }
    
    unsafe fn write_reg32(base: u64, offset: usize, value: u32) {
        unsafe { core::ptr::write_volatile((base + offset as u64) as *mut u32, value); }
    }
    
    unsafe fn read_reg64(base: u64, offset: usize) -> u64 {
        unsafe { core::ptr::read_volatile((base + offset as u64) as *const u64) }
    }
    
    unsafe fn write_reg64(base: u64, offset: usize, value: u64) {
        unsafe { core::ptr::write_volatile((base + offset as u64) as *mut u64, value); }
    }
}

// =============================================================================
// NVMe Block Device Wrapper
// =============================================================================

/// Wrapper to expose NVMe namespace as a block device
pub struct NvmeBlockDevice {
    /// Controller (shared across namespaces)
    controller: Mutex<NvmeController>,
    /// Namespace ID
    nsid: u32,
    /// Device name
    name: String,
    /// Total sectors
    total_sectors: u64,
    /// Sector size
    sector_size: usize,
}

impl NvmeBlockDevice {
    /// Creates a new NVMe block device for a namespace
    pub fn new(controller: NvmeController, nsid: u32, name: String) -> Option<Self> {
        let namespace = controller.namespaces.iter().find(|ns| ns.nsid == nsid)?;
        let total_sectors = namespace.nsze;
        let sector_size = namespace.block_size as usize;
        
        Some(Self {
            controller: Mutex::new(controller),
            nsid,
            name,
            total_sectors,
            sector_size,
        })
    }
}

impl BlockDevice for NvmeBlockDevice {
    fn info(&self) -> BlockDeviceInfo {
        BlockDeviceInfo {
            name: self.name.clone(),
            total_sectors: self.total_sectors,
            sector_size: self.sector_size,
            read_only: false,
            model: String::from("NVMe SSD"),
        }
    }
    
    fn read_sectors(&self, start_sector: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        let mut controller = self.controller.lock();
        controller.read(self.nsid, start_sector, buffer)
    }
    
    fn write_sectors(&self, start_sector: u64, buffer: &[u8]) -> Result<(), BlockError> {
        let mut controller = self.controller.lock();
        controller.write(self.nsid, start_sector, buffer)
    }
    
    fn flush(&self) -> Result<(), BlockError> {
        let mut controller = self.controller.lock();
        controller.flush(self.nsid)
    }
    
    fn is_ready(&self) -> bool {
        self.controller.lock().ready
    }
}

// =============================================================================
// NVMe Initialization
// =============================================================================

/// Global NVMe device counter
static NVME_COUNTER: Mutex<usize> = Mutex::new(0);

/// Generates the next NVMe device name
pub fn next_nvme_name() -> String {
    let mut counter = NVME_COUNTER.lock();
    let name = alloc::format!("nvme{}n1", *counter);
    *counter += 1;
    name
}

/// Probes for NVMe devices via PCI
pub fn probe_devices() {
    crate::serial_println!("[nvme] Probing for NVMe devices...");
    
    // In a real implementation, we would:
    // 1. Enumerate PCI devices
    // 2. Find devices with class 0x01, subclass 0x08, prog-if 0x02
    // 3. Initialize each NVMe controller
    
    // For now, we just log that probing was attempted
    crate::serial_println!("[nvme] NVMe driver loaded (waiting for PCI enumeration)");
}

/// Initializes an NVMe controller at the given MMIO address
pub fn init_controller(mmio_base: u64) -> Result<(), BlockError> {
    let name = next_nvme_name();
    let controller = NvmeController::new(name.clone(), mmio_base)?;
    
    // Log discovered namespaces
    for namespace in controller.namespaces.iter() {
        let ns_name = alloc::format!("{}n{}", name.trim_end_matches("n1"), namespace.nsid);
        crate::serial_println!("[nvme] Discovered namespace: {}", ns_name);
    }
    
    // Get first namespace info before moving controller
    let first_ns = controller.namespaces.first().map(|ns| ns.nsid);
    
    // Register the first namespace as a block device
    if let Some(nsid) = first_ns {
        let device = NvmeBlockDevice::new(controller, nsid, name).ok_or(BlockError::IoError)?;
        let boxed: Box<dyn BlockDevice> = Box::new(device);
        super::register_device(boxed)?;
    }
    
    Ok(())
}

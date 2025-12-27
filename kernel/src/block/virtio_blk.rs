//! # VirtIO Block Device Driver
//!
//! VirtIO block device driver for virtual disks in QEMU/KVM.
//!
//! ## Protocol
//!
//! VirtIO-blk uses a simple request/response protocol:
//! 1. Driver creates a request header (type, sector)
//! 2. Driver adds data buffer (for read/write)
//! 3. Driver adds status byte
//! 4. Device processes and writes status
//!
//! ## References
//!
//! - VirtIO Spec 1.1, Section 5.2 (Block Device)

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use super::{BlockDevice, BlockDeviceInfo, BlockError, SECTOR_SIZE};

/// VirtIO block device ID (transitional)
pub const VIRTIO_BLK_DEVICE_ID_TRANSITIONAL: u16 = 0x1001;

/// VirtIO block device ID (modern)  
pub const VIRTIO_BLK_DEVICE_ID: u16 = 0x1042;

/// VirtIO vendor ID
const VIRTIO_VENDOR_ID: u16 = 0x1AF4;

/// VirtIO block request types
pub mod request_type {
    pub const VIRTIO_BLK_T_IN: u32 = 0;      // Read
    pub const VIRTIO_BLK_T_OUT: u32 = 1;     // Write
    pub const VIRTIO_BLK_T_FLUSH: u32 = 4;   // Flush
    pub const VIRTIO_BLK_T_DISCARD: u32 = 11; // Discard
}

/// VirtIO block status codes
pub mod status {
    pub const VIRTIO_BLK_S_OK: u8 = 0;
    pub const VIRTIO_BLK_S_IOERR: u8 = 1;
    pub const VIRTIO_BLK_S_UNSUPP: u8 = 2;
}

/// VirtIO device status flags
pub mod device_status {
    pub const RESET: u8 = 0;
    pub const ACKNOWLEDGE: u8 = 1;
    pub const DRIVER: u8 = 2;
    pub const DRIVER_OK: u8 = 4;
    pub const FEATURES_OK: u8 = 8;
    pub const FAILED: u8 = 128;
}

/// VirtIO block feature bits
pub mod features {
    pub const VIRTIO_BLK_F_SIZE_MAX: u32 = 1 << 1;
    pub const VIRTIO_BLK_F_SEG_MAX: u32 = 1 << 2;
    pub const VIRTIO_BLK_F_GEOMETRY: u32 = 1 << 4;
    pub const VIRTIO_BLK_F_RO: u32 = 1 << 5;
    pub const VIRTIO_BLK_F_BLK_SIZE: u32 = 1 << 6;
    pub const VIRTIO_BLK_F_FLUSH: u32 = 1 << 9;
    pub const VIRTIO_BLK_F_TOPOLOGY: u32 = 1 << 10;
    pub const VIRTIO_BLK_F_CONFIG_WCE: u32 = 1 << 11;
    pub const VIRTIO_BLK_F_DISCARD: u32 = 1 << 13;
}

/// Legacy VirtIO PIO register offsets
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
    pub const CONFIG_GEOMETRY: u16 = 0x24;      // 4 bytes
    pub const CONFIG_BLK_SIZE: u16 = 0x28;      // 4 bytes (u32)
}

/// VirtIO block request header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VirtioBlkReqHeader {
    pub request_type: u32,
    pub reserved: u32,
    pub sector: u64,
}

/// VirtIO ring descriptor
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

/// Descriptor flags
mod desc_flags {
    pub const NEXT: u16 = 1;
    pub const WRITE: u16 = 2;
}

/// Queue size
const QUEUE_SIZE: usize = 128;

/// Page size for alignment
const PAGE_SIZE: usize = 4096;

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
    pub id: u32,
    pub len: u32,
}

/// Used ring
#[repr(C)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtqUsedElem; QUEUE_SIZE],
    pub avail_event: u16,
}

/// Virtqueue for block I/O
pub struct Virtqueue {
    pub base_addr: u64,
    pub desc: &'static mut [VirtqDesc],
    pub avail: &'static mut VirtqAvail,
    pub used: &'static mut VirtqUsed,
    pub avail_idx: u16,
    pub last_used_idx: u16,
    pub free_desc: VecDeque<u16>,
}

/// Aligns up to alignment
#[inline]
const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// VirtIO block device
pub struct VirtioBlkDevice {
    /// Device name
    name: String,
    /// PCI I/O base port
    io_base: u16,
    /// Total capacity in sectors
    capacity: u64,
    /// Sector size
    sector_size: usize,
    /// Whether device is read-only
    read_only: bool,
    /// Request virtqueue
    queue: Mutex<Option<Virtqueue>>,
    /// Device ready flag
    ready: AtomicBool,
    /// Request buffer pool
    request_buffers: Mutex<Vec<Vec<u8>>>,
}

impl VirtioBlkDevice {
    /// Creates a new VirtIO block device
    pub fn new(name: String, io_base: u16) -> Result<Self, BlockError> {
        let device = Self {
            name,
            io_base,
            capacity: 0,
            sector_size: SECTOR_SIZE,
            read_only: false,
            queue: Mutex::new(None),
            ready: AtomicBool::new(false),
            request_buffers: Mutex::new(Vec::new()),
        };

        Ok(device)
    }

    /// Initializes the device
    pub fn init(&mut self) -> Result<(), BlockError> {
        crate::serial_println!("[VIRTIO-BLK] Initializing device at I/O base 0x{:04x}", self.io_base);

        // Reset device
        self.write_status(device_status::RESET);

        // Acknowledge device
        self.write_status(device_status::ACKNOWLEDGE);
        self.write_status(device_status::ACKNOWLEDGE | device_status::DRIVER);

        // Read device features
        let features = self.read_features();
        crate::serial_println!("[VIRTIO-BLK] Device features: 0x{:08x}", features);

        // Check if read-only
        self.read_only = (features & features::VIRTIO_BLK_F_RO) != 0;
        if self.read_only {
            crate::serial_println!("[VIRTIO-BLK] Device is read-only");
        }

        // Accept features we support
        let accepted = features & (features::VIRTIO_BLK_F_FLUSH | features::VIRTIO_BLK_F_BLK_SIZE);
        self.write_features(accepted);

        // Set features OK
        self.write_status(device_status::ACKNOWLEDGE | device_status::DRIVER | device_status::FEATURES_OK);

        // Verify features accepted
        let status = self.read_status();
        if (status & device_status::FEATURES_OK) == 0 {
            crate::serial_println!("[VIRTIO-BLK] Features not accepted!");
            self.write_status(device_status::FAILED);
            return Err(BlockError::NotReady);
        }

        // Read capacity
        self.capacity = self.read_capacity();
        crate::serial_println!("[VIRTIO-BLK] Capacity: {} sectors ({} MB)",
            self.capacity,
            (self.capacity * SECTOR_SIZE as u64) / (1024 * 1024));

        // Setup virtqueue
        self.setup_queue()?;

        // Mark driver ready
        self.write_status(
            device_status::ACKNOWLEDGE | 
            device_status::DRIVER | 
            device_status::FEATURES_OK | 
            device_status::DRIVER_OK
        );

        self.ready.store(true, Ordering::SeqCst);
        crate::serial_println!("[VIRTIO-BLK] Device {} ready", self.name);

        Ok(())
    }

    /// Sets up the virtqueue
    fn setup_queue(&mut self) -> Result<(), BlockError> {
        // Select queue 0 (the only queue for block devices)
        self.port_write_u16(regs::QUEUE_SELECT, 0);

        // Read queue size
        let queue_size = self.port_read_u16(regs::QUEUE_SIZE) as usize;
        if queue_size == 0 {
            crate::serial_println!("[VIRTIO-BLK] Queue 0 not available");
            return Err(BlockError::NotReady);
        }
        crate::serial_println!("[VIRTIO-BLK] Queue size: {}", queue_size);

        // Calculate queue memory layout
        let desc_size = core::mem::size_of::<VirtqDesc>() * QUEUE_SIZE;
        let avail_size = 4 + 2 * QUEUE_SIZE + 2;
        let used_size = 4 + 8 * QUEUE_SIZE + 2;

        let desc_offset = 0;
        let avail_offset = align_up(desc_size, 2);
        let used_offset = align_up(avail_offset + avail_size, PAGE_SIZE);
        let total_size = used_offset + used_size;

        // Allocate queue memory
        let queue_mem = vec![0u8; total_size + PAGE_SIZE];
        let queue_base = align_up(queue_mem.as_ptr() as usize, PAGE_SIZE);

        // Leak the memory (it needs to persist)
        core::mem::forget(queue_mem);

        // Setup pointers
        let desc_ptr = queue_base as *mut VirtqDesc;
        let avail_ptr = (queue_base + avail_offset) as *mut VirtqAvail;
        let used_ptr = (queue_base + used_offset) as *mut VirtqUsed;

        // Create queue structure
        let desc = unsafe { core::slice::from_raw_parts_mut(desc_ptr, QUEUE_SIZE) };
        let avail = unsafe { &mut *avail_ptr };
        let used = unsafe { &mut *used_ptr };

        // Initialize free descriptor list
        let mut free_desc = VecDeque::new();
        for i in 0..QUEUE_SIZE as u16 {
            free_desc.push_back(i);
        }

        let queue = Virtqueue {
            base_addr: queue_base as u64,
            desc,
            avail,
            used,
            avail_idx: 0,
            last_used_idx: 0,
            free_desc,
        };

        // Tell device the queue address (in pages)
        let queue_pfn = (queue_base / PAGE_SIZE) as u32;
        self.port_write_u32(regs::QUEUE_ADDRESS, queue_pfn);

        *self.queue.lock() = Some(queue);

        crate::serial_println!("[VIRTIO-BLK] Queue setup at 0x{:x}", queue_base);
        Ok(())
    }

    /// Performs a block I/O request
    fn do_request(&self, request_type: u32, sector: u64, data: Option<&mut [u8]>) -> Result<(), BlockError> {
        if !self.ready.load(Ordering::SeqCst) {
            return Err(BlockError::NotReady);
        }

        let mut queue_guard = self.queue.lock();
        let queue = queue_guard.as_mut().ok_or(BlockError::NotReady)?;

        // Need 3 descriptors: header, data, status
        if queue.free_desc.len() < 3 {
            return Err(BlockError::Busy);
        }

        // Allocate request header
        let header = VirtioBlkReqHeader {
            request_type,
            reserved: 0,
            sector,
        };
        let header_buf = vec![0u8; core::mem::size_of::<VirtioBlkReqHeader>()];
        let header_ptr = header_buf.as_ptr() as *mut VirtioBlkReqHeader;
        unsafe { *header_ptr = header; }

        // Status byte
        let status_buf = vec![0u8; 1];

        // Get descriptors
        let desc_header = queue.free_desc.pop_front().unwrap();
        let desc_data = queue.free_desc.pop_front().unwrap();
        let desc_status = queue.free_desc.pop_front().unwrap();

        // Setup header descriptor
        queue.desc[desc_header as usize] = VirtqDesc {
            addr: header_buf.as_ptr() as u64,
            len: core::mem::size_of::<VirtioBlkReqHeader>() as u32,
            flags: desc_flags::NEXT,
            next: desc_data,
        };

        // Setup data descriptor
        let data_flags = if request_type == request_type::VIRTIO_BLK_T_IN {
            desc_flags::NEXT | desc_flags::WRITE  // Device writes to buffer (read)
        } else {
            desc_flags::NEXT  // Device reads from buffer (write)
        };

        let (data_ptr, data_len) = if let Some(buf) = data {
            (buf.as_ptr() as u64, buf.len() as u32)
        } else {
            (0, 0)
        };

        queue.desc[desc_data as usize] = VirtqDesc {
            addr: data_ptr,
            len: data_len,
            flags: data_flags,
            next: desc_status,
        };

        // Setup status descriptor
        queue.desc[desc_status as usize] = VirtqDesc {
            addr: status_buf.as_ptr() as u64,
            len: 1,
            flags: desc_flags::WRITE,
            next: 0,
        };

        // Add to available ring
        let avail_idx = queue.avail_idx;
        queue.avail.ring[(avail_idx as usize) % QUEUE_SIZE] = desc_header;
        core::sync::atomic::fence(Ordering::SeqCst);
        queue.avail.idx = avail_idx.wrapping_add(1);
        queue.avail_idx = avail_idx.wrapping_add(1);

        // Notify device
        self.port_write_u16(regs::QUEUE_NOTIFY, 0);

        // Wait for completion (busy polling)
        let mut timeout = 1_000_000u32;
        loop {
            core::sync::atomic::fence(Ordering::SeqCst);
            if queue.used.idx != queue.last_used_idx {
                break;
            }
            timeout -= 1;
            if timeout == 0 {
                // Return descriptors
                queue.free_desc.push_back(desc_header);
                queue.free_desc.push_back(desc_data);
                queue.free_desc.push_back(desc_status);
                return Err(BlockError::Timeout);
            }
            core::hint::spin_loop();
        }

        // Process completion
        queue.last_used_idx = queue.last_used_idx.wrapping_add(1);

        // Check status
        let result_status = status_buf[0];

        // Return descriptors
        queue.free_desc.push_back(desc_header);
        queue.free_desc.push_back(desc_data);
        queue.free_desc.push_back(desc_status);

        // Keep buffers alive until here
        drop(header_buf);
        drop(status_buf);

        match result_status {
            status::VIRTIO_BLK_S_OK => Ok(()),
            status::VIRTIO_BLK_S_IOERR => Err(BlockError::IoError),
            status::VIRTIO_BLK_S_UNSUPP => Err(BlockError::Unsupported),
            _ => Err(BlockError::IoError),
        }
    }

    // Port I/O helpers using inline assembly
    fn port_read_u8(&self, offset: u16) -> u8 {
        let port = self.io_base + offset;
        let value: u8;
        unsafe {
            core::arch::asm!(
                "in al, dx",
                out("al") value,
                in("dx") port,
                options(nomem, nostack, preserves_flags)
            );
        }
        value
    }

    fn port_read_u16(&self, offset: u16) -> u16 {
        let port = self.io_base + offset;
        let value: u16;
        unsafe {
            core::arch::asm!(
                "in ax, dx",
                out("ax") value,
                in("dx") port,
                options(nomem, nostack, preserves_flags)
            );
        }
        value
    }

    fn port_read_u32(&self, offset: u16) -> u32 {
        let port = self.io_base + offset;
        let value: u32;
        unsafe {
            core::arch::asm!(
                "in eax, dx",
                out("eax") value,
                in("dx") port,
                options(nomem, nostack, preserves_flags)
            );
        }
        value
    }

    fn port_write_u8(&self, offset: u16, value: u8) {
        let port = self.io_base + offset;
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") port,
                in("al") value,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    fn port_write_u16(&self, offset: u16, value: u16) {
        let port = self.io_base + offset;
        unsafe {
            core::arch::asm!(
                "out dx, ax",
                in("dx") port,
                in("ax") value,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    fn port_write_u32(&self, offset: u16, value: u32) {
        let port = self.io_base + offset;
        unsafe {
            core::arch::asm!(
                "out dx, eax",
                in("dx") port,
                in("eax") value,
                options(nomem, nostack, preserves_flags)
            );
        }
    }

    fn read_status(&self) -> u8 {
        self.port_read_u8(regs::DEVICE_STATUS)
    }

    fn write_status(&self, status: u8) {
        self.port_write_u8(regs::DEVICE_STATUS, status);
    }

    fn read_features(&self) -> u32 {
        self.port_read_u32(regs::DEVICE_FEATURES)
    }

    fn write_features(&self, features: u32) {
        self.port_write_u32(regs::GUEST_FEATURES, features);
    }

    fn read_capacity(&self) -> u64 {
        let low = self.port_read_u32(regs::CONFIG_CAPACITY) as u64;
        let high = self.port_read_u32(regs::CONFIG_CAPACITY + 4) as u64;
        low | (high << 32)
    }
}

impl BlockDevice for VirtioBlkDevice {
    fn info(&self) -> BlockDeviceInfo {
        BlockDeviceInfo {
            name: self.name.clone(),
            total_sectors: self.capacity,
            sector_size: self.sector_size,
            read_only: self.read_only,
            model: alloc::format!("VirtIO Block Device"),
        }
    }

    fn read_sectors(&self, start_sector: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        if start_sector >= self.capacity {
            return Err(BlockError::InvalidSector);
        }
        if buffer.len() % self.sector_size != 0 {
            return Err(BlockError::InvalidSize);
        }

        self.do_request(request_type::VIRTIO_BLK_T_IN, start_sector, Some(buffer))
    }

    fn write_sectors(&self, start_sector: u64, buffer: &[u8]) -> Result<(), BlockError> {
        if self.read_only {
            return Err(BlockError::WriteProtected);
        }
        if start_sector >= self.capacity {
            return Err(BlockError::InvalidSector);
        }
        if buffer.len() % self.sector_size != 0 {
            return Err(BlockError::InvalidSize);
        }

        // Need mutable reference for the trait, but we're not actually modifying for writes
        let buffer_ptr = buffer.as_ptr() as *mut u8;
        let buffer_mut = unsafe { core::slice::from_raw_parts_mut(buffer_ptr, buffer.len()) };
        self.do_request(request_type::VIRTIO_BLK_T_OUT, start_sector, Some(buffer_mut))
    }

    fn flush(&self) -> Result<(), BlockError> {
        self.do_request(request_type::VIRTIO_BLK_T_FLUSH, 0, None)
    }

    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

// Make the device safe to share between threads
unsafe impl Send for VirtioBlkDevice {}
unsafe impl Sync for VirtioBlkDevice {}

/// Probes for VirtIO block devices on the PCI bus
pub fn probe_devices() {
    crate::serial_println!("[VIRTIO-BLK] Probing for VirtIO block devices...");

    let mut found_count = 0u32;
    
    // Scan PCI bus for VirtIO block devices
    // Only scan bus 0 device 0-7 for speed (QEMU puts devices on bus 0)
    for device in 0..8u8 {
        // Check if device exists (vendor ID != 0xFFFF)
        let vendor_device = pci_read_config(0, device, 0, 0);
        let vendor_id = (vendor_device & 0xFFFF) as u16;
        if vendor_id == 0xFFFF || vendor_id == 0 {
            continue; // No device here
        }
        
        // Check if this is a VirtIO block device
        if let Some(io_base) = check_pci_device(0, device, 0) {
            let name = super::next_virtio_name();
            match create_and_register_device(name.clone(), io_base) {
                Ok(_) => {
                    found_count += 1;
                },
                Err(_e) => {},
            }
        }
    }
    
    crate::serial_println!("[VIRTIO-BLK] Probe complete, found {} devices", found_count);
}

/// Checks a PCI device for VirtIO block
fn check_pci_device(bus: u8, device: u8, function: u8) -> Option<u16> {
    let vendor_device = pci_read_config(bus, device, function, 0);
    let vendor_id = (vendor_device & 0xFFFF) as u16;
    let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;

    if vendor_id != VIRTIO_VENDOR_ID {
        return None;
    }

    // Check for VirtIO block device (transitional or modern)
    if device_id != VIRTIO_BLK_DEVICE_ID_TRANSITIONAL && device_id != VIRTIO_BLK_DEVICE_ID {
        return None;
    }

    // Get I/O base address from BAR0
    let bar0 = pci_read_config(bus, device, function, 0x10);
    if (bar0 & 1) == 0 {
        // Memory-mapped - not supported yet
        return None;
    }

    let io_base = (bar0 & 0xFFFC) as u16;
    crate::serial_println!("[VIRTIO-BLK] Found device at {:02x}:{:02x}.{} IO=0x{:04x}",
        bus, device, function, io_base);

    Some(io_base)
}

/// Creates and registers a VirtIO block device
fn create_and_register_device(name: String, io_base: u16) -> Result<(), BlockError> {
    let mut device = VirtioBlkDevice::new(name, io_base)?;
    device.init()?;
    super::register_device(Box::new(device))?;
    Ok(())
}

/// Reads PCI configuration space
fn pci_read_config(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = 0x8000_0000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC);

    unsafe {
        // Write address to CONFIG_ADDRESS (0xCF8)
        core::arch::asm!(
            "out dx, eax",
            in("dx") 0xCF8u16,
            in("eax") address,
            options(nomem, nostack, preserves_flags)
        );
        
        // Read data from CONFIG_DATA (0xCFC)
        let value: u32;
        core::arch::asm!(
            "in eax, dx",
            out("eax") value,
            in("dx") 0xCFCu16,
            options(nomem, nostack, preserves_flags)
        );
        value
    }
}

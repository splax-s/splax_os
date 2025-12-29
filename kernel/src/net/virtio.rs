//! # VirtIO Network Driver
//!
//! Real VirtIO network device driver for QEMU/KVM virtualization.
//!
//! This driver implements the VirtIO 1.0 legacy (transitional) interface
//! which uses PIO for device communication.

use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use spin::Mutex;

use super::device::{NetworkDevice, NetworkDeviceInfo, NetworkError};
use super::ethernet::MacAddress;

/// VirtIO vendor ID.
pub const VIRTIO_VENDOR_ID: u16 = 0x1AF4;

/// VirtIO network device ID (transitional).
pub const VIRTIO_NET_DEVICE_ID_TRANSITIONAL: u16 = 0x1000;

/// VirtIO network device ID (modern).
pub const VIRTIO_NET_DEVICE_ID: u16 = 0x1041;

/// VirtIO device status flags.
pub mod status {
    pub const RESET: u8 = 0;
    pub const ACKNOWLEDGE: u8 = 1;
    pub const DRIVER: u8 = 2;
    pub const DRIVER_OK: u8 = 4;
    pub const FEATURES_OK: u8 = 8;
    pub const DEVICE_NEEDS_RESET: u8 = 64;
    pub const FAILED: u8 = 128;
}

/// Legacy VirtIO PIO register offsets
pub mod regs {
    pub const DEVICE_FEATURES: u16 = 0x00;      // 4 bytes
    pub const GUEST_FEATURES: u16 = 0x04;       // 4 bytes
    pub const QUEUE_ADDRESS: u16 = 0x08;        // 4 bytes
    pub const QUEUE_SIZE: u16 = 0x0C;           // 2 bytes
    pub const QUEUE_SELECT: u16 = 0x0E;         // 2 bytes
    pub const QUEUE_NOTIFY: u16 = 0x10;         // 2 bytes
    pub const DEVICE_STATUS: u16 = 0x12;        // 1 byte
    pub const ISR_STATUS: u16 = 0x13;           // 1 byte
    pub const CONFIG_SPACE: u16 = 0x14;         // Device-specific config starts here
}

/// VirtIO network feature bits.
pub mod features {
    pub const VIRTIO_NET_F_CSUM: u32 = 1 << 0;
    pub const VIRTIO_NET_F_GUEST_CSUM: u32 = 1 << 1;
    pub const VIRTIO_NET_F_MAC: u32 = 1 << 5;
    pub const VIRTIO_NET_F_STATUS: u32 = 1 << 16;
}

/// VirtIO network header (10 bytes for legacy).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtioNetHeader {
    pub flags: u8,
    pub gso_type: u8,
    pub hdr_len: u16,
    pub gso_size: u16,
    pub csum_start: u16,
    pub csum_offset: u16,
}

impl VirtioNetHeader {
    pub const SIZE: usize = 10;
}

/// Device statistics with atomic counters.
pub struct DeviceStats {
    pub tx_packets: AtomicU64,
    pub tx_bytes: AtomicU64,
    pub tx_errors: AtomicU64,
    pub tx_dropped: AtomicU64,
    pub rx_packets: AtomicU64,
    pub rx_bytes: AtomicU64,
    pub rx_errors: AtomicU64,
    pub rx_dropped: AtomicU64,
}

impl DeviceStats {
    pub const fn new() -> Self {
        Self {
            tx_packets: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            tx_errors: AtomicU64::new(0),
            tx_dropped: AtomicU64::new(0),
            rx_packets: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            rx_errors: AtomicU64::new(0),
            rx_dropped: AtomicU64::new(0),
        }
    }
}

/// VirtIO descriptor flags.
pub mod descriptor_flags {
    pub const NEXT: u16 = 1;
    pub const WRITE: u16 = 2;
}

/// VirtIO ring descriptor (16 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

/// Queue size we'll use - must match QEMU's default (256)
const QUEUE_SIZE: usize = 256;

/// Size of a single RX buffer
const RX_BUFFER_SIZE: usize = 2048;

/// Page size for alignment
const PAGE_SIZE: usize = 4096;

/// Aligns a value up to the specified alignment.
#[inline]
const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

/// Converts a virtual address to physical address.
/// In identity-mapped mode (early boot), virt == phys.
/// Converts a virtual address to physical address.
/// Uses identity mapping for now - when full paging is enabled,
/// this should walk the page tables to translate addresses.
pub fn virt_to_phys(virt: u64) -> u64 {
    // For kernel addresses in identity-mapped region, virtual == physical
    // When full paging is enabled, this would:
    // 1. Walk the 4-level page table (PML4 -> PDPT -> PD -> PT)
    // 2. Handle large pages (2MB, 1GB)
    // 3. Return the physical address from the PTE
    
    // Check if address is in kernel identity-mapped region
    #[cfg(target_arch = "x86_64")]
    {
        // Kernel addresses typically start at 0xFFFF_8000_0000_0000
        // and are identity-mapped to physical memory
        const KERNEL_PHYS_OFFSET: u64 = 0xFFFF_8000_0000_0000;
        if virt >= KERNEL_PHYS_OFFSET {
            return virt - KERNEL_PHYS_OFFSET;
        }
    }
    
    // For low addresses or AArch64, assume identity mapping
    virt
}

/// A virtqueue with its memory laid out for DMA.
pub struct Virtqueue {
    /// Base physical address of the queue memory
    pub base_addr: u64,
    /// Pointer to descriptors
    pub desc: &'static mut [VirtqDesc],
    /// Available ring
    pub avail: &'static mut VirtqAvail,
    /// Used ring
    pub used: &'static mut VirtqUsed,
    /// Our index into the available ring
    pub avail_idx: u16,
    /// Last used index we processed
    pub last_used_idx: u16,
    /// Free descriptor indices
    pub free_desc: VecDeque<u16>,
    /// Buffers associated with each descriptor
    pub buffers: Vec<Option<Vec<u8>>>,
}

/// Available ring structure
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

/// Used ring structure
#[repr(C)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtqUsedElem; QUEUE_SIZE],
    pub avail_event: u16,
}

impl Virtqueue {
    /// Creates a new virtqueue with properly aligned memory for DMA.
    /// 
    /// VirtIO legacy requires:
    /// - Descriptors at page-aligned address
    /// - Available ring follows descriptors  
    /// - Used ring at next page boundary after avail
    pub fn new() -> Self {
        // Calculate sizes
        let desc_size = core::mem::size_of::<VirtqDesc>() * QUEUE_SIZE;
        let avail_size = 6 + 2 * QUEUE_SIZE; // flags(2) + idx(2) + ring(2*N) + used_event(2)
        let used_size = 6 + 8 * QUEUE_SIZE;  // flags(2) + idx(2) + ring(8*N) + avail_event(2)
        
        // Allocate page-aligned memory for the entire queue structure
        // Descriptors + Available ring contiguous, Used ring at page boundary
        let total_size = align_up(desc_size + avail_size, 4096) + align_up(used_size, 4096);
        let layout = core::alloc::Layout::from_size_align(total_size, 4096).unwrap();
        let base_ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
        
        if base_ptr.is_null() {
            panic!("Failed to allocate virtqueue memory");
        }
        
        // Set up pointers
        let desc_ptr = base_ptr as *mut VirtqDesc;
        let avail_ptr = unsafe { base_ptr.add(desc_size) as *mut VirtqAvail };
        let used_ptr = unsafe { base_ptr.add(align_up(desc_size + avail_size, 4096)) as *mut VirtqUsed };
        
        let desc = unsafe { core::slice::from_raw_parts_mut(desc_ptr, QUEUE_SIZE) };
        let avail = unsafe { &mut *avail_ptr };
        let used = unsafe { &mut *used_ptr };
        
        // Initialize free descriptor list
        let mut free_desc = VecDeque::with_capacity(QUEUE_SIZE);
        for i in 0..QUEUE_SIZE {
            free_desc.push_back(i as u16);
        }
        
        // Buffers storage
        let mut buffers = Vec::with_capacity(QUEUE_SIZE);
        for _ in 0..QUEUE_SIZE {
            buffers.push(None);
        }
        
        Self {
            base_addr: base_ptr as u64,
            desc,
            avail,
            used,
            avail_idx: 0,
            last_used_idx: 0,
            free_desc,
            buffers,
        }
    }
    
    /// Allocates a descriptor index.
    pub fn alloc_desc(&mut self) -> Option<u16> {
        self.free_desc.pop_front()
    }
    
    /// Frees a descriptor index.
    pub fn free_desc(&mut self, idx: u16) {
        self.free_desc.push_back(idx);
    }
    
    /// Adds a buffer to the available ring.
    pub fn add_buffer(&mut self, desc_idx: u16) {
        let avail_idx = self.avail_idx as usize % QUEUE_SIZE;
        self.avail.ring[avail_idx] = desc_idx;
        
        core::sync::atomic::fence(Ordering::SeqCst);
        
        self.avail_idx = self.avail_idx.wrapping_add(1);
        self.avail.idx = self.avail_idx;
    }
    
    /// Checks if there are used buffers to process.
    pub fn has_used(&self) -> bool {
        core::sync::atomic::fence(Ordering::SeqCst);
        self.used.idx != self.last_used_idx
    }
    
    /// Gets the next used buffer.
    pub fn pop_used(&mut self) -> Option<(u16, u32)> {
        if !self.has_used() {
            return None;
        }
        
        let idx = self.last_used_idx as usize % QUEUE_SIZE;
        let elem = self.used.ring[idx];
        self.last_used_idx = self.last_used_idx.wrapping_add(1);
        
        Some((elem.id as u16, elem.len))
    }
}

/// VirtIO network device with real I/O.
pub struct VirtioNetDevice {
    /// MAC address.
    mac: MacAddress,
    /// Receive queue (queue 0).
    rx_queue: Mutex<Virtqueue>,
    /// Transmit queue (queue 1).
    tx_queue: Mutex<Virtqueue>,
    /// Received packets waiting to be processed.
    rx_pending: Mutex<VecDeque<Vec<u8>>>,
    /// Link status.
    link_up: AtomicBool,
    /// MTU.
    mtu: u16,
    /// Base I/O port address.
    io_base: u16,
    /// Initialized flag.
    initialized: AtomicBool,
    /// TX packets sent counter
    tx_count: AtomicU16,
    /// RX packets received counter
    rx_count: AtomicU16,
    /// Device statistics
    stats: DeviceStats,
}

impl VirtioNetDevice {
    /// Creates a new VirtIO network device.
    pub fn new(io_base: u16, mac: MacAddress) -> Self {
        Self {
            mac,
            rx_queue: Mutex::new(Virtqueue::new()),
            tx_queue: Mutex::new(Virtqueue::new()),
            rx_pending: Mutex::new(VecDeque::new()),
            link_up: AtomicBool::new(false),
            mtu: 1500,
            io_base,
            initialized: AtomicBool::new(false),
            tx_count: AtomicU16::new(0),
            rx_count: AtomicU16::new(0),
            stats: DeviceStats::new(),
        }
    }
    
    /// Initializes the device following the VirtIO specification.
    pub fn init(&self) -> Result<(), NetworkError> {
        if self.io_base == 0 {
            // Mock device, skip real initialization
            self.link_up.store(true, Ordering::SeqCst);
            self.initialized.store(true, Ordering::SeqCst);
            return Ok(());
        }
        
        // 1. Reset the device
        self.write_status(status::RESET);
        
        // 2. Set ACKNOWLEDGE status bit
        self.write_status(status::ACKNOWLEDGE);
        
        // 3. Set DRIVER status bit
        self.write_status(status::ACKNOWLEDGE | status::DRIVER);
        
        // 4. Read device features
        let device_features = self.read_features();
        
        // 5. Negotiate features (we want MAC address feature)
        let our_features = device_features & features::VIRTIO_NET_F_MAC;
        self.write_features(our_features);
        
        // 6. Set FEATURES_OK
        self.write_status(status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK);
        
        // 7. Check that FEATURES_OK is still set
        let current_status = self.read_status();
        if current_status & status::FEATURES_OK == 0 {
            self.write_status(status::FAILED);
            return Err(NetworkError::NotReady);
        }
        
        // 8. Set up virtqueues
        self.setup_rx_queue()?;
        self.setup_tx_queue()?;
        
        // 9. Set DRIVER_OK
        self.write_status(status::ACKNOWLEDGE | status::DRIVER | status::FEATURES_OK | status::DRIVER_OK);
        
        self.link_up.store(true, Ordering::SeqCst);
        self.initialized.store(true, Ordering::SeqCst);
        
        Ok(())
    }
    
    /// Sets up the receive queue.
    fn setup_rx_queue(&self) -> Result<(), NetworkError> {
        // Select queue 0 (RX)
        self.write_queue_select(0);
        
        // Get queue size from device
        let queue_size = self.read_queue_size();
        
        if queue_size == 0 {
            return Err(NetworkError::NotReady);
        }
        
        let mut rx_queue = self.rx_queue.lock();
        
        // Tell device where queue is (page-aligned physical address >> 12)
        let queue_phys = virt_to_phys(rx_queue.base_addr);
        let queue_pfn = (queue_phys >> 12) as u32;
        
        self.write_queue_address(queue_pfn);
        
        // Pre-populate RX queue with buffers (use more buffers for better throughput)
        let num_rx_buffers = 32.min(QUEUE_SIZE);
        for _i in 0..num_rx_buffers {
            let buffer = vec![0u8; VirtioNetHeader::SIZE + RX_BUFFER_SIZE];
            
            if let Some(desc_idx) = rx_queue.alloc_desc() {
                let buf_phys = virt_to_phys(buffer.as_ptr() as u64);
                
                let desc = &mut rx_queue.desc[desc_idx as usize];
                desc.addr = buf_phys;
                desc.len = buffer.len() as u32;
                desc.flags = descriptor_flags::WRITE; // Device writes to this buffer
                desc.next = 0;
                
                rx_queue.buffers[desc_idx as usize] = Some(buffer);
                rx_queue.add_buffer(desc_idx);
            }
        }
        
        // Notify device that RX buffers are available
        drop(rx_queue);
        self.notify_queue(0);
        
        Ok(())
    }
    
    /// Sets up the transmit queue.
    fn setup_tx_queue(&self) -> Result<(), NetworkError> {
        // Select queue 1 (TX)
        self.write_queue_select(1);
        
        // Get queue size from device
        let queue_size = self.read_queue_size();
        
        if queue_size == 0 {
            return Err(NetworkError::NotReady);
        }
        
        let tx_queue = self.tx_queue.lock();
        let queue_phys = virt_to_phys(tx_queue.base_addr);
        let queue_pfn = (queue_phys >> 12) as u32;
        
        self.write_queue_address(queue_pfn);
        
        Ok(())
    }
    
    /// Transmits a packet.
    pub fn transmit(&self, data: &[u8]) -> Result<(), NetworkError> {
        if !self.initialized.load(Ordering::SeqCst) {
            return Err(NetworkError::NotReady);
        }
        
        if self.io_base == 0 {
            // Mock device - just count
            self.tx_count.fetch_add(1, Ordering::Relaxed);
            return Ok(());
        }
        
        // First, reclaim any completed TX buffers
        self.reclaim_tx_buffers();
        
        let mut tx_queue = self.tx_queue.lock();
        
        let desc_idx = tx_queue.alloc_desc().ok_or(NetworkError::NoBuffer)?;
        
        // Create buffer with virtio header + data
        // Use page-aligned buffer for better DMA performance
        let total_len = VirtioNetHeader::SIZE + data.len();
        let mut buffer = vec![0u8; total_len];
        // VirtioNetHeader is already zeroed (no checksum offload, no GSO)
        buffer[VirtioNetHeader::SIZE..].copy_from_slice(data);
        
        let buf_phys = virt_to_phys(buffer.as_ptr() as u64);
        
        let desc = &mut tx_queue.desc[desc_idx as usize];
        desc.addr = buf_phys;
        desc.len = buffer.len() as u32;
        desc.flags = 0; // No WRITE flag for TX (device reads from buffer)
        desc.next = 0;
        
        tx_queue.buffers[desc_idx as usize] = Some(buffer);
        tx_queue.add_buffer(desc_idx);
        
        // Memory barrier before notifying device
        core::sync::atomic::fence(Ordering::SeqCst);
        
        // Notify device (queue 1 = TX)
        self.notify_queue(1);
        
        self.tx_count.fetch_add(1, Ordering::Relaxed);
        
        Ok(())
    }
    
    /// Reclaims completed TX buffers from the used ring.
    fn reclaim_tx_buffers(&self) {
        let mut tx_queue = self.tx_queue.lock();
        
        while let Some((desc_idx, _len)) = tx_queue.pop_used() {
            // Free the buffer
            tx_queue.buffers[desc_idx as usize] = None;
            tx_queue.free_desc(desc_idx);
        }
    }
    
    /// Polls for received packets.
    pub fn poll_rx(&self) {
        if self.io_base == 0 {
            return;
        }
        
        // Check ISR to acknowledge interrupt
        let _isr = self.read_isr();
        
        let mut rx_queue = self.rx_queue.lock();
        let mut rx_pending = self.rx_pending.lock();
        
        while let Some((desc_idx, len)) = rx_queue.pop_used() {
            if let Some(buffer) = rx_queue.buffers[desc_idx as usize].take() {
                if len as usize > VirtioNetHeader::SIZE {
                    let packet_data = buffer[VirtioNetHeader::SIZE..len as usize].to_vec();
                    rx_pending.push_back(packet_data);
                    self.rx_count.fetch_add(1, Ordering::Relaxed);
                }
                
                // Re-add buffer to RX ring with physical address
                let buf_ptr = virt_to_phys(buffer.as_ptr() as u64);
                let buf_len = buffer.len() as u32;
                
                rx_queue.buffers[desc_idx as usize] = Some(buffer);
                
                let desc = &mut rx_queue.desc[desc_idx as usize];
                desc.addr = buf_ptr;
                desc.len = buf_len;
                desc.flags = descriptor_flags::WRITE;
                
                rx_queue.add_buffer(desc_idx);
            }
        }
        
        // Notify device that we've replenished RX buffers
        self.notify_queue(0);
    }
    
    /// Reads the ISR status register (and clears it).
    fn read_isr(&self) -> u8 {
        unsafe { port_read_u8(self.io_base + regs::ISR_STATUS) }
    }
    
    /// Gets a received packet if available.
    pub fn get_rx_packet(&self) -> Option<Vec<u8>> {
        self.poll_rx();
        self.rx_pending.lock().pop_front()
    }
    
    fn read_status(&self) -> u8 {
        unsafe { port_read_u8(self.io_base + regs::DEVICE_STATUS) }
    }
    
    fn write_status(&self, value: u8) {
        unsafe { port_write_u8(self.io_base + regs::DEVICE_STATUS, value) }
    }
    
    fn read_features(&self) -> u32 {
        unsafe { port_read_u32(self.io_base + regs::DEVICE_FEATURES) }
    }
    
    fn write_features(&self, value: u32) {
        unsafe { port_write_u32(self.io_base + regs::GUEST_FEATURES, value) }
    }
    
    fn read_queue_size(&self) -> u16 {
        unsafe { port_read_u16(self.io_base + regs::QUEUE_SIZE) }
    }
    
    fn write_queue_select(&self, queue: u16) {
        unsafe { port_write_u16(self.io_base + regs::QUEUE_SELECT, queue) }
    }
    
    fn write_queue_address(&self, pfn: u32) {
        unsafe { port_write_u32(self.io_base + regs::QUEUE_ADDRESS, pfn) }
    }
    
    fn notify_queue(&self, queue: u16) {
        unsafe { port_write_u16(self.io_base + regs::QUEUE_NOTIFY, queue) }
    }
}

impl NetworkDevice for VirtioNetDevice {
    fn info(&self) -> NetworkDeviceInfo {
        NetworkDeviceInfo {
            name: "virtio-net",
            mac: self.mac,
            mtu: self.mtu,
            link_speed: 1000,
            link_up: self.link_up.load(Ordering::SeqCst),
            tx_packets: self.stats.tx_packets.load(Ordering::Relaxed),
            tx_bytes: self.stats.tx_bytes.load(Ordering::Relaxed),
            tx_errors: self.stats.tx_errors.load(Ordering::Relaxed),
            tx_dropped: self.stats.tx_dropped.load(Ordering::Relaxed),
            rx_packets: self.stats.rx_packets.load(Ordering::Relaxed),
            rx_bytes: self.stats.rx_bytes.load(Ordering::Relaxed),
            rx_errors: self.stats.rx_errors.load(Ordering::Relaxed),
            rx_dropped: self.stats.rx_dropped.load(Ordering::Relaxed),
        }
    }
    
    fn send(&self, data: &[u8]) -> Result<(), NetworkError> {
        self.transmit(data)
    }
    
    fn receive(&self) -> Result<Vec<u8>, NetworkError> {
        match self.get_rx_packet() {
            Some(packet) => Ok(packet),
            None => Err(NetworkError::WouldBlock),
        }
    }
    
    fn is_ready(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }
    
    fn link_up(&self) -> bool {
        self.link_up.load(Ordering::SeqCst)
    }
}

// === Port I/O functions ===

#[cfg(target_arch = "x86_64")]
unsafe fn port_read_u8(port: u16) -> u8 {
    use core::arch::asm;
    let mut result: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, lateout("al") result, options(nostack));
    }
    result
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_write_u8(port: u16, value: u8) {
    use core::arch::asm;
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nostack));
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_read_u16(port: u16) -> u16 {
    use core::arch::asm;
    let mut result: u16;
    unsafe {
        asm!("in ax, dx", in("dx") port, lateout("ax") result, options(nostack));
    }
    result
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_write_u16(port: u16, value: u16) {
    use core::arch::asm;
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") value, options(nostack));
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_read_u32(port: u16) -> u32 {
    use core::arch::asm;
    let mut result: u32;
    unsafe {
        asm!("in eax, dx", in("dx") port, lateout("eax") result, options(nostack));
    }
    result
}

#[cfg(target_arch = "x86_64")]
unsafe fn port_write_u32(port: u16, value: u32) {
    use core::arch::asm;
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value, options(nostack));
    }
}

// === PCI Scanning ===

/// Global I/O base for the VirtIO device (for interrupt handling)
static VIRTIO_IO_BASE: AtomicU16 = AtomicU16::new(0);

/// Gets the global VirtIO I/O base address
pub fn get_io_base() -> u16 {
    VIRTIO_IO_BASE.load(Ordering::Relaxed)
}

/// Probes for VirtIO network devices.
/// Returns None if no real VirtIO device is found (no mock fallback).
pub fn probe_virtio_net() -> Option<Arc<Mutex<dyn NetworkDevice + Send>>> {
    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial, "[virtio] Scanning PCI bus for VirtIO network device...");
        }
        
        if let Some(device) = scan_pci_for_virtio_net() {
            return Some(device);
        }
        
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial, "[virtio] No VirtIO device found");
        }
    }
    
    None
}

/// Creates a mock VirtIO network device for testing when no real hardware is available.
pub fn create_mock_device() -> Arc<Mutex<dyn NetworkDevice + Send>> {
    let mac = MacAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    let device = VirtioNetDevice::new(0, mac);
    let _ = device.init();
    Arc::new(Mutex::new(device))
}

#[cfg(target_arch = "x86_64")]
fn scan_pci_for_virtio_net() -> Option<Arc<Mutex<dyn NetworkDevice + Send>>> {
    use core::fmt::Write;
    
    for bus in 0..8u8 {
        for device in 0..32u8 {
            for function in 0..8u8 {
                let addr = pci_address(bus, device, function, 0);
                let vendor_device = unsafe { pci_config_read(addr) };
                
                let vendor_id = (vendor_device & 0xFFFF) as u16;
                let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;
                
                // Skip non-existent devices
                if vendor_id == 0xFFFF {
                    continue;
                }
                
                if vendor_id == VIRTIO_VENDOR_ID && 
                   (device_id == VIRTIO_NET_DEVICE_ID_TRANSITIONAL || device_id == VIRTIO_NET_DEVICE_ID) {
                    
                    let subsystem = unsafe { pci_config_read(pci_address(bus, device, function, 0x2C)) };
                    let subsystem_id = ((subsystem >> 16) & 0xFFFF) as u16;
                    
                    // For transitional devices, subsystem_id == 1 means network
                    if device_id == VIRTIO_NET_DEVICE_ID_TRANSITIONAL && subsystem_id != 1 {
                        continue;
                    }
                    
                    let bar0 = unsafe { pci_config_read(pci_address(bus, device, function, 0x10)) };
                    
                    if bar0 & 1 != 0 {
                        let io_base = (bar0 & 0xFFFC) as u16;
                        let mac = read_virtio_mac(io_base);
                        
                        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                            let _ = writeln!(serial, 
                                "[virtio] Found VirtIO-net at {:02}:{:02}.{} io=0x{:04x} mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                bus, device, function, io_base,
                                mac.0[0], mac.0[1], mac.0[2], mac.0[3], mac.0[4], mac.0[5]);
                        }
                        
                        // Enable bus mastering and I/O space access
                        let cmd = unsafe { pci_config_read(pci_address(bus, device, function, 0x04)) };
                        unsafe { pci_config_write(pci_address(bus, device, function, 0x04), cmd | 0x07) };
                        
                        // Store global I/O base
                        VIRTIO_IO_BASE.store(io_base, Ordering::Relaxed);
                        
                        let dev = VirtioNetDevice::new(io_base, mac);
                        if dev.init().is_ok() {
                            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                                let _ = writeln!(serial, "[virtio] VirtIO-net initialized successfully");
                            }
                            return Some(Arc::new(Mutex::new(dev)));
                        } else {
                            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                                let _ = writeln!(serial, "[virtio] VirtIO-net init failed");
                            }
                        }
                    }
                }
            }
        }
    }
    
    None
}

#[cfg(target_arch = "x86_64")]
fn pci_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x80000000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

#[cfg(target_arch = "x86_64")]
unsafe fn pci_config_read(address: u32) -> u32 {
    use core::arch::asm;
    let mut result: u32;
    unsafe {
        asm!(
            "mov dx, 0xCF8",
            "out dx, eax",
            "mov dx, 0xCFC",
            "in eax, dx",
            in("eax") address,
            lateout("eax") result,
            out("dx") _,
        );
    }
    result
}

#[cfg(target_arch = "x86_64")]
unsafe fn pci_config_write(address: u32, value: u32) {
    use core::arch::asm;
    unsafe {
        asm!(
            "mov dx, 0xCF8",
            "out dx, eax",
            "mov dx, 0xCFC",
            "mov eax, {value:e}",
            "out dx, eax",
            in("eax") address,
            value = in(reg) value,
            out("dx") _,
        );
    }
}

#[cfg(target_arch = "x86_64")]
fn read_virtio_mac(io_base: u16) -> MacAddress {
    let config_offset = io_base + regs::CONFIG_SPACE;
    let mut mac = [0u8; 6];
    for i in 0..6 {
        mac[i] = unsafe { port_read_u8(config_offset + i as u16) };
    }
    MacAddress(mac)
}

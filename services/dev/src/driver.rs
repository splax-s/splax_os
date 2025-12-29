//! # Driver Framework
//!
//! Base driver traits and infrastructure for S-DEV.

use alloc::string::String;
use alloc::vec::Vec;

use super::{DevError, DeviceId, DeviceType};

/// Driver trait - all drivers must implement this
pub trait Driver: Send + Sync {
    /// Driver name
    fn name(&self) -> &str;

    /// Device type this driver handles
    fn device_type(&self) -> DeviceType;

    /// Returns list of supported device IDs
    fn supported_devices(&self) -> Vec<DeviceId>;

    /// Called when driver is loaded
    fn init(&mut self) -> Result<(), DevError>;

    /// Called when driver is unloaded
    fn exit(&mut self);

    /// Called when device is bound to this driver
    fn probe(&mut self, device: &DriverDevice) -> Result<(), DevError>;

    /// Called when device is unbound
    fn remove(&mut self, device: &DriverDevice);

    /// Called when device is suspended
    fn suspend(&mut self, _device: &DriverDevice) -> Result<(), DevError> {
        Ok(())
    }

    /// Called when device is resumed
    fn resume(&mut self, _device: &DriverDevice) -> Result<(), DevError> {
        Ok(())
    }
}

/// Device as seen by drivers
#[derive(Debug, Clone)]
pub struct DriverDevice {
    /// Device handle
    pub handle: u64,
    /// Device ID
    pub id: DeviceId,
    /// Device name
    pub name: String,
    /// Device path
    pub path: String,
    /// IRQ number
    pub irq: Option<u32>,
    /// MMIO mapping
    pub mmio: Option<MmioRegion>,
    /// DMA capability
    pub dma_capable: bool,
}

/// MMIO region
#[derive(Debug, Clone)]
pub struct MmioRegion {
    /// Physical base address
    pub phys_base: u64,
    /// Virtual base address (mapped)
    pub virt_base: u64,
    /// Size in bytes
    pub size: usize,
}

impl MmioRegion {
    /// Reads a 32-bit register
    pub fn read32(&self, offset: usize) -> u32 {
        if offset + 4 > self.size {
            return 0;
        }
        unsafe {
            let addr = (self.virt_base as usize + offset) as *const u32;
            core::ptr::read_volatile(addr)
        }
    }

    /// Writes a 32-bit register
    pub fn write32(&self, offset: usize, value: u32) {
        if offset + 4 > self.size {
            return;
        }
        unsafe {
            let addr = (self.virt_base as usize + offset) as *mut u32;
            core::ptr::write_volatile(addr, value);
        }
    }

    /// Reads a 16-bit register
    pub fn read16(&self, offset: usize) -> u16 {
        if offset + 2 > self.size {
            return 0;
        }
        unsafe {
            let addr = (self.virt_base as usize + offset) as *const u16;
            core::ptr::read_volatile(addr)
        }
    }

    /// Writes a 16-bit register
    pub fn write16(&self, offset: usize, value: u16) {
        if offset + 2 > self.size {
            return;
        }
        unsafe {
            let addr = (self.virt_base as usize + offset) as *mut u16;
            core::ptr::write_volatile(addr, value);
        }
    }

    /// Reads an 8-bit register
    pub fn read8(&self, offset: usize) -> u8 {
        if offset >= self.size {
            return 0;
        }
        unsafe {
            let addr = (self.virt_base as usize + offset) as *const u8;
            core::ptr::read_volatile(addr)
        }
    }

    /// Writes an 8-bit register
    pub fn write8(&self, offset: usize, value: u8) {
        if offset >= self.size {
            return;
        }
        unsafe {
            let addr = (self.virt_base as usize + offset) as *mut u8;
            core::ptr::write_volatile(addr, value);
        }
    }
}

/// DMA buffer
#[derive(Debug)]
pub struct DmaBuffer {
    /// Physical address (for hardware)
    pub phys_addr: u64,
    /// Virtual address (for CPU)
    pub virt_addr: u64,
    /// Size in bytes
    pub size: usize,
    /// Coherent (no cache management needed)
    pub coherent: bool,
}

impl DmaBuffer {
    /// Gets a slice view of the buffer
    pub fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.virt_addr as *const u8, self.size) }
    }

    /// Gets a mutable slice view
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.virt_addr as *mut u8, self.size) }
    }

    /// Syncs for device access (flush caches)
    /// 
    /// Called before the device reads from the buffer.
    /// Ensures all CPU writes are visible to the device by flushing
    /// dirty cache lines to memory.
    pub fn sync_for_device(&self) {
        if !self.coherent {
            // Memory barrier to ensure all prior writes complete
            Self::memory_barrier_write();
            
            // Flush cache lines covering this buffer
            Self::cache_flush(self.virt_addr as usize, self.size);
            
            // Full memory barrier after flush
            Self::memory_barrier_full();
        }
    }

    /// Syncs for CPU access (invalidate caches)
    ///
    /// Called before the CPU reads from the buffer after device writes.
    /// Invalidates cache lines so CPU reads fresh data from memory.
    pub fn sync_for_cpu(&self) {
        if !self.coherent {
            // Memory barrier before invalidation
            Self::memory_barrier_full();
            
            // Invalidate cache lines covering this buffer
            Self::cache_invalidate(self.virt_addr as usize, self.size);
            
            // Read memory barrier after invalidation
            Self::memory_barrier_read();
        }
    }

    /// Issues a full memory barrier (fence)
    #[inline(always)]
    fn memory_barrier_full() {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            // MFENCE: Full memory fence - serializes all loads and stores
            core::arch::asm!("mfence", options(nostack, preserves_flags));
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            // DSB SY: Data Synchronization Barrier, full system
            core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        }
        #[cfg(target_arch = "riscv64")]
        unsafe {
            // FENCE: Full fence for RISC-V
            core::arch::asm!("fence iorw, iorw", options(nostack, preserves_flags));
        }
    }

    /// Issues a write memory barrier
    #[inline(always)]
    fn memory_barrier_write() {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            // SFENCE: Store fence - ensures all prior stores are visible
            core::arch::asm!("sfence", options(nostack, preserves_flags));
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            // DSB ST: Data Synchronization Barrier, stores only
            core::arch::asm!("dsb st", options(nostack, preserves_flags));
        }
        #[cfg(target_arch = "riscv64")]
        unsafe {
            // FENCE: Write fence for RISC-V
            core::arch::asm!("fence ow, ow", options(nostack, preserves_flags));
        }
    }

    /// Issues a read memory barrier
    #[inline(always)]
    fn memory_barrier_read() {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            // LFENCE: Load fence - ensures all prior loads complete
            core::arch::asm!("lfence", options(nostack, preserves_flags));
        }
        #[cfg(target_arch = "aarch64")]
        unsafe {
            // DSB LD: Data Synchronization Barrier, loads only
            core::arch::asm!("dsb ld", options(nostack, preserves_flags));
        }
        #[cfg(target_arch = "riscv64")]
        unsafe {
            // FENCE: Read fence for RISC-V
            core::arch::asm!("fence ir, ir", options(nostack, preserves_flags));
        }
    }

    /// Flushes cache lines for a memory region
    ///
    /// Writes dirty cache lines back to memory without invalidating.
    fn cache_flush(addr: usize, size: usize) {
        #[cfg(target_arch = "x86_64")]
        {
            // x86-64 cache line size is typically 64 bytes
            const CACHE_LINE_SIZE: usize = 64;
            let start = addr & !(CACHE_LINE_SIZE - 1);
            let end = (addr + size + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1);
            
            let mut current = start;
            while current < end {
                unsafe {
                    // CLFLUSH: Cache Line Flush - writes back and invalidates
                    // CLWB would be preferred (write-back without invalidate) but requires newer CPUs
                    core::arch::asm!(
                        "clflush [{}]",
                        in(reg) current,
                        options(nostack, preserves_flags)
                    );
                }
                current += CACHE_LINE_SIZE;
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            // ARM64 cache line size is typically 64 bytes (can vary)
            const CACHE_LINE_SIZE: usize = 64;
            let start = addr & !(CACHE_LINE_SIZE - 1);
            let end = (addr + size + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1);
            
            let mut current = start;
            while current < end {
                unsafe {
                    // DC CVAC: Data Cache Clean by VA to Point of Coherency
                    core::arch::asm!(
                        "dc cvac, {}",
                        in(reg) current,
                        options(nostack, preserves_flags)
                    );
                }
                current += CACHE_LINE_SIZE;
            }
        }
        #[cfg(target_arch = "riscv64")]
        {
            // RISC-V cache management depends on extensions
            // CBO (Cache Block Operations) extension provides cbo.flush
            // Without CBO, use fence.i for instruction cache coherence
            let _ = (addr, size);
            // Fallback: full fence
            unsafe {
                core::arch::asm!("fence.i", options(nostack, preserves_flags));
            }
        }
    }

    /// Invalidates cache lines for a memory region
    ///
    /// Discards cache lines, forcing fresh read from memory.
    fn cache_invalidate(addr: usize, size: usize) {
        #[cfg(target_arch = "x86_64")]
        {
            // x86-64: CLFLUSH both flushes and invalidates
            // There's no invalidate-only instruction available in user mode
            const CACHE_LINE_SIZE: usize = 64;
            let start = addr & !(CACHE_LINE_SIZE - 1);
            let end = (addr + size + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1);
            
            let mut current = start;
            while current < end {
                unsafe {
                    core::arch::asm!(
                        "clflush [{}]",
                        in(reg) current,
                        options(nostack, preserves_flags)
                    );
                }
                current += CACHE_LINE_SIZE;
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            const CACHE_LINE_SIZE: usize = 64;
            let start = addr & !(CACHE_LINE_SIZE - 1);
            let end = (addr + size + CACHE_LINE_SIZE - 1) & !(CACHE_LINE_SIZE - 1);
            
            let mut current = start;
            while current < end {
                unsafe {
                    // DC IVAC: Data Cache Invalidate by VA to Point of Coherency
                    // Note: IVAC may cause data loss if line is dirty; 
                    // use CIVAC (clean+invalidate) if unsure
                    core::arch::asm!(
                        "dc ivac, {}",
                        in(reg) current,
                        options(nostack, preserves_flags)
                    );
                }
                current += CACHE_LINE_SIZE;
            }
        }
        #[cfg(target_arch = "riscv64")]
        {
            // RISC-V: Similar to flush, depends on CBO extension
            let _ = (addr, size);
            unsafe {
                core::arch::asm!("fence.i", options(nostack, preserves_flags));
            }
        }
    }
}

/// Driver operations for specific device classes
pub trait BlockDriver: Driver {
    /// Reads blocks
    fn read_blocks(&mut self, device: &DriverDevice, start_lba: u64, buf: &mut [u8]) -> Result<usize, DevError>;
    
    /// Writes blocks
    fn write_blocks(&mut self, device: &DriverDevice, start_lba: u64, buf: &[u8]) -> Result<usize, DevError>;
    
    /// Gets block size
    fn block_size(&self, device: &DriverDevice) -> u32;
    
    /// Gets total blocks
    fn total_blocks(&self, device: &DriverDevice) -> u64;
    
    /// Flushes write cache
    fn flush(&mut self, device: &DriverDevice) -> Result<(), DevError>;
}

/// Input driver operations
pub trait InputDriver: Driver {
    /// Gets input events
    fn poll_events(&mut self, device: &DriverDevice) -> Vec<InputEvent>;
    
    /// Sets LED state (for keyboards)
    fn set_leds(&mut self, device: &DriverDevice, leds: u8) -> Result<(), DevError>;
}

/// Input event
#[derive(Debug, Clone, Copy)]
pub struct InputEvent {
    /// Event type
    pub event_type: InputEventType,
    /// Event code (key code, axis, etc.)
    pub code: u16,
    /// Event value
    pub value: i32,
    /// Timestamp (ms)
    pub timestamp: u64,
}

/// Input event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEventType {
    /// Key press/release
    Key,
    /// Relative movement (mouse)
    Relative,
    /// Absolute position (touchscreen)
    Absolute,
    /// Misc events
    Misc,
}

/// Sound driver operations
pub trait SoundDriver: Driver {
    /// Opens a stream
    fn open_stream(&mut self, device: &DriverDevice, config: AudioConfig) -> Result<u32, DevError>;
    
    /// Closes a stream
    fn close_stream(&mut self, device: &DriverDevice, stream_id: u32) -> Result<(), DevError>;
    
    /// Writes audio data
    fn write_audio(&mut self, device: &DriverDevice, stream_id: u32, data: &[u8]) -> Result<usize, DevError>;
    
    /// Reads audio data
    fn read_audio(&mut self, device: &DriverDevice, stream_id: u32, buf: &mut [u8]) -> Result<usize, DevError>;
    
    /// Gets supported formats
    fn supported_formats(&self, device: &DriverDevice) -> Vec<AudioFormat>;
}

/// Audio configuration
#[derive(Debug, Clone, Copy)]
pub struct AudioConfig {
    /// Sample rate (Hz)
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u8,
    /// Bits per sample
    pub bits_per_sample: u8,
    /// Format
    pub format: AudioFormat,
}

/// Audio format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    /// Signed 16-bit little-endian
    S16Le,
    /// Signed 24-bit little-endian
    S24Le,
    /// Signed 32-bit little-endian
    S32Le,
    /// 32-bit float
    Float32,
}

/// USB driver operations
pub trait UsbDriver: Driver {
    /// Called when USB device is attached
    fn attach(&mut self, device: &UsbDevice) -> Result<(), DevError>;
    
    /// Called when USB device is detached
    fn detach(&mut self, device: &UsbDevice);
    
    /// Submits a control transfer
    fn control_transfer(&mut self, device: &UsbDevice, setup: &UsbSetup, data: &mut [u8]) -> Result<usize, DevError>;
    
    /// Submits a bulk transfer
    fn bulk_transfer(&mut self, device: &UsbDevice, endpoint: u8, data: &mut [u8], out: bool) -> Result<usize, DevError>;
}

/// USB device information
#[derive(Debug, Clone)]
pub struct UsbDevice {
    /// Device address
    pub address: u8,
    /// Speed (1=low, 2=full, 3=high, 4=super)
    pub speed: u8,
    /// Vendor ID
    pub vendor_id: u16,
    /// Product ID
    pub product_id: u16,
    /// Device class
    pub device_class: u8,
    /// Device subclass
    pub device_subclass: u8,
    /// Device protocol
    pub device_protocol: u8,
    /// Configuration count
    pub num_configurations: u8,
}

/// USB setup packet
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct UsbSetup {
    /// Request type
    pub bm_request_type: u8,
    /// Request
    pub b_request: u8,
    /// Value
    pub w_value: u16,
    /// Index
    pub w_index: u16,
    /// Length
    pub w_length: u16,
}

impl UsbSetup {
    /// Creates a GET_DESCRIPTOR request
    pub fn get_descriptor(desc_type: u8, desc_index: u8, length: u16) -> Self {
        Self {
            bm_request_type: 0x80, // Device-to-host, standard, device
            b_request: 6,          // GET_DESCRIPTOR
            w_value: ((desc_type as u16) << 8) | (desc_index as u16),
            w_index: 0,
            w_length: length,
        }
    }

    /// Creates a SET_ADDRESS request
    pub fn set_address(address: u8) -> Self {
        Self {
            bm_request_type: 0x00, // Host-to-device, standard, device
            b_request: 5,          // SET_ADDRESS
            w_value: address as u16,
            w_index: 0,
            w_length: 0,
        }
    }

    /// Creates a SET_CONFIGURATION request
    pub fn set_configuration(config: u8) -> Self {
        Self {
            bm_request_type: 0x00,
            b_request: 9, // SET_CONFIGURATION
            w_value: config as u16,
            w_index: 0,
            w_length: 0,
        }
    }
}

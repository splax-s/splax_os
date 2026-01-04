//! # Intel Integrated Graphics Driver
//!
//! Driver for Intel HD Graphics and newer Intel Xe graphics.
//!
//! ## Supported Hardware
//!
//! - Intel HD Graphics (Gen 4+)
//! - Intel UHD Graphics
//! - Intel Iris Graphics
//! - Intel Xe Graphics
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Intel GPU Driver                              │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Command Streamer  │  Render Engine  │  Display Engine          │
//! ├────────────────────┼─────────────────┼──────────────────────────┤
//! │     Ring Buffer    │   EU (Exec Units) │  Pipes & Planes        │
//! └────────────────────┴─────────────────┴──────────────────────────┘
//! ```

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// =============================================================================
// PCI Device IDs
// =============================================================================

/// Intel GPU vendor ID.
pub const INTEL_VENDOR_ID: u16 = 0x8086;

/// Known Intel GPU device IDs.
pub mod device_ids {
    // Haswell
    pub const HSW_GT1: u16 = 0x0402;
    pub const HSW_GT2: u16 = 0x0412;
    
    // Skylake
    pub const SKL_GT2: u16 = 0x1912;
    pub const SKL_GT3: u16 = 0x1926;
    
    // Kaby Lake
    pub const KBL_GT2: u16 = 0x5912;
    
    // Coffee Lake
    pub const CFL_GT2: u16 = 0x3E92;
    
    // Ice Lake
    pub const ICL_GT2: u16 = 0x8A52;
    
    // Tiger Lake
    pub const TGL_GT2: u16 = 0x9A49;
    
    // Alder Lake
    pub const ADL_GT2: u16 = 0x4680;
}

// =============================================================================
// Register Definitions
// =============================================================================

/// MMIO register offsets.
pub mod regs {
    // Graphics registers
    pub const RING_BUFFER_START: u64 = 0x02034;
    pub const RING_BUFFER_HEAD: u64 = 0x02034;
    pub const RING_BUFFER_TAIL: u64 = 0x02030;
    pub const RING_BUFFER_CTL: u64 = 0x0203C;
    
    // Display registers
    pub const PIPE_A_CONF: u64 = 0x70008;
    pub const PIPE_B_CONF: u64 = 0x71008;
    pub const PIPE_A_SRC: u64 = 0x6001C;
    pub const PIPE_B_SRC: u64 = 0x6101C;
    
    // Plane registers
    pub const PLANE_A_CTL: u64 = 0x70180;
    pub const PLANE_A_STRIDE: u64 = 0x70188;
    pub const PLANE_A_SURF: u64 = 0x7019C;
    
    // Power management
    pub const FORCEWAKE: u64 = 0xA18C;
    pub const FORCEWAKE_MT: u64 = 0xA188;
    
    // Interrupt
    pub const RENDER_INTR: u64 = 0x44024;
    pub const DISPLAY_INTR: u64 = 0x44400;
    
    // GTT (Graphics Translation Table)
    pub const PGTBL_CTL: u64 = 0x02020;
    pub const PGTBL_ER: u64 = 0x02024;
}

// =============================================================================
// GPU Generation
// =============================================================================

/// Intel GPU generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntelGen {
    Gen4,   // 965
    Gen5,   // Ironlake
    Gen6,   // Sandy Bridge
    Gen7,   // Ivy Bridge
    Gen7_5, // Haswell
    Gen8,   // Broadwell
    Gen9,   // Skylake
    Gen9_5, // Kaby Lake
    Gen10,  // Cannon Lake
    Gen11,  // Ice Lake
    Gen12,  // Tiger Lake
    Gen12_5, // Alder Lake
    Xe,     // Discrete Xe
    Unknown,
}

impl IntelGen {
    /// Detect generation from device ID.
    pub fn from_device_id(device_id: u16) -> Self {
        match device_id {
            0x0402..=0x0A2E => Self::Gen7_5, // Haswell
            0x1602..=0x163E => Self::Gen8,   // Broadwell
            0x1902..=0x193E => Self::Gen9,   // Skylake
            0x5902..=0x593E => Self::Gen9_5, // Kaby Lake
            0x3E90..=0x3E9F => Self::Gen9_5, // Coffee Lake
            0x8A50..=0x8A5F => Self::Gen11,  // Ice Lake
            0x9A40..=0x9A4F => Self::Gen12,  // Tiger Lake
            0x4680..=0x46FF => Self::Gen12_5, // Alder Lake
            _ => Self::Unknown,
        }
    }

    /// Get number of execution units.
    pub fn max_eus(&self) -> u32 {
        match self {
            Self::Gen7 | Self::Gen7_5 => 40,
            Self::Gen8 => 48,
            Self::Gen9 | Self::Gen9_5 => 72,
            Self::Gen11 => 64,
            Self::Gen12 | Self::Gen12_5 => 96,
            Self::Xe => 512,
            _ => 24,
        }
    }
}

// =============================================================================
// Ring Buffer (Command Streamer)
// =============================================================================

/// Ring buffer for GPU command submission.
pub struct RingBuffer {
    /// Physical address of ring buffer
    base: u64,
    /// Size in pages
    size_pages: u32,
    /// Current head position
    head: u32,
    /// Current tail position
    tail: u32,
    /// Ring buffer ID
    id: u32,
}

impl RingBuffer {
    /// Create a new ring buffer.
    pub fn new(base: u64, size_pages: u32, id: u32) -> Self {
        Self {
            base,
            size_pages,
            head: 0,
            tail: 0,
            id,
        }
    }

    /// Get available space in dwords.
    pub fn available_space(&self) -> u32 {
        let size = self.size_pages * 4096 / 4;
        if self.tail >= self.head {
            size - (self.tail - self.head) - 1
        } else {
            self.head - self.tail - 1
        }
    }

    /// Emit a command to the ring buffer.
    pub fn emit(&mut self, cmd: u32) -> Result<(), GpuError> {
        if self.available_space() < 1 {
            return Err(GpuError::RingFull);
        }
        
        // Write command to ring buffer memory
        let offset = (self.tail * 4) as u64;
        unsafe {
            let ptr = (self.base + offset) as *mut u32;
            core::ptr::write_volatile(ptr, cmd);
        }
        
        self.tail = (self.tail + 1) % (self.size_pages * 4096 / 4);
        
        Ok(())
    }

    /// Emit multiple commands.
    pub fn emit_batch(&mut self, cmds: &[u32]) -> Result<(), GpuError> {
        if self.available_space() < cmds.len() as u32 {
            return Err(GpuError::RingFull);
        }
        
        for &cmd in cmds {
            self.emit(cmd)?;
        }
        
        Ok(())
    }
}

// =============================================================================
// Graphics Translation Table (GTT)
// =============================================================================

/// GTT entry flags.
pub mod gtt_flags {
    pub const VALID: u64 = 1 << 0;
    pub const CACHE_LLC: u64 = 1 << 1;
    pub const CACHE_L3: u64 = 1 << 2;
}

/// Graphics Translation Table for GPU memory management.
pub struct Gtt {
    /// Base address of GTT
    base: u64,
    /// Number of entries
    entries: u32,
    /// Allocation bitmap
    allocated: Mutex<Vec<bool>>,
}

impl Gtt {
    /// Create a new GTT.
    pub fn new(base: u64, entries: u32) -> Self {
        Self {
            base,
            entries,
            allocated: Mutex::new(alloc::vec![false; entries as usize]),
        }
    }

    /// Map a physical page into the GTT.
    pub fn map(&self, index: u32, physical: u64, flags: u64) -> Result<(), GpuError> {
        if index >= self.entries {
            return Err(GpuError::InvalidAddress);
        }
        
        let entry = physical | flags | gtt_flags::VALID;
        
        unsafe {
            let ptr = (self.base + (index as u64 * 8)) as *mut u64;
            core::ptr::write_volatile(ptr, entry);
        }
        
        self.allocated.lock()[index as usize] = true;
        
        Ok(())
    }

    /// Unmap a GTT entry.
    pub fn unmap(&self, index: u32) -> Result<(), GpuError> {
        if index >= self.entries {
            return Err(GpuError::InvalidAddress);
        }
        
        unsafe {
            let ptr = (self.base + (index as u64 * 8)) as *mut u64;
            core::ptr::write_volatile(ptr, 0);
        }
        
        self.allocated.lock()[index as usize] = false;
        
        Ok(())
    }

    /// Allocate GTT entries.
    pub fn allocate(&self, count: u32) -> Result<u32, GpuError> {
        let mut allocated = self.allocated.lock();
        
        // Find contiguous free entries
        let mut start = None;
        let mut run = 0;
        
        for (i, &alloc) in allocated.iter().enumerate() {
            if !alloc {
                if start.is_none() {
                    start = Some(i);
                }
                run += 1;
                if run >= count {
                    break;
                }
            } else {
                start = None;
                run = 0;
            }
        }
        
        if run >= count {
            let start_idx = start.unwrap();
            for i in start_idx..start_idx + count as usize {
                allocated[i] = true;
            }
            Ok(start_idx as u32)
        } else {
            Err(GpuError::OutOfMemory)
        }
    }
}

// =============================================================================
// Display Pipe
// =============================================================================

/// Display pipe identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pipe {
    A,
    B,
    C,
    D,
}

/// Display plane configuration.
#[derive(Debug, Clone)]
pub struct PlaneConfig {
    /// Pixel format
    pub format: PixelFormat,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Stride in bytes
    pub stride: u32,
    /// Surface address
    pub surface: u64,
}

/// Pixel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Xrgb8888,
    Argb8888,
    Rgb565,
    Xrgb2101010,
}

impl PixelFormat {
    fn bits(&self) -> u32 {
        match self {
            Self::Xrgb8888 | Self::Argb8888 | Self::Xrgb2101010 => 32,
            Self::Rgb565 => 16,
        }
    }

    fn to_hw(&self) -> u32 {
        match self {
            Self::Rgb565 => 0b0101,
            Self::Xrgb8888 => 0b0110,
            Self::Argb8888 => 0b0111,
            Self::Xrgb2101010 => 0b1000,
        }
    }
}

// =============================================================================
// Intel GPU Driver
// =============================================================================

/// GPU error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuError {
    NotFound,
    InitFailed,
    RingFull,
    Timeout,
    InvalidAddress,
    OutOfMemory,
    UnsupportedGen,
    PowerError,
}

/// Intel GPU driver state.
pub struct IntelGpu {
    /// PCI device ID
    device_id: u16,
    /// GPU generation
    generation: IntelGen,
    /// MMIO base address
    mmio_base: u64,
    /// GTT base address
    gtt_base: u64,
    /// Aperture (GGTT) base
    aperture_base: u64,
    /// Aperture size
    aperture_size: u64,
    /// Ring buffer
    ring: Mutex<Option<RingBuffer>>,
    /// GTT manager
    gtt: Mutex<Option<Gtt>>,
    /// Driver initialized
    initialized: AtomicBool,
    /// Active display pipe
    active_pipe: Mutex<Option<Pipe>>,
}

impl IntelGpu {
    /// Create a new Intel GPU driver instance.
    pub const fn new() -> Self {
        Self {
            device_id: 0,
            generation: IntelGen::Unknown,
            mmio_base: 0,
            gtt_base: 0,
            aperture_base: 0,
            aperture_size: 0,
            ring: Mutex::new(None),
            gtt: Mutex::new(None),
            initialized: AtomicBool::new(false),
            active_pipe: Mutex::new(None),
        }
    }

    /// Probe for Intel GPU on PCI bus.
    pub fn probe(&mut self) -> Result<(), GpuError> {
        // Would scan PCI bus for Intel GPU
        // For now, assume no GPU found
        Err(GpuError::NotFound)
    }

    /// Initialize the GPU.
    pub fn init(&mut self, device_id: u16, mmio_base: u64, aperture_base: u64, aperture_size: u64) -> Result<(), GpuError> {
        self.device_id = device_id;
        self.generation = IntelGen::from_device_id(device_id);
        self.mmio_base = mmio_base;
        self.aperture_base = aperture_base;
        self.aperture_size = aperture_size;
        
        if self.generation == IntelGen::Unknown {
            return Err(GpuError::UnsupportedGen);
        }

        // Force wake the GPU
        self.force_wake()?;
        
        // Initialize GTT
        let gtt_entries = (aperture_size / 4096) as u32;
        let gtt = Gtt::new(self.gtt_base, gtt_entries);
        *self.gtt.lock() = Some(gtt);
        
        // Initialize ring buffer
        let ring_size = 4; // 4 pages = 16KB
        let ring = RingBuffer::new(0, ring_size, 0);
        *self.ring.lock() = Some(ring);
        
        self.initialized.store(true, Ordering::SeqCst);
        
        crate::serial_println!(
            "[INTEL GPU] Initialized: gen={:?}, eus={}", 
            self.generation, 
            self.generation.max_eus()
        );
        
        Ok(())
    }

    /// Force wake the GPU.
    fn force_wake(&self) -> Result<(), GpuError> {
        // Write to forcewake register
        self.write_reg(regs::FORCEWAKE_MT, 0x00010001);
        
        // Poll for acknowledgment
        for _ in 0..100 {
            let val = self.read_reg(regs::FORCEWAKE_MT);
            if val & 1 != 0 {
                return Ok(());
            }
            // Small delay
            for _ in 0..1000 { core::hint::spin_loop(); }
        }
        
        Err(GpuError::PowerError)
    }

    /// Read a GPU register.
    fn read_reg(&self, offset: u64) -> u32 {
        unsafe {
            let ptr = (self.mmio_base + offset) as *const u32;
            core::ptr::read_volatile(ptr)
        }
    }

    /// Write a GPU register.
    fn write_reg(&self, offset: u64, value: u32) {
        unsafe {
            let ptr = (self.mmio_base + offset) as *mut u32;
            core::ptr::write_volatile(ptr, value);
        }
    }

    /// Configure display output.
    pub fn configure_display(&self, pipe: Pipe, config: &PlaneConfig) -> Result<(), GpuError> {
        if !self.initialized.load(Ordering::Relaxed) {
            return Err(GpuError::InitFailed);
        }

        let (pipe_conf, plane_ctl, plane_stride, plane_surf) = match pipe {
            Pipe::A => (regs::PIPE_A_CONF, regs::PLANE_A_CTL, regs::PLANE_A_STRIDE, regs::PLANE_A_SURF),
            Pipe::B => (regs::PIPE_B_CONF, regs::PLANE_A_CTL + 0x1000, regs::PLANE_A_STRIDE + 0x1000, regs::PLANE_A_SURF + 0x1000),
            _ => return Err(GpuError::InvalidAddress),
        };

        // Disable pipe first
        self.write_reg(pipe_conf, 0);
        
        // Configure plane
        let plane_ctl_val = (1 << 31) | // Enable
                           (config.format.to_hw() << 26); // Format
        
        self.write_reg(plane_ctl, plane_ctl_val);
        self.write_reg(plane_stride, config.stride / 64); // Stride in tiles
        self.write_reg(plane_surf, config.surface as u32);
        
        // Enable pipe
        self.write_reg(pipe_conf, 1 << 31);
        
        *self.active_pipe.lock() = Some(pipe);
        
        Ok(())
    }

    /// Submit a batch buffer.
    pub fn submit_batch(&self, batch_addr: u64, _size: u32) -> Result<(), GpuError> {
        let mut ring_guard = self.ring.lock();
        let ring = ring_guard.as_mut().ok_or(GpuError::InitFailed)?;
        
        // MI_BATCH_BUFFER_START command
        ring.emit(0x31000000)?; // Opcode
        ring.emit(batch_addr as u32)?; // Address low
        ring.emit((batch_addr >> 32) as u32)?; // Address high
        
        // Update tail register
        self.write_reg(regs::RING_BUFFER_TAIL, ring.tail);
        
        Ok(())
    }

    /// Get GPU info.
    pub fn info(&self) -> GpuInfo {
        GpuInfo {
            vendor: String::from("Intel"),
            device_id: self.device_id,
            generation: self.generation,
            aperture_size: self.aperture_size,
            max_eus: self.generation.max_eus(),
        }
    }
}

/// GPU information.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub vendor: String,
    pub device_id: u16,
    pub generation: IntelGen,
    pub aperture_size: u64,
    pub max_eus: u32,
}

// =============================================================================
// Global Instance
// =============================================================================

static INTEL_GPU: Mutex<IntelGpu> = Mutex::new(IntelGpu::new());

/// Get the Intel GPU driver.
pub fn driver() -> &'static Mutex<IntelGpu> {
    &INTEL_GPU
}

/// Initialize Intel GPU driver.
pub fn init() {
    let mut gpu = INTEL_GPU.lock();
    match gpu.probe() {
        Ok(()) => crate::serial_println!("[INTEL GPU] GPU found and initialized"),
        Err(GpuError::NotFound) => crate::serial_println!("[INTEL GPU] No Intel GPU found"),
        Err(e) => crate::serial_println!("[INTEL GPU] Init error: {:?}", e),
    }
}

//! S-GPU - Userspace Graphics Service
//!
//! This service handles all GPU and framebuffer operations in userspace,
//! providing a clean abstraction over hardware graphics devices.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

pub mod framebuffer;
pub mod console;
pub mod renderer;

/// GPU service configuration
#[derive(Debug, Clone)]
pub struct GpuConfig {
    /// Default resolution width
    pub width: u32,
    /// Default resolution height
    pub height: u32,
    /// Default bits per pixel
    pub bpp: u8,
    /// Enable hardware acceleration if available
    pub hw_accel: bool,
    /// VSync enabled
    pub vsync: bool,
    /// Double buffering
    pub double_buffer: bool,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 768,
            bpp: 32,
            hw_accel: true,
            vsync: true,
            double_buffer: true,
        }
    }
}

/// GPU device type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuDeviceType {
    /// VGA text mode
    VgaText,
    /// Linear framebuffer (VESA, EFI GOP)
    Framebuffer,
    /// VirtIO GPU
    VirtioGpu,
    /// Intel integrated graphics
    IntelHd,
    /// AMD/ATI graphics
    AmdGpu,
    /// NVIDIA graphics
    NvidiaGpu,
    /// Software renderer
    Software,
}

/// Display mode
#[derive(Debug, Clone, Copy)]
pub struct DisplayMode {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Bits per pixel (8, 16, 24, 32)
    pub bpp: u8,
    /// Refresh rate in Hz
    pub refresh_rate: u8,
    /// Pitch (bytes per scanline)
    pub pitch: u32,
}

/// GPU device information
#[derive(Debug, Clone)]
pub struct GpuDevice {
    /// Device ID
    pub id: u64,
    /// Device type
    pub device_type: GpuDeviceType,
    /// Device name
    pub name: String,
    /// Current display mode
    pub current_mode: DisplayMode,
    /// Supported display modes
    pub supported_modes: Vec<DisplayMode>,
    /// VRAM size in bytes
    pub vram_size: u64,
    /// Framebuffer physical address
    pub fb_phys_addr: u64,
    /// Framebuffer virtual address (after mapping)
    pub fb_virt_addr: u64,
}

/// GPU error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuError {
    /// No GPU device found
    NoDevice,
    /// Unsupported operation
    NotSupported,
    /// Invalid mode
    InvalidMode,
    /// Out of VRAM
    OutOfMemory,
    /// Device busy
    DeviceBusy,
    /// IPC error
    IpcError,
    /// Permission denied
    PermissionDenied,
}

/// IPC message types for GPU service
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum GpuMessage {
    // Display operations
    GetDevices = 0x0001,
    GetModes = 0x0002,
    SetMode = 0x0003,
    GetCurrentMode = 0x0004,

    // Framebuffer operations
    MapFramebuffer = 0x0100,
    UnmapFramebuffer = 0x0101,
    SwapBuffers = 0x0102,
    SetDirtyRect = 0x0103,

    // Drawing operations
    Clear = 0x0200,
    FillRect = 0x0201,
    DrawRect = 0x0202,
    DrawLine = 0x0203,
    DrawPixel = 0x0204,
    Blit = 0x0205,

    // Text operations
    PutChar = 0x0300,
    PutString = 0x0301,
    SetFont = 0x0302,
    GetTextSize = 0x0303,

    // Cursor operations
    SetCursorPos = 0x0400,
    SetCursorVisible = 0x0401,
    SetCursorShape = 0x0402,

    // Responses
    Success = 0x8000,
    Error = 0x8001,
    DeviceList = 0x8002,
    ModeList = 0x8003,
}

/// Color in RGBA format
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Convert to 32-bit ARGB
    pub fn to_argb32(&self) -> u32 {
        ((self.a as u32) << 24)
            | ((self.r as u32) << 16)
            | ((self.g as u32) << 8)
            | (self.b as u32)
    }

    /// Convert to 16-bit RGB565
    pub fn to_rgb565(&self) -> u16 {
        (((self.r as u16) >> 3) << 11)
            | (((self.g as u16) >> 2) << 5)
            | ((self.b as u16) >> 3)
    }

    // Predefined colors
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const RED: Color = Color::rgb(255, 0, 0);
    pub const GREEN: Color = Color::rgb(0, 255, 0);
    pub const BLUE: Color = Color::rgb(0, 0, 255);
    pub const YELLOW: Color = Color::rgb(255, 255, 0);
    pub const CYAN: Color = Color::rgb(0, 255, 255);
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);
    pub const GRAY: Color = Color::rgb(128, 128, 128);
    pub const DARK_GRAY: Color = Color::rgb(64, 64, 64);
    pub const LIGHT_GRAY: Color = Color::rgb(192, 192, 192);
}

/// Rectangle structure
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// Check if rectangle contains a point
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && x < self.x + self.width as i32
            && y >= self.y
            && y < self.y + self.height as i32
    }

    /// Check if rectangles intersect
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width as i32
            && self.x + self.width as i32 > other.x
            && self.y < other.y + other.height as i32
            && self.y + self.height as i32 > other.y
    }

    /// Get intersection of two rectangles
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        if !self.intersects(other) {
            return None;
        }

        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width as i32).min(other.x + other.width as i32);
        let bottom = (self.y + self.height as i32).min(other.y + other.height as i32);

        Some(Rect {
            x,
            y,
            width: (right - x) as u32,
            height: (bottom - y) as u32,
        })
    }
}

/// Point structure
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// GPU service
pub struct GpuService {
    config: GpuConfig,
    devices: BTreeMap<u64, GpuDevice>,
    active_device: Option<u64>,
    next_device_id: u64,
}

impl GpuService {
    /// Create a new GPU service
    pub fn new(config: GpuConfig) -> Self {
        Self {
            config,
            devices: BTreeMap::new(),
            active_device: None,
            next_device_id: 1,
        }
    }

    /// Initialize the GPU service
    pub fn init(&mut self) -> Result<(), GpuError> {
        // Probe for GPU devices
        self.probe_devices()?;
        
        // Set up default device if any found
        if let Some((&id, _)) = self.devices.iter().next() {
            self.active_device = Some(id);
        }
        
        Ok(())
    }

    /// Probe for GPU devices
    fn probe_devices(&mut self) -> Result<(), GpuError> {
        // Query kernel for bootloader-provided framebuffer info
        // The native Splax bootloader or GRUB provides framebuffer information via BootInfo
        
        // Create device from boot framebuffer (always available after boot)
        // Framebuffer address would be obtained via kernel syscall or memory-mapped region
        let fb_device = GpuDevice {
            id: self.next_device_id,
            device_type: GpuDeviceType::Framebuffer,
            name: String::from("Boot Framebuffer"),
            current_mode: DisplayMode {
                width: self.config.width,
                height: self.config.height,
                bpp: self.config.bpp,
                refresh_rate: 60,
                pitch: self.config.width * (self.config.bpp as u32 / 8),
            },
            supported_modes: Vec::new(),
            vram_size: (self.config.width * self.config.height * 4) as u64,
            fb_phys_addr: 0, // Would be obtained from boot info
            fb_virt_addr: 0, // Would be mapped by memory manager
        };
        self.devices.insert(self.next_device_id, fb_device);
        self.next_device_id += 1;

        // Enumerate PCI bus for additional GPU devices
        // Class 0x03 = Display controller
        #[cfg(feature = "pci")]
        {
            for dev in pci::enumerate_devices() {
                let class = dev.class_code();
                if class == 0x03 { // Display controller
                    let subclass = dev.subclass();
                    let device_type = match subclass {
                        0x00 => GpuDeviceType::VgaCompatible,
                        0x01 => GpuDeviceType::XgaController,
                        0x02 => GpuDeviceType::Controller3D,
                        _ => GpuDeviceType::Other,
                    };
                    
                    let vendor = dev.vendor_id();
                    let device_id = dev.device_id();
                    let name = match vendor {
                        0x1234 => String::from("QEMU VGA"),
                        0x1AF4 => String::from("VirtIO GPU"),
                        0x10DE => String::from("NVIDIA GPU"),
                        0x1002 => String::from("AMD/ATI GPU"),
                        0x8086 => String::from("Intel Graphics"),
                        _ => alloc::format!("GPU {:04x}:{:04x}", vendor, device_id),
                    };
                    
                    // Read BAR0 for VRAM/MMIO base
                    let bar0 = dev.read_bar(0);
                    
                    let gpu_device = GpuDevice {
                        id: self.next_device_id,
                        device_type,
                        name,
                        current_mode: DisplayMode::default(),
                        supported_modes: Vec::new(),
                        vram_size: 0, // Would query from device
                        fb_phys_addr: bar0 as u64,
                        fb_virt_addr: 0,
                    };
                    
                    self.devices.insert(self.next_device_id, gpu_device);
                    self.next_device_id += 1;
                }
            }
        }

        Ok(())
    }

    /// Get active GPU device
    pub fn active_device(&self) -> Option<&GpuDevice> {
        self.active_device
            .and_then(|id| self.devices.get(&id))
    }

    /// Set display mode
    pub fn set_mode(
        &mut self,
        device_id: u64,
        mode: DisplayMode,
    ) -> Result<(), GpuError> {
        let device = self.devices.get_mut(&device_id)
            .ok_or(GpuError::NoDevice)?;

        // Validate mode
        if mode.bpp != 16 && mode.bpp != 24 && mode.bpp != 32 {
            return Err(GpuError::InvalidMode);
        }

        // Would send mode change request to kernel/hardware here
        device.current_mode = mode;
        
        Ok(())
    }

    /// List all GPU devices
    pub fn list_devices(&self) -> Vec<&GpuDevice> {
        self.devices.values().collect()
    }

    /// Handle incoming IPC message
    pub fn handle_message(
        &mut self,
        msg_type: GpuMessage,
        payload: &[u8],
    ) -> Result<Vec<u8>, GpuError> {
        match msg_type {
            GpuMessage::GetDevices => {
                // Serialize device list
                let mut response = Vec::new();
                let count = self.devices.len() as u32;
                response.extend_from_slice(&count.to_le_bytes());
                
                for device in self.devices.values() {
                    response.extend_from_slice(&device.id.to_le_bytes());
                    response.push(device.device_type as u8);
                    // ... serialize more device info
                }
                
                Ok(response)
            }
            GpuMessage::GetModes => {
                if payload.len() < 8 {
                    return Err(GpuError::IpcError);
                }
                
                let device_id = u64::from_le_bytes(
                    payload[0..8].try_into().unwrap()
                );
                
                let device = self.devices.get(&device_id)
                    .ok_or(GpuError::NoDevice)?;
                
                let mut response = Vec::new();
                let count = device.supported_modes.len() as u32;
                response.extend_from_slice(&count.to_le_bytes());
                
                for mode in &device.supported_modes {
                    response.extend_from_slice(&mode.width.to_le_bytes());
                    response.extend_from_slice(&mode.height.to_le_bytes());
                    response.push(mode.bpp);
                    response.push(mode.refresh_rate);
                }
                
                Ok(response)
            }
            GpuMessage::SetMode => {
                if payload.len() < 18 {
                    return Err(GpuError::IpcError);
                }
                
                let device_id = u64::from_le_bytes(
                    payload[0..8].try_into().unwrap()
                );
                let width = u32::from_le_bytes(
                    payload[8..12].try_into().unwrap()
                );
                let height = u32::from_le_bytes(
                    payload[12..16].try_into().unwrap()
                );
                let bpp = payload[16];
                let refresh_rate = payload[17];
                
                let mode = DisplayMode {
                    width,
                    height,
                    bpp,
                    refresh_rate,
                    pitch: width * (bpp as u32 / 8),
                };
                
                self.set_mode(device_id, mode)?;
                Ok(Vec::new())
            }
            _ => Err(GpuError::NotSupported),
        }
    }
}

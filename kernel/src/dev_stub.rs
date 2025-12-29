//! Device Driver Stub - Forwards driver operations to S-DEV userspace service
//!
//! This module replaces the monolithic driver subsystem with IPC calls to the
//! S-DEV service. Only MMIO mapping, DMA primitives, and IRQ forwarding remain in kernel.

use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;

use crate::ipc::{Channel, Message, IpcError};
use crate::cap::CapabilityToken;

/// S-DEV service endpoint ID
const SDEV_SERVICE_ID: u64 = 0x44455600; // "DEV\0"

/// Device stub error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevStubError {
    /// S-DEV service not available
    ServiceUnavailable,
    /// IPC communication failed
    IpcError,
    /// Invalid response from service
    InvalidResponse,
    /// Device not found
    DeviceNotFound,
    /// Driver not loaded
    DriverNotLoaded,
    /// Operation not supported
    NotSupported,
    /// Permission denied
    PermissionDenied,
    /// Resource busy
    ResourceBusy,
    /// I/O error
    IoError,
}

/// Message types for S-DEV IPC protocol
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum DevMessageType {
    // Device enumeration
    DeviceList = 0x0001,
    DeviceInfo = 0x0002,
    DeviceAttach = 0x0003,
    DeviceDetach = 0x0004,

    // Driver management
    DriverLoad = 0x0100,
    DriverUnload = 0x0101,
    DriverList = 0x0102,
    DriverInfo = 0x0103,

    // IRQ operations
    IrqRegister = 0x0200,
    IrqUnregister = 0x0201,
    IrqNotify = 0x0202,
    IrqAck = 0x0203,
    IrqSetAffinity = 0x0204,

    // MMIO/DMA operations (kernel-side primitives)
    MmioMap = 0x0300,
    MmioUnmap = 0x0301,
    DmaAlloc = 0x0302,
    DmaFree = 0x0303,

    // USB operations
    UsbGetDescriptor = 0x0400,
    UsbControlTransfer = 0x0401,
    UsbBulkTransfer = 0x0402,
    UsbInterruptTransfer = 0x0403,

    // Sound operations
    SoundOpenStream = 0x0500,
    SoundCloseStream = 0x0501,
    SoundWrite = 0x0502,
    SoundRead = 0x0503,
    SoundSetVolume = 0x0504,

    // Input operations
    InputPoll = 0x0600,
    InputSetLeds = 0x0601,

    // Responses
    Success = 0x8000,
    Error = 0x8001,
    Data = 0x8002,
}

/// Device IPC message
#[repr(C)]
pub struct DevMessage {
    pub msg_type: DevMessageType,
    pub sequence: u32,
    pub device_id: u64,
    pub payload_len: u32,
    pub payload: [u8; 1024],
}

impl Default for DevMessage {
    fn default() -> Self {
        Self {
            msg_type: DevMessageType::Success,
            sequence: 0,
            device_id: 0,
            payload_len: 0,
            payload: [0; 1024],
        }
    }
}

/// S-DEV service client stub
pub struct DevStub {
    channel: Option<Channel>,
    capability: Option<CapabilityToken>,
    sequence: u32,
}

impl DevStub {
    /// Create a new device stub
    pub const fn new() -> Self {
        Self {
            channel: None,
            capability: None,
            sequence: 0,
        }
    }

    /// Connect to the S-DEV service
    pub fn connect(&mut self) -> Result<(), DevStubError> {
        // Request channel to S-DEV service from S-INIT
        Ok(())
    }

    /// List all devices
    pub fn device_list(&mut self) -> Result<Vec<DeviceInfo>, DevStubError> {
        let mut msg = DevMessage::default();
        msg.msg_type = DevMessageType::DeviceList;
        msg.sequence = self.next_sequence();

        let response = self.send_receive(&msg)?;
        
        // Parse device list from response
        let mut devices = Vec::new();
        
        if matches!(response.msg_type, DevMessageType::Data) {
            // Parse device info entries from payload
            let count = u32::from_le_bytes(
                response.payload[0..4].try_into().unwrap()
            ) as usize;
            
            for i in 0..count.min(16) {
                let offset = 4 + i * 48;
                if offset + 48 <= response.payload_len as usize {
                    let device_id = u64::from_le_bytes(
                        response.payload[offset..offset + 8].try_into().unwrap()
                    );
                    let device_type = u32::from_le_bytes(
                        response.payload[offset + 8..offset + 12].try_into().unwrap()
                    );
                    
                    devices.push(DeviceInfo {
                        id: device_id,
                        device_type: device_type_from_u32(device_type),
                        driver_bound: response.payload[offset + 12] != 0,
                    });
                }
            }
        }
        
        Ok(devices)
    }

    /// Register for IRQ notifications
    pub fn irq_register(
        &mut self,
        irq: u8,
        device_id: u64,
    ) -> Result<(), DevStubError> {
        let mut msg = DevMessage::default();
        msg.msg_type = DevMessageType::IrqRegister;
        msg.sequence = self.next_sequence();
        msg.device_id = device_id;
        
        msg.payload[0] = irq;
        msg.payload_len = 1;

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            DevMessageType::Success => Ok(()),
            _ => Err(DevStubError::IpcError),
        }
    }

    /// Send IRQ notification to userspace driver
    pub fn irq_notify(&mut self, irq: u8) -> Result<(), DevStubError> {
        let mut msg = DevMessage::default();
        msg.msg_type = DevMessageType::IrqNotify;
        msg.sequence = self.next_sequence();
        
        msg.payload[0] = irq;
        msg.payload_len = 1;

        // This is a one-way notification, no response expected
        self.send_async(&msg)?;
        Ok(())
    }

    /// USB control transfer
    pub fn usb_control_transfer(
        &mut self,
        device_id: u64,
        request_type: u8,
        request: u8,
        value: u16,
        index: u16,
        data: &mut [u8],
    ) -> Result<usize, DevStubError> {
        let mut msg = DevMessage::default();
        msg.msg_type = DevMessageType::UsbControlTransfer;
        msg.sequence = self.next_sequence();
        msg.device_id = device_id;
        
        msg.payload[0] = request_type;
        msg.payload[1] = request;
        msg.payload[2..4].copy_from_slice(&value.to_le_bytes());
        msg.payload[4..6].copy_from_slice(&index.to_le_bytes());
        msg.payload[6..8].copy_from_slice(&(data.len() as u16).to_le_bytes());
        
        if request_type & 0x80 == 0 {
            // OUT transfer - include data
            let len = data.len().min(1016);
            msg.payload[8..8 + len].copy_from_slice(&data[..len]);
            msg.payload_len = 8 + len as u32;
        } else {
            msg.payload_len = 8;
        }

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            DevMessageType::Data => {
                // IN transfer - copy response data
                let len = (response.payload_len as usize).min(data.len());
                data[..len].copy_from_slice(&response.payload[..len]);
                Ok(len)
            }
            DevMessageType::Success => {
                Ok(data.len()) // OUT transfer completed
            }
            _ => Err(DevStubError::IoError),
        }
    }

    /// Open an audio stream
    pub fn sound_open_stream(
        &mut self,
        device_id: u64,
        sample_rate: u32,
        channels: u8,
        bits_per_sample: u8,
        direction: u8, // 0 = playback, 1 = capture
    ) -> Result<u64, DevStubError> {
        let mut msg = DevMessage::default();
        msg.msg_type = DevMessageType::SoundOpenStream;
        msg.sequence = self.next_sequence();
        msg.device_id = device_id;
        
        msg.payload[0..4].copy_from_slice(&sample_rate.to_le_bytes());
        msg.payload[4] = channels;
        msg.payload[5] = bits_per_sample;
        msg.payload[6] = direction;
        msg.payload_len = 7;

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            DevMessageType::Success => {
                let stream_id = u64::from_le_bytes(
                    response.payload[0..8].try_into().unwrap()
                );
                Ok(stream_id)
            }
            _ => Err(DevStubError::IoError),
        }
    }

    /// Write audio data to a stream
    pub fn sound_write(
        &mut self,
        stream_id: u64,
        data: &[u8],
    ) -> Result<usize, DevStubError> {
        let mut msg = DevMessage::default();
        msg.msg_type = DevMessageType::SoundWrite;
        msg.sequence = self.next_sequence();
        msg.device_id = stream_id;
        
        let len = data.len().min(1024);
        msg.payload[..len].copy_from_slice(&data[..len]);
        msg.payload_len = len as u32;

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            DevMessageType::Success => {
                let written = u64::from_le_bytes(
                    response.payload[0..8].try_into().unwrap()
                );
                Ok(written as usize)
            }
            _ => Err(DevStubError::IoError),
        }
    }

    /// Poll for input events
    pub fn input_poll(&mut self) -> Result<Vec<InputEvent>, DevStubError> {
        let mut msg = DevMessage::default();
        msg.msg_type = DevMessageType::InputPoll;
        msg.sequence = self.next_sequence();

        let response = self.send_receive(&msg)?;
        
        let mut events = Vec::new();
        
        if matches!(response.msg_type, DevMessageType::Data) {
            let count = u32::from_le_bytes(
                response.payload[0..4].try_into().unwrap()
            ) as usize;
            
            for i in 0..count.min(64) {
                let offset = 4 + i * 16;
                if offset + 16 <= response.payload_len as usize {
                    let event_type = u16::from_le_bytes(
                        response.payload[offset..offset + 2].try_into().unwrap()
                    );
                    let code = u16::from_le_bytes(
                        response.payload[offset + 2..offset + 4].try_into().unwrap()
                    );
                    let value = i32::from_le_bytes(
                        response.payload[offset + 4..offset + 8].try_into().unwrap()
                    );
                    let timestamp = u64::from_le_bytes(
                        response.payload[offset + 8..offset + 16].try_into().unwrap()
                    );
                    
                    events.push(InputEvent {
                        event_type,
                        code,
                        value,
                        timestamp,
                    });
                }
            }
        }
        
        Ok(events)
    }

    /// Get next sequence number
    fn next_sequence(&mut self) -> u32 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    /// Send message and wait for response
    fn send_receive(&mut self, _msg: &DevMessage) -> Result<DevMessage, DevStubError> {
        let _channel = self.channel.as_ref()
            .ok_or(DevStubError::ServiceUnavailable)?;
        
        // Placeholder - would use actual IPC
        Ok(DevMessage::default())
    }

    /// Send async notification (no response)
    fn send_async(&mut self, _msg: &DevMessage) -> Result<(), DevStubError> {
        let _channel = self.channel.as_ref()
            .ok_or(DevStubError::ServiceUnavailable)?;
        
        Ok(())
    }
}

/// Device type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Unknown,
    Block,
    Network,
    Input,
    Sound,
    Usb,
    Gpu,
    Serial,
}

fn device_type_from_u32(v: u32) -> DeviceType {
    match v {
        1 => DeviceType::Block,
        2 => DeviceType::Network,
        3 => DeviceType::Input,
        4 => DeviceType::Sound,
        5 => DeviceType::Usb,
        6 => DeviceType::Gpu,
        7 => DeviceType::Serial,
        _ => DeviceType::Unknown,
    }
}

/// Device information
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub id: u64,
    pub device_type: DeviceType,
    pub driver_bound: bool,
}

/// Input event from userspace driver
#[derive(Debug, Clone, Copy)]
pub struct InputEvent {
    pub event_type: u16,
    pub code: u16,
    pub value: i32,
    pub timestamp: u64,
}

/// Global device stub instance
pub static DEV_STUB: Mutex<DevStub> = Mutex::new(DevStub::new());

/// Initialize the device stub (called during kernel init)
pub fn init() -> Result<(), DevStubError> {
    DEV_STUB.lock().connect()
}

// ============================================================================
// Kernel-side primitives that remain in kernel (MMIO/DMA)
// ============================================================================

/// MMIO region for device access
#[derive(Debug)]
pub struct MmioRegion {
    pub phys_addr: u64,
    pub virt_addr: u64,
    pub size: usize,
}

impl MmioRegion {
    /// Map a physical MMIO region into kernel address space
    pub fn map(phys_addr: u64, size: usize) -> Result<Self, DevStubError> {
        // This stays in kernel - actual page table manipulation
        // Use kernel's MM subsystem to map the region
        
        // Placeholder - would call mm::map_device_memory
        let virt_addr = phys_addr; // In real impl, allocate virtual address
        
        Ok(Self {
            phys_addr,
            virt_addr,
            size,
        })
    }

    /// Read from MMIO region
    pub unsafe fn read_u32(&self, offset: usize) -> u32 {
        let ptr = (self.virt_addr + offset as u64) as *const u32;
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// Write to MMIO region
    pub unsafe fn write_u32(&self, offset: usize, value: u32) {
        let ptr = (self.virt_addr + offset as u64) as *mut u32;
        unsafe { core::ptr::write_volatile(ptr, value) }
    }
}

/// DMA buffer for device transfers
#[derive(Debug)]
pub struct DmaBuffer {
    pub phys_addr: u64,
    pub virt_addr: u64,
    pub size: usize,
}

impl DmaBuffer {
    /// Allocate a DMA-capable buffer
    pub fn alloc(size: usize) -> Result<Self, DevStubError> {
        // This stays in kernel - allocate physically contiguous memory
        // that is accessible to both CPU and device
        
        // Placeholder - would use mm::alloc_dma_buffer
        Ok(Self {
            phys_addr: 0,
            virt_addr: 0,
            size,
        })
    }

    /// Get a slice to the buffer contents
    pub fn as_slice(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self.virt_addr as *const u8, self.size)
        }
    }

    /// Get a mutable slice to the buffer contents
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.virt_addr as *mut u8, self.size)
        }
    }
}

// ============================================================================
// IRQ forwarding - stays in kernel, but forwards to userspace
// ============================================================================

/// Forward an IRQ to the userspace driver service
pub fn forward_irq(irq: u8) {
    // Called from interrupt handler
    // Send async notification to S-DEV service
    if let Some(mut stub) = DEV_STUB.try_lock() {
        let _ = stub.irq_notify(irq);
    }
}

/// IRQ handler that forwards to userspace
pub extern "x86-interrupt" fn irq_forward_handler(irq: u8) {
    forward_irq(irq);
    
    // Acknowledge the IRQ in the interrupt controller
    // (PIC or APIC depending on architecture)
    unsafe {
        // EOI to PIC - in real impl use proper APIC/PIC handling
        if irq >= 8 {
            core::arch::asm!("out 0xA0, al", in("al") 0x20u8);
        }
        core::arch::asm!("out 0x20, al", in("al") 0x20u8);
    }
}

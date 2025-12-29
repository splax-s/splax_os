//! # USB Subsystem for Splax OS
//!
//! This module implements USB support including:
//! - USB core types and descriptors
//! - xHCI host controller driver
//! - EHCI host controller driver (legacy)
//! - USB device enumeration
//! - USB HID keyboard driver
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     USB Device Drivers                      │
//! │  (HID Keyboard, Mass Storage, Hub, Audio, etc.)            │
//! ├─────────────────────────────────────────────────────────────┤
//! │                      USB Core Layer                         │
//! │  (Device enumeration, descriptor parsing, transfers)        │
//! ├─────────────────────────────────────────────────────────────┤
//! │              Host Controller Drivers                        │
//! │  (xHCI for USB 3.x, EHCI for USB 2.0)                      │
//! └─────────────────────────────────────────────────────────────┘
//! ```

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};
use spin::Mutex;

pub mod descriptor;
pub mod hid;
pub mod xhci;

/// Maximum number of USB devices that can be connected
pub const MAX_USB_DEVICES: usize = 127;

/// Maximum number of endpoints per device
pub const MAX_ENDPOINTS: usize = 32;

/// USB transfer types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransferType {
    Control = 0,
    Isochronous = 1,
    Bulk = 2,
    Interrupt = 3,
}

/// USB device speed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UsbSpeed {
    Low = 0,      // 1.5 Mbps (USB 1.0)
    Full = 1,     // 12 Mbps (USB 1.1)
    High = 2,     // 480 Mbps (USB 2.0)
    Super = 3,    // 5 Gbps (USB 3.0)
    SuperPlus = 4, // 10 Gbps (USB 3.1)
}

impl UsbSpeed {
    pub fn as_str(&self) -> &'static str {
        match self {
            UsbSpeed::Low => "Low Speed (1.5 Mbps)",
            UsbSpeed::Full => "Full Speed (12 Mbps)",
            UsbSpeed::High => "High Speed (480 Mbps)",
            UsbSpeed::Super => "SuperSpeed (5 Gbps)",
            UsbSpeed::SuperPlus => "SuperSpeed+ (10 Gbps)",
        }
    }
}

/// USB device state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Detached,
    Attached,
    Powered,
    Default,
    Addressed,
    Configured,
    Suspended,
}

/// USB endpoint direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Out = 0, // Host to device
    In = 1,  // Device to host
}

/// USB endpoint
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// Endpoint address (includes direction bit)
    pub address: u8,
    /// Endpoint number (0-15)
    pub number: u8,
    /// Transfer direction
    pub direction: Direction,
    /// Transfer type
    pub transfer_type: TransferType,
    /// Maximum packet size
    pub max_packet_size: u16,
    /// Polling interval (for interrupt endpoints)
    pub interval: u8,
}

impl Endpoint {
    pub fn new(address: u8, transfer_type: TransferType, max_packet_size: u16, interval: u8) -> Self {
        Self {
            address,
            number: address & 0x0F,
            direction: if address & 0x80 != 0 { Direction::In } else { Direction::Out },
            transfer_type,
            max_packet_size,
            interval,
        }
    }
}

/// USB device interface
#[derive(Debug, Clone)]
pub struct UsbInterface {
    /// Interface number
    pub number: u8,
    /// Alternate setting
    pub alt_setting: u8,
    /// Interface class
    pub class: u8,
    /// Interface subclass
    pub subclass: u8,
    /// Interface protocol
    pub protocol: u8,
    /// Interface name (from string descriptor)
    pub name: Option<String>,
    /// Endpoints belonging to this interface
    pub endpoints: Vec<Endpoint>,
}

/// USB device configuration
#[derive(Debug, Clone)]
pub struct UsbConfiguration {
    /// Configuration value
    pub value: u8,
    /// Configuration attributes
    pub attributes: u8,
    /// Maximum power consumption (in 2mA units)
    pub max_power: u8,
    /// Configuration name
    pub name: Option<String>,
    /// Interfaces in this configuration
    pub interfaces: Vec<UsbInterface>,
}

impl UsbConfiguration {
    /// Check if device is self-powered
    pub fn is_self_powered(&self) -> bool {
        (self.attributes & 0x40) != 0
    }

    /// Check if device supports remote wakeup
    pub fn supports_remote_wakeup(&self) -> bool {
        (self.attributes & 0x20) != 0
    }

    /// Get max power in milliamps
    pub fn max_power_ma(&self) -> u16 {
        self.max_power as u16 * 2
    }
}

/// USB device information
#[derive(Debug, Clone)]
pub struct UsbDevice {
    /// Device address (1-127)
    pub address: u8,
    /// Device speed
    pub speed: UsbSpeed,
    /// Device state
    pub state: DeviceState,
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
    /// USB specification version (BCD)
    pub usb_version: u16,
    /// Device version (BCD)
    pub device_version: u16,
    /// Manufacturer string
    pub manufacturer: Option<String>,
    /// Product string
    pub product: Option<String>,
    /// Serial number string
    pub serial_number: Option<String>,
    /// Available configurations
    pub configurations: Vec<UsbConfiguration>,
    /// Currently active configuration index
    pub active_config: Option<u8>,
    /// Parent hub address (0 for root hub)
    pub parent_hub: u8,
    /// Port number on parent hub
    pub port_number: u8,
}

impl UsbDevice {
    /// Create a new USB device with default values
    pub fn new(address: u8, speed: UsbSpeed) -> Self {
        Self {
            address,
            speed,
            state: DeviceState::Default,
            vendor_id: 0,
            product_id: 0,
            device_class: 0,
            device_subclass: 0,
            device_protocol: 0,
            usb_version: 0,
            device_version: 0,
            manufacturer: None,
            product: None,
            serial_number: None,
            configurations: Vec::new(),
            active_config: None,
            parent_hub: 0,
            port_number: 0,
        }
    }

    /// Get device class name
    pub fn class_name(&self) -> &'static str {
        match self.device_class {
            0x00 => "Interface-defined",
            0x01 => "Audio",
            0x02 => "Communications",
            0x03 => "HID",
            0x05 => "Physical",
            0x06 => "Image",
            0x07 => "Printer",
            0x08 => "Mass Storage",
            0x09 => "Hub",
            0x0A => "CDC-Data",
            0x0B => "Smart Card",
            0x0D => "Content Security",
            0x0E => "Video",
            0x0F => "Personal Healthcare",
            0x10 => "Audio/Video",
            0xDC => "Diagnostic",
            0xE0 => "Wireless Controller",
            0xEF => "Miscellaneous",
            0xFE => "Application Specific",
            0xFF => "Vendor Specific",
            _ => "Unknown",
        }
    }
}

/// USB setup packet for control transfers
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SetupPacket {
    /// Request type (direction, type, recipient)
    pub request_type: u8,
    /// Request code
    pub request: u8,
    /// Value
    pub value: u16,
    /// Index
    pub index: u16,
    /// Length of data stage
    pub length: u16,
}

impl SetupPacket {
    /// Create a new setup packet
    pub fn new(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Self {
        Self {
            request_type,
            request,
            value,
            index,
            length,
        }
    }

    /// GET_DESCRIPTOR request
    pub fn get_descriptor(desc_type: u8, desc_index: u8, length: u16) -> Self {
        Self::new(
            0x80, // Device to host, standard, device
            0x06, // GET_DESCRIPTOR
            ((desc_type as u16) << 8) | (desc_index as u16),
            0,
            length,
        )
    }

    /// SET_ADDRESS request
    pub fn set_address(address: u8) -> Self {
        Self::new(
            0x00, // Host to device, standard, device
            0x05, // SET_ADDRESS
            address as u16,
            0,
            0,
        )
    }

    /// SET_CONFIGURATION request
    pub fn set_configuration(config_value: u8) -> Self {
        Self::new(
            0x00, // Host to device, standard, device
            0x09, // SET_CONFIGURATION
            config_value as u16,
            0,
            0,
        )
    }

    /// GET_STATUS request
    pub fn get_status() -> Self {
        Self::new(
            0x80, // Device to host, standard, device
            0x00, // GET_STATUS
            0,
            0,
            2,
        )
    }
}

/// USB standard request codes
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum StandardRequest {
    GetStatus = 0,
    ClearFeature = 1,
    SetFeature = 3,
    SetAddress = 5,
    GetDescriptor = 6,
    SetDescriptor = 7,
    GetConfiguration = 8,
    SetConfiguration = 9,
    GetInterface = 10,
    SetInterface = 11,
    SynchFrame = 12,
}

/// USB class codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClassCode {
    InterfaceDefined = 0x00,
    Audio = 0x01,
    Communications = 0x02,
    Hid = 0x03,
    Physical = 0x05,
    Image = 0x06,
    Printer = 0x07,
    MassStorage = 0x08,
    Hub = 0x09,
    CdcData = 0x0A,
    SmartCard = 0x0B,
    ContentSecurity = 0x0D,
    Video = 0x0E,
    Healthcare = 0x0F,
    AudioVideo = 0x10,
    Billboard = 0x11,
    UsbTypeCBridge = 0x12,
    Diagnostic = 0xDC,
    WirelessController = 0xE0,
    Miscellaneous = 0xEF,
    ApplicationSpecific = 0xFE,
    VendorSpecific = 0xFF,
}

/// USB transfer result
#[derive(Debug, Clone)]
pub enum TransferResult {
    Success(usize),
    Stall,
    DataToggleError,
    Timeout,
    BabbleError,
    BufferOverrun,
    BufferUnderrun,
    NotResponding,
    CrcError,
    BitStuffError,
    UnexpectedPid,
    Cancelled,
    HostError,
}

/// USB host controller trait
pub trait UsbHostController: Send + Sync {
    /// Get the controller name
    fn name(&self) -> &'static str;

    /// Initialize the controller
    fn init(&mut self) -> Result<(), &'static str>;

    /// Reset the controller
    fn reset(&mut self) -> Result<(), &'static str>;

    /// Get number of ports
    fn port_count(&self) -> u8;

    /// Check if a port has a device connected
    fn port_connected(&self, port: u8) -> bool;

    /// Get port speed
    fn port_speed(&self, port: u8) -> Option<UsbSpeed>;

    /// Reset a port
    fn port_reset(&mut self, port: u8) -> Result<(), &'static str>;

    /// Enable a port
    fn port_enable(&mut self, port: u8) -> Result<(), &'static str>;

    /// Disable a port
    fn port_disable(&mut self, port: u8) -> Result<(), &'static str>;

    /// Perform a control transfer
    fn control_transfer(
        &mut self,
        device: u8,
        setup: SetupPacket,
        data: Option<&mut [u8]>,
    ) -> TransferResult;

    /// Perform a bulk transfer
    fn bulk_transfer(
        &mut self,
        device: u8,
        endpoint: u8,
        data: &mut [u8],
        direction: Direction,
    ) -> TransferResult;

    /// Perform an interrupt transfer
    fn interrupt_transfer(
        &mut self,
        device: u8,
        endpoint: u8,
        data: &mut [u8],
        direction: Direction,
    ) -> TransferResult;

    /// Allocate a device address
    fn allocate_address(&mut self) -> Option<u8>;

    /// Free a device address
    fn free_address(&mut self, address: u8);
}

/// Global USB subsystem
pub struct UsbSubsystem {
    /// Host controllers
    controllers: Vec<Box<dyn UsbHostController>>,
    /// Connected devices
    devices: Vec<UsbDevice>,
    /// Next device address
    next_address: AtomicU8,
}

impl UsbSubsystem {
    /// Create a new USB subsystem
    pub fn new() -> Self {
        Self {
            controllers: Vec::new(),
            devices: Vec::new(),
            next_address: AtomicU8::new(1),
        }
    }

    /// Register a host controller
    pub fn register_controller(&mut self, controller: Box<dyn UsbHostController>) {
        self.controllers.push(controller);
    }

    /// Initialize all controllers and enumerate devices
    pub fn init(&mut self) -> Result<(), &'static str> {
        for controller in &mut self.controllers {
            controller.init()?;
        }
        self.enumerate_devices()?;
        Ok(())
    }

    /// Enumerate all connected devices
    pub fn enumerate_devices(&mut self) -> Result<(), &'static str> {
        for controller in &mut self.controllers {
            let port_count = controller.port_count();
            for port in 0..port_count {
                if controller.port_connected(port) {
                    if let Some(speed) = controller.port_speed(port) {
                        controller.port_reset(port)?;
                        
                        // Allocate address
                        if let Some(address) = controller.allocate_address() {
                            let mut device = UsbDevice::new(address, speed);
                            device.port_number = port;
                            
                            // Set device address
                            let setup = SetupPacket::set_address(address);
                            if let TransferResult::Success(_) = controller.control_transfer(0, setup, None) {
                                device.state = DeviceState::Addressed;
                                
                                // Read device descriptor
                                let mut desc_buf = [0u8; 18];
                                let setup = SetupPacket::get_descriptor(1, 0, 18);
                                if let TransferResult::Success(_) = controller.control_transfer(address, setup, Some(&mut desc_buf)) {
                                    // Parse device descriptor
                                    device.usb_version = u16::from_le_bytes([desc_buf[2], desc_buf[3]]);
                                    device.device_class = desc_buf[4];
                                    device.device_subclass = desc_buf[5];
                                    device.device_protocol = desc_buf[6];
                                    device.vendor_id = u16::from_le_bytes([desc_buf[8], desc_buf[9]]);
                                    device.product_id = u16::from_le_bytes([desc_buf[10], desc_buf[11]]);
                                    device.device_version = u16::from_le_bytes([desc_buf[12], desc_buf[13]]);
                                }
                                
                                self.devices.push(device);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Get connected device count
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Get a device by address
    pub fn get_device(&self, address: u8) -> Option<&UsbDevice> {
        self.devices.iter().find(|d| d.address == address)
    }

    /// Get all devices
    pub fn devices(&self) -> &[UsbDevice] {
        &self.devices
    }

    /// Find devices by class
    pub fn find_by_class(&self, class: ClassCode) -> Vec<&UsbDevice> {
        self.devices
            .iter()
            .filter(|d| d.device_class == class as u8)
            .collect()
    }

    /// Perform a control transfer to a device
    pub fn control_transfer(
        &mut self,
        device: u8,
        setup: SetupPacket,
        data: Option<&mut [u8]>,
    ) -> TransferResult {
        // Find the controller that manages this device and perform the transfer
        if let Some(controller) = self.controllers.first_mut() {
            controller.control_transfer(device, setup, data)
        } else {
            TransferResult::HostError
        }
    }
}

/// Global USB subsystem instance
pub static USB_SUBSYSTEM: Mutex<Option<UsbSubsystem>> = Mutex::new(None);

/// Wrapper for USB subsystem access from HID module
/// This provides safe access methods for interrupt and control transfers
pub struct UsbSubsystemHandle;

impl UsbSubsystemHandle {
    /// Perform an interrupt IN transfer
    pub fn interrupt_transfer_in(
        &self,
        device: u8,
        endpoint: u8,
        data: &mut [u8],
    ) -> TransferResult {
        let mut subsystem = USB_SUBSYSTEM.lock();
        if let Some(ref mut usb) = *subsystem {
            if let Some(controller) = usb.controllers.first_mut() {
                controller.interrupt_transfer(device, endpoint, data, Direction::In)
            } else {
                TransferResult::HostError
            }
        } else {
            TransferResult::HostError
        }
    }
    
    /// Perform a control transfer with OUT data stage
    pub fn control_transfer_out(
        &self,
        device: u8,
        setup: SetupPacket,
        data: &mut [u8],
    ) -> TransferResult {
        let mut subsystem = USB_SUBSYSTEM.lock();
        if let Some(ref mut usb) = *subsystem {
            if let Some(controller) = usb.controllers.first_mut() {
                if data.is_empty() {
                    controller.control_transfer(device, setup, None)
                } else {
                    controller.control_transfer(device, setup, Some(data))
                }
            } else {
                TransferResult::HostError
            }
        } else {
            TransferResult::HostError
        }
    }
}

/// Get a handle to the USB subsystem for HID operations
pub fn get_usb_subsystem() -> Option<UsbSubsystemHandle> {
    let subsystem = USB_SUBSYSTEM.lock();
    if subsystem.is_some() {
        Some(UsbSubsystemHandle)
    } else {
        None
    }
}

/// Initialize the USB subsystem
pub fn init() -> Result<(), &'static str> {
    let mut subsystem = USB_SUBSYSTEM.lock();
    if subsystem.is_some() {
        return Err("USB subsystem already initialized");
    }
    
    let mut usb = UsbSubsystem::new();
    
    // Probe for xHCI controllers
    if let Some(controller) = xhci::probe_xhci() {
        usb.register_controller(controller);
    }
    
    // Initialize and enumerate
    usb.init()?;
    
    *subsystem = Some(usb);
    Ok(())
}

/// Get the USB subsystem
pub fn subsystem() -> spin::MutexGuard<'static, Option<UsbSubsystem>> {
    USB_SUBSYSTEM.lock()
}

/// Print USB device tree
pub fn print_device_tree() {
    let subsystem = USB_SUBSYSTEM.lock();
    if let Some(ref usb) = *subsystem {
        crate::serial_println!("USB Device Tree:");
        crate::serial_println!("================");
        crate::serial_println!("Controllers: {}", usb.controllers.len());
        crate::serial_println!("Devices: {}", usb.device_count());
        crate::serial_println!();
        
        for device in usb.devices() {
            crate::serial_println!("  Device {} ({:04x}:{:04x})", 
                device.address, device.vendor_id, device.product_id);
            crate::serial_println!("    Speed: {}", device.speed.as_str());
            crate::serial_println!("    Class: {} (0x{:02x})", device.class_name(), device.device_class);
            if let Some(ref mfr) = device.manufacturer {
                crate::serial_println!("    Manufacturer: {}", mfr);
            }
            if let Some(ref prod) = device.product {
                crate::serial_println!("    Product: {}", prod);
            }
            crate::serial_println!();
        }
    } else {
        crate::serial_println!("USB subsystem not initialized");
    }
}

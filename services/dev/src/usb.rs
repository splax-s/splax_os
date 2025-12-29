//! # USB Subsystem for S-DEV
//!
//! Userspace USB host controller and device management.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;

use super::driver::{UsbDevice, UsbSetup};
use super::DevError;

/// USB transfer types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferType {
    Control,
    Bulk,
    Interrupt,
    Isochronous,
}

/// USB endpoint direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointDirection {
    Out,
    In,
}

/// USB endpoint descriptor
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// Endpoint address (includes direction bit)
    pub address: u8,
    /// Transfer type
    pub transfer_type: TransferType,
    /// Direction
    pub direction: EndpointDirection,
    /// Max packet size
    pub max_packet_size: u16,
    /// Interval (for interrupt/isochronous)
    pub interval: u8,
}

impl Endpoint {
    /// Gets endpoint number (0-15)
    pub fn number(&self) -> u8 {
        self.address & 0x0F
    }
}

/// USB interface descriptor
#[derive(Debug, Clone)]
pub struct UsbInterface {
    /// Interface number
    pub number: u8,
    /// Alternate setting
    pub alternate: u8,
    /// Interface class
    pub class: u8,
    /// Interface subclass
    pub subclass: u8,
    /// Interface protocol
    pub protocol: u8,
    /// Endpoints
    pub endpoints: Vec<Endpoint>,
}

/// USB configuration
#[derive(Debug, Clone)]
pub struct UsbConfiguration {
    /// Configuration value
    pub value: u8,
    /// Max power (2mA units)
    pub max_power: u8,
    /// Self-powered
    pub self_powered: bool,
    /// Remote wakeup capable
    pub remote_wakeup: bool,
    /// Interfaces
    pub interfaces: Vec<UsbInterface>,
}

/// USB device state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    /// Not attached
    Detached,
    /// Attached but not addressed
    Attached,
    /// Addressed
    Addressed,
    /// Configured
    Configured,
    /// Suspended
    Suspended,
}

/// Full USB device info
#[derive(Debug, Clone)]
pub struct FullUsbDevice {
    /// Basic device info
    pub info: UsbDevice,
    /// Device state
    pub state: DeviceState,
    /// Manufacturer string
    pub manufacturer: Option<String>,
    /// Product string
    pub product: Option<String>,
    /// Serial number string
    pub serial: Option<String>,
    /// Configurations
    pub configurations: Vec<UsbConfiguration>,
    /// Current configuration
    pub current_config: Option<u8>,
    /// Hub port (if connected through hub)
    pub hub_port: Option<(u8, u8)>, // (hub_addr, port)
}

/// USB transfer request
#[derive(Debug)]
pub struct UsbTransfer {
    /// Unique transfer ID
    pub id: u64,
    /// Device address
    pub device: u8,
    /// Endpoint address
    pub endpoint: u8,
    /// Transfer type
    pub transfer_type: TransferType,
    /// Setup packet (for control transfers)
    pub setup: Option<UsbSetup>,
    /// Data direction (true = IN)
    pub is_in: bool,
    /// Expected/provided data length
    pub length: usize,
    /// Actual transferred length
    pub actual_length: usize,
    /// Transfer status
    pub status: TransferStatus,
    /// Callback ID
    pub callback: u64,
}

/// Transfer status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    /// Pending
    Pending,
    /// In progress
    InProgress,
    /// Completed successfully
    Completed,
    /// Short packet (not necessarily error)
    Short,
    /// Stall
    Stall,
    /// Device not responding
    NoResponse,
    /// Babble
    Babble,
    /// CRC error
    CrcError,
    /// Cancelled
    Cancelled,
    /// Other error
    Error,
}

/// USB hub
#[derive(Debug)]
pub struct UsbHub {
    /// Device address
    pub address: u8,
    /// Number of ports
    pub num_ports: u8,
    /// Port status
    pub port_status: Vec<PortStatus>,
    /// Hub characteristics
    pub characteristics: u16,
}

/// USB port status
#[derive(Debug, Clone, Copy, Default)]
pub struct PortStatus {
    /// Device connected
    pub connected: bool,
    /// Port enabled
    pub enabled: bool,
    /// Port suspended
    pub suspended: bool,
    /// Over-current
    pub over_current: bool,
    /// Port reset
    pub reset: bool,
    /// Port power
    pub powered: bool,
    /// Low-speed device
    pub low_speed: bool,
    /// High-speed device
    pub high_speed: bool,
    /// Connection change
    pub connect_change: bool,
    /// Enable change
    pub enable_change: bool,
}

/// USB Host Controller interface
pub trait UsbHostController {
    /// Resets the controller
    fn reset(&mut self) -> Result<(), DevError>;
    
    /// Starts the controller
    fn start(&mut self) -> Result<(), DevError>;
    
    /// Stops the controller
    fn stop(&mut self) -> Result<(), DevError>;
    
    /// Gets port status
    fn get_port_status(&self, port: u8) -> Option<PortStatus>;
    
    /// Resets a port
    fn reset_port(&mut self, port: u8) -> Result<(), DevError>;
    
    /// Enables a port
    fn enable_port(&mut self, port: u8) -> Result<(), DevError>;
    
    /// Disables a port
    fn disable_port(&mut self, port: u8) -> Result<(), DevError>;
    
    /// Submits a transfer
    fn submit_transfer(&mut self, transfer: &mut UsbTransfer, data: &mut [u8]) -> Result<(), DevError>;
    
    /// Polls for completed transfers
    fn poll_completed(&mut self) -> Option<u64>;
    
    /// Cancels a transfer
    fn cancel_transfer(&mut self, id: u64) -> Result<(), DevError>;
}

/// USB subsystem manager
pub struct UsbManager {
    /// Connected devices
    devices: BTreeMap<u8, FullUsbDevice>,
    /// Connected hubs
    hubs: BTreeMap<u8, UsbHub>,
    /// Next device address
    next_address: u8,
    /// Pending transfers
    pending_transfers: VecDeque<UsbTransfer>,
    /// Next transfer ID
    next_transfer_id: u64,
    /// Root hub ports
    root_ports: u8,
}

impl UsbManager {
    /// Creates a new USB manager
    pub fn new(root_ports: u8) -> Self {
        Self {
            devices: BTreeMap::new(),
            hubs: BTreeMap::new(),
            next_address: 1,
            pending_transfers: VecDeque::new(),
            next_transfer_id: 1,
            root_ports,
        }
    }

    /// Allocates a new device address
    fn allocate_address(&mut self) -> Option<u8> {
        if self.next_address > 127 {
            return None;
        }
        let addr = self.next_address;
        self.next_address += 1;
        Some(addr)
    }

    /// Enumerates a new device
    pub fn enumerate_device(&mut self, port: u8, speed: u8) -> Result<u8, DevError> {
        let address = self.allocate_address().ok_or(DevError::OutOfMemory)?;

        let device = FullUsbDevice {
            info: UsbDevice {
                address,
                speed,
                vendor_id: 0,
                product_id: 0,
                device_class: 0,
                device_subclass: 0,
                device_protocol: 0,
                num_configurations: 0,
            },
            state: DeviceState::Attached,
            manufacturer: None,
            product: None,
            serial: None,
            configurations: Vec::new(),
            current_config: None,
            hub_port: Some((0, port)),
        };

        self.devices.insert(address, device);
        Ok(address)
    }

    /// Updates device descriptor
    pub fn update_device_descriptor(
        &mut self,
        address: u8,
        vendor_id: u16,
        product_id: u16,
        device_class: u8,
        device_subclass: u8,
        device_protocol: u8,
        num_configurations: u8,
    ) -> Result<(), DevError> {
        let device = self.devices.get_mut(&address).ok_or(DevError::DeviceNotFound)?;
        device.info.vendor_id = vendor_id;
        device.info.product_id = product_id;
        device.info.device_class = device_class;
        device.info.device_subclass = device_subclass;
        device.info.device_protocol = device_protocol;
        device.info.num_configurations = num_configurations;
        Ok(())
    }

    /// Sets device address
    pub fn set_device_address(&mut self, old_address: u8, new_address: u8) -> Result<(), DevError> {
        let mut device = self.devices.remove(&old_address).ok_or(DevError::DeviceNotFound)?;
        device.info.address = new_address;
        device.state = DeviceState::Addressed;
        self.devices.insert(new_address, device);
        Ok(())
    }

    /// Configures a device
    pub fn configure_device(&mut self, address: u8, config: u8) -> Result<(), DevError> {
        let device = self.devices.get_mut(&address).ok_or(DevError::DeviceNotFound)?;
        device.current_config = Some(config);
        device.state = DeviceState::Configured;
        Ok(())
    }

    /// Detaches a device
    pub fn detach_device(&mut self, address: u8) -> Option<FullUsbDevice> {
        self.devices.remove(&address)
    }

    /// Gets a device
    pub fn get_device(&self, address: u8) -> Option<&FullUsbDevice> {
        self.devices.get(&address)
    }

    /// Lists all devices
    pub fn list_devices(&self) -> Vec<&FullUsbDevice> {
        self.devices.values().collect()
    }

    /// Creates a control transfer
    pub fn control_transfer(
        &mut self,
        address: u8,
        setup: UsbSetup,
        callback: u64,
    ) -> u64 {
        let id = self.next_transfer_id;
        self.next_transfer_id += 1;

        let transfer = UsbTransfer {
            id,
            device: address,
            endpoint: 0,
            transfer_type: TransferType::Control,
            setup: Some(setup),
            is_in: (setup.bm_request_type & 0x80) != 0,
            length: setup.w_length as usize,
            actual_length: 0,
            status: TransferStatus::Pending,
            callback,
        };

        self.pending_transfers.push_back(transfer);
        id
    }

    /// Creates a bulk transfer
    pub fn bulk_transfer(
        &mut self,
        address: u8,
        endpoint: u8,
        is_in: bool,
        length: usize,
        callback: u64,
    ) -> u64 {
        let id = self.next_transfer_id;
        self.next_transfer_id += 1;

        let transfer = UsbTransfer {
            id,
            device: address,
            endpoint,
            transfer_type: TransferType::Bulk,
            setup: None,
            is_in,
            length,
            actual_length: 0,
            status: TransferStatus::Pending,
            callback,
        };

        self.pending_transfers.push_back(transfer);
        id
    }

    /// Gets next pending transfer
    pub fn poll_pending(&mut self) -> Option<UsbTransfer> {
        self.pending_transfers.pop_front()
    }

    /// Returns number of pending transfers
    pub fn pending_count(&self) -> usize {
        self.pending_transfers.len()
    }
}

impl Default for UsbManager {
    fn default() -> Self {
        Self::new(4) // 4 root hub ports typical
    }
}

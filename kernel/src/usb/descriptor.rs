//! USB Descriptor Definitions
//!
//! This module contains all USB descriptor types as defined in the USB specification.

/// USB descriptor types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DescriptorType {
    Device = 1,
    Configuration = 2,
    String = 3,
    Interface = 4,
    Endpoint = 5,
    DeviceQualifier = 6,
    OtherSpeedConfig = 7,
    InterfacePower = 8,
    Otg = 9,
    Debug = 10,
    InterfaceAssociation = 11,
    Bos = 15,
    DeviceCapability = 16,
    SuperSpeedEndpointCompanion = 48,
    SuperSpeedPlusIsochEndpointCompanion = 49,
    HidClass = 33,
    HidReport = 34,
    HidPhysical = 35,
    Hub = 41,
    SuperSpeedHub = 42,
}

/// USB Device Descriptor (18 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct DeviceDescriptor {
    /// Size of this descriptor (18)
    pub length: u8,
    /// Descriptor type (1 = Device)
    pub descriptor_type: u8,
    /// USB specification version (BCD)
    pub usb_version: u16,
    /// Device class code
    pub device_class: u8,
    /// Device subclass code
    pub device_subclass: u8,
    /// Device protocol code
    pub device_protocol: u8,
    /// Maximum packet size for endpoint 0
    pub max_packet_size_0: u8,
    /// Vendor ID
    pub vendor_id: u16,
    /// Product ID
    pub product_id: u16,
    /// Device version (BCD)
    pub device_version: u16,
    /// Index of manufacturer string
    pub manufacturer_index: u8,
    /// Index of product string
    pub product_index: u8,
    /// Index of serial number string
    pub serial_number_index: u8,
    /// Number of configurations
    pub num_configurations: u8,
}

impl DeviceDescriptor {
    /// Parse from raw bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 18 {
            return None;
        }
        
        Some(Self {
            length: data[0],
            descriptor_type: data[1],
            usb_version: u16::from_le_bytes([data[2], data[3]]),
            device_class: data[4],
            device_subclass: data[5],
            device_protocol: data[6],
            max_packet_size_0: data[7],
            vendor_id: u16::from_le_bytes([data[8], data[9]]),
            product_id: u16::from_le_bytes([data[10], data[11]]),
            device_version: u16::from_le_bytes([data[12], data[13]]),
            manufacturer_index: data[14],
            product_index: data[15],
            serial_number_index: data[16],
            num_configurations: data[17],
        })
    }

    /// Get USB version as string (e.g., "2.0", "3.1")
    pub fn usb_version_string(&self) -> &'static str {
        match self.usb_version {
            0x0100 => "1.0",
            0x0110 => "1.1",
            0x0200 => "2.0",
            0x0201 => "2.0.1",
            0x0210 => "2.1",
            0x0300 => "3.0",
            0x0310 => "3.1",
            0x0320 => "3.2",
            _ => "Unknown",
        }
    }
}

/// USB Configuration Descriptor (9 bytes, variable total length)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ConfigurationDescriptor {
    /// Size of this descriptor (9)
    pub length: u8,
    /// Descriptor type (2 = Configuration)
    pub descriptor_type: u8,
    /// Total length of configuration data
    pub total_length: u16,
    /// Number of interfaces
    pub num_interfaces: u8,
    /// Configuration value for SET_CONFIGURATION
    pub configuration_value: u8,
    /// Index of configuration string
    pub configuration_index: u8,
    /// Attributes (self-powered, remote wakeup)
    pub attributes: u8,
    /// Maximum power in 2mA units
    pub max_power: u8,
}

impl ConfigurationDescriptor {
    /// Parse from raw bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 9 {
            return None;
        }
        
        Some(Self {
            length: data[0],
            descriptor_type: data[1],
            total_length: u16::from_le_bytes([data[2], data[3]]),
            num_interfaces: data[4],
            configuration_value: data[5],
            configuration_index: data[6],
            attributes: data[7],
            max_power: data[8],
        })
    }

    /// Check if self-powered
    pub fn is_self_powered(&self) -> bool {
        (self.attributes & 0x40) != 0
    }

    /// Check if remote wakeup supported
    pub fn supports_remote_wakeup(&self) -> bool {
        (self.attributes & 0x20) != 0
    }

    /// Get max power in milliamps
    pub fn max_power_ma(&self) -> u16 {
        self.max_power as u16 * 2
    }
}

/// USB Interface Descriptor (9 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct InterfaceDescriptor {
    /// Size of this descriptor (9)
    pub length: u8,
    /// Descriptor type (4 = Interface)
    pub descriptor_type: u8,
    /// Interface number
    pub interface_number: u8,
    /// Alternate setting
    pub alternate_setting: u8,
    /// Number of endpoints
    pub num_endpoints: u8,
    /// Interface class
    pub interface_class: u8,
    /// Interface subclass
    pub interface_subclass: u8,
    /// Interface protocol
    pub interface_protocol: u8,
    /// Index of interface string
    pub interface_index: u8,
}

impl InterfaceDescriptor {
    /// Parse from raw bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 9 {
            return None;
        }
        
        Some(Self {
            length: data[0],
            descriptor_type: data[1],
            interface_number: data[2],
            alternate_setting: data[3],
            num_endpoints: data[4],
            interface_class: data[5],
            interface_subclass: data[6],
            interface_protocol: data[7],
            interface_index: data[8],
        })
    }

    /// Get interface class name
    pub fn class_name(&self) -> &'static str {
        match self.interface_class {
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

/// USB Endpoint Descriptor (7 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct EndpointDescriptor {
    /// Size of this descriptor (7)
    pub length: u8,
    /// Descriptor type (5 = Endpoint)
    pub descriptor_type: u8,
    /// Endpoint address
    pub endpoint_address: u8,
    /// Endpoint attributes
    pub attributes: u8,
    /// Maximum packet size
    pub max_packet_size: u16,
    /// Polling interval
    pub interval: u8,
}

impl EndpointDescriptor {
    /// Parse from raw bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 7 {
            return None;
        }
        
        Some(Self {
            length: data[0],
            descriptor_type: data[1],
            endpoint_address: data[2],
            attributes: data[3],
            max_packet_size: u16::from_le_bytes([data[4], data[5]]),
            interval: data[6],
        })
    }

    /// Get endpoint number (0-15)
    pub fn endpoint_number(&self) -> u8 {
        self.endpoint_address & 0x0F
    }

    /// Check if endpoint is IN (device to host)
    pub fn is_in(&self) -> bool {
        (self.endpoint_address & 0x80) != 0
    }

    /// Check if endpoint is OUT (host to device)
    pub fn is_out(&self) -> bool {
        !self.is_in()
    }

    /// Get transfer type
    pub fn transfer_type(&self) -> super::TransferType {
        match self.attributes & 0x03 {
            0 => super::TransferType::Control,
            1 => super::TransferType::Isochronous,
            2 => super::TransferType::Bulk,
            3 => super::TransferType::Interrupt,
            _ => unreachable!(),
        }
    }

    /// Get synchronization type (for isochronous endpoints)
    pub fn sync_type(&self) -> u8 {
        (self.attributes >> 2) & 0x03
    }

    /// Get usage type (for isochronous endpoints)
    pub fn usage_type(&self) -> u8 {
        (self.attributes >> 4) & 0x03
    }
}

/// USB String Descriptor (variable length)
#[derive(Debug, Clone)]
pub struct StringDescriptor {
    /// Descriptor length
    pub length: u8,
    /// Descriptor type (3 = String)
    pub descriptor_type: u8,
    /// Unicode string data
    pub data: alloc::vec::Vec<u16>,
}

impl StringDescriptor {
    /// Parse from raw bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 2 {
            return None;
        }
        
        let length = data[0];
        if data.len() < length as usize {
            return None;
        }
        
        let mut chars = alloc::vec::Vec::new();
        for i in (2..length as usize).step_by(2) {
            if i + 1 < data.len() {
                chars.push(u16::from_le_bytes([data[i], data[i + 1]]));
            }
        }
        
        Some(Self {
            length,
            descriptor_type: data[1],
            data: chars,
        })
    }

    /// Convert to Rust string
    pub fn to_string(&self) -> alloc::string::String {
        alloc::string::String::from_utf16_lossy(&self.data)
    }
}

/// USB HID Descriptor (9 bytes minimum)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct HidDescriptor {
    /// Size of this descriptor
    pub length: u8,
    /// Descriptor type (0x21 = HID)
    pub descriptor_type: u8,
    /// HID specification version (BCD)
    pub hid_version: u16,
    /// Country code
    pub country_code: u8,
    /// Number of class descriptors
    pub num_descriptors: u8,
    /// Type of first class descriptor
    pub descriptor_type_1: u8,
    /// Length of first class descriptor
    pub descriptor_length_1: u16,
}

impl HidDescriptor {
    /// Parse from raw bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 9 {
            return None;
        }
        
        Some(Self {
            length: data[0],
            descriptor_type: data[1],
            hid_version: u16::from_le_bytes([data[2], data[3]]),
            country_code: data[4],
            num_descriptors: data[5],
            descriptor_type_1: data[6],
            descriptor_length_1: u16::from_le_bytes([data[7], data[8]]),
        })
    }
}

/// USB Hub Descriptor
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct HubDescriptor {
    /// Size of this descriptor
    pub length: u8,
    /// Descriptor type (0x29 = Hub)
    pub descriptor_type: u8,
    /// Number of downstream ports
    pub num_ports: u8,
    /// Hub characteristics
    pub characteristics: u16,
    /// Power on to power good time (in 2ms units)
    pub power_on_time: u8,
    /// Maximum hub current requirement
    pub hub_current: u8,
}

impl HubDescriptor {
    /// Parse from raw bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 7 {
            return None;
        }
        
        Some(Self {
            length: data[0],
            descriptor_type: data[1],
            num_ports: data[2],
            characteristics: u16::from_le_bytes([data[3], data[4]]),
            power_on_time: data[5],
            hub_current: data[6],
        })
    }

    /// Get power switching mode
    pub fn power_switching_mode(&self) -> &'static str {
        match self.characteristics & 0x03 {
            0 => "Ganged",
            1 => "Individual",
            _ => "Reserved",
        }
    }

    /// Check if compound device
    pub fn is_compound(&self) -> bool {
        (self.characteristics & 0x04) != 0
    }

    /// Get overcurrent protection mode
    pub fn overcurrent_mode(&self) -> &'static str {
        match (self.characteristics >> 3) & 0x03 {
            0 => "Global",
            1 => "Individual",
            _ => "None",
        }
    }
}

/// USB Device Qualifier Descriptor (10 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct DeviceQualifierDescriptor {
    /// Size of this descriptor (10)
    pub length: u8,
    /// Descriptor type (6 = Device Qualifier)
    pub descriptor_type: u8,
    /// USB specification version (BCD)
    pub usb_version: u16,
    /// Device class code
    pub device_class: u8,
    /// Device subclass code
    pub device_subclass: u8,
    /// Device protocol code
    pub device_protocol: u8,
    /// Maximum packet size for endpoint 0
    pub max_packet_size_0: u8,
    /// Number of other-speed configurations
    pub num_configurations: u8,
    /// Reserved
    pub reserved: u8,
}

impl DeviceQualifierDescriptor {
    /// Parse from raw bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 10 {
            return None;
        }
        
        Some(Self {
            length: data[0],
            descriptor_type: data[1],
            usb_version: u16::from_le_bytes([data[2], data[3]]),
            device_class: data[4],
            device_subclass: data[5],
            device_protocol: data[6],
            max_packet_size_0: data[7],
            num_configurations: data[8],
            reserved: data[9],
        })
    }
}

/// Parse all descriptors from configuration data
pub fn parse_configuration_descriptors(data: &[u8]) -> alloc::vec::Vec<ParsedDescriptor> {
    let mut descriptors = alloc::vec::Vec::new();
    let mut offset = 0;
    
    while offset + 2 <= data.len() {
        let length = data[offset] as usize;
        let desc_type = data[offset + 1];
        
        if length < 2 || offset + length > data.len() {
            break;
        }
        
        let desc_data = &data[offset..offset + length];
        
        let parsed = match desc_type {
            2 => ConfigurationDescriptor::from_bytes(desc_data).map(ParsedDescriptor::Configuration),
            4 => InterfaceDescriptor::from_bytes(desc_data).map(ParsedDescriptor::Interface),
            5 => EndpointDescriptor::from_bytes(desc_data).map(ParsedDescriptor::Endpoint),
            0x21 => HidDescriptor::from_bytes(desc_data).map(ParsedDescriptor::Hid),
            _ => Some(ParsedDescriptor::Unknown(desc_type, length as u8)),
        };
        
        if let Some(desc) = parsed {
            descriptors.push(desc);
        }
        
        offset += length;
    }
    
    descriptors
}

/// Parsed descriptor enum
#[derive(Debug, Clone)]
pub enum ParsedDescriptor {
    Device(DeviceDescriptor),
    Configuration(ConfigurationDescriptor),
    Interface(InterfaceDescriptor),
    Endpoint(EndpointDescriptor),
    String(StringDescriptor),
    Hid(HidDescriptor),
    Hub(HubDescriptor),
    Unknown(u8, u8), // type, length
}

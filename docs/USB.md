# USB Subsystem Documentation

## Overview

Splax OS implements a comprehensive USB (Universal Serial Bus) subsystem supporting USB 1.0 through USB 3.x devices. The subsystem provides a layered architecture with host controller drivers, a core USB stack, and device-class drivers including USB Audio Class (UAC) and USB Video Class (UVC).

```text
┌─────────────────────────────────────────────────────────────┐
│                     USB Device Drivers                      │
│    (HID Keyboard, Mass Storage, Hub, Audio UAC, Video UVC) │
├─────────────────────────────────────────────────────────────┤
│                      USB Core Layer                         │
│      (Device enumeration, descriptor parsing, transfers)    │
├─────────────────────────────────────────────────────────────┤
│              Host Controller Drivers                        │
│           (xHCI for USB 3.x, EHCI for USB 2.0)             │
├─────────────────────────────────────────────────────────────┤
│                   PCI/MMIO Interface                        │
└─────────────────────────────────────────────────────────────┘
```

## USB Video Class (UVC) Driver

**Location:** `kernel/src/usb/uvc.rs`

Full USB Video Class support for webcams and capture devices:

### Features

- **UVC 1.0/1.1/1.5**: Complete descriptor parsing
- **Video Formats**: YUYV, MJPEG, H.264, NV12
- **Controls**:
  - Camera: exposure, focus, zoom, pan/tilt
  - Processing: brightness, contrast, saturation, sharpness
- **Streaming**: Isochronous and bulk transfer modes
- **Multi-resolution**: Dynamic resolution switching

### Usage

```rust
// Enumerate UVC devices
let devices = usb::uvc::enumerate_cameras();

// Open a camera
let camera = usb::uvc::open(device_id)?;

// Start streaming
camera.start_streaming(|frame| {
    // Process video frame
    process_frame(frame.data, frame.width, frame.height);
})?;
```

---

## USB Speed Classes

The subsystem supports all USB speed classes:

| Speed | Version | Data Rate | Use Cases |
|-------|---------|-----------|-----------|
| Low Speed | USB 1.0 | 1.5 Mbps | Keyboards, mice |
| Full Speed | USB 1.1 | 12 Mbps | Legacy devices |
| High Speed | USB 2.0 | 480 Mbps | Storage, webcams |
| SuperSpeed | USB 3.0 | 5 Gbps | Fast storage, video |
| SuperSpeed+ | USB 3.1+ | 10+ Gbps | High-performance devices |

```rust
/// USB device speed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UsbSpeed {
    Low = 0,       // 1.5 Mbps (USB 1.0)
    Full = 1,      // 12 Mbps (USB 1.1)
    High = 2,      // 480 Mbps (USB 2.0)
    Super = 3,     // 5 Gbps (USB 3.0)
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
```

---

## Core Types

### Transfer Types

USB defines four transfer types for different use cases:

```rust
/// USB transfer types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TransferType {
    Control = 0,     // Device configuration, bidirectional
    Isochronous = 1, // Streaming (audio/video), guaranteed bandwidth
    Bulk = 2,        // Large data transfers, error-corrected
    Interrupt = 3,   // Small periodic data (HID)
}
```

### Device State Machine

USB devices transition through defined states:

```rust
/// USB device state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Detached,    // Not connected
    Attached,    // Physically connected
    Powered,     // VBUS applied
    Default,     // Reset, address = 0
    Addressed,   // Unique address assigned
    Configured,  // Configuration selected
    Suspended,   // Power-saving mode
}
```

### Endpoint Structure

Endpoints are the communication channels between host and device:

```rust
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

/// USB endpoint direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Out = 0, // Host to device
    In = 1,  // Device to host
}
```

### USB Device Information

Complete device representation:

```rust
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
```

### Device Class Codes

Standard USB device classes:

```rust
/// USB class codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClassCode {
    InterfaceDefined = 0x00, // Class defined at interface level
    Audio = 0x01,            // Speakers, microphones
    Communications = 0x02,   // Modems, network adapters
    Hid = 0x03,             // Keyboards, mice, joysticks
    Physical = 0x05,         // Force feedback
    Image = 0x06,            // Scanners, cameras
    Printer = 0x07,          // Printers
    MassStorage = 0x08,      // Flash drives, HDDs
    Hub = 0x09,              // USB hubs
    CdcData = 0x0A,          // CDC data interface
    SmartCard = 0x0B,        // Smart card readers
    ContentSecurity = 0x0D,  // DRM devices
    Video = 0x0E,            // Webcams
    Healthcare = 0x0F,       // Medical devices
    AudioVideo = 0x10,       // A/V devices
    Billboard = 0x11,        // USB Type-C info
    UsbTypeCBridge = 0x12,   // Type-C bridge
    Diagnostic = 0xDC,       // Debug devices
    WirelessController = 0xE0, // Bluetooth, Wi-Fi
    Miscellaneous = 0xEF,    // Interface association
    ApplicationSpecific = 0xFE, // DFU, IrDA
    VendorSpecific = 0xFF,   // Custom protocols
}
```

---

## USB Descriptors

### Descriptor Types

```rust
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
    HidClass = 33,
    HidReport = 34,
    Hub = 41,
    SuperSpeedHub = 42,
}
```

### Device Descriptor (18 bytes)

The root descriptor for every USB device:

```rust
/// USB Device Descriptor (18 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct DeviceDescriptor {
    pub length: u8,              // Always 18
    pub descriptor_type: u8,     // 1 = Device
    pub usb_version: u16,        // BCD (e.g., 0x0200 = USB 2.0)
    pub device_class: u8,        // Class code
    pub device_subclass: u8,     // Subclass code
    pub device_protocol: u8,     // Protocol code
    pub max_packet_size_0: u8,   // Max packet size for EP0
    pub vendor_id: u16,          // Vendor ID (assigned by USB-IF)
    pub product_id: u16,         // Product ID (assigned by vendor)
    pub device_version: u16,     // Device release number (BCD)
    pub manufacturer_index: u8,  // String descriptor index
    pub product_index: u8,       // String descriptor index
    pub serial_number_index: u8, // String descriptor index
    pub num_configurations: u8,  // Number of configurations
}
```

### Configuration Descriptor (9 bytes)

Describes a device configuration:

```rust
/// USB Configuration Descriptor (9 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ConfigurationDescriptor {
    pub length: u8,              // 9
    pub descriptor_type: u8,     // 2 = Configuration
    pub total_length: u16,       // Total length including interfaces
    pub num_interfaces: u8,      // Number of interfaces
    pub configuration_value: u8, // Value for SET_CONFIGURATION
    pub configuration_index: u8, // String descriptor index
    pub attributes: u8,          // Self-powered, remote wakeup
    pub max_power: u8,           // Power in 2mA units
}

impl ConfigurationDescriptor {
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
```

### Interface Descriptor (9 bytes)

Describes a function within a configuration:

```rust
/// USB Interface Descriptor (9 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct InterfaceDescriptor {
    pub length: u8,             // 9
    pub descriptor_type: u8,    // 4 = Interface
    pub interface_number: u8,   // Interface number
    pub alternate_setting: u8,  // Alternate setting
    pub num_endpoints: u8,      // Number of endpoints
    pub interface_class: u8,    // Class code
    pub interface_subclass: u8, // Subclass code
    pub interface_protocol: u8, // Protocol code
    pub interface_index: u8,    // String descriptor index
}
```

### Endpoint Descriptor (7 bytes)

Describes an endpoint within an interface:

```rust
/// USB Endpoint Descriptor (7 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct EndpointDescriptor {
    pub length: u8,            // 7
    pub descriptor_type: u8,   // 5 = Endpoint
    pub endpoint_address: u8,  // Address + direction
    pub attributes: u8,        // Transfer type
    pub max_packet_size: u16,  // Max packet size
    pub interval: u8,          // Polling interval
}

impl EndpointDescriptor {
    /// Get endpoint number (0-15)
    pub fn endpoint_number(&self) -> u8 {
        self.endpoint_address & 0x0F
    }

    /// Check if endpoint is IN (device to host)
    pub fn is_in(&self) -> bool {
        (self.endpoint_address & 0x80) != 0
    }

    /// Get transfer type
    pub fn transfer_type(&self) -> TransferType {
        match self.attributes & 0x03 {
            0 => TransferType::Control,
            1 => TransferType::Isochronous,
            2 => TransferType::Bulk,
            3 => TransferType::Interrupt,
            _ => unreachable!(),
        }
    }
}
```

### String Descriptor

Unicode strings for device identification:

```rust
/// USB String Descriptor (variable length)
#[derive(Debug, Clone)]
pub struct StringDescriptor {
    pub length: u8,
    pub descriptor_type: u8,     // 3 = String
    pub data: Vec<u16>,          // UTF-16LE data
}

impl StringDescriptor {
    /// Convert to Rust string
    pub fn to_string(&self) -> String {
        String::from_utf16_lossy(&self.data)
    }
}
```

### HID Descriptor

For Human Interface Devices:

```rust
/// USB HID Descriptor (9 bytes minimum)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct HidDescriptor {
    pub length: u8,
    pub descriptor_type: u8,      // 0x21 = HID
    pub hid_version: u16,         // HID spec version (BCD)
    pub country_code: u8,         // Localized hardware
    pub num_descriptors: u8,      // Number of class descriptors
    pub descriptor_type_1: u8,    // Type of first descriptor
    pub descriptor_length_1: u16, // Length of first descriptor
}
```

### Hub Descriptor

For USB hub devices:

```rust
/// USB Hub Descriptor
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct HubDescriptor {
    pub length: u8,
    pub descriptor_type: u8,   // 0x29 = Hub
    pub num_ports: u8,         // Downstream port count
    pub characteristics: u16,  // Hub characteristics
    pub power_on_time: u8,     // Power-on to good (2ms units)
    pub hub_current: u8,       // Max hub current
}

impl HubDescriptor {
    /// Get power switching mode
    pub fn power_switching_mode(&self) -> &'static str {
        match self.characteristics & 0x03 {
            0 => "Ganged",
            1 => "Individual",
            _ => "Reserved",
        }
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
```

---

## Control Transfers

### Setup Packet

Control transfers begin with a setup packet:

```rust
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
```

### Request Type Field

```text
Bit 7:     Direction (0=OUT, 1=IN)
Bits 6-5:  Type (0=Standard, 1=Class, 2=Vendor)
Bits 4-0:  Recipient (0=Device, 1=Interface, 2=Endpoint)
```

### Standard Requests

```rust
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
```

### Common Setup Packet Helpers

```rust
impl SetupPacket {
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
```

---

## Transfer Results

```rust
/// USB transfer result
#[derive(Debug, Clone)]
pub enum TransferResult {
    Success(usize),      // Bytes transferred
    Stall,               // Endpoint stalled
    DataToggleError,     // Data toggle mismatch
    Timeout,             // No response
    BabbleError,         // Device sent too much data
    BufferOverrun,       // Buffer overflow
    BufferUnderrun,      // Buffer underflow
    NotResponding,       // Device not responding
    CrcError,            // CRC check failed
    BitStuffError,       // Bit stuffing error
    UnexpectedPid,       // Unexpected PID
    Cancelled,           // Transfer cancelled
    HostError,           // Host controller error
}
```

---

## Host Controller Interface

### UsbHostController Trait

Abstract interface for host controllers:

```rust
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
```

---

## xHCI Driver (USB 3.x)

### Architecture

```text
┌─────────────────────────────────────────────────────────┐
│                    xHCI Controller                       │
├─────────────────────────────────────────────────────────┤
│  Capability Registers  │  Operational Registers         │
├─────────────────────────────────────────────────────────┤
│  Runtime Registers     │  Doorbell Registers            │
├─────────────────────────────────────────────────────────┤
│  Device Context Array  │  Command Ring                  │
├─────────────────────────────────────────────────────────┤
│  Event Ring            │  Transfer Rings                │
└─────────────────────────────────────────────────────────┘
```

### Capability Registers

```rust
mod cap_regs {
    pub const CAPLENGTH: usize = 0x00;   // Capability length
    pub const HCIVERSION: usize = 0x02;  // HCI version
    pub const HCSPARAMS1: usize = 0x04;  // Structural parameters 1
    pub const HCSPARAMS2: usize = 0x08;  // Structural parameters 2
    pub const HCSPARAMS3: usize = 0x0C;  // Structural parameters 3
    pub const HCCPARAMS1: usize = 0x10;  // Capability parameters 1
    pub const DBOFF: usize = 0x14;       // Doorbell offset
    pub const RTSOFF: usize = 0x18;      // Runtime registers offset
    pub const HCCPARAMS2: usize = 0x1C;  // Capability parameters 2
}
```

### Operational Registers

```rust
mod op_regs {
    pub const USBCMD: usize = 0x00;   // USB Command
    pub const USBSTS: usize = 0x04;   // USB Status
    pub const PAGESIZE: usize = 0x08; // Page Size
    pub const DNCTRL: usize = 0x14;   // Device Notification Control
    pub const CRCR: usize = 0x18;     // Command Ring Control
    pub const DCBAAP: usize = 0x30;   // Device Context Base Address Array Pointer
    pub const CONFIG: usize = 0x38;   // Configure
}
```

### USB Command Register

```rust
mod usbcmd {
    pub const RUN: u32 = 1 << 0;    // Run/Stop
    pub const HCRST: u32 = 1 << 1;  // Host Controller Reset
    pub const INTE: u32 = 1 << 2;   // Interrupt Enable
    pub const HSEE: u32 = 1 << 3;   // Host System Error Enable
    pub const LHCRST: u32 = 1 << 7; // Light HC Reset
    pub const CSS: u32 = 1 << 8;    // Controller Save State
    pub const CRS: u32 = 1 << 9;    // Controller Restore State
    pub const EWE: u32 = 1 << 10;   // Enable Wrap Event
    pub const EU3S: u32 = 1 << 11;  // Enable U3 MFINDEX Stop
}
```

### USB Status Register

```rust
mod usbsts {
    pub const HCH: u32 = 1 << 0;   // HC Halted
    pub const HSE: u32 = 1 << 2;   // Host System Error
    pub const EINT: u32 = 1 << 3;  // Event Interrupt
    pub const PCD: u32 = 1 << 4;   // Port Change Detect
    pub const SSS: u32 = 1 << 8;   // Save State Status
    pub const RSS: u32 = 1 << 9;   // Restore State Status
    pub const SRE: u32 = 1 << 10;  // Save/Restore Error
    pub const CNR: u32 = 1 << 11;  // Controller Not Ready
    pub const HCE: u32 = 1 << 12;  // Host Controller Error
}
```

### Port Status and Control

```rust
mod portsc {
    pub const CCS: u32 = 1 << 0;          // Current Connect Status
    pub const PED: u32 = 1 << 1;          // Port Enabled/Disabled
    pub const OCA: u32 = 1 << 3;          // Over-current Active
    pub const PR: u32 = 1 << 4;           // Port Reset
    pub const PLS_MASK: u32 = 0xF << 5;   // Port Link State
    pub const PP: u32 = 1 << 9;           // Port Power
    pub const SPEED_MASK: u32 = 0xF << 10; // Port Speed
    pub const CSC: u32 = 1 << 17;         // Connect Status Change
    pub const PEC: u32 = 1 << 18;         // Port Enable Change
    pub const WRC: u32 = 1 << 19;         // Warm Reset Change
    pub const OCC: u32 = 1 << 20;         // Over-current Change
    pub const PRC: u32 = 1 << 21;         // Port Reset Change
    pub const PLC: u32 = 1 << 22;         // Port Link State Change
}
```

### Transfer Request Block (TRB)

xHCI uses TRBs for all communication:

```rust
/// Transfer Request Block
#[derive(Debug, Clone, Copy)]
#[repr(C, align(16))]
pub struct Trb {
    pub parameter: u64,
    pub status: u32,
    pub control: u32,
}

impl Trb {
    /// Set TRB type
    pub fn set_type(&mut self, trb_type: TrbType) {
        self.control = (self.control & !0xFC00) | ((trb_type as u32) << 10);
    }

    /// Get TRB type
    pub fn trb_type(&self) -> u8 {
        ((self.control >> 10) & 0x3F) as u8
    }

    /// Set cycle bit
    pub fn set_cycle(&mut self, cycle: bool) {
        if cycle {
            self.control |= 1;
        } else {
            self.control &= !1;
        }
    }

    /// Get cycle bit
    pub fn cycle(&self) -> bool {
        (self.control & 1) != 0
    }

    /// Get completion code
    pub fn completion_code(&self) -> u8 {
        ((self.status >> 24) & 0xFF) as u8
    }
}
```

### TRB Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrbType {
    Normal = 1,
    SetupStage = 2,
    DataStage = 3,
    StatusStage = 4,
    Isoch = 5,
    Link = 6,
    EventData = 7,
    NoOp = 8,
    EnableSlotCommand = 9,
    DisableSlotCommand = 10,
    AddressDeviceCommand = 11,
    ConfigureEndpointCommand = 12,
    EvaluateContextCommand = 13,
    ResetEndpointCommand = 14,
    StopEndpointCommand = 15,
    SetTrDequeuePointerCommand = 16,
    ResetDeviceCommand = 17,
    // ... event TRBs
    TransferEvent = 32,
    CommandCompletionEvent = 33,
    PortStatusChangeEvent = 34,
}
```

### TRB Completion Codes

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrbCompletionCode {
    Invalid = 0,
    Success = 1,
    DataBufferError = 2,
    BabbleDetectedError = 3,
    UsbTransactionError = 4,
    TrbError = 5,
    StallError = 6,
    ResourceError = 7,
    BandwidthError = 8,
    NoSlotsAvailableError = 9,
    SlotNotEnabledError = 11,
    EndpointNotEnabledError = 12,
    ShortPacket = 13,
    RingUnderrun = 14,
    RingOverrun = 15,
    ParameterError = 17,
    ContextStateError = 19,
    CommandRingStopped = 24,
    CommandAborted = 25,
    Stopped = 26,
}
```

### Ring Structures

#### Command Ring

```rust
/// Command Ring
pub struct CommandRing {
    trbs: Vec<Trb>,
    enqueue_ptr: usize,
    cycle_bit: bool,
}

impl CommandRing {
    pub fn new(size: usize) -> Self {
        let mut trbs = Vec::with_capacity(size);
        for _ in 0..size {
            trbs.push(Trb::new());
        }
        
        // Add link TRB at the end
        let last_idx = size - 1;
        trbs[last_idx].set_type(TrbType::Link);
        trbs[last_idx].control |= 1 << 1; // Toggle Cycle
        
        Self { trbs, enqueue_ptr: 0, cycle_bit: true }
    }

    /// Enqueue a command TRB
    pub fn enqueue(&mut self, mut trb: Trb) -> usize {
        trb.set_cycle(self.cycle_bit);
        let idx = self.enqueue_ptr;
        self.trbs[idx] = trb;
        
        self.enqueue_ptr += 1;
        if self.enqueue_ptr >= self.trbs.len() - 1 {
            // Wrap around, toggle cycle bit
            self.trbs[self.trbs.len() - 1].set_cycle(self.cycle_bit);
            self.cycle_bit = !self.cycle_bit;
            self.enqueue_ptr = 0;
        }
        
        idx
    }
}
```

#### Event Ring

```rust
/// Event Ring Segment Table Entry
#[derive(Debug, Clone, Copy)]
#[repr(C, align(64))]
pub struct EventRingSegmentTableEntry {
    pub ring_segment_base: u64,
    pub ring_segment_size: u16,
    pub reserved: [u8; 6],
}

/// Event Ring
pub struct EventRing {
    trbs: Vec<Trb>,
    segment_table: Vec<EventRingSegmentTableEntry>,
    dequeue_ptr: usize,
    cycle_bit: bool,
}

impl EventRing {
    /// Check if there's an event to process
    pub fn has_event(&self) -> bool {
        self.trbs[self.dequeue_ptr].cycle() == self.cycle_bit
    }

    /// Dequeue an event TRB
    pub fn dequeue(&mut self) -> Option<Trb> {
        if !self.has_event() {
            return None;
        }
        
        let trb = self.trbs[self.dequeue_ptr];
        self.dequeue_ptr += 1;
        
        if self.dequeue_ptr >= self.trbs.len() {
            self.dequeue_ptr = 0;
            self.cycle_bit = !self.cycle_bit;
        }
        
        Some(trb)
    }
}
```

#### Transfer Ring

```rust
/// Transfer Ring
pub struct TransferRing {
    trbs: Vec<Trb>,
    enqueue_ptr: usize,
    cycle_bit: bool,
}

impl TransferRing {
    /// Enqueue a transfer TRB
    pub fn enqueue(&mut self, mut trb: Trb) -> usize {
        trb.set_cycle(self.cycle_bit);
        let idx = self.enqueue_ptr;
        self.trbs[idx] = trb;
        
        self.enqueue_ptr += 1;
        if self.enqueue_ptr >= self.trbs.len() - 1 {
            self.trbs[self.trbs.len() - 1].set_cycle(self.cycle_bit);
            self.cycle_bit = !self.cycle_bit;
            self.enqueue_ptr = 0;
        }
        
        idx
    }
}
```

### xHCI Controller Initialization

```rust
impl UsbHostController for XhciController {
    fn init(&mut self) -> Result<(), &'static str> {
        // 1. Read capability length
        self.cap_length = (self.read_cap_reg(cap_regs::CAPLENGTH) & 0xFF) as u8;
        
        // 2. Read structural parameters
        let hcsparams1 = self.read_cap_reg(cap_regs::HCSPARAMS1);
        self.num_slots = (hcsparams1 & 0xFF) as u8;
        self.num_ports = ((hcsparams1 >> 24) & 0xFF) as u8;
        
        // 3. Wait for controller ready
        self.wait_cnr_clear()?;
        
        // 4. Stop controller if running
        let usbcmd = self.read_op_reg(op_regs::USBCMD);
        if (usbcmd & usbcmd::RUN) != 0 {
            self.write_op_reg(op_regs::USBCMD, usbcmd & !usbcmd::RUN);
            self.wait_halt()?;
        }
        
        // 5. Reset the controller
        self.write_op_reg(op_regs::USBCMD, usbcmd::HCRST);
        // ... wait for reset
        
        // 6. Set number of device slots
        self.write_op_reg(op_regs::CONFIG, self.num_slots as u32);
        
        // 7. Set up Device Context Base Address Array
        self.write_op_reg(op_regs::DCBAAP, dcbaa_addr as u32);
        
        // 8. Set up Command Ring
        self.write_op_reg(op_regs::CRCR, crcr as u32);
        
        // 9. Set up Event Ring via Runtime Registers
        // ... ERSTSZ, ERDP, ERSTBA
        
        // 10. Enable interrupts and start controller
        self.write_op_reg(op_regs::USBCMD, usbcmd | usbcmd::RUN | usbcmd::INTE);
        
        Ok(())
    }
}
```

---

## USB HID (Human Interface Device)

### HID Usage Pages

```rust
/// HID Usage Page codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UsagePage {
    GenericDesktop = 0x01,    // Mouse, joystick, gamepad
    Keyboard = 0x07,          // Keyboard/keypad
    Led = 0x08,               // LED indicators
    Button = 0x09,            // Buttons
    Consumer = 0x0C,          // Media controls
    Digitizer = 0x0D,         // Touch screens
}
```

### Keyboard Modifiers

```rust
/// HID keyboard modifiers
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyboardModifiers {
    pub left_ctrl: bool,
    pub left_shift: bool,
    pub left_alt: bool,
    pub left_gui: bool,
    pub right_ctrl: bool,
    pub right_shift: bool,
    pub right_alt: bool,
    pub right_gui: bool,
}

impl KeyboardModifiers {
    /// Create from modifier byte
    pub fn from_byte(byte: u8) -> Self {
        Self {
            left_ctrl: (byte & 0x01) != 0,
            left_shift: (byte & 0x02) != 0,
            left_alt: (byte & 0x04) != 0,
            left_gui: (byte & 0x08) != 0,
            right_ctrl: (byte & 0x10) != 0,
            right_shift: (byte & 0x20) != 0,
            right_alt: (byte & 0x40) != 0,
            right_gui: (byte & 0x80) != 0,
        }
    }
}
```

### Keyboard LEDs

```rust
/// Keyboard LED state
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyboardLeds {
    pub num_lock: bool,
    pub caps_lock: bool,
    pub scroll_lock: bool,
    pub compose: bool,
    pub kana: bool,
}

impl KeyboardLeds {
    /// Convert to LED report byte
    pub fn to_byte(&self) -> u8 {
        let mut byte = 0u8;
        if self.num_lock { byte |= 0x01; }
        if self.caps_lock { byte |= 0x02; }
        if self.scroll_lock { byte |= 0x04; }
        if self.compose { byte |= 0x08; }
        if self.kana { byte |= 0x10; }
        byte
    }
}
```

### Keyboard Report (Boot Protocol)

```rust
/// Standard HID keyboard report (8 bytes)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub struct KeyboardReport {
    /// Modifier keys (Ctrl, Shift, Alt, GUI)
    pub modifiers: u8,
    /// Reserved byte
    pub reserved: u8,
    /// Up to 6 simultaneous key presses
    pub keys: [u8; 6],
}

impl KeyboardReport {
    /// Check if a key is pressed in this report
    pub fn key_pressed(&self, scancode: u8) -> bool {
        self.keys.iter().any(|&k| k == scancode)
    }

    /// Check for rollover error (all keys = 0x01)
    pub fn is_rollover_error(&self) -> bool {
        self.keys.iter().all(|&k| k == 0x01)
    }
}
```

### Scancode Translation

```rust
/// USB HID scancode to ASCII mapping
pub fn scancode_to_char(scancode: u8, shift: bool) -> Option<char> {
    match scancode {
        // Letters A-Z (0x04-0x1D)
        0x04 => Some(if shift { 'A' } else { 'a' }),
        0x05 => Some(if shift { 'B' } else { 'b' }),
        // ... etc
        
        // Numbers 1-0 (0x1E-0x27)
        0x1E => Some(if shift { '!' } else { '1' }),
        0x1F => Some(if shift { '@' } else { '2' }),
        // ... etc
        
        // Special keys
        0x28 => Some('\n'),   // Enter
        0x29 => Some('\x1B'), // Escape
        0x2A => Some('\x08'), // Backspace
        0x2B => Some('\t'),   // Tab
        0x2C => Some(' '),    // Space
        
        // Punctuation
        0x2D => Some(if shift { '_' } else { '-' }),
        0x2E => Some(if shift { '+' } else { '=' }),
        // ... etc
        
        _ => None,
    }
}
```

### USB Keyboard Driver

```rust
/// USB HID Keyboard driver
pub struct UsbKeyboard {
    pub device_address: u8,
    pub interface: u8,
    pub endpoint_in: u8,
    pub leds: KeyboardLeds,
    previous_report: KeyboardReport,
    event_queue: VecDeque<KeyEvent>,
    char_queue: VecDeque<char>,
    attached: AtomicBool,
}

impl UsbKeyboard {
    /// Process a new keyboard report
    pub fn process_report(&mut self, report: KeyboardReport) {
        if report.is_rollover_error() {
            return;
        }

        let modifiers = report.modifiers();

        // Find released keys
        for &prev_key in &self.previous_report.keys {
            if prev_key != 0 && !report.key_pressed(prev_key) {
                self.event_queue.push_back(KeyEvent::Released(prev_key));
            }
        }

        // Find pressed keys
        for key in report.pressed_keys() {
            if !self.previous_report.key_pressed(key) {
                self.event_queue.push_back(KeyEvent::Pressed(key));

                // Translate to character
                if let Some(ch) = scancode_to_char(key, modifiers.shift()) {
                    // Handle Caps Lock
                    let ch = if self.leds.caps_lock && ch.is_ascii_alphabetic() {
                        if modifiers.shift() {
                            ch.to_ascii_lowercase()
                        } else {
                            ch.to_ascii_uppercase()
                        }
                    } else {
                        ch
                    };
                    self.char_queue.push_back(ch);
                }

                // Toggle lock keys
                match key {
                    0x39 => self.leds.caps_lock = !self.leds.caps_lock,
                    0x53 => self.leds.num_lock = !self.leds.num_lock,
                    0x47 => self.leds.scroll_lock = !self.leds.scroll_lock,
                    _ => {}
                }
            }
        }

        self.previous_report = report;
    }

    /// Get next key event
    pub fn next_event(&mut self) -> Option<KeyEvent> {
        self.event_queue.pop_front()
    }

    /// Get next character
    pub fn next_char(&mut self) -> Option<char> {
        self.char_queue.pop_front()
    }
}
```

### HID Report Descriptor Parser

```rust
/// HID Item types
#[derive(Debug, Clone, Copy)]
pub enum HidItemType {
    Main,
    Global,
    Local,
    Reserved,
}

/// HID Item
#[derive(Debug, Clone)]
pub struct HidItem {
    pub item_type: HidItemType,
    pub tag: u8,
    pub data: u32,
    pub size: u8,
}

impl HidReportDescriptor {
    /// Parse HID report descriptor
    pub fn parse(data: &[u8]) -> Self {
        let mut items = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let prefix = data[offset];
            
            // Long item (0xFE)
            if prefix == 0xFE {
                // ... handle long item
                continue;
            }

            // Short item
            let size = match prefix & 0x03 {
                0 => 0, 1 => 1, 2 => 2, 3 => 4, _ => 0,
            };

            let item_type = match (prefix >> 2) & 0x03 {
                0 => HidItemType::Main,
                1 => HidItemType::Global,
                2 => HidItemType::Local,
                _ => HidItemType::Reserved,
            };

            let tag = (prefix >> 4) & 0x0F;
            // ... parse data
            
            offset += 1 + size as usize;
        }

        Self { data: data.to_vec(), items }
    }
}
```

---

## USB Subsystem

### Global USB Subsystem

```rust
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

    /// Get connected device count
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    /// Find devices by class
    pub fn find_by_class(&self, class: ClassCode) -> Vec<&UsbDevice> {
        self.devices
            .iter()
            .filter(|d| d.device_class == class as u8)
            .collect()
    }
}
```

### Device Enumeration

```rust
impl UsbSubsystem {
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
                            if let TransferResult::Success(_) = 
                                controller.control_transfer(0, setup, None) {
                                device.state = DeviceState::Addressed;
                                
                                // Read device descriptor
                                let mut desc_buf = [0u8; 18];
                                let setup = SetupPacket::get_descriptor(1, 0, 18);
                                if let TransferResult::Success(_) = 
                                    controller.control_transfer(address, setup, Some(&mut desc_buf)) {
                                    // Parse device descriptor
                                    device.vendor_id = u16::from_le_bytes([desc_buf[8], desc_buf[9]]);
                                    device.product_id = u16::from_le_bytes([desc_buf[10], desc_buf[11]]);
                                    device.device_class = desc_buf[4];
                                    // ... etc
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
}
```

---

## Usage Examples

### Initialize USB Subsystem

```rust
use crate::usb;

pub fn init_usb() -> Result<(), &'static str> {
    // Initialize USB subsystem (probes for controllers)
    usb::init()?;
    
    // Print device tree
    usb::print_device_tree();
    
    Ok(())
}
```

### Print Device Tree

```rust
pub fn print_device_tree() {
    let subsystem = USB_SUBSYSTEM.lock();
    if let Some(ref usb) = *subsystem {
        serial_println!("USB Device Tree:");
        serial_println!("Controllers: {}", usb.controllers.len());
        serial_println!("Devices: {}", usb.device_count());
        
        for device in usb.devices() {
            serial_println!("  Device {} ({:04x}:{:04x})", 
                device.address, device.vendor_id, device.product_id);
            serial_println!("    Speed: {}", device.speed.as_str());
            serial_println!("    Class: {} (0x{:02x})", 
                device.class_name(), device.device_class);
            if let Some(ref mfr) = device.manufacturer {
                serial_println!("    Manufacturer: {}", mfr);
            }
        }
    }
}
```

### Find HID Devices

```rust
pub fn find_keyboards() -> Vec<u8> {
    let subsystem = USB_SUBSYSTEM.lock();
    if let Some(ref usb) = *subsystem {
        usb.find_by_class(ClassCode::Hid)
            .iter()
            .map(|d| d.address)
            .collect()
    } else {
        Vec::new()
    }
}
```

### Read Keyboard Input

```rust
use crate::usb::hid;

pub fn read_key() -> Option<char> {
    hid::get_char()
}

pub fn has_keyboard_input() -> bool {
    hid::has_input()
}
```

---

## Shell Commands

```text
usb list         - List all USB devices
usb tree         - Show USB device tree
usb info <addr>  - Show detailed device info
usb reset <port> - Reset a USB port
usb hid          - Show HID device status
```

---

## File Structure

```text
kernel/src/usb/
├── mod.rs           # Core types, UsbSubsystem, traits
├── descriptor.rs    # USB descriptor definitions
├── hid.rs          # HID keyboard driver
└── xhci.rs         # xHCI host controller driver
```

---

## Future Work

- [ ] EHCI driver for USB 2.0 legacy support
- [ ] USB Mass Storage driver
- [ ] USB Hub driver for cascading
- [ ] USB Audio Class driver
- [ ] USB Video Class driver
- [ ] USB networking (CDC ECM/NCM)
- [ ] USB power management
- [ ] Hot-plug detection and handling
- [ ] Isochronous transfer support
- [ ] USB debug interface

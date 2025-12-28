# PCI Subsystem Documentation

## Overview

The PCI (Peripheral Component Interconnect) subsystem provides device enumeration, configuration space access, and driver binding for PCI/PCIe devices in Splax OS.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      PCI Subsystem                          │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   Scanner   │  │  Config     │  │   Driver Binding    │  │
│  │             │  │  Space      │  │                     │  │
│  │ - Bus enum  │  │ - Read/Write│  │ - Match by class    │  │
│  │ - Device    │  │ - BAR parse │  │ - Match by vendor   │  │
│  │   discovery │  │ - Capability│  │ - Probe callbacks   │  │
│  └─────────────┘  └─────────────┘  └─────────────────────┘  │
├─────────────────────────────────────────────────────────────┤
│                    Hardware Access Layer                     │
│  ┌─────────────────────────────────────────────────────────┐│
│  │  I/O Ports (0xCF8/0xCFC) │ Memory-Mapped Config (ECAM)  ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

## Key Structures

### PciDevice

Represents a discovered PCI device:

```rust
pub struct PciDevice {
    pub bus: u8,           // Bus number (0-255)
    pub device: u8,        // Device number (0-31)
    pub function: u8,      // Function number (0-7)
    pub vendor_id: u16,    // Vendor ID (0xFFFF = no device)
    pub device_id: u16,    // Device ID
    pub class_code: u8,    // Class code
    pub subclass: u8,      // Subclass code
    pub prog_if: u8,       // Programming interface
    pub revision: u8,      // Revision ID
    pub header_type: u8,   // Header type (0, 1, or 2)
    pub bars: [Bar; 6],    // Base Address Registers
    pub interrupt_line: u8,// IRQ line
    pub interrupt_pin: u8, // Interrupt pin (A-D)
}
```

### Bar (Base Address Register)

```rust
pub enum Bar {
    None,
    Memory32 {
        address: u32,
        size: u32,
        prefetchable: bool,
    },
    Memory64 {
        address: u64,
        size: u64,
        prefetchable: bool,
    },
    Io {
        port: u16,
        size: u16,
    },
}
```

## Configuration Space Access

### Legacy I/O Method (Type 1)

Uses I/O ports 0xCF8 (CONFIG_ADDRESS) and 0xCFC (CONFIG_DATA):

```rust
pub fn config_read32(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    let address = 0x80000000
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);
    
    unsafe {
        outl(0xCF8, address);
        inl(0xCFC)
    }
}
```

### Enhanced Configuration Access Mechanism (ECAM)

For PCIe devices, uses memory-mapped configuration space:

```rust
pub fn ecam_read32(base: u64, bus: u8, device: u8, func: u8, offset: u16) -> u32 {
    let addr = base
        + ((bus as u64) << 20)
        | ((device as u64) << 15)
        | ((func as u64) << 12)
        | (offset as u64);
    
    unsafe { *(addr as *const u32) }
}
```

## Device Enumeration

### Scanning Algorithm

```rust
pub fn scan_all_buses() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    
    for bus in 0..=255 {
        for device in 0..32 {
            let vendor = config_read16(bus, device, 0, 0x00);
            if vendor == 0xFFFF {
                continue; // No device
            }
            
            let header_type = config_read8(bus, device, 0, 0x0E);
            let functions = if header_type & 0x80 != 0 { 8 } else { 1 };
            
            for func in 0..functions {
                if config_read16(bus, device, func, 0x00) != 0xFFFF {
                    devices.push(PciDevice::new(bus, device, func));
                }
            }
        }
    }
    
    devices
}
```

## PCI Class Codes

| Class | Description | Common Subclasses |
|-------|-------------|-------------------|
| 0x00 | Unclassified | VGA-compatible, Non-VGA |
| 0x01 | Mass Storage | SCSI, IDE, SATA, NVMe |
| 0x02 | Network | Ethernet, WiFi |
| 0x03 | Display | VGA, 3D Controller |
| 0x04 | Multimedia | Audio, Video |
| 0x06 | Bridge | Host, ISA, PCI-PCI |
| 0x0C | Serial Bus | USB, FireWire |

### Mass Storage Subclasses

| Subclass | Prog IF | Description |
|----------|---------|-------------|
| 0x01 | 0x80-0x85 | IDE Controller |
| 0x06 | 0x00 | SATA (Vendor Specific) |
| 0x06 | 0x01 | SATA (AHCI) |
| 0x08 | 0x02 | NVMe Controller |

## BAR Parsing

Base Address Registers indicate memory or I/O regions used by the device:

```rust
pub fn parse_bar(bus: u8, dev: u8, func: u8, bar_index: u8) -> Bar {
    let offset = 0x10 + (bar_index * 4);
    let original = config_read32(bus, dev, func, offset);
    
    if original == 0 {
        return Bar::None;
    }
    
    // Write all 1s to determine size
    config_write32(bus, dev, func, offset, 0xFFFFFFFF);
    let size_mask = config_read32(bus, dev, func, offset);
    config_write32(bus, dev, func, offset, original);
    
    if original & 1 != 0 {
        // I/O BAR
        let port = (original & 0xFFFC) as u16;
        let size = (!(size_mask & 0xFFFC) + 1) as u16;
        Bar::Io { port, size }
    } else {
        // Memory BAR
        let bar_type = (original >> 1) & 0x03;
        let prefetchable = (original & 0x08) != 0;
        
        if bar_type == 0x02 {
            // 64-bit BAR
            let high = config_read32(bus, dev, func, offset + 4);
            let address = ((high as u64) << 32) | ((original & 0xFFFFFFF0) as u64);
            // Size calculation for 64-bit...
            Bar::Memory64 { address, size: 0, prefetchable }
        } else {
            // 32-bit BAR
            let address = original & 0xFFFFFFF0;
            let size = !(size_mask & 0xFFFFFFF0) + 1;
            Bar::Memory32 { address, size, prefetchable }
        }
    }
}
```

## Capabilities

PCI capabilities are accessed via a linked list starting at offset 0x34:

### MSI (Message Signaled Interrupts)

```rust
pub struct MsiCapability {
    pub address: u64,      // Target address for interrupt
    pub data: u16,         // Data to write
    pub vectors: u8,       // Number of vectors (1, 2, 4, 8, 16, 32)
    pub per_vector: bool,  // Per-vector masking support
}

pub fn find_msi_capability(dev: &PciDevice) -> Option<MsiCapability> {
    let mut offset = config_read8(dev.bus, dev.device, dev.function, 0x34);
    
    while offset != 0 {
        let cap_id = config_read8(dev.bus, dev.device, dev.function, offset);
        if cap_id == 0x05 { // MSI capability ID
            return Some(parse_msi(dev, offset));
        }
        offset = config_read8(dev.bus, dev.device, dev.function, offset + 1);
    }
    
    None
}
```

### MSI-X

```rust
pub struct MsixCapability {
    pub table_bar: u8,     // BAR containing MSI-X table
    pub table_offset: u32, // Offset within BAR
    pub pba_bar: u8,       // BAR containing PBA
    pub pba_offset: u32,   // PBA offset
    pub table_size: u16,   // Number of entries
}
```

## Driver Registration

```rust
pub struct PciDriver {
    pub name: &'static str,
    pub class: Option<(u8, u8)>,        // (class, subclass)
    pub vendor_device: Option<(u16, u16)>,
    pub probe: fn(&PciDevice) -> Result<(), PciError>,
    pub remove: fn(&PciDevice),
}

pub fn register_driver(driver: PciDriver) {
    DRIVERS.lock().push(driver);
    
    // Probe existing devices
    for device in DEVICES.lock().iter() {
        if driver_matches(&driver, device) {
            let _ = (driver.probe)(device);
        }
    }
}
```

## Shell Commands

### lspci

Lists all PCI devices:

```
splax> lspci
00:00.0 Host bridge [0600]: Intel Corporation [8086:1237]
00:01.0 ISA bridge [0601]: Intel Corporation [8086:7000]
00:02.0 VGA controller [0300]: Red Hat Virtio [1af4:1050]
00:03.0 Ethernet controller [0200]: Intel Corporation [8086:100e]
00:04.0 SCSI storage [0100]: Red Hat Virtio [1af4:1001]
```

### lspci -v

Verbose output with BARs:

```
splax> lspci -v
00:03.0 Ethernet controller [0200]: Intel Corporation [8086:100e]
    BAR0: Memory at 0xfebc0000 (32-bit, non-prefetchable) [size=128K]
    BAR1: I/O ports at 0xc000 [size=64]
    IRQ: 11
    Capabilities: [dc] MSI
```

## Usage Examples

### Finding a Device by Class

```rust
use crate::pci;

// Find first NVMe controller
let nvme = pci::find_device_by_class(0x01, 0x08);

// Find all Ethernet controllers
let nics = pci::find_devices_by_class(0x02, 0x00);
```

### Enabling Bus Mastering

```rust
pub fn enable_bus_master(dev: &PciDevice) {
    let cmd = config_read16(dev.bus, dev.device, dev.function, 0x04);
    config_write16(dev.bus, dev.device, dev.function, 0x04, cmd | 0x04);
}
```

### Memory Mapping a BAR

```rust
pub fn map_bar(dev: &PciDevice, bar_index: usize) -> Option<*mut u8> {
    match &dev.bars[bar_index] {
        Bar::Memory32 { address, size, .. } => {
            // Map physical address to virtual
            let virt = mm::map_mmio(*address as u64, *size as u64);
            Some(virt as *mut u8)
        }
        Bar::Memory64 { address, size, .. } => {
            let virt = mm::map_mmio(*address, *size);
            Some(virt as *mut u8)
        }
        _ => None,
    }
}
```

## Error Handling

```rust
pub enum PciError {
    DeviceNotFound,
    InvalidBar,
    ConfigAccessFailed,
    DriverNotFound,
    ProbeError(String),
}
```

## Supported Devices

| Vendor | Device ID | Description | Driver |
|--------|-----------|-------------|--------|
| 0x8086 | 0x100E | Intel E1000 NIC | e1000.rs |
| 0x10EC | 0x8139 | Realtek RTL8139 | rtl8139.rs |
| 0x1AF4 | 0x1001 | Virtio Block | virtio_blk.rs |
| 0x1AF4 | 0x1000 | Virtio Net | virtio.rs |
| 0x8086 | 0x2922 | Intel ICH9 AHCI | ahci.rs |

## Future Enhancements

1. **PCIe Link Training** - Negotiate link width and speed
2. **AER (Advanced Error Reporting)** - Handle PCIe errors
3. **SR-IOV** - Single Root I/O Virtualization
4. **Hot-plug Support** - Runtime device insertion/removal
5. **Power Management** - D-states, ASPM

//! # PCI Bus Subsystem
//!
//! PCI/PCIe device enumeration and management for Splax OS.
//!
//! ## Features
//!
//! - PCI configuration space access (I/O and MMIO)
//! - Device enumeration across all buses
//! - MSI/MSI-X interrupt support
//! - Device driver binding
//! - Power management (D-states)
//! - PCIe extended configuration space

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;
use spin::RwLock;

/// PCI configuration space I/O port (address).
const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
/// PCI configuration space I/O port (data).
const PCI_CONFIG_DATA: u16 = 0xCFC;

/// PCI vendor ID for invalid device.
const PCI_VENDOR_INVALID: u16 = 0xFFFF;

/// Maximum PCI buses.
const MAX_BUS: u8 = 255;
/// Maximum devices per bus.
const MAX_DEVICE: u8 = 32;
/// Maximum functions per device.
const MAX_FUNCTION: u8 = 8;

/// PCI class codes.
pub mod class {
    pub const UNCLASSIFIED: u8 = 0x00;
    pub const MASS_STORAGE: u8 = 0x01;
    pub const NETWORK: u8 = 0x02;
    pub const DISPLAY: u8 = 0x03;
    pub const MULTIMEDIA: u8 = 0x04;
    pub const MEMORY: u8 = 0x05;
    pub const BRIDGE: u8 = 0x06;
    pub const COMMUNICATION: u8 = 0x07;
    pub const SYSTEM: u8 = 0x08;
    pub const INPUT: u8 = 0x09;
    pub const DOCKING: u8 = 0x0A;
    pub const PROCESSOR: u8 = 0x0B;
    pub const SERIAL_BUS: u8 = 0x0C;
    pub const WIRELESS: u8 = 0x0D;
}

/// PCI subclass codes for mass storage.
pub mod storage_subclass {
    pub const SCSI: u8 = 0x00;
    pub const IDE: u8 = 0x01;
    pub const FLOPPY: u8 = 0x02;
    pub const IPI: u8 = 0x03;
    pub const RAID: u8 = 0x04;
    pub const ATA: u8 = 0x05;
    pub const SATA: u8 = 0x06;
    pub const SAS: u8 = 0x07;
    pub const NVM: u8 = 0x08;
}

/// PCI subclass codes for network.
pub mod network_subclass {
    pub const ETHERNET: u8 = 0x00;
    pub const TOKEN_RING: u8 = 0x01;
    pub const FDDI: u8 = 0x02;
    pub const ATM: u8 = 0x03;
    pub const ISDN: u8 = 0x04;
    pub const OTHER: u8 = 0x80;
}

/// PCI subclass codes for bridge.
pub mod bridge_subclass {
    pub const HOST: u8 = 0x00;
    pub const ISA: u8 = 0x01;
    pub const EISA: u8 = 0x02;
    pub const MCA: u8 = 0x03;
    pub const PCI_TO_PCI: u8 = 0x04;
    pub const PCMCIA: u8 = 0x05;
    pub const NUBUS: u8 = 0x06;
    pub const CARDBUS: u8 = 0x07;
    pub const RACEWAY: u8 = 0x08;
    pub const OTHER: u8 = 0x80;
}

/// PCI subclass codes for serial bus.
pub mod serial_subclass {
    pub const FIREWIRE: u8 = 0x00;
    pub const ACCESS_BUS: u8 = 0x01;
    pub const SSA: u8 = 0x02;
    pub const USB: u8 = 0x03;
    pub const FIBRE_CHANNEL: u8 = 0x04;
    pub const SMBUS: u8 = 0x05;
}

/// PCI configuration space registers.
pub mod reg {
    pub const VENDOR_ID: u8 = 0x00;
    pub const DEVICE_ID: u8 = 0x02;
    pub const COMMAND: u8 = 0x04;
    pub const STATUS: u8 = 0x06;
    pub const REVISION_ID: u8 = 0x08;
    pub const PROG_IF: u8 = 0x09;
    pub const SUBCLASS: u8 = 0x0A;
    pub const CLASS_CODE: u8 = 0x0B;
    pub const CACHE_LINE_SIZE: u8 = 0x0C;
    pub const LATENCY_TIMER: u8 = 0x0D;
    pub const HEADER_TYPE: u8 = 0x0E;
    pub const BIST: u8 = 0x0F;
    pub const BAR0: u8 = 0x10;
    pub const BAR1: u8 = 0x14;
    pub const BAR2: u8 = 0x18;
    pub const BAR3: u8 = 0x1C;
    pub const BAR4: u8 = 0x20;
    pub const BAR5: u8 = 0x24;
    pub const CARDBUS_CIS: u8 = 0x28;
    pub const SUBSYSTEM_VENDOR_ID: u8 = 0x2C;
    pub const SUBSYSTEM_ID: u8 = 0x2E;
    pub const EXPANSION_ROM: u8 = 0x30;
    pub const CAPABILITIES_PTR: u8 = 0x34;
    pub const INTERRUPT_LINE: u8 = 0x3C;
    pub const INTERRUPT_PIN: u8 = 0x3D;
    pub const MIN_GRANT: u8 = 0x3E;
    pub const MAX_LATENCY: u8 = 0x3F;
}

/// PCI command register bits.
pub mod cmd {
    pub const IO_SPACE: u16 = 1 << 0;
    pub const MEMORY_SPACE: u16 = 1 << 1;
    pub const BUS_MASTER: u16 = 1 << 2;
    pub const SPECIAL_CYCLES: u16 = 1 << 3;
    pub const MWI_ENABLE: u16 = 1 << 4;
    pub const VGA_PALETTE_SNOOP: u16 = 1 << 5;
    pub const PARITY_ERROR_RESPONSE: u16 = 1 << 6;
    pub const SERR_ENABLE: u16 = 1 << 8;
    pub const FAST_BACK_TO_BACK: u16 = 1 << 9;
    pub const INTERRUPT_DISABLE: u16 = 1 << 10;
}

/// PCI status register bits.
pub mod status {
    pub const INTERRUPT_STATUS: u16 = 1 << 3;
    pub const CAPABILITIES_LIST: u16 = 1 << 4;
    pub const MHZ_66_CAPABLE: u16 = 1 << 5;
    pub const FAST_BACK_TO_BACK: u16 = 1 << 7;
    pub const MASTER_DATA_PARITY_ERROR: u16 = 1 << 8;
    pub const SIGNALED_TARGET_ABORT: u16 = 1 << 11;
    pub const RECEIVED_TARGET_ABORT: u16 = 1 << 12;
    pub const RECEIVED_MASTER_ABORT: u16 = 1 << 13;
    pub const SIGNALED_SYSTEM_ERROR: u16 = 1 << 14;
    pub const DETECTED_PARITY_ERROR: u16 = 1 << 15;
}

/// PCI capability IDs.
pub mod cap_id {
    pub const POWER_MANAGEMENT: u8 = 0x01;
    pub const AGP: u8 = 0x02;
    pub const VPD: u8 = 0x03;
    pub const SLOT_ID: u8 = 0x04;
    pub const MSI: u8 = 0x05;
    pub const HOT_SWAP: u8 = 0x06;
    pub const PCI_X: u8 = 0x07;
    pub const HYPERTRANSPORT: u8 = 0x08;
    pub const VENDOR_SPECIFIC: u8 = 0x09;
    pub const DEBUG_PORT: u8 = 0x0A;
    pub const COMPACT_PCI: u8 = 0x0B;
    pub const HOT_PLUG: u8 = 0x0C;
    pub const BRIDGE_SUBSYSTEM_VENDOR_ID: u8 = 0x0D;
    pub const AGP_8X: u8 = 0x0E;
    pub const SECURE_DEVICE: u8 = 0x0F;
    pub const PCI_EXPRESS: u8 = 0x10;
    pub const MSI_X: u8 = 0x11;
    pub const SATA: u8 = 0x12;
    pub const AF: u8 = 0x13;
}

/// PCI address (bus, device, function).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PciAddress {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciAddress {
    /// Creates a new PCI address.
    pub const fn new(bus: u8, device: u8, function: u8) -> Self {
        Self { bus, device, function }
    }
    
    /// Converts to configuration address format.
    fn to_config_address(&self, offset: u8) -> u32 {
        ((1u32) << 31) | // Enable bit
        ((self.bus as u32) << 16) |
        ((self.device as u32) << 11) |
        ((self.function as u32) << 8) |
        ((offset as u32) & 0xFC)
    }
}

impl core::fmt::Display for PciAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:02x}:{:02x}.{}", self.bus, self.device, self.function)
    }
}

/// Output a 32-bit value to a port.
#[cfg(target_arch = "x86_64")]
unsafe fn outl(port: u16, value: u32) {
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
    }
}

/// Input a 32-bit value from a port.
#[cfg(target_arch = "x86_64")]
unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    unsafe {
        asm!("in eax, dx", out("eax") value, in("dx") port, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Reads a 32-bit value from PCI configuration space.
pub fn config_read32(addr: PciAddress, offset: u8) -> u32 {
    let config_addr = addr.to_config_address(offset);
    unsafe {
        outl(PCI_CONFIG_ADDRESS, config_addr);
        inl(PCI_CONFIG_DATA)
    }
}

/// Writes a 32-bit value to PCI configuration space.
pub fn config_write32(addr: PciAddress, offset: u8, value: u32) {
    let config_addr = addr.to_config_address(offset);
    unsafe {
        outl(PCI_CONFIG_ADDRESS, config_addr);
        outl(PCI_CONFIG_DATA, value);
    }
}

/// Reads a 16-bit value from PCI configuration space.
pub fn config_read16(addr: PciAddress, offset: u8) -> u16 {
    let value = config_read32(addr, offset & 0xFC);
    ((value >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

/// Writes a 16-bit value to PCI configuration space.
pub fn config_write16(addr: PciAddress, offset: u8, value: u16) {
    let aligned_offset = offset & 0xFC;
    let mut dword = config_read32(addr, aligned_offset);
    let shift = (offset & 2) * 8;
    dword &= !(0xFFFF << shift);
    dword |= (value as u32) << shift;
    config_write32(addr, aligned_offset, dword);
}

/// Reads an 8-bit value from PCI configuration space.
pub fn config_read8(addr: PciAddress, offset: u8) -> u8 {
    let value = config_read32(addr, offset & 0xFC);
    ((value >> ((offset & 3) * 8)) & 0xFF) as u8
}

/// Writes an 8-bit value to PCI configuration space.
pub fn config_write8(addr: PciAddress, offset: u8, value: u8) {
    let aligned_offset = offset & 0xFC;
    let mut dword = config_read32(addr, aligned_offset);
    let shift = (offset & 3) * 8;
    dword &= !(0xFF << shift);
    dword |= (value as u32) << shift;
    config_write32(addr, aligned_offset, dword);
}

/// BAR (Base Address Register) type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarType {
    /// Memory-mapped I/O (32-bit address)
    Memory32,
    /// Memory-mapped I/O (64-bit address)
    Memory64,
    /// I/O port
    Io,
}

/// Base Address Register information.
#[derive(Debug, Clone)]
pub struct Bar {
    /// BAR index (0-5)
    pub index: u8,
    /// BAR type
    pub bar_type: BarType,
    /// Base address
    pub address: u64,
    /// Size in bytes
    pub size: u64,
    /// Is prefetchable (memory BARs only)
    pub prefetchable: bool,
}

/// PCI capability.
#[derive(Debug, Clone)]
pub struct PciCapability {
    /// Capability ID
    pub id: u8,
    /// Offset in configuration space
    pub offset: u8,
    /// Capability-specific data length
    pub length: u8,
}

/// MSI capability.
#[derive(Debug, Clone)]
pub struct MsiCapability {
    /// Offset in configuration space
    pub offset: u8,
    /// Supports 64-bit address
    pub is_64bit: bool,
    /// Supports per-vector masking
    pub per_vector_mask: bool,
    /// Maximum number of vectors (2^n)
    pub max_vectors: u8,
}

/// MSI-X capability.
#[derive(Debug, Clone)]
pub struct MsixCapability {
    /// Offset in configuration space
    pub offset: u8,
    /// Number of table entries
    pub table_size: u16,
    /// BAR containing the table
    pub table_bar: u8,
    /// Offset within BAR
    pub table_offset: u32,
    /// BAR containing the PBA
    pub pba_bar: u8,
    /// PBA offset within BAR
    pub pba_offset: u32,
}

/// A discovered PCI device.
#[derive(Debug, Clone)]
pub struct PciDevice {
    /// PCI address
    pub address: PciAddress,
    /// Vendor ID
    pub vendor_id: u16,
    /// Device ID
    pub device_id: u16,
    /// Class code
    pub class_code: u8,
    /// Subclass
    pub subclass: u8,
    /// Programming interface
    pub prog_if: u8,
    /// Revision ID
    pub revision: u8,
    /// Header type
    pub header_type: u8,
    /// Subsystem vendor ID
    pub subsystem_vendor_id: u16,
    /// Subsystem ID
    pub subsystem_id: u16,
    /// Interrupt line
    pub interrupt_line: u8,
    /// Interrupt pin
    pub interrupt_pin: u8,
    /// Base Address Registers
    pub bars: Vec<Bar>,
    /// Capabilities
    pub capabilities: Vec<PciCapability>,
    /// MSI capability
    pub msi: Option<MsiCapability>,
    /// MSI-X capability
    pub msix: Option<MsixCapability>,
}

impl PciDevice {
    /// Reads device information from PCI configuration space.
    pub fn read(addr: PciAddress) -> Option<Self> {
        let vendor_id = config_read16(addr, reg::VENDOR_ID);
        if vendor_id == PCI_VENDOR_INVALID {
            return None;
        }
        
        let device_id = config_read16(addr, reg::DEVICE_ID);
        let class_code = config_read8(addr, reg::CLASS_CODE);
        let subclass = config_read8(addr, reg::SUBCLASS);
        let prog_if = config_read8(addr, reg::PROG_IF);
        let revision = config_read8(addr, reg::REVISION_ID);
        let header_type = config_read8(addr, reg::HEADER_TYPE);
        let subsystem_vendor_id = config_read16(addr, reg::SUBSYSTEM_VENDOR_ID);
        let subsystem_id = config_read16(addr, reg::SUBSYSTEM_ID);
        let interrupt_line = config_read8(addr, reg::INTERRUPT_LINE);
        let interrupt_pin = config_read8(addr, reg::INTERRUPT_PIN);
        
        // Read BARs (only for type 0 headers)
        let bars = if (header_type & 0x7F) == 0 {
            Self::read_bars(addr)
        } else {
            Vec::new()
        };
        
        // Read capabilities
        let (capabilities, msi, msix) = Self::read_capabilities(addr);
        
        Some(Self {
            address: addr,
            vendor_id,
            device_id,
            class_code,
            subclass,
            prog_if,
            revision,
            header_type,
            subsystem_vendor_id,
            subsystem_id,
            interrupt_line,
            interrupt_pin,
            bars,
            capabilities,
            msi,
            msix,
        })
    }
    
    /// Reads all BARs.
    fn read_bars(addr: PciAddress) -> Vec<Bar> {
        let mut bars = Vec::new();
        let mut i = 0u8;
        
        while i < 6 {
            let bar_offset = reg::BAR0 + i * 4;
            let bar_value = config_read32(addr, bar_offset);
            
            if bar_value == 0 {
                i += 1;
                continue;
            }
            
            // Determine BAR type
            let is_io = (bar_value & 1) != 0;
            let is_64bit = !is_io && ((bar_value >> 1) & 3) == 2;
            let prefetchable = !is_io && ((bar_value >> 3) & 1) != 0;
            
            // Calculate size by writing all 1s and reading back
            config_write32(addr, bar_offset, 0xFFFFFFFF);
            let size_mask = config_read32(addr, bar_offset);
            config_write32(addr, bar_offset, bar_value); // Restore
            
            let (address, size) = if is_io {
                let bar_addr = (bar_value & 0xFFFFFFFC) as u64;
                let size = (!(size_mask & 0xFFFFFFFC) + 1) as u64;
                (bar_addr, size & 0xFFFF) // I/O limited to 64KB
            } else if is_64bit && i < 5 {
                let bar_high = config_read32(addr, bar_offset + 4);
                let bar_addr = ((bar_high as u64) << 32) | ((bar_value & 0xFFFFFFF0) as u64);
                
                // Get high size
                config_write32(addr, bar_offset + 4, 0xFFFFFFFF);
                let size_high = config_read32(addr, bar_offset + 4);
                config_write32(addr, bar_offset + 4, bar_high);
                
                let full_size_mask = ((size_high as u64) << 32) | ((size_mask & 0xFFFFFFF0) as u64);
                let size = if full_size_mask == 0 { 0 } else { (!full_size_mask) + 1 };
                (bar_addr, size)
            } else {
                let bar_addr = (bar_value & 0xFFFFFFF0) as u64;
                let size = if (size_mask & 0xFFFFFFF0) == 0 {
                    0
                } else {
                    (!(size_mask & 0xFFFFFFF0) + 1) as u64
                };
                (bar_addr, size)
            };
            
            if size > 0 {
                let bar_type = if is_io {
                    BarType::Io
                } else if is_64bit {
                    BarType::Memory64
                } else {
                    BarType::Memory32
                };
                
                bars.push(Bar {
                    index: i,
                    bar_type,
                    address,
                    size,
                    prefetchable,
                });
            }
            
            // 64-bit BAR uses two slots
            if is_64bit {
                i += 2;
            } else {
                i += 1;
            }
        }
        
        bars
    }
    
    /// Reads capabilities list.
    fn read_capabilities(addr: PciAddress) -> (Vec<PciCapability>, Option<MsiCapability>, Option<MsixCapability>) {
        let status = config_read16(addr, reg::STATUS);
        if (status & status::CAPABILITIES_LIST) == 0 {
            return (Vec::new(), None, None);
        }
        
        let mut caps = Vec::new();
        let mut msi = None;
        let mut msix = None;
        let mut offset = config_read8(addr, reg::CAPABILITIES_PTR) & 0xFC;
        
        let mut visited = 0u64;
        while offset != 0 && (visited & (1 << (offset / 4))) == 0 {
            visited |= 1 << (offset / 4);
            
            let cap_id = config_read8(addr, offset);
            let next_ptr = config_read8(addr, offset + 1);
            
            match cap_id {
                cap_id::MSI => {
                    let msg_ctrl = config_read16(addr, offset + 2);
                    let is_64bit = (msg_ctrl & (1 << 7)) != 0;
                    let per_vector_mask = (msg_ctrl & (1 << 8)) != 0;
                    let max_vectors = 1 << ((msg_ctrl >> 1) & 0x7);
                    
                    msi = Some(MsiCapability {
                        offset,
                        is_64bit,
                        per_vector_mask,
                        max_vectors,
                    });
                }
                cap_id::MSI_X => {
                    let msg_ctrl = config_read16(addr, offset + 2);
                    let table_size = (msg_ctrl & 0x7FF) + 1;
                    let table_bir = config_read32(addr, offset + 4);
                    let pba_bir = config_read32(addr, offset + 8);
                    
                    msix = Some(MsixCapability {
                        offset,
                        table_size,
                        table_bar: (table_bir & 0x7) as u8,
                        table_offset: table_bir & !0x7,
                        pba_bar: (pba_bir & 0x7) as u8,
                        pba_offset: pba_bir & !0x7,
                    });
                }
                _ => {}
            }
            
            caps.push(PciCapability {
                id: cap_id,
                offset,
                length: 0, // Would need per-capability parsing
            });
            
            offset = next_ptr & 0xFC;
        }
        
        (caps, msi, msix)
    }
    
    /// Enables bus mastering.
    pub fn enable_bus_master(&self) {
        let cmd = config_read16(self.address, reg::COMMAND);
        config_write16(self.address, reg::COMMAND, cmd | cmd::BUS_MASTER);
    }
    
    /// Enables memory space access.
    pub fn enable_memory(&self) {
        let cmd = config_read16(self.address, reg::COMMAND);
        config_write16(self.address, reg::COMMAND, cmd | cmd::MEMORY_SPACE);
    }
    
    /// Enables I/O space access.
    pub fn enable_io(&self) {
        let cmd = config_read16(self.address, reg::COMMAND);
        config_write16(self.address, reg::COMMAND, cmd | cmd::IO_SPACE);
    }
    
    /// Disables legacy interrupts.
    pub fn disable_interrupts(&self) {
        let cmd = config_read16(self.address, reg::COMMAND);
        config_write16(self.address, reg::COMMAND, cmd | cmd::INTERRUPT_DISABLE);
    }
    
    /// Enables MSI interrupts.
    pub fn enable_msi(&self, vector: u8, dest_cpu: u8) -> bool {
        let Some(msi) = &self.msi else {
            return false;
        };
        
        // Build MSI address and data
        let address = 0xFEE00000u32 | ((dest_cpu as u32) << 12);
        let data = vector as u16;
        
        // Write address
        config_write32(self.address, msi.offset + 4, address);
        
        // Write data (offset depends on 64-bit capability)
        let data_offset = if msi.is_64bit {
            config_write32(self.address, msi.offset + 8, 0); // High address
            msi.offset + 12
        } else {
            msi.offset + 8
        };
        config_write16(self.address, data_offset, data);
        
        // Enable MSI
        let ctrl = config_read16(self.address, msi.offset + 2);
        config_write16(self.address, msi.offset + 2, ctrl | 1);
        
        // Disable legacy interrupts
        self.disable_interrupts();
        
        true
    }
    
    /// Gets a BAR by index.
    pub fn bar(&self, index: u8) -> Option<&Bar> {
        self.bars.iter().find(|b| b.index == index)
    }
    
    /// Checks if this is a specific vendor/device.
    pub fn is_device(&self, vendor: u16, device: u16) -> bool {
        self.vendor_id == vendor && self.device_id == device
    }
    
    /// Returns a human-readable class name.
    pub fn class_name(&self) -> &'static str {
        match self.class_code {
            class::UNCLASSIFIED => "Unclassified",
            class::MASS_STORAGE => "Mass Storage",
            class::NETWORK => "Network",
            class::DISPLAY => "Display",
            class::MULTIMEDIA => "Multimedia",
            class::MEMORY => "Memory",
            class::BRIDGE => "Bridge",
            class::COMMUNICATION => "Communication",
            class::SYSTEM => "System",
            class::INPUT => "Input",
            class::DOCKING => "Docking",
            class::PROCESSOR => "Processor",
            class::SERIAL_BUS => "Serial Bus",
            class::WIRELESS => "Wireless",
            _ => "Unknown",
        }
    }
    
    // ========== BDF (Bus/Device/Function) Accessors ==========
    
    /// Returns the PCI bus number.
    pub fn bus(&self) -> u8 {
        self.address.bus
    }
    
    /// Returns the PCI slot (device) number.
    pub fn slot(&self) -> u8 {
        self.address.device
    }
    
    /// Returns the PCI function number.
    pub fn function(&self) -> u8 {
        self.address.function
    }
    
    // ========== Vendor/Device ID Accessors ==========
    
    /// Returns the vendor ID.
    pub fn vendor_id(&self) -> u16 {
        self.vendor_id
    }
    
    /// Returns the device ID.
    pub fn device_id(&self) -> u16 {
        self.device_id
    }
    
    // ========== Class Information Accessors ==========
    
    /// Returns the class code.
    pub fn class_code(&self) -> u8 {
        self.class_code
    }
    
    /// Returns the subclass code.
    pub fn subclass(&self) -> u8 {
        self.subclass
    }
    
    /// Returns the programming interface.
    pub fn prog_if(&self) -> u8 {
        self.prog_if
    }
    
    // ========== Command Register Accessors ==========
    
    /// Returns the current value of the command register.
    pub fn command(&self) -> u16 {
        config_read16(self.address, reg::COMMAND)
    }
    
    /// Sets the command register value.
    pub fn set_command(&self, value: u16) {
        config_write16(self.address, reg::COMMAND, value);
    }
}

/// PCI subsystem.
pub struct PciSubsystem {
    /// Discovered devices
    devices: RwLock<Vec<PciDevice>>,
    /// Devices by class (class, subclass) -> device indices
    by_class: RwLock<BTreeMap<(u8, u8), Vec<usize>>>,
}

impl PciSubsystem {
    /// Creates a new PCI subsystem.
    pub const fn new() -> Self {
        Self {
            devices: RwLock::new(Vec::new()),
            by_class: RwLock::new(BTreeMap::new()),
        }
    }
    
    /// Enumerates all PCI devices.
    pub fn enumerate(&self) {
        let mut devices = self.devices.write();
        let mut by_class = self.by_class.write();
        
        devices.clear();
        by_class.clear();
        
        for bus in 0..=MAX_BUS {
            for device in 0..MAX_DEVICE {
                // Check function 0
                let addr = PciAddress::new(bus, device, 0);
                if let Some(dev) = PciDevice::read(addr) {
                    let is_multifunction = (dev.header_type & 0x80) != 0;
                    let idx = devices.len();
                    
                    by_class.entry((dev.class_code, dev.subclass))
                        .or_insert_with(Vec::new)
                        .push(idx);
                    devices.push(dev);
                    
                    // Check other functions if multifunction
                    if is_multifunction {
                        for function in 1..MAX_FUNCTION {
                            let addr = PciAddress::new(bus, device, function);
                            if let Some(dev) = PciDevice::read(addr) {
                                let idx = devices.len();
                                by_class.entry((dev.class_code, dev.subclass))
                                    .or_insert_with(Vec::new)
                                    .push(idx);
                                devices.push(dev);
                            }
                        }
                    }
                }
            }
        }
    }
    
    /// Returns all discovered devices.
    pub fn devices(&self) -> Vec<PciDevice> {
        self.devices.read().clone()
    }
    
    /// Finds devices by class and subclass.
    pub fn find_by_class(&self, class: u8, subclass: u8) -> Vec<PciDevice> {
        let devices = self.devices.read();
        let by_class = self.by_class.read();
        
        if let Some(indices) = by_class.get(&(class, subclass)) {
            indices.iter().filter_map(|&i| devices.get(i).cloned()).collect()
        } else {
            Vec::new()
        }
    }
    
    /// Finds a device by vendor and device ID.
    pub fn find_device(&self, vendor: u16, device: u16) -> Option<PciDevice> {
        self.devices.read().iter().find(|d| d.is_device(vendor, device)).cloned()
    }
    
    /// Finds all devices by vendor ID.
    pub fn find_by_vendor(&self, vendor: u16) -> Vec<PciDevice> {
        self.devices.read().iter().filter(|d| d.vendor_id == vendor).cloned().collect()
    }
    
    /// Returns the number of discovered devices.
    pub fn device_count(&self) -> usize {
        self.devices.read().len()
    }
}

/// Global PCI subsystem.
static PCI: spin::Once<PciSubsystem> = spin::Once::new();

/// Gets the global PCI subsystem.
pub fn pci() -> &'static PciSubsystem {
    PCI.call_once(|| PciSubsystem::new())
}

/// Returns an iterator over all discovered PCI devices.
///
/// This is a convenience function that calls `pci().devices()`.
pub fn enumerate_devices() -> Vec<PciDevice> {
    pci().devices()
}

/// Finds a PCI device by vendor and device ID.
///
/// This is a convenience function that calls `pci().find_device()`.
pub fn find_device(vendor: u16, device: u16) -> Option<PciDevice> {
    pci().find_device(vendor, device)
}

/// Initializes the PCI subsystem.
pub fn init() {
    let pci = pci();
    pci.enumerate();
    
    let count = pci.device_count();
    crate::serial_println!("[pci] Enumerated {} device(s)", count);
    
    // Log discovered devices
    for dev in pci.devices() {
        crate::serial_println!(
            "[pci] {:04x}:{:04x} at {} - {} (class {:02x}:{:02x})",
            dev.vendor_id,
            dev.device_id,
            dev.address,
            dev.class_name(),
            dev.class_code,
            dev.subclass
        );
    }
}

/// Known vendor IDs.
pub mod vendor {
    pub const INTEL: u16 = 0x8086;
    pub const AMD: u16 = 0x1022;
    pub const NVIDIA: u16 = 0x10DE;
    pub const REALTEK: u16 = 0x10EC;
    pub const BROADCOM: u16 = 0x14E4;
    pub const QUALCOMM: u16 = 0x168C;
    pub const RED_HAT: u16 = 0x1AF4;  // VirtIO
    pub const QEMU: u16 = 0x1234;
}

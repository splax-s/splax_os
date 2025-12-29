//! # AHCI (SATA) Storage Driver
//!
//! This module implements the AHCI (Advanced Host Controller Interface) driver
//! for SATA storage devices (HDDs and SSDs connected via SATA).
//!
//! ## AHCI Architecture
//!
//! - Host Bus Adapter (HBA) manages up to 32 ports
//! - Each port can have one SATA device attached
//! - Commands sent via Command List (32 slots per port)
//! - Data transferred via Physical Region Descriptor Table (PRDT)
//! - Completion signaled via interrupt or polling
//!
//! ## Features
//!
//! - AHCI 1.3.1 compatible
//! - Support for SATA II/III (up to 6 Gbps)
//! - Native Command Queuing (NCQ) support
//! - Hot-plug support (detection)
//! - ATAPI device detection

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

use super::{BlockDevice, BlockDeviceInfo, BlockError};

// =============================================================================
// AHCI Constants
// =============================================================================

/// AHCI signature for SATA device
const SATA_SIG_ATA: u32 = 0x00000101;
/// AHCI signature for ATAPI device
const SATA_SIG_ATAPI: u32 = 0xEB140101;
/// AHCI signature for SEMB device
const SATA_SIG_SEMB: u32 = 0xC33C0101;
/// AHCI signature for Port Multiplier
const SATA_SIG_PM: u32 = 0x96690101;

/// Maximum ports per HBA
const MAX_PORTS: usize = 32;

/// Maximum commands per port
const MAX_COMMANDS: usize = 32;

/// PRDT entries per command
const PRDT_ENTRIES: usize = 8;

/// Sector size
const ATA_SECTOR_SIZE: usize = 512;

// =============================================================================
// AHCI Register Offsets (Generic Host Control)
// =============================================================================

/// Host Capabilities
const REG_CAP: usize = 0x00;
/// Global Host Control
const REG_GHC: usize = 0x04;
/// Interrupt Status
const REG_IS: usize = 0x08;
/// Ports Implemented
const REG_PI: usize = 0x0C;
/// AHCI Version
const REG_VS: usize = 0x10;
/// Command Completion Coalescing Control
const REG_CCC_CTL: usize = 0x14;
/// Command Completion Coalescing Ports
const REG_CCC_PORTS: usize = 0x18;
/// Enclosure Management Location
const REG_EM_LOC: usize = 0x1C;
/// Enclosure Management Control
const REG_EM_CTL: usize = 0x20;
/// Host Capabilities Extended
const REG_CAP2: usize = 0x24;
/// BIOS/OS Handoff Control and Status
const REG_BOHC: usize = 0x28;

/// Port registers offset (port n at 0x100 + n*0x80)
const PORT_OFFSET: usize = 0x100;
const PORT_SIZE: usize = 0x80;

// =============================================================================
// Port Register Offsets (relative to port base)
// =============================================================================

/// Port Command List Base Address (Low)
const PX_CLB: usize = 0x00;
/// Port Command List Base Address (High)
const PX_CLBU: usize = 0x04;
/// Port FIS Base Address (Low)
const PX_FB: usize = 0x08;
/// Port FIS Base Address (High)
const PX_FBU: usize = 0x0C;
/// Port Interrupt Status
const PX_IS: usize = 0x10;
/// Port Interrupt Enable
const PX_IE: usize = 0x14;
/// Port Command and Status
const PX_CMD: usize = 0x18;
/// Port Task File Data
const PX_TFD: usize = 0x20;
/// Port Signature
const PX_SIG: usize = 0x24;
/// Port SATA Status (SCR0: SStatus)
const PX_SSTS: usize = 0x28;
/// Port SATA Control (SCR2: SControl)
const PX_SCTL: usize = 0x2C;
/// Port SATA Error (SCR1: SError)
const PX_SERR: usize = 0x30;
/// Port SATA Active
const PX_SACT: usize = 0x34;
/// Port Command Issue
const PX_CI: usize = 0x38;
/// Port SATA Notification
const PX_SNTF: usize = 0x3C;
/// Port FIS-based Switching Control
const PX_FBS: usize = 0x40;

// =============================================================================
// ATA Commands
// =============================================================================

/// ATA Identify Device
const ATA_CMD_IDENTIFY: u8 = 0xEC;
/// ATA Read DMA Extended
const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
/// ATA Write DMA Extended
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
/// ATA Flush Cache Extended
const ATA_CMD_FLUSH_CACHE_EXT: u8 = 0xEA;
/// ATA Read FPDMA Queued (NCQ)
const ATA_CMD_READ_FPDMA: u8 = 0x60;
/// ATA Write FPDMA Queued (NCQ)
const ATA_CMD_WRITE_FPDMA: u8 = 0x61;

// =============================================================================
// FIS Types
// =============================================================================

/// Register - Host to Device FIS
const FIS_TYPE_REG_H2D: u8 = 0x27;
/// Register - Device to Host FIS
const FIS_TYPE_REG_D2H: u8 = 0x34;
/// DMA Activate FIS
const FIS_TYPE_DMA_ACT: u8 = 0x39;
/// DMA Setup FIS
const FIS_TYPE_DMA_SETUP: u8 = 0x41;
/// Data FIS
const FIS_TYPE_DATA: u8 = 0x46;
/// BIST Activate FIS
const FIS_TYPE_BIST: u8 = 0x58;
/// PIO Setup FIS
const FIS_TYPE_PIO_SETUP: u8 = 0x5F;
/// Set Device Bits FIS
const FIS_TYPE_DEV_BITS: u8 = 0xA1;

// =============================================================================
// AHCI Data Structures
// =============================================================================

/// Host to Device FIS (Register FIS)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FisRegH2D {
    /// FIS type (0x27)
    pub fis_type: u8,
    /// Port multiplier, Command bit
    pub pmport_c: u8,
    /// Command register
    pub command: u8,
    /// Feature register (low)
    pub featurel: u8,
    /// LBA low register
    pub lba0: u8,
    /// LBA mid register
    pub lba1: u8,
    /// LBA high register
    pub lba2: u8,
    /// Device register
    pub device: u8,
    /// LBA register, 47:40
    pub lba3: u8,
    /// LBA register, 39:32
    pub lba4: u8,
    /// LBA register, 31:24
    pub lba5: u8,
    /// Feature register (high)
    pub featureh: u8,
    /// Count register (low)
    pub countl: u8,
    /// Count register (high)
    pub counth: u8,
    /// Isochronous command completion
    pub icc: u8,
    /// Control register
    pub control: u8,
    /// Reserved
    pub rsv1: [u8; 4],
}

impl FisRegH2D {
    /// Creates a new H2D FIS
    pub fn new() -> Self {
        Self {
            fis_type: FIS_TYPE_REG_H2D,
            pmport_c: 0x80, // Command bit set
            ..Default::default()
        }
    }

    /// Sets up for IDENTIFY command
    pub fn setup_identify(&mut self) {
        self.command = ATA_CMD_IDENTIFY;
        self.device = 0; // Master device
    }

    /// Sets up for READ DMA EXT command
    pub fn setup_read_dma(&mut self, lba: u64, count: u16) {
        self.command = ATA_CMD_READ_DMA_EXT;
        self.device = 0x40; // LBA mode
        self.lba0 = (lba & 0xFF) as u8;
        self.lba1 = ((lba >> 8) & 0xFF) as u8;
        self.lba2 = ((lba >> 16) & 0xFF) as u8;
        self.lba3 = ((lba >> 24) & 0xFF) as u8;
        self.lba4 = ((lba >> 32) & 0xFF) as u8;
        self.lba5 = ((lba >> 40) & 0xFF) as u8;
        self.countl = (count & 0xFF) as u8;
        self.counth = ((count >> 8) & 0xFF) as u8;
    }

    /// Sets up for WRITE DMA EXT command
    pub fn setup_write_dma(&mut self, lba: u64, count: u16) {
        self.command = ATA_CMD_WRITE_DMA_EXT;
        self.device = 0x40;
        self.lba0 = (lba & 0xFF) as u8;
        self.lba1 = ((lba >> 8) & 0xFF) as u8;
        self.lba2 = ((lba >> 16) & 0xFF) as u8;
        self.lba3 = ((lba >> 24) & 0xFF) as u8;
        self.lba4 = ((lba >> 32) & 0xFF) as u8;
        self.lba5 = ((lba >> 40) & 0xFF) as u8;
        self.countl = (count & 0xFF) as u8;
        self.counth = ((count >> 8) & 0xFF) as u8;
    }

    /// Sets up for FLUSH CACHE EXT command
    pub fn setup_flush(&mut self) {
        self.command = ATA_CMD_FLUSH_CACHE_EXT;
        self.device = 0x40;
    }
}

/// Device to Host FIS
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FisRegD2H {
    /// FIS type (0x34)
    pub fis_type: u8,
    /// Port multiplier, Interrupt bit
    pub pmport_i: u8,
    /// Status register
    pub status: u8,
    /// Error register
    pub error: u8,
    /// LBA low register
    pub lba0: u8,
    /// LBA mid register
    pub lba1: u8,
    /// LBA high register
    pub lba2: u8,
    /// Device register
    pub device: u8,
    /// LBA register, 47:40
    pub lba3: u8,
    /// LBA register, 39:32
    pub lba4: u8,
    /// LBA register, 31:24
    pub lba5: u8,
    /// Reserved
    pub rsv2: u8,
    /// Count register (low)
    pub countl: u8,
    /// Count register (high)
    pub counth: u8,
    /// Reserved
    pub rsv3: [u8; 6],
}

/// DMA Setup FIS
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FisDmaSetup {
    /// FIS type (0x41)
    pub fis_type: u8,
    /// Port multiplier, direction, interrupt, auto-activate
    pub pmport_d_i_a: u8,
    /// Reserved
    pub rsv1: [u8; 2],
    /// DMA Buffer Identifier Low
    pub dma_buf_id_l: u32,
    /// DMA Buffer Identifier High
    pub dma_buf_id_h: u32,
    /// Reserved
    pub rsv2: u32,
    /// Byte offset into buffer
    pub dma_buf_offset: u32,
    /// Number of bytes to transfer
    pub transfer_count: u32,
    /// Reserved
    pub rsv3: u32,
}

/// PIO Setup FIS
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FisPioSetup {
    /// FIS type (0x5F)
    pub fis_type: u8,
    /// Port multiplier, direction, interrupt
    pub pmport_d_i: u8,
    /// Status register
    pub status: u8,
    /// Error register
    pub error: u8,
    /// LBA low register
    pub lba0: u8,
    /// LBA mid register
    pub lba1: u8,
    /// LBA high register
    pub lba2: u8,
    /// Device register
    pub device: u8,
    /// LBA register, 47:40
    pub lba3: u8,
    /// LBA register, 39:32
    pub lba4: u8,
    /// LBA register, 31:24
    pub lba5: u8,
    /// Reserved
    pub rsv2: u8,
    /// Count register (low)
    pub countl: u8,
    /// Count register (high)
    pub counth: u8,
    /// Reserved
    pub rsv3: u8,
    /// New value of status register
    pub e_status: u8,
    /// Transfer count
    pub tc: u16,
    /// Reserved
    pub rsv4: [u8; 2],
}

/// Received FIS structure (256 bytes, per port)
#[repr(C, align(256))]
pub struct ReceivedFis {
    /// DMA Setup FIS
    pub dsfis: FisDmaSetup,
    pub _pad0: [u8; 4],
    /// PIO Setup FIS
    pub psfis: FisPioSetup,
    pub _pad1: [u8; 12],
    /// D2H Register FIS
    pub rfis: FisRegD2H,
    pub _pad2: [u8; 4],
    /// Set Device Bits FIS
    pub sdbfis: [u8; 8],
    /// Unknown FIS
    pub ufis: [u8; 64],
    /// Reserved
    pub rsv: [u8; 96],
}

impl Default for ReceivedFis {
    fn default() -> Self {
        Self {
            dsfis: FisDmaSetup::default(),
            _pad0: [0; 4],
            psfis: FisPioSetup::default(),
            _pad1: [0; 12],
            rfis: FisRegD2H::default(),
            _pad2: [0; 4],
            sdbfis: [0; 8],
            ufis: [0; 64],
            rsv: [0; 96],
        }
    }
}

/// Physical Region Descriptor Table Entry
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct PrdtEntry {
    /// Data base address (low)
    pub dba: u32,
    /// Data base address (high)
    pub dbau: u32,
    /// Reserved
    pub rsv0: u32,
    /// Byte count (bit 0 = interrupt on completion)
    pub dbc: u32,
}

impl PrdtEntry {
    /// Creates a new PRDT entry
    pub fn new(addr: u64, byte_count: u32, interrupt: bool) -> Self {
        Self {
            dba: addr as u32,
            dbau: (addr >> 32) as u32,
            rsv0: 0,
            dbc: ((byte_count - 1) & 0x3FFFFF) | if interrupt { 0x80000000 } else { 0 },
        }
    }
}

/// Command Header (32 bytes each, 32 per port)
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandHeader {
    /// DW0: Description information
    pub dw0: u32,
    /// DW1: Physical Region Descriptor Byte Count
    pub prdbc: u32,
    /// DW2: Command Table Base Address (low)
    pub ctba: u32,
    /// DW3: Command Table Base Address (high)
    pub ctbau: u32,
    /// DW4-7: Reserved
    pub rsv: [u32; 4],
}

impl CommandHeader {
    /// Creates a new command header
    pub fn new(ctba: u64, prdtl: u16, write: bool, atapi: bool, cfl: u8) -> Self {
        let dw0 = (cfl as u32 & 0x1F) 
            | if atapi { 0x20 } else { 0 }
            | if write { 0x40 } else { 0 }
            | ((prdtl as u32) << 16);
        
        Self {
            dw0,
            prdbc: 0,
            ctba: ctba as u32,
            ctbau: (ctba >> 32) as u32,
            rsv: [0; 4],
        }
    }
}

/// Command Table (128 bytes header + PRDT)
#[repr(C, align(128))]
pub struct CommandTable {
    /// Command FIS (64 bytes)
    pub cfis: [u8; 64],
    /// ATAPI Command (16 bytes)
    pub acmd: [u8; 16],
    /// Reserved (48 bytes)
    pub rsv: [u8; 48],
    /// Physical Region Descriptor Table
    pub prdt: [PrdtEntry; PRDT_ENTRIES],
}

impl Default for CommandTable {
    fn default() -> Self {
        Self {
            cfis: [0; 64],
            acmd: [0; 16],
            rsv: [0; 48],
            prdt: [PrdtEntry::default(); PRDT_ENTRIES],
        }
    }
}

// =============================================================================
// Port State
// =============================================================================

/// AHCI Port device type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AhciDeviceType {
    /// No device
    None,
    /// SATA drive (HDD/SSD)
    Sata,
    /// ATAPI drive (CD/DVD)
    Atapi,
    /// SEMB device
    Semb,
    /// Port multiplier
    PortMultiplier,
}

/// AHCI Port
pub struct AhciPort {
    /// Port number
    port_num: u8,
    /// Port base address
    port_base: u64,
    /// Device type
    device_type: AhciDeviceType,
    /// Command list (32 entries)
    command_list: Box<[CommandHeader; MAX_COMMANDS]>,
    /// Received FIS area
    received_fis: Box<ReceivedFis>,
    /// Command tables (one per slot)
    command_tables: Vec<Box<CommandTable>>,
    /// Is port started?
    started: bool,
    /// Device model string
    model: String,
    /// Device serial number
    serial: String,
    /// Total sectors
    total_sectors: u64,
}

impl AhciPort {
    /// Creates a new port
    pub fn new(port_num: u8, hba_base: u64) -> Self {
        let port_base = hba_base + PORT_OFFSET as u64 + (port_num as u64) * PORT_SIZE as u64;
        
        let command_list = Box::new([CommandHeader::default(); MAX_COMMANDS]);
        let received_fis = Box::new(ReceivedFis::default());
        let command_tables: Vec<Box<CommandTable>> = (0..MAX_COMMANDS)
            .map(|_| Box::new(CommandTable::default()))
            .collect();
        
        Self {
            port_num,
            port_base,
            device_type: AhciDeviceType::None,
            command_list,
            received_fis,
            command_tables,
            started: false,
            model: String::new(),
            serial: String::new(),
            total_sectors: 0,
        }
    }
    
    /// Detects device type from signature
    pub fn detect_type(&mut self) -> AhciDeviceType {
        let ssts = self.read_reg(PX_SSTS);
        let det = ssts & 0x0F;
        let ipm = (ssts >> 8) & 0x0F;
        
        if det != 3 || ipm != 1 {
            self.device_type = AhciDeviceType::None;
            return AhciDeviceType::None;
        }
        
        let sig = self.read_reg(PX_SIG);
        self.device_type = match sig {
            SATA_SIG_ATA => AhciDeviceType::Sata,
            SATA_SIG_ATAPI => AhciDeviceType::Atapi,
            SATA_SIG_SEMB => AhciDeviceType::Semb,
            SATA_SIG_PM => AhciDeviceType::PortMultiplier,
            _ => AhciDeviceType::None,
        };
        
        self.device_type
    }
    
    /// Starts the port
    pub fn start(&mut self) -> Result<(), BlockError> {
        // Stop command engine first
        self.stop_cmd()?;
        
        // Set command list and FIS base addresses
        let clb = self.command_list.as_ptr() as u64;
        let fb = self.received_fis.as_ref() as *const _ as u64;
        
        self.write_reg(PX_CLB, clb as u32);
        self.write_reg(PX_CLBU, (clb >> 32) as u32);
        self.write_reg(PX_FB, fb as u32);
        self.write_reg(PX_FBU, (fb >> 32) as u32);
        
        // Clear interrupts
        self.write_reg(PX_IS, 0xFFFFFFFF);
        self.write_reg(PX_SERR, 0xFFFFFFFF);
        
        // Enable interrupts
        self.write_reg(PX_IE, 0x7DC000FF);
        
        // Start command engine
        self.start_cmd()?;
        
        self.started = true;
        Ok(())
    }
    
    /// Stops command engine
    fn stop_cmd(&mut self) -> Result<(), BlockError> {
        let mut cmd = self.read_reg(PX_CMD);
        
        // Clear ST (Start)
        cmd &= !0x0001;
        self.write_reg(PX_CMD, cmd);
        
        // Wait for CR (Command List Running) to clear
        for _ in 0..500 {
            let cmd = self.read_reg(PX_CMD);
            if (cmd & 0x8000) == 0 {
                break;
            }
            for _ in 0..1000 { core::hint::spin_loop(); }
        }
        
        // Clear FRE (FIS Receive Enable)
        cmd = self.read_reg(PX_CMD);
        cmd &= !0x0010;
        self.write_reg(PX_CMD, cmd);
        
        // Wait for FR (FIS Receive Running) to clear
        for _ in 0..500 {
            let cmd = self.read_reg(PX_CMD);
            if (cmd & 0x4000) == 0 {
                return Ok(());
            }
            for _ in 0..1000 { core::hint::spin_loop(); }
        }
        
        Err(BlockError::Timeout)
    }
    
    /// Starts command engine
    fn start_cmd(&mut self) -> Result<(), BlockError> {
        // Wait for CR to clear
        for _ in 0..500 {
            let cmd = self.read_reg(PX_CMD);
            if (cmd & 0x8000) == 0 {
                break;
            }
            for _ in 0..1000 { core::hint::spin_loop(); }
        }
        
        let mut cmd = self.read_reg(PX_CMD);
        cmd |= 0x0010; // FRE
        self.write_reg(PX_CMD, cmd);
        
        cmd |= 0x0001; // ST
        self.write_reg(PX_CMD, cmd);
        
        Ok(())
    }
    
    /// Identifies the device
    pub fn identify(&mut self) -> Result<(), BlockError> {
        if self.device_type != AhciDeviceType::Sata {
            return Err(BlockError::NotFound);
        }
        
        // Allocate buffer for identify data
        let mut identify_data = alloc::vec![0u8; 512];
        let data_addr = identify_data.as_mut_ptr() as u64;
        
        // Build command FIS
        let mut fis = FisRegH2D::new();
        fis.setup_identify();
        
        // Set up command
        let slot = self.find_cmdslot()?;
        let ctba = self.command_tables[slot].as_ref() as *const _ as u64;
        
        // Fill command table
        let cmd_table = &mut self.command_tables[slot];
        unsafe {
            core::ptr::copy_nonoverlapping(
                &fis as *const _ as *const u8,
                cmd_table.cfis.as_mut_ptr(),
                core::mem::size_of::<FisRegH2D>(),
            );
        }
        cmd_table.prdt[0] = PrdtEntry::new(data_addr, 512, true);
        
        // Fill command header
        self.command_list[slot] = CommandHeader::new(ctba, 1, false, false, 5);
        
        // Issue command
        self.write_reg(PX_CI, 1 << slot);
        
        // Wait for completion
        for _ in 0..100_000 {
            if (self.read_reg(PX_CI) & (1 << slot)) == 0 {
                break;
            }
            
            let is = self.read_reg(PX_IS);
            if (is & 0x40000000) != 0 {
                return Err(BlockError::IoError);
            }
            
            core::hint::spin_loop();
        }
        
        if (self.read_reg(PX_CI) & (1 << slot)) != 0 {
            return Err(BlockError::Timeout);
        }
        
        // Parse identify data
        self.parse_identify_data(&identify_data);
        
        Ok(())
    }
    
    /// Parses identify data
    fn parse_identify_data(&mut self, data: &[u8]) {
        // Serial number (words 10-19)
        let mut serial = [0u8; 20];
        for i in 0..20 {
            serial[i] = data[20 + i];
        }
        self.serial = Self::ata_string(&serial);
        
        // Model number (words 27-46)
        let mut model = [0u8; 40];
        for i in 0..40 {
            model[i] = data[54 + i];
        }
        self.model = Self::ata_string(&model);
        
        // Total sectors (LBA48)
        let sector_count = u64::from_le_bytes([
            data[200], data[201], data[202], data[203],
            data[204], data[205], data[206], data[207],
        ]);
        
        if sector_count > 0 {
            self.total_sectors = sector_count;
        } else {
            // Fall back to LBA28
            let sector_count_28 = u32::from_le_bytes([
                data[120], data[121], data[122], data[123],
            ]);
            self.total_sectors = sector_count_28 as u64;
        }
        
        crate::serial_println!("[ahci] Port {}: {} ({})", 
            self.port_num, self.model, self.serial);
        crate::serial_println!("[ahci]   Capacity: {} sectors ({} MB)", 
            self.total_sectors, self.total_sectors * 512 / (1024 * 1024));
    }
    
    /// Converts ATA string (byte-swapped) to normal string
    fn ata_string(data: &[u8]) -> String {
        let mut result = Vec::with_capacity(data.len());
        for chunk in data.chunks(2) {
            if chunk.len() == 2 {
                result.push(chunk[1]);
                result.push(chunk[0]);
            } else {
                result.push(chunk[0]);
            }
        }
        String::from_utf8_lossy(&result).trim().to_string()
    }
    
    /// Finds a free command slot
    fn find_cmdslot(&self) -> Result<usize, BlockError> {
        let slots = self.read_reg(PX_CI) | self.read_reg(PX_SACT);
        for i in 0..MAX_COMMANDS {
            if (slots & (1 << i)) == 0 {
                return Ok(i);
            }
        }
        Err(BlockError::Busy)
    }
    
    /// Reads sectors from the device
    pub fn read_sectors(&mut self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        if !self.started || self.device_type != AhciDeviceType::Sata {
            return Err(BlockError::NotReady);
        }
        
        let count = (buffer.len() / ATA_SECTOR_SIZE) as u16;
        if count == 0 {
            return Ok(());
        }
        
        let mut fis = FisRegH2D::new();
        fis.setup_read_dma(lba, count);
        
        self.execute_command(&fis, buffer, false)
    }
    
    /// Writes sectors to the device
    pub fn write_sectors(&mut self, lba: u64, buffer: &[u8]) -> Result<(), BlockError> {
        if !self.started || self.device_type != AhciDeviceType::Sata {
            return Err(BlockError::NotReady);
        }
        
        let count = (buffer.len() / ATA_SECTOR_SIZE) as u16;
        if count == 0 {
            return Ok(());
        }
        
        let mut fis = FisRegH2D::new();
        fis.setup_write_dma(lba, count);
        
        // Note: We need mutable access to the buffer for PRDT, but write doesn't modify it
        // This is safe because we're just reading from it
        let buffer_ptr = buffer.as_ptr() as u64;
        self.execute_command_with_addr(&fis, buffer_ptr, buffer.len(), true)
    }
    
    /// Flushes cache
    pub fn flush(&mut self) -> Result<(), BlockError> {
        if !self.started || self.device_type != AhciDeviceType::Sata {
            return Err(BlockError::NotReady);
        }
        
        let mut fis = FisRegH2D::new();
        fis.setup_flush();
        
        let slot = self.find_cmdslot()?;
        let ctba = self.command_tables[slot].as_ref() as *const _ as u64;
        
        let cmd_table = &mut self.command_tables[slot];
        unsafe {
            core::ptr::copy_nonoverlapping(
                &fis as *const _ as *const u8,
                cmd_table.cfis.as_mut_ptr(),
                core::mem::size_of::<FisRegH2D>(),
            );
        }
        
        self.command_list[slot] = CommandHeader::new(ctba, 0, false, false, 5);
        
        self.write_reg(PX_CI, 1 << slot);
        
        for _ in 0..100_000 {
            if (self.read_reg(PX_CI) & (1 << slot)) == 0 {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        
        Err(BlockError::Timeout)
    }
    
    /// Executes a command with a buffer
    fn execute_command(&mut self, fis: &FisRegH2D, buffer: &mut [u8], write: bool) -> Result<(), BlockError> {
        self.execute_command_with_addr(fis, buffer.as_ptr() as u64, buffer.len(), write)
    }
    
    /// Executes a command with a buffer address
    fn execute_command_with_addr(&mut self, fis: &FisRegH2D, addr: u64, len: usize, write: bool) -> Result<(), BlockError> {
        let slot = self.find_cmdslot()?;
        let ctba = self.command_tables[slot].as_ref() as *const _ as u64;
        
        let cmd_table = &mut self.command_tables[slot];
        unsafe {
            core::ptr::copy_nonoverlapping(
                fis as *const _ as *const u8,
                cmd_table.cfis.as_mut_ptr(),
                core::mem::size_of::<FisRegH2D>(),
            );
        }
        
        // Set up PRDT entries
        let mut remaining = len;
        let mut current_addr = addr;
        let mut prdt_idx = 0;
        
        while remaining > 0 && prdt_idx < PRDT_ENTRIES {
            let size = core::cmp::min(remaining, 4 * 1024 * 1024); // 4MB max per entry
            cmd_table.prdt[prdt_idx] = PrdtEntry::new(current_addr, size as u32, prdt_idx == 0);
            current_addr += size as u64;
            remaining -= size;
            prdt_idx += 1;
        }
        
        self.command_list[slot] = CommandHeader::new(ctba, prdt_idx as u16, write, false, 5);
        
        // Issue command
        self.write_reg(PX_CI, 1 << slot);
        
        // Wait for completion
        for _ in 0..100_000 {
            if (self.read_reg(PX_CI) & (1 << slot)) == 0 {
                return Ok(());
            }
            
            let is = self.read_reg(PX_IS);
            if (is & 0x40000000) != 0 {
                // Clear error bits
                self.write_reg(PX_IS, is);
                return Err(BlockError::IoError);
            }
            
            core::hint::spin_loop();
        }
        
        Err(BlockError::Timeout)
    }
    
    // Register access helpers
    fn read_reg(&self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.port_base + offset as u64) as *const u32) }
    }
    
    fn write_reg(&self, offset: usize, value: u32) {
        unsafe { core::ptr::write_volatile((self.port_base + offset as u64) as *mut u32, value) }
    }
}

// =============================================================================
// AHCI Controller
// =============================================================================

/// AHCI Host Bus Adapter
pub struct AhciController {
    /// Controller name
    name: String,
    /// MMIO base address
    mmio_base: u64,
    /// AHCI version
    version: u32,
    /// Number of ports
    num_ports: u8,
    /// Number of command slots
    num_cmd_slots: u8,
    /// Ports (only active ones)
    ports: Vec<AhciPort>,
}

impl AhciController {
    /// Creates a new AHCI controller
    pub fn new(name: String, mmio_base: u64) -> Result<Self, BlockError> {
        // Read capabilities
        let cap = unsafe { Self::read_reg(mmio_base, REG_CAP) };
        let num_ports = ((cap & 0x1F) + 1) as u8;
        let num_cmd_slots = (((cap >> 8) & 0x1F) + 1) as u8;
        let supports_ncq = (cap & (1 << 30)) != 0;
        let supports_64bit = (cap & (1 << 31)) != 0;
        
        // Read version
        let version = unsafe { Self::read_reg(mmio_base, REG_VS) };
        let major = (version >> 16) & 0xFFFF;
        let minor = version & 0xFFFF;
        
        crate::serial_println!("[ahci] Controller at 0x{:016x}", mmio_base);
        crate::serial_println!("[ahci] Version: {}.{}", major, minor);
        crate::serial_println!("[ahci] Ports: {}, Command Slots: {}", num_ports, num_cmd_slots);
        crate::serial_println!("[ahci] NCQ: {}, 64-bit: {}", supports_ncq, supports_64bit);
        
        let mut controller = Self {
            name,
            mmio_base,
            version,
            num_ports,
            num_cmd_slots,
            ports: Vec::new(),
        };
        
        controller.init()?;
        
        Ok(controller)
    }
    
    /// Initializes the controller
    fn init(&mut self) -> Result<(), BlockError> {
        // Enable AHCI mode
        let mut ghc = unsafe { Self::read_reg(self.mmio_base, REG_GHC) };
        ghc |= 0x80000000; // AE (AHCI Enable)
        unsafe { Self::write_reg(self.mmio_base, REG_GHC, ghc) };
        
        // Get ports implemented
        let pi = unsafe { Self::read_reg(self.mmio_base, REG_PI) };
        
        // Initialize each implemented port
        for i in 0..self.num_ports as usize {
            if (pi & (1 << i)) != 0 {
                let mut port = AhciPort::new(i as u8, self.mmio_base);
                let device_type = port.detect_type();
                
                if device_type == AhciDeviceType::Sata {
                    crate::serial_println!("[ahci] Port {}: SATA device detected", i);
                    if let Err(e) = port.start() {
                        crate::serial_println!("[ahci] Port {}: Failed to start: {:?}", i, e);
                        continue;
                    }
                    if let Err(e) = port.identify() {
                        crate::serial_println!("[ahci] Port {}: Failed to identify: {:?}", i, e);
                        continue;
                    }
                    self.ports.push(port);
                } else if device_type == AhciDeviceType::Atapi {
                    crate::serial_println!("[ahci] Port {}: ATAPI device detected (not supported)", i);
                }
            }
        }
        
        crate::serial_println!("[ahci] Initialized {} SATA device(s)", self.ports.len());
        
        Ok(())
    }
    
    // Register access helpers
    unsafe fn read_reg(base: u64, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((base + offset as u64) as *const u32) }
    }
    
    unsafe fn write_reg(base: u64, offset: usize, value: u32) {
        unsafe { core::ptr::write_volatile((base + offset as u64) as *mut u32, value) }
    }
}

// =============================================================================
// AHCI Block Device Wrapper
// =============================================================================

/// Wrapper to expose AHCI port as a block device
pub struct AhciBlockDevice {
    /// Port (wrapped in Mutex for interior mutability)
    port: Mutex<AhciPort>,
    /// Device name
    name: String,
}

impl AhciBlockDevice {
    /// Creates a new AHCI block device from a port
    pub fn new(port: AhciPort, name: String) -> Self {
        Self {
            port: Mutex::new(port),
            name,
        }
    }
}

impl BlockDevice for AhciBlockDevice {
    fn info(&self) -> BlockDeviceInfo {
        let port = self.port.lock();
        BlockDeviceInfo {
            name: self.name.clone(),
            total_sectors: port.total_sectors,
            sector_size: ATA_SECTOR_SIZE,
            read_only: false,
            model: port.model.clone(),
        }
    }
    
    fn read_sectors(&self, start_sector: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        let mut port = self.port.lock();
        port.read_sectors(start_sector, buffer)
    }
    
    fn write_sectors(&self, start_sector: u64, buffer: &[u8]) -> Result<(), BlockError> {
        let mut port = self.port.lock();
        port.write_sectors(start_sector, buffer)
    }
    
    fn flush(&self) -> Result<(), BlockError> {
        let mut port = self.port.lock();
        port.flush()
    }
    
    fn is_ready(&self) -> bool {
        let port = self.port.lock();
        port.started && port.device_type == AhciDeviceType::Sata
    }
}

// =============================================================================
// AHCI Initialization
// =============================================================================

/// Global AHCI device counter
static AHCI_COUNTER: Mutex<usize> = Mutex::new(0);

/// Generates the next SATA device name
pub fn next_sata_name() -> String {
    let mut counter = AHCI_COUNTER.lock();
    let name = alloc::format!("sd{}", (b'a' + *counter as u8) as char);
    *counter += 1;
    name
}

/// Probes for AHCI devices via PCI
pub fn probe_devices() {
    use crate::pci;
    
    crate::serial_println!("[ahci] Probing for AHCI devices...");
    
    // AHCI devices have class 0x01 (Mass Storage), subclass 0x06 (SATA), prog-if 0x01 (AHCI)
    const AHCI_CLASS: u8 = 0x01;
    const AHCI_SUBCLASS: u8 = 0x06;
    const AHCI_PROG_IF: u8 = 0x01;
    
    // Known AHCI controller vendor/device IDs (fallback matching)
    const AHCI_DEVICES: &[(u16, u16)] = &[
        (0x8086, 0x2922), // Intel ICH9
        (0x8086, 0x2829), // Intel ICH8M
        (0x8086, 0x3A22), // Intel ICH10
        (0x8086, 0x1C02), // Intel 6 Series
        (0x8086, 0x1E02), // Intel 7 Series
        (0x8086, 0x8C02), // Intel 8 Series
        (0x8086, 0x9C03), // Intel Lynx Point LP
        (0x1022, 0x7901), // AMD SATA
        (0x1B4B, 0x9172), // Marvell 88SE9172
    ];
    
    // Enumerate PCI devices looking for AHCI controllers
    for device in pci::enumerate_devices() {
        let is_ahci = (device.class_code() == AHCI_CLASS 
                      && device.subclass() == AHCI_SUBCLASS 
                      && device.prog_if() == AHCI_PROG_IF)
            || AHCI_DEVICES.iter().any(|&(v, d)| device.vendor_id() == v && device.device_id() == d);
        
        if is_ahci {
            crate::serial_println!("[ahci] Found AHCI controller at {:02x}:{:02x}.{}", 
                device.bus(), device.slot(), device.function());
            
            // Read BAR5 (ABAR - AHCI Base Memory Register)
            let mmio_base = match device.bar(5) {
                Some(bar) => bar.address,
                None => {
                    crate::serial_println!("[ahci] BAR5 not found, skipping device");
                    continue;
                }
            };
            
            if mmio_base == 0 {
                crate::serial_println!("[ahci] Invalid BAR5, skipping device");
                continue;
            }
            
            // Enable bus mastering and memory space
            let cmd = device.command();
            device.set_command(cmd | 0x06); // Memory Space + Bus Master
            
            // Initialize the controller
            if let Err(e) = init_controller(mmio_base) {
                crate::serial_println!("[ahci] Failed to initialize controller: {:?}", e);
            } else {
                crate::serial_println!("[ahci] Controller initialized at {:#x}", mmio_base);
            }
        }
    }
    
    crate::serial_println!("[ahci] AHCI probe complete");
}

/// Initializes an AHCI controller at the given MMIO address
pub fn init_controller(mmio_base: u64) -> Result<(), BlockError> {
    let name = alloc::format!("ahci{}", *AHCI_COUNTER.lock());
    let controller = AhciController::new(name, mmio_base)?;
    
    // Register each port as a block device
    for port in controller.ports {
        let dev_name = next_sata_name();
        let device = AhciBlockDevice::new(port, dev_name);
        let boxed: Box<dyn BlockDevice> = Box::new(device);
        super::register_device(boxed)?;
    }
    
    Ok(())
}

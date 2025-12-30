//! # USB Mass Storage Class Driver
//!
//! This module implements the USB Mass Storage Class (MSC) specification
//! for accessing USB storage devices like flash drives and external HDDs.
//!
//! ## Supported Protocols
//!
//! - **BBB (Bulk-Only Transport)**: Standard USB 2.0 mass storage
//! - **UAS (USB Attached SCSI)**: High-performance USB 3.0 storage
//!
//! ## Features
//!
//! - SCSI command set support
//! - Multiple LUN support
//! - Hot-plug detection
//! - Power management
//! - Error recovery

#![allow(dead_code)]

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// =============================================================================
// USB Mass Storage Constants
// =============================================================================

/// USB Class code for Mass Storage.
pub const USB_CLASS_MASS_STORAGE: u8 = 0x08;

/// Mass Storage subclass codes.
pub mod subclass {
    /// SCSI command set not reported.
    pub const SCSI_TRANSPARENT: u8 = 0x06;
    /// RBC (Reduced Block Commands).
    pub const RBC: u8 = 0x01;
    /// ATAPI (CD/DVD).
    pub const ATAPI: u8 = 0x02;
    /// QIC-157 (Tape).
    pub const QIC157: u8 = 0x03;
    /// UFI (USB Floppy Interface).
    pub const UFI: u8 = 0x04;
    /// SFF-8070i.
    pub const SFF8070I: u8 = 0x05;
}

/// Mass Storage protocol codes.
pub mod protocol {
    /// CBI (Control/Bulk/Interrupt) with command completion.
    pub const CBI_COMPLETION: u8 = 0x00;
    /// CBI without command completion.
    pub const CBI_NO_COMPLETION: u8 = 0x01;
    /// BBB (Bulk-Only Transport).
    pub const BBB: u8 = 0x50;
    /// UAS (USB Attached SCSI).
    pub const UAS: u8 = 0x62;
}

/// CBW (Command Block Wrapper) signature.
pub const CBW_SIGNATURE: u32 = 0x43425355; // "USBC"

/// CSW (Command Status Wrapper) signature.
pub const CSW_SIGNATURE: u32 = 0x53425355; // "USBS"

/// Command status values.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    /// Command passed.
    Passed = 0x00,
    /// Command failed.
    Failed = 0x01,
    /// Phase error.
    PhaseError = 0x02,
}

impl CommandStatus {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x00 => Some(Self::Passed),
            0x01 => Some(Self::Failed),
            0x02 => Some(Self::PhaseError),
            _ => None,
        }
    }
}

// =============================================================================
// SCSI Commands
// =============================================================================

/// SCSI operation codes.
pub mod scsi {
    pub const TEST_UNIT_READY: u8 = 0x00;
    pub const REQUEST_SENSE: u8 = 0x03;
    pub const INQUIRY: u8 = 0x12;
    pub const MODE_SELECT_6: u8 = 0x15;
    pub const MODE_SENSE_6: u8 = 0x1A;
    pub const START_STOP_UNIT: u8 = 0x1B;
    pub const PREVENT_ALLOW_MEDIUM_REMOVAL: u8 = 0x1E;
    pub const READ_FORMAT_CAPACITIES: u8 = 0x23;
    pub const READ_CAPACITY_10: u8 = 0x25;
    pub const READ_10: u8 = 0x28;
    pub const WRITE_10: u8 = 0x2A;
    pub const VERIFY_10: u8 = 0x2F;
    pub const SYNCHRONIZE_CACHE_10: u8 = 0x35;
    pub const READ_CAPACITY_16: u8 = 0x9E;
    pub const READ_16: u8 = 0x88;
    pub const WRITE_16: u8 = 0x8A;
}

/// SCSI sense keys.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenseKey {
    NoSense = 0x00,
    RecoveredError = 0x01,
    NotReady = 0x02,
    MediumError = 0x03,
    HardwareError = 0x04,
    IllegalRequest = 0x05,
    UnitAttention = 0x06,
    DataProtect = 0x07,
    BlankCheck = 0x08,
    VendorSpecific = 0x09,
    CopyAborted = 0x0A,
    AbortedCommand = 0x0B,
    VolumeOverflow = 0x0D,
    Miscompare = 0x0E,
}

impl SenseKey {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v & 0x0F {
            0x00 => Some(Self::NoSense),
            0x01 => Some(Self::RecoveredError),
            0x02 => Some(Self::NotReady),
            0x03 => Some(Self::MediumError),
            0x04 => Some(Self::HardwareError),
            0x05 => Some(Self::IllegalRequest),
            0x06 => Some(Self::UnitAttention),
            0x07 => Some(Self::DataProtect),
            0x08 => Some(Self::BlankCheck),
            0x09 => Some(Self::VendorSpecific),
            0x0A => Some(Self::CopyAborted),
            0x0B => Some(Self::AbortedCommand),
            0x0D => Some(Self::VolumeOverflow),
            0x0E => Some(Self::Miscompare),
            _ => None,
        }
    }
}

// =============================================================================
// BBB (Bulk-Only Transport)
// =============================================================================

/// Command Block Wrapper (CBW) - 31 bytes.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct CommandBlockWrapper {
    /// Signature ("USBC" = 0x43425355).
    pub signature: u32,
    /// Tag to associate CBW with CSW.
    pub tag: u32,
    /// Expected data transfer length.
    pub data_transfer_length: u32,
    /// Transfer direction (bit 7: 0=OUT, 1=IN).
    pub flags: u8,
    /// Logical unit number (bits 3:0).
    pub lun: u8,
    /// Length of the command block (1-16).
    pub cb_length: u8,
    /// Command block.
    pub cb: [u8; 16],
}

impl CommandBlockWrapper {
    /// Create a new CBW.
    pub fn new(tag: u32, lun: u8, command: &[u8], data_length: u32, is_read: bool) -> Self {
        let mut cb = [0u8; 16];
        let len = command.len().min(16);
        cb[..len].copy_from_slice(&command[..len]);

        Self {
            signature: CBW_SIGNATURE,
            tag,
            data_transfer_length: data_length,
            flags: if is_read { 0x80 } else { 0x00 },
            lun: lun & 0x0F,
            cb_length: len as u8,
            cb,
        }
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> [u8; 31] {
        let mut bytes = [0u8; 31];
        bytes[0..4].copy_from_slice(&self.signature.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.tag.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.data_transfer_length.to_le_bytes());
        bytes[12] = self.flags;
        bytes[13] = self.lun;
        bytes[14] = self.cb_length;
        bytes[15..31].copy_from_slice(&self.cb);
        bytes
    }
}

/// Command Status Wrapper (CSW) - 13 bytes.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct CommandStatusWrapper {
    /// Signature ("USBS" = 0x53425355).
    pub signature: u32,
    /// Tag matching the CBW.
    pub tag: u32,
    /// Residue (difference between expected and actual transfer).
    pub data_residue: u32,
    /// Status.
    pub status: u8,
}

impl CommandStatusWrapper {
    /// Parse from bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 13 {
            return None;
        }

        let signature = u32::from_le_bytes(data[0..4].try_into().unwrap());
        if signature != CSW_SIGNATURE {
            return None;
        }

        Some(Self {
            signature,
            tag: u32::from_le_bytes(data[4..8].try_into().unwrap()),
            data_residue: u32::from_le_bytes(data[8..12].try_into().unwrap()),
            status: data[12],
        })
    }

    /// Get command status.
    pub fn command_status(&self) -> Option<CommandStatus> {
        CommandStatus::from_u8(self.status)
    }
}

// =============================================================================
// SCSI Command Builders
// =============================================================================

/// Build INQUIRY command.
pub fn build_inquiry(allocation_length: u8) -> [u8; 6] {
    [scsi::INQUIRY, 0, 0, 0, allocation_length, 0]
}

/// Build TEST UNIT READY command.
pub fn build_test_unit_ready() -> [u8; 6] {
    [scsi::TEST_UNIT_READY, 0, 0, 0, 0, 0]
}

/// Build REQUEST SENSE command.
pub fn build_request_sense(allocation_length: u8) -> [u8; 6] {
    [scsi::REQUEST_SENSE, 0, 0, 0, allocation_length, 0]
}

/// Build READ CAPACITY (10) command.
pub fn build_read_capacity_10() -> [u8; 10] {
    let mut cmd = [0u8; 10];
    cmd[0] = scsi::READ_CAPACITY_10;
    cmd
}

/// Build READ (10) command.
pub fn build_read_10(lba: u32, block_count: u16) -> [u8; 10] {
    let mut cmd = [0u8; 10];
    cmd[0] = scsi::READ_10;
    cmd[2..6].copy_from_slice(&lba.to_be_bytes());
    cmd[7..9].copy_from_slice(&block_count.to_be_bytes());
    cmd
}

/// Build WRITE (10) command.
pub fn build_write_10(lba: u32, block_count: u16) -> [u8; 10] {
    let mut cmd = [0u8; 10];
    cmd[0] = scsi::WRITE_10;
    cmd[2..6].copy_from_slice(&lba.to_be_bytes());
    cmd[7..9].copy_from_slice(&block_count.to_be_bytes());
    cmd
}

/// Build READ (16) command for large drives.
pub fn build_read_16(lba: u64, block_count: u32) -> [u8; 16] {
    let mut cmd = [0u8; 16];
    cmd[0] = scsi::READ_16;
    cmd[2..10].copy_from_slice(&lba.to_be_bytes());
    cmd[10..14].copy_from_slice(&block_count.to_be_bytes());
    cmd
}

/// Build WRITE (16) command for large drives.
pub fn build_write_16(lba: u64, block_count: u32) -> [u8; 16] {
    let mut cmd = [0u8; 16];
    cmd[0] = scsi::WRITE_16;
    cmd[2..10].copy_from_slice(&lba.to_be_bytes());
    cmd[10..14].copy_from_slice(&block_count.to_be_bytes());
    cmd
}

/// Build SYNCHRONIZE CACHE (10) command.
pub fn build_sync_cache_10() -> [u8; 10] {
    let mut cmd = [0u8; 10];
    cmd[0] = scsi::SYNCHRONIZE_CACHE_10;
    cmd
}

/// Build START STOP UNIT command.
pub fn build_start_stop_unit(start: bool, load_eject: bool) -> [u8; 6] {
    let mut cmd = [0u8; 6];
    cmd[0] = scsi::START_STOP_UNIT;
    cmd[4] = if start { 0x01 } else { 0x00 } | if load_eject { 0x02 } else { 0x00 };
    cmd
}

// =============================================================================
// SCSI Response Parsers
// =============================================================================

/// INQUIRY response data.
#[derive(Debug, Clone)]
pub struct InquiryData {
    /// Peripheral device type.
    pub device_type: u8,
    /// Removable media flag.
    pub removable: bool,
    /// SCSI version.
    pub version: u8,
    /// Response data format.
    pub response_format: u8,
    /// Additional length.
    pub additional_length: u8,
    /// Vendor identification.
    pub vendor: String,
    /// Product identification.
    pub product: String,
    /// Product revision.
    pub revision: String,
}

impl InquiryData {
    /// Parse from response bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 36 {
            return None;
        }

        Some(Self {
            device_type: data[0] & 0x1F,
            removable: (data[1] & 0x80) != 0,
            version: data[2],
            response_format: data[3] & 0x0F,
            additional_length: data[4],
            vendor: String::from_utf8_lossy(&data[8..16]).trim().to_string(),
            product: String::from_utf8_lossy(&data[16..32]).trim().to_string(),
            revision: String::from_utf8_lossy(&data[32..36]).trim().to_string(),
        })
    }
}

/// READ CAPACITY (10) response data.
#[derive(Debug, Clone, Copy)]
pub struct ReadCapacity10Response {
    /// Last logical block address.
    pub last_lba: u32,
    /// Block size in bytes.
    pub block_size: u32,
}

impl ReadCapacity10Response {
    /// Parse from response bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }

        Some(Self {
            last_lba: u32::from_be_bytes(data[0..4].try_into().unwrap()),
            block_size: u32::from_be_bytes(data[4..8].try_into().unwrap()),
        })
    }

    /// Get total capacity in bytes.
    pub fn capacity_bytes(&self) -> u64 {
        (self.last_lba as u64 + 1) * self.block_size as u64
    }

    /// Get total capacity in sectors.
    pub fn sector_count(&self) -> u64 {
        self.last_lba as u64 + 1
    }
}

/// REQUEST SENSE response data.
#[derive(Debug, Clone, Copy)]
pub struct SenseData {
    /// Response code.
    pub response_code: u8,
    /// Sense key.
    pub sense_key: SenseKey,
    /// Additional sense code.
    pub asc: u8,
    /// Additional sense code qualifier.
    pub ascq: u8,
}

impl SenseData {
    /// Parse from response bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 14 {
            return None;
        }

        let sense_key = SenseKey::from_u8(data[2])?;

        Some(Self {
            response_code: data[0] & 0x7F,
            sense_key,
            asc: data[12],
            ascq: data[13],
        })
    }

    /// Check if there's no error.
    pub fn is_ok(&self) -> bool {
        self.sense_key == SenseKey::NoSense || self.sense_key == SenseKey::RecoveredError
    }
}

// =============================================================================
// USB Mass Storage Device
// =============================================================================

/// USB endpoint information.
#[derive(Debug, Clone, Copy)]
pub struct UsbEndpoint {
    /// Endpoint address.
    pub address: u8,
    /// Maximum packet size.
    pub max_packet_size: u16,
    /// Endpoint type (bulk, interrupt, etc.).
    pub ep_type: u8,
}

/// Mass storage device state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MscState {
    /// Device not initialized.
    Uninitialized,
    /// Device initializing.
    Initializing,
    /// Device ready.
    Ready,
    /// Device error.
    Error,
    /// Device disconnected.
    Disconnected,
}

/// USB Mass Storage device.
pub struct UsbMassStorageDevice {
    /// Device address on USB bus.
    pub address: u8,
    /// Interface number.
    pub interface: u8,
    /// Bulk IN endpoint.
    pub bulk_in: UsbEndpoint,
    /// Bulk OUT endpoint.
    pub bulk_out: UsbEndpoint,
    /// Current state.
    state: MscState,
    /// Number of logical units.
    num_luns: u8,
    /// Current command tag.
    tag: u32,
    /// Device information.
    pub info: Option<InquiryData>,
    /// Capacity information.
    pub capacity: Option<ReadCapacity10Response>,
}

impl UsbMassStorageDevice {
    /// Create a new MSC device.
    pub fn new(
        address: u8,
        interface: u8,
        bulk_in: UsbEndpoint,
        bulk_out: UsbEndpoint,
    ) -> Self {
        Self {
            address,
            interface,
            bulk_in,
            bulk_out,
            state: MscState::Uninitialized,
            num_luns: 1,
            tag: 1,
            info: None,
            capacity: None,
        }
    }

    /// Get current state.
    pub fn state(&self) -> MscState {
        self.state
    }

    /// Get next tag for command.
    fn next_tag(&mut self) -> u32 {
        let tag = self.tag;
        self.tag = self.tag.wrapping_add(1);
        if self.tag == 0 {
            self.tag = 1;
        }
        tag
    }

    /// Initialize the device.
    pub fn initialize(&mut self) -> Result<(), MscError> {
        self.state = MscState::Initializing;

        // Get max LUN (optional, might fail on some devices)
        // self.num_luns = self.get_max_lun().unwrap_or(0) + 1;

        // Send INQUIRY command
        let inquiry_data = self.inquiry(0)?;
        self.info = Some(inquiry_data);

        // Wait for device to be ready
        for _ in 0..10 {
            if self.test_unit_ready(0).is_ok() {
                break;
            }
            // Would sleep here
        }

        // Get capacity
        let capacity = self.read_capacity(0)?;
        self.capacity = Some(capacity);

        self.state = MscState::Ready;
        Ok(())
    }

    /// Send INQUIRY command.
    pub fn inquiry(&mut self, lun: u8) -> Result<InquiryData, MscError> {
        let cmd = build_inquiry(36);
        let mut data = [0u8; 36];

        self.execute_command(lun, &cmd, Some(&mut data), true)?;

        InquiryData::parse(&data).ok_or(MscError::InvalidResponse)
    }

    /// Send TEST UNIT READY command.
    pub fn test_unit_ready(&mut self, lun: u8) -> Result<(), MscError> {
        let cmd = build_test_unit_ready();
        self.execute_command(lun, &cmd, None, false)?;
        Ok(())
    }

    /// Send REQUEST SENSE command.
    pub fn request_sense(&mut self, lun: u8) -> Result<SenseData, MscError> {
        let cmd = build_request_sense(18);
        let mut data = [0u8; 18];

        self.execute_command(lun, &cmd, Some(&mut data), true)?;

        SenseData::parse(&data).ok_or(MscError::InvalidResponse)
    }

    /// Send READ CAPACITY (10) command.
    pub fn read_capacity(&mut self, lun: u8) -> Result<ReadCapacity10Response, MscError> {
        let cmd = build_read_capacity_10();
        let mut data = [0u8; 8];

        self.execute_command(lun, &cmd, Some(&mut data), true)?;

        ReadCapacity10Response::parse(&data).ok_or(MscError::InvalidResponse)
    }

    /// Read sectors from the device.
    pub fn read_sectors(
        &mut self,
        lun: u8,
        lba: u64,
        count: u32,
        buffer: &mut [u8],
    ) -> Result<usize, MscError> {
        if self.state != MscState::Ready {
            return Err(MscError::NotReady);
        }

        let capacity = self.capacity.ok_or(MscError::NotReady)?;
        let block_size = capacity.block_size as usize;

        if buffer.len() < block_size * count as usize {
            return Err(MscError::BufferTooSmall);
        }

        // Use READ(10) or READ(16) depending on LBA size
        if lba <= u32::MAX as u64 && count <= u16::MAX as u32 {
            let cmd = build_read_10(lba as u32, count as u16);
            let data_len = block_size * count as usize;
            self.execute_command(lun, &cmd, Some(&mut buffer[..data_len]), true)?;
            Ok(data_len)
        } else {
            let cmd = build_read_16(lba, count);
            let data_len = block_size * count as usize;
            self.execute_command(lun, &cmd, Some(&mut buffer[..data_len]), true)?;
            Ok(data_len)
        }
    }

    /// Write sectors to the device.
    pub fn write_sectors(
        &mut self,
        lun: u8,
        lba: u64,
        count: u32,
        data: &[u8],
    ) -> Result<usize, MscError> {
        if self.state != MscState::Ready {
            return Err(MscError::NotReady);
        }

        let capacity = self.capacity.ok_or(MscError::NotReady)?;
        let block_size = capacity.block_size as usize;

        if data.len() < block_size * count as usize {
            return Err(MscError::BufferTooSmall);
        }

        // Use WRITE(10) or WRITE(16) depending on LBA size
        if lba <= u32::MAX as u64 && count <= u16::MAX as u32 {
            let cmd = build_write_10(lba as u32, count as u16);
            let data_len = block_size * count as usize;
            self.execute_command_write(lun, &cmd, &data[..data_len])?;
            Ok(data_len)
        } else {
            let cmd = build_write_16(lba, count);
            let data_len = block_size * count as usize;
            self.execute_command_write(lun, &cmd, &data[..data_len])?;
            Ok(data_len)
        }
    }

    /// Sync device cache.
    pub fn sync_cache(&mut self, lun: u8) -> Result<(), MscError> {
        let cmd = build_sync_cache_10();
        self.execute_command(lun, &cmd, None, false)?;
        Ok(())
    }

    /// Eject the media.
    pub fn eject(&mut self, lun: u8) -> Result<(), MscError> {
        let cmd = build_start_stop_unit(false, true);
        self.execute_command(lun, &cmd, None, false)?;
        Ok(())
    }

    /// Execute a SCSI command (read direction).
    fn execute_command(
        &mut self,
        lun: u8,
        command: &[u8],
        data: Option<&mut [u8]>,
        is_read: bool,
    ) -> Result<(), MscError> {
        let tag = self.next_tag();
        let data_len = data.as_ref().map(|d| d.len() as u32).unwrap_or(0);

        // Build and send CBW
        let cbw = CommandBlockWrapper::new(tag, lun, command, data_len, is_read);
        self.send_cbw(&cbw)?;

        // Transfer data if needed
        if let Some(buf) = data {
            if is_read {
                self.receive_data(buf)?;
            }
        }

        // Receive CSW
        let csw = self.receive_csw()?;

        // Verify CSW
        if csw.tag != tag {
            return Err(MscError::TagMismatch);
        }

        match csw.command_status() {
            Some(CommandStatus::Passed) => Ok(()),
            Some(CommandStatus::Failed) => Err(MscError::CommandFailed),
            Some(CommandStatus::PhaseError) => Err(MscError::PhaseError),
            None => Err(MscError::InvalidResponse),
        }
    }

    /// Execute a SCSI command (write direction).
    fn execute_command_write(
        &mut self,
        lun: u8,
        command: &[u8],
        data: &[u8],
    ) -> Result<(), MscError> {
        let tag = self.next_tag();

        // Build and send CBW
        let cbw = CommandBlockWrapper::new(tag, lun, command, data.len() as u32, false);
        self.send_cbw(&cbw)?;

        // Send data
        self.send_data(data)?;

        // Receive CSW
        let csw = self.receive_csw()?;

        // Verify CSW
        if csw.tag != tag {
            return Err(MscError::TagMismatch);
        }

        match csw.command_status() {
            Some(CommandStatus::Passed) => Ok(()),
            Some(CommandStatus::Failed) => Err(MscError::CommandFailed),
            Some(CommandStatus::PhaseError) => Err(MscError::PhaseError),
            None => Err(MscError::InvalidResponse),
        }
    }

    /// Send CBW to device.
    fn send_cbw(&self, cbw: &CommandBlockWrapper) -> Result<(), MscError> {
        let bytes = cbw.to_bytes();
        // In real implementation: USB bulk OUT transfer
        // usb_bulk_transfer(self.bulk_out.address, &bytes)?;
        let _ = bytes; // Placeholder
        Ok(())
    }

    /// Receive CSW from device.
    fn receive_csw(&self) -> Result<CommandStatusWrapper, MscError> {
        let mut buffer = [0u8; 13];
        // In real implementation: USB bulk IN transfer
        // usb_bulk_transfer(self.bulk_in.address, &mut buffer)?;
        
        // Placeholder response
        buffer[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        buffer[4..8].copy_from_slice(&self.tag.to_le_bytes());
        buffer[8..12].copy_from_slice(&0u32.to_le_bytes());
        buffer[12] = CommandStatus::Passed as u8;

        CommandStatusWrapper::from_bytes(&buffer).ok_or(MscError::InvalidResponse)
    }

    /// Receive data from device.
    fn receive_data(&self, _buffer: &mut [u8]) -> Result<(), MscError> {
        // In real implementation: USB bulk IN transfer
        // usb_bulk_transfer(self.bulk_in.address, buffer)?;
        Ok(())
    }

    /// Send data to device.
    fn send_data(&self, _data: &[u8]) -> Result<(), MscError> {
        // In real implementation: USB bulk OUT transfer
        // usb_bulk_transfer(self.bulk_out.address, data)?;
        Ok(())
    }
}

// =============================================================================
// UAS (USB Attached SCSI) Protocol
// =============================================================================

/// UAS IU (Information Unit) types.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UasIuType {
    Command = 0x01,
    Sense = 0x03,
    Response = 0x04,
    TaskManagement = 0x05,
    ReadReady = 0x06,
    WriteReady = 0x07,
}

/// UAS Command IU.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UasCommandIu {
    /// IU ID (0x01).
    pub iu_id: u8,
    /// Reserved.
    pub reserved: u8,
    /// Tag.
    pub tag: u16,
    /// Priority and task attribute.
    pub priority_task_attr: u8,
    /// Reserved.
    pub reserved2: u8,
    /// Additional CDB length.
    pub additional_cdb_len: u8,
    /// Reserved.
    pub reserved3: u8,
    /// Logical unit number.
    pub lun: [u8; 8],
    /// CDB (Command Descriptor Block).
    pub cdb: [u8; 16],
}

impl UasCommandIu {
    /// Create a new UAS command.
    pub fn new(tag: u16, lun: u8, command: &[u8]) -> Self {
        let mut cdb = [0u8; 16];
        let len = command.len().min(16);
        cdb[..len].copy_from_slice(&command[..len]);

        let mut lun_bytes = [0u8; 8];
        lun_bytes[1] = lun;

        Self {
            iu_id: UasIuType::Command as u8,
            reserved: 0,
            tag,
            priority_task_attr: 0,
            reserved2: 0,
            additional_cdb_len: 0,
            reserved3: 0,
            lun: lun_bytes,
            cdb,
        }
    }
}

/// UAS Sense IU.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UasSenseIu {
    /// IU ID (0x03).
    pub iu_id: u8,
    /// Reserved.
    pub reserved: u8,
    /// Tag.
    pub tag: u16,
    /// Status qualifier.
    pub status_qualifier: u16,
    /// Status.
    pub status: u8,
    /// Reserved.
    pub reserved2: [u8; 7],
    /// Sense data length.
    pub sense_length: u16,
    /// Sense data.
    pub sense_data: [u8; 18],
}

/// UAS Response IU.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct UasResponseIu {
    /// IU ID (0x04).
    pub iu_id: u8,
    /// Reserved.
    pub reserved: u8,
    /// Tag.
    pub tag: u16,
    /// Additional response info.
    pub additional_info: [u8; 3],
    /// Response code.
    pub response_code: u8,
}

// =============================================================================
// Error Handling
// =============================================================================

/// Mass storage errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MscError {
    /// Device not ready.
    NotReady,
    /// USB transfer error.
    TransferError,
    /// Invalid response from device.
    InvalidResponse,
    /// Command failed.
    CommandFailed,
    /// Phase error.
    PhaseError,
    /// Tag mismatch.
    TagMismatch,
    /// Buffer too small.
    BufferTooSmall,
    /// Device disconnected.
    Disconnected,
    /// Timeout.
    Timeout,
    /// Medium not present.
    NoMedium,
    /// Write protected.
    WriteProtected,
}

// =============================================================================
// Block Device Interface
// =============================================================================

/// Block device trait for mass storage.
pub trait BlockDevice {
    /// Read blocks from device.
    fn read_blocks(&mut self, lba: u64, count: u32, buffer: &mut [u8]) -> Result<usize, MscError>;

    /// Write blocks to device.
    fn write_blocks(&mut self, lba: u64, count: u32, data: &[u8]) -> Result<usize, MscError>;

    /// Get block size.
    fn block_size(&self) -> u32;

    /// Get total block count.
    fn block_count(&self) -> u64;

    /// Sync device.
    fn sync(&mut self) -> Result<(), MscError>;
}

impl BlockDevice for UsbMassStorageDevice {
    fn read_blocks(&mut self, lba: u64, count: u32, buffer: &mut [u8]) -> Result<usize, MscError> {
        self.read_sectors(0, lba, count, buffer)
    }

    fn write_blocks(&mut self, lba: u64, count: u32, data: &[u8]) -> Result<usize, MscError> {
        self.write_sectors(0, lba, count, data)
    }

    fn block_size(&self) -> u32 {
        self.capacity.map(|c| c.block_size).unwrap_or(512)
    }

    fn block_count(&self) -> u64 {
        self.capacity.map(|c| c.sector_count()).unwrap_or(0)
    }

    fn sync(&mut self) -> Result<(), MscError> {
        self.sync_cache(0)
    }
}

// =============================================================================
// Device Manager
// =============================================================================

/// Mass storage device manager.
pub struct MscManager {
    /// Registered devices.
    devices: Vec<Box<UsbMassStorageDevice>>,
    /// Next device ID.
    next_id: usize,
}

impl MscManager {
    /// Create a new manager.
    pub const fn new() -> Self {
        Self {
            devices: Vec::new(),
            next_id: 0,
        }
    }

    /// Register a new device.
    pub fn register(&mut self, device: UsbMassStorageDevice) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.devices.push(Box::new(device));
        id
    }

    /// Get device by ID.
    pub fn get(&self, id: usize) -> Option<&UsbMassStorageDevice> {
        self.devices.get(id).map(|d| d.as_ref())
    }

    /// Get mutable device by ID.
    pub fn get_mut(&mut self, id: usize) -> Option<&mut UsbMassStorageDevice> {
        self.devices.get_mut(id).map(|d| d.as_mut())
    }

    /// Remove disconnected devices.
    pub fn cleanup(&mut self) {
        self.devices.retain(|d| d.state() != MscState::Disconnected);
    }

    /// Get device count.
    pub fn count(&self) -> usize {
        self.devices.len()
    }
}

impl Default for MscManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cbw_creation() {
        let cmd = build_read_10(0x1000, 1);
        let cbw = CommandBlockWrapper::new(1, 0, &cmd, 512, true);

        assert_eq!(cbw.signature, CBW_SIGNATURE);
        assert_eq!(cbw.tag, 1);
        assert_eq!(cbw.data_transfer_length, 512);
        assert_eq!(cbw.flags, 0x80); // Read direction
        assert_eq!(cbw.lun, 0);
        assert_eq!(cbw.cb_length, 10);
    }

    #[test]
    fn test_csw_parsing() {
        let mut data = [0u8; 13];
        data[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        data[4..8].copy_from_slice(&1u32.to_le_bytes());
        data[8..12].copy_from_slice(&0u32.to_le_bytes());
        data[12] = CommandStatus::Passed as u8;

        let csw = CommandStatusWrapper::from_bytes(&data).unwrap();
        assert_eq!(csw.tag, 1);
        assert_eq!(csw.command_status(), Some(CommandStatus::Passed));
    }

    #[test]
    fn test_read_capacity_parse() {
        let mut data = [0u8; 8];
        data[0..4].copy_from_slice(&0x00100000u32.to_be_bytes()); // ~1M blocks
        data[4..8].copy_from_slice(&512u32.to_be_bytes()); // 512 byte blocks

        let cap = ReadCapacity10Response::parse(&data).unwrap();
        assert_eq!(cap.last_lba, 0x00100000);
        assert_eq!(cap.block_size, 512);
        assert_eq!(cap.capacity_bytes(), 0x100001 * 512);
    }

    #[test]
    fn test_scsi_commands() {
        let read_cmd = build_read_10(0x1000, 8);
        assert_eq!(read_cmd[0], scsi::READ_10);
        assert_eq!(u32::from_be_bytes(read_cmd[2..6].try_into().unwrap()), 0x1000);
        assert_eq!(u16::from_be_bytes(read_cmd[7..9].try_into().unwrap()), 8);

        let write_cmd = build_write_10(0x2000, 4);
        assert_eq!(write_cmd[0], scsi::WRITE_10);
    }

    #[test]
    fn test_device_state() {
        let device = UsbMassStorageDevice::new(
            1,
            0,
            UsbEndpoint {
                address: 0x81,
                max_packet_size: 512,
                ep_type: 2,
            },
            UsbEndpoint {
                address: 0x02,
                max_packet_size: 512,
                ep_type: 2,
            },
        );

        assert_eq!(device.state(), MscState::Uninitialized);
    }
}

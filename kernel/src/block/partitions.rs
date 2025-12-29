//! # Partition Table Support
//!
//! Support for reading and managing disk partition tables.
//!
//! ## Supported Formats
//!
//! - **MBR**: Master Boot Record (legacy, up to 2TB)
//! - **GPT**: GUID Partition Table (modern, up to 9.4ZB)
//!
//! ## Design
//!
//! Partitions are exposed as virtual block devices that transparently
//! remap sector addresses to the parent device.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use super::{BlockDevice, BlockDeviceInfo, BlockError, SECTOR_SIZE};

/// Partition type GUIDs (GPT)
pub mod gpt_types {
    /// Unused entry
    pub const UNUSED: u128 = 0;
    /// EFI System Partition
    pub const EFI_SYSTEM: u128 = 0xC12A7328_F81F_11D2_BA4B_00A0C93EC93B;
    /// Microsoft Basic Data
    pub const MICROSOFT_BASIC_DATA: u128 = 0xEBD0A0A2_B9E5_4433_87C0_68B6B72699C7;
    /// Linux Filesystem
    pub const LINUX_FILESYSTEM: u128 = 0x0FC63DAF_8483_4772_8E79_3D69D8477DE4;
    /// Linux Swap
    pub const LINUX_SWAP: u128 = 0x0657FD6D_A4AB_43C4_84E5_0933C84B4F4F;
    /// Linux LVM
    pub const LINUX_LVM: u128 = 0xE6D6D379_F507_44C2_A23C_238F2A3DF928;
    /// Linux Root (x86-64)
    pub const LINUX_ROOT_X86_64: u128 = 0x4F68BCE3_E8CD_4DB1_96E7_FBCAF984B709;
    /// Linux Home
    pub const LINUX_HOME: u128 = 0x933AC7E1_2EB4_4F13_B844_0E14E2AEF915;
}

/// Partition type IDs (MBR)
pub mod mbr_types {
    /// Empty
    pub const EMPTY: u8 = 0x00;
    /// FAT12
    pub const FAT12: u8 = 0x01;
    /// FAT16 < 32MB
    pub const FAT16_SMALL: u8 = 0x04;
    /// Extended partition
    pub const EXTENDED: u8 = 0x05;
    /// FAT16 >= 32MB
    pub const FAT16: u8 = 0x06;
    /// NTFS/exFAT
    pub const NTFS: u8 = 0x07;
    /// FAT32
    pub const FAT32: u8 = 0x0B;
    /// FAT32 (LBA)
    pub const FAT32_LBA: u8 = 0x0C;
    /// FAT16 (LBA)
    pub const FAT16_LBA: u8 = 0x0E;
    /// Extended (LBA)
    pub const EXTENDED_LBA: u8 = 0x0F;
    /// Linux Swap
    pub const LINUX_SWAP: u8 = 0x82;
    /// Linux Native
    pub const LINUX: u8 = 0x83;
    /// Linux LVM
    pub const LINUX_LVM: u8 = 0x8E;
    /// EFI System Partition
    pub const EFI_SYSTEM: u8 = 0xEF;
    /// GPT Protective MBR
    pub const GPT_PROTECTIVE: u8 = 0xEE;
}

/// Information about a partition
#[derive(Debug, Clone)]
pub struct PartitionInfo {
    /// Partition number (1-based)
    pub number: u32,
    /// Starting sector (LBA)
    pub start_sector: u64,
    /// Size in sectors
    pub sector_count: u64,
    /// Partition type (for MBR)
    pub mbr_type: u8,
    /// Partition type GUID (for GPT)
    pub gpt_type: u128,
    /// Unique partition GUID (for GPT)
    pub unique_guid: u128,
    /// Partition name (for GPT)
    pub name: String,
    /// Whether this is a bootable partition
    pub bootable: bool,
    /// Partition flags
    pub flags: u64,
}

impl PartitionInfo {
    /// Returns partition size in bytes
    pub fn size_bytes(&self) -> u64 {
        self.sector_count * SECTOR_SIZE as u64
    }

    /// Returns partition type as string
    pub fn type_name(&self) -> &'static str {
        if self.gpt_type != 0 {
            match self.gpt_type {
                gpt_types::EFI_SYSTEM => "EFI System",
                gpt_types::LINUX_FILESYSTEM => "Linux Filesystem",
                gpt_types::LINUX_SWAP => "Linux Swap",
                gpt_types::LINUX_ROOT_X86_64 => "Linux Root (x86-64)",
                gpt_types::LINUX_HOME => "Linux Home",
                gpt_types::MICROSOFT_BASIC_DATA => "Microsoft Basic Data",
                _ => "Unknown GPT",
            }
        } else {
            match self.mbr_type {
                mbr_types::FAT12 => "FAT12",
                mbr_types::FAT16_SMALL | mbr_types::FAT16 | mbr_types::FAT16_LBA => "FAT16",
                mbr_types::FAT32 | mbr_types::FAT32_LBA => "FAT32",
                mbr_types::NTFS => "NTFS",
                mbr_types::LINUX => "Linux",
                mbr_types::LINUX_SWAP => "Linux Swap",
                mbr_types::LINUX_LVM => "Linux LVM",
                mbr_types::EFI_SYSTEM => "EFI System",
                mbr_types::EXTENDED | mbr_types::EXTENDED_LBA => "Extended",
                _ => "Unknown",
            }
        }
    }
}

/// Partition table type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionTableType {
    /// No partition table found
    None,
    /// Master Boot Record
    Mbr,
    /// GUID Partition Table
    Gpt,
}

/// Result of parsing a partition table
#[derive(Debug)]
pub struct PartitionTable {
    /// Table type
    pub table_type: PartitionTableType,
    /// List of partitions
    pub partitions: Vec<PartitionInfo>,
    /// Disk GUID (for GPT)
    pub disk_guid: u128,
}

// ============================================================================
// MBR Parsing
// ============================================================================

/// MBR partition entry (16 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct MbrPartitionEntry {
    boot_indicator: u8,
    start_head: u8,
    start_sector_cylinder: u16,
    partition_type: u8,
    end_head: u8,
    end_sector_cylinder: u16,
    start_lba: u32,
    size_sectors: u32,
}

/// Parses an MBR partition table
fn parse_mbr(sector: &[u8]) -> Option<Vec<PartitionInfo>> {
    if sector.len() < 512 {
        return None;
    }

    // Check MBR signature (0x55AA at offset 510)
    if sector[510] != 0x55 || sector[511] != 0xAA {
        return None;
    }

    let mut partitions = Vec::new();

    // Parse 4 partition entries (starting at offset 446)
    for i in 0..4 {
        let offset = 446 + i * 16;
        let entry = unsafe {
            core::ptr::read_unaligned(sector.as_ptr().add(offset) as *const MbrPartitionEntry)
        };

        if entry.partition_type != mbr_types::EMPTY && entry.size_sectors > 0 {
            partitions.push(PartitionInfo {
                number: (i + 1) as u32,
                start_sector: entry.start_lba as u64,
                sector_count: entry.size_sectors as u64,
                mbr_type: entry.partition_type,
                gpt_type: 0,
                unique_guid: 0,
                name: String::new(),
                bootable: entry.boot_indicator == 0x80,
                flags: 0,
            });
        }
    }

    Some(partitions)
}

// ============================================================================
// GPT Parsing
// ============================================================================

/// GPT header (92 bytes minimum)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct GptHeader {
    signature: [u8; 8],
    revision: u32,
    header_size: u32,
    header_crc32: u32,
    reserved: u32,
    current_lba: u64,
    backup_lba: u64,
    first_usable_lba: u64,
    last_usable_lba: u64,
    disk_guid: [u8; 16],
    partition_table_lba: u64,
    num_partition_entries: u32,
    partition_entry_size: u32,
    partition_table_crc32: u32,
}

/// GPT partition entry (128 bytes)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct GptPartitionEntry {
    type_guid: [u8; 16],
    unique_guid: [u8; 16],
    start_lba: u64,
    end_lba: u64,
    attributes: u64,
    name: [u16; 36],
}

/// Converts bytes to u128 (GUID)
fn bytes_to_guid(bytes: &[u8; 16]) -> u128 {
    u128::from_le_bytes(*bytes)
}

/// Parses a GPT partition table
fn parse_gpt(device: &dyn BlockDevice) -> Option<(Vec<PartitionInfo>, u128)> {
    // Read GPT header (LBA 1)
    let mut header_sector = [0u8; 512];
    device.read_sectors(1, &mut header_sector).ok()?;

    let header = unsafe {
        core::ptr::read_unaligned(header_sector.as_ptr() as *const GptHeader)
    };

    // Verify signature "EFI PART"
    if &header.signature != b"EFI PART" {
        return None;
    }

    let disk_guid = bytes_to_guid(&header.disk_guid);
    let mut partitions = Vec::new();

    // Read partition entries
    let entries_per_sector = 512 / header.partition_entry_size as usize;
    let sectors_needed = (header.num_partition_entries as usize + entries_per_sector - 1) / entries_per_sector;

    let mut entry_buffer = alloc::vec![0u8; sectors_needed * 512];
    device.read_sectors(header.partition_table_lba, &mut entry_buffer).ok()?;

    for i in 0..header.num_partition_entries as usize {
        let offset = i * header.partition_entry_size as usize;
        if offset + 128 > entry_buffer.len() {
            break;
        }

        let entry = unsafe {
            core::ptr::read_unaligned(entry_buffer.as_ptr().add(offset) as *const GptPartitionEntry)
        };

        let type_guid = bytes_to_guid(&entry.type_guid);
        
        // Skip empty entries
        if type_guid == gpt_types::UNUSED {
            continue;
        }

        // Parse name (UTF-16LE null-terminated)
        // Use raw pointer arithmetic to avoid creating references to packed struct fields
        let name: String = unsafe {
            let mut chars = Vec::new();
            let name_ptr = core::ptr::addr_of!(entry.name) as *const u16;
            for i in 0..36 {
                let c = core::ptr::read_unaligned(name_ptr.add(i));
                if c == 0 {
                    break;
                }
                chars.push(char::from_u32(c as u32).unwrap_or('?'));
            }
            chars.into_iter().collect()
        };

        partitions.push(PartitionInfo {
            number: (i + 1) as u32,
            start_sector: entry.start_lba,
            sector_count: entry.end_lba - entry.start_lba + 1,
            mbr_type: 0,
            gpt_type: type_guid,
            unique_guid: bytes_to_guid(&entry.unique_guid),
            name,
            bootable: (entry.attributes & 0x04) != 0,
            flags: entry.attributes,
        });
    }

    Some((partitions, disk_guid))
}

// ============================================================================
// Partition Discovery
// ============================================================================

/// Reads and parses the partition table from a device
pub fn read_partition_table(device: &dyn BlockDevice) -> Result<PartitionTable, BlockError> {
    // Read first sector (MBR or protective MBR)
    let mut sector0 = [0u8; 512];
    device.read_sectors(0, &mut sector0)?;

    // Try GPT first (check for protective MBR)
    if let Some(mbr_parts) = parse_mbr(&sector0) {
        // Check if this is a protective MBR
        if mbr_parts.len() == 1 && mbr_parts[0].mbr_type == mbr_types::GPT_PROTECTIVE {
            // This is GPT
            if let Some((partitions, disk_guid)) = parse_gpt(device) {
                return Ok(PartitionTable {
                    table_type: PartitionTableType::Gpt,
                    partitions,
                    disk_guid,
                });
            }
        }

        // Regular MBR
        return Ok(PartitionTable {
            table_type: PartitionTableType::Mbr,
            partitions: mbr_parts,
            disk_guid: 0,
        });
    }

    // No partition table
    Ok(PartitionTable {
        table_type: PartitionTableType::None,
        partitions: Vec::new(),
        disk_guid: 0,
    })
}

// ============================================================================
// Partition Block Device
// ============================================================================

/// A partition exposed as a block device
pub struct PartitionDevice {
    /// Parent device name
    parent: &'static str,
    /// Partition info
    info: PartitionInfo,
    /// Device info
    device_info: BlockDeviceInfo,
}

impl PartitionDevice {
    /// Creates a new partition device
    pub fn new(parent: &'static str, info: PartitionInfo, parent_sector_size: usize) -> Self {
        let device_info = BlockDeviceInfo {
            name: alloc::format!("{}p{}", parent, info.number),
            total_sectors: info.sector_count,
            sector_size: parent_sector_size,
            read_only: false,
            model: alloc::format!("{} Partition {}", info.type_name(), info.number),
        };

        Self {
            parent,
            info,
            device_info,
        }
    }
}

impl BlockDevice for PartitionDevice {
    fn info(&self) -> BlockDeviceInfo {
        self.device_info.clone()
    }

    fn read_sectors(&self, start_sector: u64, buffer: &mut [u8]) -> Result<(), BlockError> {
        // Check bounds
        let sector_count = (buffer.len() / SECTOR_SIZE) as u64;
        if start_sector + sector_count > self.info.sector_count {
            return Err(BlockError::InvalidSector);
        }

        // Translate to parent sector
        let parent_sector = self.info.start_sector + start_sector;
        
        // Read from parent device
        super::with_device(self.parent, |dev| {
            dev.read_sectors(parent_sector, buffer)
        })?
    }

    fn write_sectors(&self, start_sector: u64, buffer: &[u8]) -> Result<(), BlockError> {
        let sector_count = (buffer.len() / SECTOR_SIZE) as u64;
        if start_sector + sector_count > self.info.sector_count {
            return Err(BlockError::InvalidSector);
        }

        let parent_sector = self.info.start_sector + start_sector;
        
        super::with_device(self.parent, |dev| {
            dev.write_sectors(parent_sector, buffer)
        })?
    }

    fn flush(&self) -> Result<(), BlockError> {
        super::with_device(self.parent, |dev| {
            dev.flush()
        })?
    }

    fn is_ready(&self) -> bool {
        super::with_device(self.parent, |dev| dev.is_ready()).unwrap_or(false)
    }
}

/// Probes a device for partitions and registers partition devices
pub fn probe_partitions(device_name: &'static str) -> Result<Vec<String>, BlockError> {
    let table = super::with_device(device_name, |dev| {
        read_partition_table(dev)
    })??;

    let mut registered = Vec::new();

    for partition in table.partitions {
        let sector_size = super::with_device(device_name, |dev| {
            dev.info().sector_size
        })?;

        let part_dev = PartitionDevice::new(device_name, partition, sector_size);
        let name = part_dev.info().name.clone();
        
        // Convert to static lifetime for registration
        // In real implementation, this would use a proper lifetime management
        // For now, we just log the partition
        crate::serial_println!("[PART] Found partition: {} ({})", name, part_dev.info().model);
        registered.push(name);
    }

    Ok(registered)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mbr_signature() {
        let mut sector = [0u8; 512];
        sector[510] = 0x55;
        sector[511] = 0xAA;
        
        let result = parse_mbr(&sector);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 0); // No partitions
    }

    #[test]
    fn test_partition_info() {
        let info = PartitionInfo {
            number: 1,
            start_sector: 2048,
            sector_count: 1048576,
            mbr_type: mbr_types::LINUX,
            gpt_type: 0,
            unique_guid: 0,
            name: String::new(),
            bootable: true,
            flags: 0,
        };

        assert_eq!(info.size_bytes(), 1048576 * 512);
        assert_eq!(info.type_name(), "Linux");
    }
}

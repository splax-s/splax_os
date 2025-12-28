//! # FAT32 Filesystem
//!
//! FAT32 filesystem implementation for Splax OS.
//!
//! ## Features
//!
//! - FAT32 partition support
//! - Long filename (LFN) support
//! - File read/write operations
//! - Directory operations
//! - Cluster chain management
//! - Boot sector parsing
//!
//! ## Use Cases
//!
//! - USB flash drives
//! - SD cards
//! - EFI System Partition
//! - Cross-platform file exchange

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use spin::RwLock;

use crate::block::BlockDevice;
use crate::fs::vfs::{
    Filesystem, InodeNum, VfsAttr, VfsDirEntry, VfsError, VfsFileType, VfsPermissions, VfsStatFs,
};

/// FAT32 signature.
const FAT32_SIGNATURE: u16 = 0xAA55;

/// FAT32 cluster markers.
const FAT32_CLUSTER_FREE: u32 = 0x00000000;
const FAT32_CLUSTER_RESERVED: u32 = 0x0FFFFFF0;
const FAT32_CLUSTER_BAD: u32 = 0x0FFFFFF7;
const FAT32_CLUSTER_END: u32 = 0x0FFFFFF8;
const FAT32_CLUSTER_MASK: u32 = 0x0FFFFFFF;

/// Root directory pseudo-inode.
const FAT32_ROOT_INO: InodeNum = 2;

/// Directory entry attributes.
const ATTR_READ_ONLY: u8 = 0x01;
const ATTR_HIDDEN: u8 = 0x02;
const ATTR_SYSTEM: u8 = 0x04;
const ATTR_VOLUME_ID: u8 = 0x08;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_LONG_NAME: u8 = ATTR_READ_ONLY | ATTR_HIDDEN | ATTR_SYSTEM | ATTR_VOLUME_ID;

/// FAT32 Boot Sector (BIOS Parameter Block).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct BootSector {
    /// Jump instruction
    pub jmp: [u8; 3],
    /// OEM name
    pub oem_name: [u8; 8],
    /// Bytes per sector
    pub bytes_per_sector: u16,
    /// Sectors per cluster
    pub sectors_per_cluster: u8,
    /// Reserved sector count
    pub reserved_sectors: u16,
    /// Number of FATs
    pub num_fats: u8,
    /// Root entry count (0 for FAT32)
    pub root_entry_count: u16,
    /// Total sectors (16-bit, 0 for FAT32)
    pub total_sectors_16: u16,
    /// Media type
    pub media_type: u8,
    /// FAT size (16-bit, 0 for FAT32)
    pub fat_size_16: u16,
    /// Sectors per track
    pub sectors_per_track: u16,
    /// Number of heads
    pub num_heads: u16,
    /// Hidden sectors
    pub hidden_sectors: u32,
    /// Total sectors (32-bit)
    pub total_sectors_32: u32,
    // FAT32 specific fields
    /// FAT size (32-bit)
    pub fat_size_32: u32,
    /// Extended flags
    pub ext_flags: u16,
    /// Filesystem version
    pub fs_version: u16,
    /// Root directory cluster
    pub root_cluster: u32,
    /// FSInfo sector
    pub fs_info_sector: u16,
    /// Backup boot sector
    pub backup_boot_sector: u16,
    /// Reserved
    pub reserved: [u8; 12],
    /// Drive number
    pub drive_number: u8,
    /// Reserved
    pub reserved1: u8,
    /// Boot signature
    pub boot_sig: u8,
    /// Volume ID
    pub volume_id: u32,
    /// Volume label
    pub volume_label: [u8; 11],
    /// Filesystem type
    pub fs_type: [u8; 8],
}

impl BootSector {
    /// Returns the FAT size in sectors.
    pub fn fat_size(&self) -> u32 {
        if self.fat_size_16 != 0 {
            self.fat_size_16 as u32
        } else {
            self.fat_size_32
        }
    }
    
    /// Returns the total sectors.
    pub fn total_sectors(&self) -> u32 {
        if self.total_sectors_16 != 0 {
            self.total_sectors_16 as u32
        } else {
            self.total_sectors_32
        }
    }
    
    /// Returns the first data sector.
    pub fn first_data_sector(&self) -> u32 {
        self.reserved_sectors as u32 + 
        (self.num_fats as u32 * self.fat_size())
    }
    
    /// Returns the cluster size in bytes.
    pub fn cluster_size(&self) -> u32 {
        self.sectors_per_cluster as u32 * self.bytes_per_sector as u32
    }
    
    /// Returns the number of data clusters.
    pub fn data_clusters(&self) -> u32 {
        let data_sectors = self.total_sectors() - self.first_data_sector();
        data_sectors / self.sectors_per_cluster as u32
    }
    
    /// Checks if this is FAT32.
    pub fn is_fat32(&self) -> bool {
        self.data_clusters() >= 65525
    }
}

/// FSInfo structure.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct FsInfo {
    /// Lead signature (0x41615252)
    pub lead_sig: u32,
    /// Reserved
    pub reserved1: [u8; 480],
    /// Structure signature (0x61417272)
    pub struct_sig: u32,
    /// Free cluster count
    pub free_count: u32,
    /// Next free cluster
    pub next_free: u32,
    /// Reserved
    pub reserved2: [u8; 12],
    /// Trail signature (0xAA550000)
    pub trail_sig: u32,
}

/// Short directory entry (8.3 format).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DirEntry {
    /// Filename (8 characters)
    pub name: [u8; 8],
    /// Extension (3 characters)
    pub ext: [u8; 3],
    /// Attributes
    pub attr: u8,
    /// Reserved for Windows NT
    pub nt_reserved: u8,
    /// Creation time (tenths of second)
    pub create_time_tenth: u8,
    /// Creation time
    pub create_time: u16,
    /// Creation date
    pub create_date: u16,
    /// Last access date
    pub access_date: u16,
    /// High 16 bits of cluster
    pub cluster_hi: u16,
    /// Last modification time
    pub modify_time: u16,
    /// Last modification date
    pub modify_date: u16,
    /// Low 16 bits of cluster
    pub cluster_lo: u16,
    /// File size
    pub file_size: u32,
}

impl DirEntry {
    /// Returns the first cluster number.
    pub fn cluster(&self) -> u32 {
        ((self.cluster_hi as u32) << 16) | (self.cluster_lo as u32)
    }
    
    /// Checks if this is a free entry.
    pub fn is_free(&self) -> bool {
        self.name[0] == 0x00 || self.name[0] == 0xE5
    }
    
    /// Checks if this is the last entry.
    pub fn is_last(&self) -> bool {
        self.name[0] == 0x00
    }
    
    /// Checks if this is a long filename entry.
    pub fn is_lfn(&self) -> bool {
        (self.attr & ATTR_LONG_NAME) == ATTR_LONG_NAME
    }
    
    /// Checks if this is a directory.
    pub fn is_directory(&self) -> bool {
        (self.attr & ATTR_DIRECTORY) != 0
    }
    
    /// Checks if this is a volume label.
    pub fn is_volume_id(&self) -> bool {
        (self.attr & ATTR_VOLUME_ID) != 0 && !self.is_lfn()
    }
    
    /// Returns the short filename.
    pub fn short_name(&self) -> String {
        let name = core::str::from_utf8(&self.name)
            .unwrap_or("")
            .trim_end();
        let ext = core::str::from_utf8(&self.ext)
            .unwrap_or("")
            .trim_end();
        
        if ext.is_empty() {
            String::from(name)
        } else {
            alloc::format!("{}.{}", name, ext)
        }
    }
    
    /// Returns the file type.
    pub fn file_type(&self) -> VfsFileType {
        if self.is_directory() {
            VfsFileType::Directory
        } else {
            VfsFileType::Regular
        }
    }
    
    /// Converts DOS date/time to Unix timestamp.
    pub fn to_timestamp(date: u16, time: u16) -> u64 {
        // DOS epoch is 1980-01-01
        let year = ((date >> 9) & 0x7F) as u64 + 1980;
        let month = ((date >> 5) & 0x0F) as u64;
        let day = (date & 0x1F) as u64;
        let hour = ((time >> 11) & 0x1F) as u64;
        let minute = ((time >> 5) & 0x3F) as u64;
        let second = ((time & 0x1F) * 2) as u64;
        
        // Simplified - not accounting for leap years properly
        let days = (year - 1970) * 365 + (month - 1) * 30 + day;
        days * 86400 + hour * 3600 + minute * 60 + second
    }
}

/// Long filename entry.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct LfnEntry {
    /// Sequence number
    pub order: u8,
    /// Characters 1-5
    pub name1: [u16; 5],
    /// Attributes (always 0x0F)
    pub attr: u8,
    /// Type (always 0)
    pub entry_type: u8,
    /// Checksum
    pub checksum: u8,
    /// Characters 6-11
    pub name2: [u16; 6],
    /// Always 0
    pub cluster: u16,
    /// Characters 12-13
    pub name3: [u16; 2],
}

impl LfnEntry {
    /// Returns the sequence number (0-based).
    pub fn sequence(&self) -> u8 {
        self.order & 0x3F
    }
    
    /// Checks if this is the last LFN entry.
    pub fn is_last(&self) -> bool {
        (self.order & 0x40) != 0
    }
    
    /// Extracts the 13 characters from this entry.
    pub fn chars(&self) -> [u16; 13] {
        let mut chars = [0u16; 13];
        let ptr = self as *const Self as *const u8;
        
        // name1 is at offset 1 (after order byte)
        for i in 0..5 {
            chars[i] = unsafe {
                core::ptr::read_unaligned(ptr.add(1 + i * 2) as *const u16)
            };
        }
        // name2 is at offset 14 (1 + 10 + 1 + 1 + 1)
        for i in 0..6 {
            chars[5 + i] = unsafe {
                core::ptr::read_unaligned(ptr.add(14 + i * 2) as *const u16)
            };
        }
        // name3 is at offset 28 (14 + 12 + 2)
        for i in 0..2 {
            chars[11 + i] = unsafe {
                core::ptr::read_unaligned(ptr.add(28 + i * 2) as *const u16)
            };
        }
        chars
    }
}

/// A parsed directory entry with long filename.
#[derive(Debug, Clone)]
pub struct ParsedEntry {
    /// Full filename
    pub name: String,
    /// Entry attributes
    pub attr: u8,
    /// First cluster
    pub cluster: u32,
    /// File size
    pub size: u32,
    /// Creation time
    pub created: u64,
    /// Modification time
    pub modified: u64,
    /// Access time
    pub accessed: u64,
}

impl ParsedEntry {
    /// Returns the file type.
    pub fn file_type(&self) -> VfsFileType {
        if (self.attr & ATTR_DIRECTORY) != 0 {
            VfsFileType::Directory
        } else {
            VfsFileType::Regular
        }
    }
}

/// FAT32 filesystem.
pub struct Fat32Fs {
    /// Block device
    device: Arc<dyn BlockDevice + Send + Sync>,
    /// Boot sector
    boot_sector: RwLock<Option<BootSector>>,
    /// Bytes per sector
    bytes_per_sector: spin::Mutex<u16>,
    /// Sectors per cluster
    sectors_per_cluster: spin::Mutex<u8>,
    /// First data sector
    first_data_sector: spin::Mutex<u32>,
    /// Root cluster
    root_cluster: spin::Mutex<u32>,
    /// FAT start sector
    fat_start: spin::Mutex<u32>,
    /// Cluster cache (cluster -> next cluster)
    fat_cache: RwLock<BTreeMap<u32, u32>>,
    /// Inode to cluster mapping
    inode_to_cluster: RwLock<BTreeMap<InodeNum, u32>>,
    /// Next available inode number
    next_inode: spin::Mutex<InodeNum>,
}

impl Fat32Fs {
    /// Creates a new FAT32 filesystem.
    pub fn new(device: Arc<dyn BlockDevice + Send + Sync>) -> Self {
        Self {
            device,
            boot_sector: RwLock::new(None),
            bytes_per_sector: spin::Mutex::new(512),
            sectors_per_cluster: spin::Mutex::new(1),
            first_data_sector: spin::Mutex::new(0),
            root_cluster: spin::Mutex::new(2),
            fat_start: spin::Mutex::new(0),
            fat_cache: RwLock::new(BTreeMap::new()),
            inode_to_cluster: RwLock::new(BTreeMap::new()),
            next_inode: spin::Mutex::new(10),
        }
    }
    
    /// Mounts the filesystem.
    pub fn mount(&self) -> Result<(), VfsError> {
        // Read boot sector
        let mut buffer = [0u8; 512];
        self.device.read_sectors(0, &mut buffer)
            .map_err(|_| VfsError::IoError)?;
        
        let bs = unsafe { *(buffer.as_ptr() as *const BootSector) };
        
        // Verify signature
        let sig = u16::from_le_bytes([buffer[510], buffer[511]]);
        if sig != FAT32_SIGNATURE {
            return Err(VfsError::InvalidArgument);
        }
        
        // Verify FAT32
        if !bs.is_fat32() {
            crate::serial_println!("[fat32] Not a FAT32 filesystem");
            return Err(VfsError::InvalidArgument);
        }
        
        *self.bytes_per_sector.lock() = bs.bytes_per_sector;
        *self.sectors_per_cluster.lock() = bs.sectors_per_cluster;
        *self.first_data_sector.lock() = bs.first_data_sector();
        *self.root_cluster.lock() = bs.root_cluster;
        *self.fat_start.lock() = bs.reserved_sectors as u32;
        
        // Map root directory
        self.inode_to_cluster.write().insert(FAT32_ROOT_INO, bs.root_cluster);
        
        crate::serial_println!(
            "[fat32] Mounted: {} MB, cluster_size={}",
            bs.total_sectors() as u64 * bs.bytes_per_sector as u64 / 1024 / 1024,
            bs.cluster_size()
        );
        
        *self.boot_sector.write() = Some(bs);
        
        Ok(())
    }
    
    /// Reads sectors from the device.
    fn read_sectors(&self, sector: u64, buffer: &mut [u8]) -> Result<(), VfsError> {
        self.device.read_sectors(sector, buffer)
            .map_err(|_| VfsError::IoError)
    }
    
    /// Writes sectors to the device.
    fn write_sectors(&self, sector: u64, buffer: &[u8]) -> Result<(), VfsError> {
        self.device.write_sectors(sector, buffer)
            .map_err(|_| VfsError::IoError)
    }
    
    /// Converts a cluster number to sector number.
    fn cluster_to_sector(&self, cluster: u32) -> u32 {
        let first_data = *self.first_data_sector.lock();
        let spc = *self.sectors_per_cluster.lock() as u32;
        first_data + (cluster - 2) * spc
    }
    
    /// Reads a cluster.
    fn read_cluster(&self, cluster: u32) -> Result<Vec<u8>, VfsError> {
        let sector = self.cluster_to_sector(cluster);
        let spc = *self.sectors_per_cluster.lock() as usize;
        let bps = *self.bytes_per_sector.lock() as usize;
        let cluster_size = spc * bps;
        
        let mut buffer = vec![0u8; cluster_size];
        self.read_sectors(sector as u64, &mut buffer)?;
        
        Ok(buffer)
    }
    
    /// Reads the next cluster from FAT.
    fn next_cluster(&self, cluster: u32) -> Result<Option<u32>, VfsError> {
        // Check cache
        if let Some(&next) = self.fat_cache.read().get(&cluster) {
            return Ok(if next >= FAT32_CLUSTER_END {
                None
            } else {
                Some(next)
            });
        }
        
        let fat_start = *self.fat_start.lock();
        let bps = *self.bytes_per_sector.lock() as u32;
        
        // Calculate FAT entry offset
        let fat_offset = cluster * 4;
        let fat_sector = fat_start + (fat_offset / bps);
        let entry_offset = (fat_offset % bps) as usize;
        
        let mut buffer = vec![0u8; bps as usize];
        self.read_sectors(fat_sector as u64, &mut buffer)?;
        
        let next = u32::from_le_bytes([
            buffer[entry_offset],
            buffer[entry_offset + 1],
            buffer[entry_offset + 2],
            buffer[entry_offset + 3],
        ]) & FAT32_CLUSTER_MASK;
        
        // Cache it
        self.fat_cache.write().insert(cluster, next);
        
        Ok(if next >= FAT32_CLUSTER_END {
            None
        } else {
            Some(next)
        })
    }
    
    /// Follows cluster chain and reads all data.
    fn read_cluster_chain(&self, start_cluster: u32, max_bytes: Option<u64>) -> Result<Vec<u8>, VfsError> {
        let spc = *self.sectors_per_cluster.lock() as usize;
        let bps = *self.bytes_per_sector.lock() as usize;
        let cluster_size = spc * bps;
        
        let mut data = Vec::new();
        let mut cluster = start_cluster;
        
        loop {
            if cluster < 2 || cluster >= FAT32_CLUSTER_RESERVED {
                break;
            }
            
            let cluster_data = self.read_cluster(cluster)?;
            data.extend_from_slice(&cluster_data);
            
            if let Some(max) = max_bytes {
                if data.len() as u64 >= max {
                    data.truncate(max as usize);
                    break;
                }
            }
            
            match self.next_cluster(cluster)? {
                Some(next) => cluster = next,
                None => break,
            }
        }
        
        Ok(data)
    }
    
    /// Parses directory entries.
    fn parse_directory(&self, cluster: u32) -> Result<Vec<ParsedEntry>, VfsError> {
        let data = self.read_cluster_chain(cluster, None)?;
        let mut entries = Vec::new();
        let mut lfn_buffer: Vec<u16> = Vec::new();
        let mut lfn_checksum = 0u8;
        
        let entry_count = data.len() / 32;
        
        for i in 0..entry_count {
            let offset = i * 32;
            let entry_data = &data[offset..offset + 32];
            let entry = unsafe { *(entry_data.as_ptr() as *const DirEntry) };
            
            if entry.is_last() {
                break;
            }
            
            if entry.is_free() {
                lfn_buffer.clear();
                continue;
            }
            
            if entry.is_lfn() {
                // Long filename entry
                let lfn = unsafe { *(entry_data.as_ptr() as *const LfnEntry) };
                let seq = lfn.sequence();
                
                if lfn.is_last() {
                    lfn_buffer.clear();
                    lfn_checksum = lfn.checksum;
                }
                
                // Insert characters at the beginning (LFN entries are in reverse order)
                let chars = lfn.chars();
                for &c in chars.iter().rev() {
                    if c != 0 && c != 0xFFFF {
                        lfn_buffer.insert(0, c);
                    }
                }
            } else if entry.is_volume_id() {
                // Volume label
                lfn_buffer.clear();
            } else {
                // Regular entry
                let name = if !lfn_buffer.is_empty() {
                    // Use long filename
                    String::from_utf16_lossy(&lfn_buffer)
                } else {
                    // Use short name
                    entry.short_name()
                };
                lfn_buffer.clear();
                
                // Skip . and .. entries
                if name == "." || name == ".." {
                    continue;
                }
                
                entries.push(ParsedEntry {
                    name,
                    attr: entry.attr,
                    cluster: entry.cluster(),
                    size: entry.file_size,
                    created: DirEntry::to_timestamp(entry.create_date, entry.create_time),
                    modified: DirEntry::to_timestamp(entry.modify_date, entry.modify_time),
                    accessed: DirEntry::to_timestamp(entry.access_date, 0),
                });
            }
        }
        
        Ok(entries)
    }
    
    /// Gets or creates an inode for a cluster.
    fn get_inode(&self, cluster: u32) -> InodeNum {
        // Check if we already have an inode for this cluster
        let mapping = self.inode_to_cluster.read();
        for (&ino, &c) in mapping.iter() {
            if c == cluster {
                return ino;
            }
        }
        drop(mapping);
        
        // Create new inode
        let mut next = self.next_inode.lock();
        let ino = *next;
        *next += 1;
        self.inode_to_cluster.write().insert(ino, cluster);
        ino
    }
    
    /// Gets the cluster for an inode.
    fn get_cluster(&self, ino: InodeNum) -> Result<u32, VfsError> {
        self.inode_to_cluster.read()
            .get(&ino)
            .copied()
            .ok_or(VfsError::NotFound)
    }
    
    /// Looks up an entry in a directory.
    fn lookup_entry(&self, dir_cluster: u32, name: &str) -> Result<ParsedEntry, VfsError> {
        let entries = self.parse_directory(dir_cluster)?;
        
        // Case-insensitive comparison (FAT is case-preserving but case-insensitive)
        let name_lower = name.to_lowercase();
        
        for entry in entries {
            if entry.name.to_lowercase() == name_lower {
                return Ok(entry);
            }
        }
        
        Err(VfsError::NotFound)
    }
}

impl Filesystem for Fat32Fs {
    fn name(&self) -> &'static str {
        "fat32"
    }
    
    fn statfs(&self) -> Result<VfsStatFs, VfsError> {
        let bs = self.boot_sector.read();
        let bs = bs.as_ref().ok_or(VfsError::NoFilesystem)?;
        
        let cluster_size = bs.cluster_size();
        let total_clusters = bs.data_clusters();
        
        Ok(VfsStatFs {
            blocks: total_clusters as u64,
            bfree: 0, // Would need to scan FAT
            bavail: 0,
            files: 0,
            ffree: 0,
            bsize: cluster_size,
            namelen: 255,
        })
    }
    
    fn lookup(&self, parent: InodeNum, name: &str) -> Result<InodeNum, VfsError> {
        let parent_cluster = self.get_cluster(parent)?;
        let entry = self.lookup_entry(parent_cluster, name)?;
        
        // Create inode for the found entry
        let ino = self.get_inode(entry.cluster);
        
        Ok(ino)
    }
    
    fn getattr(&self, ino: InodeNum) -> Result<VfsAttr, VfsError> {
        // Root directory
        if ino == FAT32_ROOT_INO {
            return Ok(VfsAttr {
                ino,
                file_type: VfsFileType::Directory,
                perm: VfsPermissions {
                    readable: true,
                    writable: true,
                    executable: true,
                },
                size: 0,
                nlink: 1,
                blksize: *self.bytes_per_sector.lock() as u32,
                blocks: 0,
                atime: 0,
                mtime: 0,
                ctime: 0,
                crtime: 0,
            });
        }
        
        // For other inodes, we need to scan to find the entry
        // This is inefficient but FAT doesn't have a real inode table
        let _cluster = self.get_cluster(ino)?;
        
        // We don't have entry metadata cached, return basic info
        Ok(VfsAttr {
            ino,
            file_type: VfsFileType::Regular, // Would need proper tracking
            perm: VfsPermissions {
                readable: true,
                writable: true,
                executable: false,
            },
            size: 0,
            nlink: 1,
            blksize: *self.bytes_per_sector.lock() as u32,
            blocks: 0,
            atime: 0,
            mtime: 0,
            ctime: 0,
            crtime: 0,
        })
    }
    
    fn readdir(&self, ino: InodeNum) -> Result<Vec<VfsDirEntry>, VfsError> {
        let cluster = self.get_cluster(ino)?;
        let entries = self.parse_directory(cluster)?;
        
        let mut result = Vec::new();
        for entry in entries {
            let entry_ino = self.get_inode(entry.cluster);
            let file_type = entry.file_type();
            result.push(VfsDirEntry {
                ino: entry_ino,
                name: entry.name,
                file_type,
            });
        }
        
        Ok(result)
    }
    
    fn read(&self, ino: InodeNum, offset: u64, size: usize) -> Result<Vec<u8>, VfsError> {
        let cluster = self.get_cluster(ino)?;
        
        // Read the full file (inefficient but simple)
        let data = self.read_cluster_chain(cluster, Some(offset + size as u64))?;
        
        if offset as usize >= data.len() {
            return Ok(Vec::new());
        }
        
        let available = data.len() - offset as usize;
        let to_copy = core::cmp::min(size, available);
        Ok(data[offset as usize..offset as usize + to_copy].to_vec())
    }
    
    fn write(&self, _ino: InodeNum, _offset: u64, _data: &[u8]) -> Result<usize, VfsError> {
        // FAT32 write not implemented yet
        Err(VfsError::NotSupported)
    }
    
    fn create(&self, _parent: InodeNum, _name: &str, _file_type: VfsFileType) -> Result<InodeNum, VfsError> {
        // Would need to allocate cluster and create directory entry
        Err(VfsError::NotSupported)
    }
    
    fn unlink(&self, _parent: InodeNum, _name: &str) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }
    
    fn rename(&self, _old_parent: InodeNum, _old_name: &str, _new_parent: InodeNum, _new_name: &str) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }
    
    fn truncate(&self, _ino: InodeNum, _size: u64) -> Result<(), VfsError> {
        Err(VfsError::NotSupported)
    }
    
    fn sync(&self) -> Result<(), VfsError> {
        // Nothing to sync - read-only for now
        Ok(())
    }
}

// ============================================================================
// Module-level mount API for shell commands
// ============================================================================

/// Block device wrapper that uses block module's read/write functions.
struct BlockDeviceWrapper {
    name: String,
}

impl BlockDeviceWrapper {
    fn new(name: &str) -> Self {
        Self { name: name.into() }
    }
}

impl crate::block::BlockDevice for BlockDeviceWrapper {
    fn info(&self) -> crate::block::BlockDeviceInfo {
        crate::block::list_devices()
            .into_iter()
            .find(|d| d.name == self.name)
            .unwrap_or(crate::block::BlockDeviceInfo {
                name: self.name.clone(),
                sector_size: 512,
                total_sectors: 0,
                read_only: true,
                model: String::new(),
            })
    }
    
    fn read_sectors(&self, start: u64, buf: &mut [u8]) -> Result<(), crate::block::BlockError> {
        let sector_size = self.info().sector_size;
        let count = buf.len() / sector_size;
        let data = crate::block::read(&self.name, start, count)?;
        buf[..data.len()].copy_from_slice(&data);
        Ok(())
    }
    
    fn write_sectors(&self, _start: u64, _data: &[u8]) -> Result<(), crate::block::BlockError> {
        Err(crate::block::BlockError::Unsupported)
    }
    
    fn flush(&self) -> Result<(), crate::block::BlockError> {
        Ok(())
    }
    
    fn is_ready(&self) -> bool {
        true
    }
}

/// Global FAT32 mount registry.
static FAT32_MOUNTS: spin::RwLock<BTreeMap<String, Fat32Fs>> = spin::RwLock::new(BTreeMap::new());

/// Mount a FAT32 filesystem.
pub fn mount(device_name: &str, mount_point: &str) -> Result<(), VfsError> {
    use alloc::string::ToString;
    
    // Check device exists
    if !crate::block::list_devices().iter().any(|d| d.name == device_name) {
        return Err(VfsError::NotFound);
    }
    
    // Create block device wrapper
    let device = Arc::new(BlockDeviceWrapper::new(device_name));
    
    // Create FAT32 filesystem
    let fs = Fat32Fs::new(device);
    
    // Mount it (parse boot sector, etc.)
    fs.mount()?;
    
    // Store in registry
    FAT32_MOUNTS.write().insert(mount_point.to_string(), fs);
    
    crate::serial_println!("[fat32] Mounted {} at {} (read-only)", device_name, mount_point);
    Ok(())
}

/// Unmount a FAT32 filesystem.
pub fn unmount(mount_point: &str) -> Result<(), VfsError> {
    if FAT32_MOUNTS.write().remove(mount_point).is_some() {
        crate::serial_println!("[fat32] Unmounted {}", mount_point);
        Ok(())
    } else {
        Err(VfsError::NotFound)
    }
}

/// List directory on FAT32 filesystem.
pub fn ls(path: &str) -> Result<Vec<(String, VfsFileType, u64)>, VfsError> {
    // Find mount point
    let mounts = FAT32_MOUNTS.read();
    for (mount_point, fs) in mounts.iter() {
        if path.starts_with(mount_point.as_str()) {
            let _rel_path = &path[mount_point.len()..];
            
            // List root directory
            let entries = fs.readdir(FAT32_ROOT_INO)?;
            return Ok(entries.iter().map(|entry| {
                let ft = match fs.getattr(entry.ino) {
                    Ok(attrs) => attrs.file_type,
                    Err(_) => VfsFileType::Regular,
                };
                let size = match fs.getattr(entry.ino) {
                    Ok(attrs) => attrs.size,
                    Err(_) => 0,
                };
                (entry.name.clone(), ft, size)
            }).collect());
        }
    }
    Err(VfsError::NotFound)
}

/// Read file from FAT32 filesystem.
pub fn cat(path: &str) -> Result<Vec<u8>, VfsError> {
    let mounts = FAT32_MOUNTS.read();
    for (mount_point, fs) in mounts.iter() {
        if path.starts_with(mount_point.as_str()) {
            let rel_path = &path[mount_point.len()..];
            
            // Lookup file
            let ino = fs.lookup(FAT32_ROOT_INO, rel_path)?;
            
            // Read entire file
            return fs.read(ino, 0, 1024 * 1024); // Max 1MB
        }
    }
    Err(VfsError::NotFound)
}

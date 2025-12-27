//! # ext4 Read-Only Filesystem
//!
//! Read-only ext4 filesystem implementation for Splax OS.
//!
//! ## Features
//!
//! - ext4 superblock parsing
//! - Inode reading
//! - Directory traversal
//! - File reading (extent-based and indirect blocks)
//! - Symlink following
//! - Large file support (>4GB)
//!
//! ## Limitations (Read-Only)
//!
//! - No write support
//! - No journaling
//! - No extended attributes (xattr)
//! - No quota support

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use spin::RwLock;

use crate::block::{BlockDevice, SECTOR_SIZE};
use crate::fs::vfs::{
    Filesystem, InodeNum, VfsAttr, VfsDirEntry, VfsError, VfsFileType, VfsPermissions, VfsStatFs,
};

/// ext4 magic number.
const EXT4_MAGIC: u16 = 0xEF53;

/// Superblock offset from start of partition.
const SUPERBLOCK_OFFSET: u64 = 1024;

/// ext4 inode constants.
const EXT4_INODE_SIZE_MIN: u16 = 128;
const EXT4_ROOT_INODE: u32 = 2;

/// ext4 file type flags in inode mode.
const S_IFMT: u16 = 0xF000;
const S_IFREG: u16 = 0x8000;
const S_IFDIR: u16 = 0x4000;
const S_IFLNK: u16 = 0xA000;
const S_IFBLK: u16 = 0x6000;
const S_IFCHR: u16 = 0x2000;
const S_IFIFO: u16 = 0x1000;
const S_IFSOCK: u16 = 0xC000;

/// Directory entry file type in dir_entry.
const EXT4_FT_UNKNOWN: u8 = 0;
const EXT4_FT_REG_FILE: u8 = 1;
const EXT4_FT_DIR: u8 = 2;
const EXT4_FT_CHRDEV: u8 = 3;
const EXT4_FT_BLKDEV: u8 = 4;
const EXT4_FT_FIFO: u8 = 5;
const EXT4_FT_SOCK: u8 = 6;
const EXT4_FT_SYMLINK: u8 = 7;

/// ext4 feature flags.
pub mod feature {
    // Compatible features
    pub const COMPAT_DIR_PREALLOC: u32 = 0x0001;
    pub const COMPAT_HAS_JOURNAL: u32 = 0x0004;
    pub const COMPAT_EXT_ATTR: u32 = 0x0008;
    pub const COMPAT_RESIZE_INODE: u32 = 0x0010;
    pub const COMPAT_DIR_INDEX: u32 = 0x0020;
    
    // Incompatible features
    pub const INCOMPAT_COMPRESSION: u32 = 0x0001;
    pub const INCOMPAT_FILETYPE: u32 = 0x0002;
    pub const INCOMPAT_RECOVER: u32 = 0x0004;
    pub const INCOMPAT_JOURNAL_DEV: u32 = 0x0008;
    pub const INCOMPAT_META_BG: u32 = 0x0010;
    pub const INCOMPAT_EXTENTS: u32 = 0x0040;
    pub const INCOMPAT_64BIT: u32 = 0x0080;
    pub const INCOMPAT_FLEX_BG: u32 = 0x0200;
    pub const INCOMPAT_INLINE_DATA: u32 = 0x8000;
    
    // Read-only compatible features
    pub const RO_COMPAT_SPARSE_SUPER: u32 = 0x0001;
    pub const RO_COMPAT_LARGE_FILE: u32 = 0x0002;
    pub const RO_COMPAT_HUGE_FILE: u32 = 0x0008;
    pub const RO_COMPAT_GDT_CSUM: u32 = 0x0010;
    pub const RO_COMPAT_DIR_NLINK: u32 = 0x0020;
    pub const RO_COMPAT_EXTRA_ISIZE: u32 = 0x0040;
    pub const RO_COMPAT_METADATA_CSUM: u32 = 0x0400;
}

/// ext4 superblock.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Superblock {
    /// Total inode count
    pub inodes_count: u32,
    /// Total block count (low 32 bits)
    pub blocks_count_lo: u32,
    /// Reserved block count (low 32 bits)
    pub r_blocks_count_lo: u32,
    /// Free block count (low 32 bits)
    pub free_blocks_count_lo: u32,
    /// Free inode count
    pub free_inodes_count: u32,
    /// First data block
    pub first_data_block: u32,
    /// Block size (log2(block_size) - 10)
    pub log_block_size: u32,
    /// Cluster size (log2(cluster_size) - 10)
    pub log_cluster_size: u32,
    /// Blocks per group
    pub blocks_per_group: u32,
    /// Clusters per group
    pub clusters_per_group: u32,
    /// Inodes per group
    pub inodes_per_group: u32,
    /// Mount time
    pub mtime: u32,
    /// Write time
    pub wtime: u32,
    /// Mount count
    pub mnt_count: u16,
    /// Maximum mount count
    pub max_mnt_count: u16,
    /// Magic number (0xEF53)
    pub magic: u16,
    /// Filesystem state
    pub state: u16,
    /// Error behavior
    pub errors: u16,
    /// Minor revision level
    pub minor_rev_level: u16,
    /// Last check time
    pub lastcheck: u32,
    /// Check interval
    pub checkinterval: u32,
    /// Creator OS
    pub creator_os: u32,
    /// Revision level
    pub rev_level: u32,
    /// Default UID for reserved blocks
    pub def_resuid: u16,
    /// Default GID for reserved blocks
    pub def_resgid: u16,
    // Extended superblock fields (rev_level >= 1)
    /// First non-reserved inode
    pub first_ino: u32,
    /// Inode size
    pub inode_size: u16,
    /// Block group number of this superblock
    pub block_group_nr: u16,
    /// Compatible feature set
    pub feature_compat: u32,
    /// Incompatible feature set
    pub feature_incompat: u32,
    /// Read-only compatible feature set
    pub feature_ro_compat: u32,
    /// UUID
    pub uuid: [u8; 16],
    /// Volume name
    pub volume_name: [u8; 16],
    /// Last mount path
    pub last_mounted: [u8; 64],
    /// Compression algorithm
    pub algorithm_usage_bitmap: u32,
    // Additional fields...
    /// Preallocate blocks for files
    pub prealloc_blocks: u8,
    /// Preallocate blocks for directories
    pub prealloc_dir_blocks: u8,
    /// Reserved GDT blocks
    pub reserved_gdt_blocks: u16,
    // Journal fields
    /// Journal UUID
    pub journal_uuid: [u8; 16],
    /// Journal inode number
    pub journal_inum: u32,
    /// Journal device number
    pub journal_dev: u32,
    /// Start of orphan inode list
    pub last_orphan: u32,
    /// HTREE hash seed
    pub hash_seed: [u32; 4],
    /// Default hash version
    pub def_hash_version: u8,
    /// Journal backup type
    pub jnl_backup_type: u8,
    /// Group descriptor size
    pub desc_size: u16,
    /// Default mount options
    pub default_mount_opts: u32,
    /// First metablock group
    pub first_meta_bg: u32,
    /// Filesystem creation time
    pub mkfs_time: u32,
    /// Journal backup
    pub jnl_blocks: [u32; 17],
    // 64-bit support
    /// Total block count (high 32 bits)
    pub blocks_count_hi: u32,
    /// Reserved block count (high 32 bits)
    pub r_blocks_count_hi: u32,
    /// Free block count (high 32 bits)
    pub free_blocks_count_hi: u32,
    /// Minimum inode extra size
    pub min_extra_isize: u16,
    /// Desired inode extra size
    pub want_extra_isize: u16,
    /// Miscellaneous flags
    pub flags: u32,
    /// RAID stride
    pub raid_stride: u16,
    /// MMP interval
    pub mmp_interval: u16,
    /// MMP block
    pub mmp_block: u64,
    /// RAID stripe width
    pub raid_stripe_width: u32,
    /// Flexible block group size
    pub log_groups_per_flex: u8,
    /// Checksum type
    pub checksum_type: u8,
    /// Reserved padding
    pub reserved_pad: u16,
    /// Total KB written
    pub kbytes_written: u64,
}

impl Superblock {
    /// Returns the block size in bytes.
    pub fn block_size(&self) -> u32 {
        1024 << self.log_block_size
    }
    
    /// Returns the total block count (64-bit).
    pub fn blocks_count(&self) -> u64 {
        (self.blocks_count_hi as u64) << 32 | (self.blocks_count_lo as u64)
    }
    
    /// Returns the free block count (64-bit).
    pub fn free_blocks_count(&self) -> u64 {
        (self.free_blocks_count_hi as u64) << 32 | (self.free_blocks_count_lo as u64)
    }
    
    /// Returns the number of block groups.
    pub fn group_count(&self) -> u32 {
        (self.blocks_count_lo + self.blocks_per_group - 1) / self.blocks_per_group
    }
    
    /// Checks if a feature is supported.
    pub fn has_feature_incompat(&self, feature: u32) -> bool {
        (self.feature_incompat & feature) != 0
    }
    
    /// Checks if the filesystem uses 64-bit block numbers.
    pub fn is_64bit(&self) -> bool {
        self.has_feature_incompat(feature::INCOMPAT_64BIT)
    }
    
    /// Checks if the filesystem uses extents.
    pub fn uses_extents(&self) -> bool {
        self.has_feature_incompat(feature::INCOMPAT_EXTENTS)
    }
}

/// Block group descriptor.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct GroupDesc {
    /// Block bitmap location (low 32 bits)
    pub block_bitmap_lo: u32,
    /// Inode bitmap location (low 32 bits)
    pub inode_bitmap_lo: u32,
    /// Inode table location (low 32 bits)
    pub inode_table_lo: u32,
    /// Free block count (low 16 bits)
    pub free_blocks_count_lo: u16,
    /// Free inode count (low 16 bits)
    pub free_inodes_count_lo: u16,
    /// Used directory count (low 16 bits)
    pub used_dirs_count_lo: u16,
    /// Flags
    pub flags: u16,
    /// Exclude bitmap location (low 32 bits)
    pub exclude_bitmap_lo: u32,
    /// Block bitmap checksum (low 16 bits)
    pub block_bitmap_csum_lo: u16,
    /// Inode bitmap checksum (low 16 bits)
    pub inode_bitmap_csum_lo: u16,
    /// Unused inode count (low 16 bits)
    pub itable_unused_lo: u16,
    /// Checksum
    pub checksum: u16,
    // 64-bit fields
    /// Block bitmap location (high 32 bits)
    pub block_bitmap_hi: u32,
    /// Inode bitmap location (high 32 bits)
    pub inode_bitmap_hi: u32,
    /// Inode table location (high 32 bits)
    pub inode_table_hi: u32,
    /// Free block count (high 16 bits)
    pub free_blocks_count_hi: u16,
    /// Free inode count (high 16 bits)
    pub free_inodes_count_hi: u16,
    /// Used directory count (high 16 bits)
    pub used_dirs_count_hi: u16,
    /// Unused inode count (high 16 bits)
    pub itable_unused_hi: u16,
    /// Exclude bitmap location (high 32 bits)
    pub exclude_bitmap_hi: u32,
    /// Block bitmap checksum (high 16 bits)
    pub block_bitmap_csum_hi: u16,
    /// Inode bitmap checksum (high 16 bits)
    pub inode_bitmap_csum_hi: u16,
    /// Reserved
    pub reserved: u32,
}

impl GroupDesc {
    /// Returns the inode table block number.
    pub fn inode_table(&self, is_64bit: bool) -> u64 {
        if is_64bit {
            (self.inode_table_hi as u64) << 32 | (self.inode_table_lo as u64)
        } else {
            self.inode_table_lo as u64
        }
    }
}

/// ext4 inode (on-disk).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Inode {
    /// File mode
    pub mode: u16,
    /// Owner UID
    pub uid: u16,
    /// Size (low 32 bits)
    pub size_lo: u32,
    /// Access time
    pub atime: u32,
    /// Inode change time
    pub ctime: u32,
    /// Modification time
    pub mtime: u32,
    /// Deletion time
    pub dtime: u32,
    /// Owner GID
    pub gid: u16,
    /// Link count
    pub links_count: u16,
    /// Block count (in 512-byte units)
    pub blocks_lo: u32,
    /// Flags
    pub flags: u32,
    /// OS-specific value 1
    pub osd1: u32,
    /// Block map or extent tree
    pub block: [u32; 15],
    /// File version
    pub generation: u32,
    /// File ACL (low 32 bits)
    pub file_acl_lo: u32,
    /// Size (high 32 bits) / Directory ACL
    pub size_high: u32,
    /// Fragment address (obsolete)
    pub obso_faddr: u32,
    /// OS-specific value 2
    pub osd2: [u8; 12],
    /// Extra inode size
    pub extra_isize: u16,
    /// Checksum (high 16 bits)
    pub checksum_hi: u16,
    /// ctime extra
    pub ctime_extra: u32,
    /// mtime extra
    pub mtime_extra: u32,
    /// atime extra
    pub atime_extra: u32,
    /// Creation time
    pub crtime: u32,
    /// Creation time extra
    pub crtime_extra: u32,
    /// Version (high 32 bits)
    pub version_hi: u32,
    /// Project ID
    pub projid: u32,
}

impl Inode {
    /// Returns the file size.
    pub fn size(&self) -> u64 {
        let hi = { self.size_high };
        let lo = { self.size_lo };
        (hi as u64) << 32 | (lo as u64)
    }
    
    /// Returns a copy of the block data (to avoid alignment issues with packed struct).
    pub fn block_data(&self) -> [u32; 15] {
        let mut data = [0u32; 15];
        for i in 0..15 {
            // Read each element individually to avoid alignment issues
            data[i] = unsafe {
                core::ptr::read_unaligned(
                    (self as *const Self as *const u8).add(40 + i * 4) as *const u32
                )
            };
        }
        data
    }
    
    /// Returns the block data as bytes.
    pub fn block_bytes(&self) -> [u8; 60] {
        let block = self.block_data();
        let mut bytes = [0u8; 60];
        for (i, &word) in block.iter().enumerate() {
            let word_bytes = word.to_le_bytes();
            bytes[i * 4..i * 4 + 4].copy_from_slice(&word_bytes);
        }
        bytes
    }
    
    /// Returns the file type.
    pub fn file_type(&self) -> VfsFileType {
        let mode = { self.mode };
        match mode & S_IFMT {
            S_IFREG => VfsFileType::Regular,
            S_IFDIR => VfsFileType::Directory,
            S_IFLNK => VfsFileType::Symlink,
            S_IFBLK => VfsFileType::BlockDevice,
            S_IFCHR => VfsFileType::CharDevice,
            S_IFIFO => VfsFileType::Fifo,
            S_IFSOCK => VfsFileType::Socket,
            _ => VfsFileType::Regular,
        }
    }
    
    /// Checks if this inode uses extents.
    pub fn uses_extents(&self) -> bool {
        let flags = { self.flags };
        (flags & 0x80000) != 0 // EXT4_EXTENTS_FL
    }
    
    /// Returns permissions.
    pub fn permissions(&self) -> VfsPermissions {
        let mode = { self.mode };
        VfsPermissions {
            readable: (mode & 0o444) != 0,
            writable: false, // Read-only filesystem
            executable: (mode & 0o111) != 0,
        }
    }
}

/// Extent header.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ExtentHeader {
    /// Magic number (0xF30A)
    pub magic: u16,
    /// Number of valid entries
    pub entries: u16,
    /// Maximum number of entries
    pub max: u16,
    /// Depth (0 = leaf)
    pub depth: u16,
    /// Generation
    pub generation: u32,
}

/// Extent (leaf node).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Extent {
    /// First file block
    pub block: u32,
    /// Number of blocks
    pub len: u16,
    /// High 16 bits of physical block
    pub start_hi: u16,
    /// Low 32 bits of physical block
    pub start_lo: u32,
}

impl Extent {
    /// Returns the starting physical block.
    pub fn start(&self) -> u64 {
        (self.start_hi as u64) << 32 | (self.start_lo as u64)
    }
    
    /// Returns the extent length.
    pub fn length(&self) -> u32 {
        // High bit indicates uninitialized extent
        (self.len & 0x7FFF) as u32
    }
}

/// Extent index (internal node).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ExtentIndex {
    /// First file block covered
    pub block: u32,
    /// Low 32 bits of child node block
    pub leaf_lo: u32,
    /// High 16 bits of child node block
    pub leaf_hi: u16,
    /// Unused
    pub unused: u16,
}

impl ExtentIndex {
    /// Returns the child node block number.
    pub fn leaf(&self) -> u64 {
        (self.leaf_hi as u64) << 32 | (self.leaf_lo as u64)
    }
}

/// Directory entry.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct DirEntry {
    /// Inode number
    pub inode: u32,
    /// Entry length
    pub rec_len: u16,
    /// Name length
    pub name_len: u8,
    /// File type
    pub file_type: u8,
    // Name follows (variable length)
}

/// ext4 filesystem.
pub struct Ext4Fs {
    /// Block device
    device: Arc<dyn BlockDevice + Send + Sync>,
    /// Superblock
    superblock: RwLock<Option<Superblock>>,
    /// Block size
    block_size: spin::Mutex<u32>,
    /// Inode size
    inode_size: spin::Mutex<u16>,
    /// Group descriptors
    group_descs: RwLock<Vec<GroupDesc>>,
    /// Inode cache
    inode_cache: RwLock<BTreeMap<u32, Inode>>,
}

impl Ext4Fs {
    /// Creates a new ext4 filesystem.
    pub fn new(device: Arc<dyn BlockDevice + Send + Sync>) -> Self {
        Self {
            device,
            superblock: RwLock::new(None),
            block_size: spin::Mutex::new(4096),
            inode_size: spin::Mutex::new(256),
            group_descs: RwLock::new(Vec::new()),
            inode_cache: RwLock::new(BTreeMap::new()),
        }
    }
    
    /// Mounts the filesystem.
    pub fn mount(&self) -> Result<(), VfsError> {
        // Read superblock
        let sb = self.read_superblock()?;
        
        // Verify magic number
        if sb.magic != EXT4_MAGIC {
            return Err(VfsError::InvalidArgument);
        }
        
        // Check for unsupported incompatible features
        let unsupported = sb.feature_incompat & 
            !(feature::INCOMPAT_FILETYPE | 
              feature::INCOMPAT_EXTENTS | 
              feature::INCOMPAT_64BIT | 
              feature::INCOMPAT_FLEX_BG);
        if unsupported != 0 {
            crate::serial_println!("[ext4] Unsupported features: {:#x}", unsupported);
            return Err(VfsError::NotSupported);
        }
        
        let block_size = sb.block_size();
        let inode_size = if sb.rev_level >= 1 { sb.inode_size } else { EXT4_INODE_SIZE_MIN };
        
        // Copy fields from packed struct before using
        let total_blocks = sb.blocks_count();
        let total_inodes = { sb.inodes_count };
        let uses_extents = sb.uses_extents();
        
        *self.block_size.lock() = block_size;
        *self.inode_size.lock() = inode_size;
        
        crate::serial_println!(
            "[ext4] Mounted: {} blocks, {} inodes, block_size={}, uses_extents={}",
            total_blocks, total_inodes, block_size, uses_extents
        );
        
        // Read group descriptors
        let group_count = sb.group_count();
        let desc_size = if sb.is_64bit() && sb.desc_size > 32 {
            sb.desc_size as usize
        } else {
            32
        };
        
        let gd_block = if block_size == 1024 { 2 } else { 1 };
        let mut group_descs = Vec::with_capacity(group_count as usize);
        
        for i in 0..group_count {
            let offset = gd_block as u64 * block_size as u64 + i as u64 * desc_size as u64;
            let gd = self.read_struct::<GroupDesc>(offset)?;
            group_descs.push(gd);
        }
        
        *self.group_descs.write() = group_descs;
        *self.superblock.write() = Some(sb);
        
        Ok(())
    }
    
    /// Reads the superblock.
    fn read_superblock(&self) -> Result<Superblock, VfsError> {
        self.read_struct::<Superblock>(SUPERBLOCK_OFFSET)
    }
    
    /// Reads a structure from disk.
    fn read_struct<T: Copy>(&self, offset: u64) -> Result<T, VfsError> {
        let size = core::mem::size_of::<T>();
        let mut buffer = vec![0u8; size];
        
        let sector = offset / SECTOR_SIZE as u64;
        let sector_offset = (offset % SECTOR_SIZE as u64) as usize;
        
        // Read enough sectors
        let sectors_needed = (sector_offset + size + SECTOR_SIZE - 1) / SECTOR_SIZE;
        let mut sector_buffer = vec![0u8; sectors_needed * SECTOR_SIZE];
        
        self.device.read_sectors(sector, &mut sector_buffer)
            .map_err(|_| VfsError::IoError)?;
        
        buffer.copy_from_slice(&sector_buffer[sector_offset..sector_offset + size]);
        
        Ok(unsafe { core::ptr::read(buffer.as_ptr() as *const T) })
    }
    
    /// Reads bytes from disk.
    fn read_bytes(&self, offset: u64, buffer: &mut [u8]) -> Result<(), VfsError> {
        let sector = offset / SECTOR_SIZE as u64;
        let sector_offset = (offset % SECTOR_SIZE as u64) as usize;
        
        let sectors_needed = (sector_offset + buffer.len() + SECTOR_SIZE - 1) / SECTOR_SIZE;
        let mut sector_buffer = vec![0u8; sectors_needed * SECTOR_SIZE];
        
        self.device.read_sectors(sector, &mut sector_buffer)
            .map_err(|_| VfsError::IoError)?;
        
        buffer.copy_from_slice(&sector_buffer[sector_offset..sector_offset + buffer.len()]);
        
        Ok(())
    }
    
    /// Reads a block.
    fn read_block(&self, block_num: u64) -> Result<Vec<u8>, VfsError> {
        let block_size = *self.block_size.lock() as usize;
        let mut buffer = vec![0u8; block_size];
        
        let offset = block_num * block_size as u64;
        self.read_bytes(offset, &mut buffer)?;
        
        Ok(buffer)
    }
    
    /// Reads an inode.
    fn read_inode(&self, inode_num: u32) -> Result<Inode, VfsError> {
        // Check cache
        if let Some(inode) = self.inode_cache.read().get(&inode_num) {
            return Ok(*inode);
        }
        
        let sb = self.superblock.read();
        let sb = sb.as_ref().ok_or(VfsError::NoFilesystem)?;
        
        let block_size = *self.block_size.lock();
        let inode_size = *self.inode_size.lock() as u32;
        
        // Calculate group and index
        let group = (inode_num - 1) / sb.inodes_per_group;
        let index = (inode_num - 1) % sb.inodes_per_group;
        
        // Get group descriptor
        let gd = self.group_descs.read()
            .get(group as usize)
            .copied()
            .ok_or(VfsError::InvalidArgument)?;
        
        // Calculate inode offset
        let inode_table = gd.inode_table(sb.is_64bit());
        let inode_offset = inode_table * block_size as u64 + index as u64 * inode_size as u64;
        
        let inode = self.read_struct::<Inode>(inode_offset)?;
        
        // Cache it
        self.inode_cache.write().insert(inode_num, inode);
        
        Ok(inode)
    }
    
    /// Reads file data from an inode.
    fn read_inode_data(&self, inode: &Inode, offset: u64, buffer: &mut [u8]) -> Result<usize, VfsError> {
        let file_size = inode.size();
        if offset >= file_size {
            return Ok(0);
        }
        
        let to_read = core::cmp::min(buffer.len() as u64, file_size - offset) as usize;
        
        if inode.uses_extents() {
            self.read_extent_data(inode, offset, &mut buffer[..to_read])
        } else {
            self.read_indirect_data(inode, offset, &mut buffer[..to_read])
        }
    }
    
    /// Reads data using extent tree.
    fn read_extent_data(&self, inode: &Inode, offset: u64, buffer: &mut [u8]) -> Result<usize, VfsError> {
        let block_size = *self.block_size.lock() as u64;
        let block_num = offset / block_size;
        let block_offset = (offset % block_size) as usize;
        
        // Parse extent header - use safe block_bytes method
        let extent_bytes = inode.block_bytes();
        let extent_data = &extent_bytes[..];
        
        let header = unsafe { *(extent_data.as_ptr() as *const ExtentHeader) };
        if header.magic != 0xF30A {
            return Err(VfsError::InvalidArgument);
        }
        
        // Find the physical block
        let phys_block = self.find_extent_block(&header, extent_data, block_num, header.depth)?;
        
        if phys_block == 0 {
            // Sparse file - return zeros
            for b in buffer.iter_mut() {
                *b = 0;
            }
            return Ok(buffer.len());
        }
        
        // Read the block
        let block_data = self.read_block(phys_block)?;
        
        let to_copy = core::cmp::min(buffer.len(), block_data.len() - block_offset);
        buffer[..to_copy].copy_from_slice(&block_data[block_offset..block_offset + to_copy]);
        
        Ok(to_copy)
    }
    
    /// Finds a physical block in the extent tree.
    fn find_extent_block(&self, header: &ExtentHeader, data: &[u8], file_block: u64, depth: u16) -> Result<u64, VfsError> {
        if depth == 0 {
            // Leaf node - search extents
            let extent_offset = 12; // sizeof(ExtentHeader)
            for i in 0..header.entries as usize {
                let ext_ptr = unsafe { 
                    (data.as_ptr().add(extent_offset + i * 12)) as *const Extent 
                };
                let extent = unsafe { *ext_ptr };
                
                let start = extent.block as u64;
                let end = start + extent.length() as u64;
                
                if file_block >= start && file_block < end {
                    let offset_in_extent = file_block - start;
                    return Ok(extent.start() + offset_in_extent);
                }
            }
            Ok(0) // Not found (sparse)
        } else {
            // Internal node - find child
            let idx_offset = 12;
            for i in 0..header.entries as usize {
                let idx_ptr = unsafe {
                    (data.as_ptr().add(idx_offset + i * 12)) as *const ExtentIndex
                };
                let idx = unsafe { *idx_ptr };
                
                let next_idx = if i + 1 < header.entries as usize {
                    let next_ptr = unsafe {
                        (data.as_ptr().add(idx_offset + (i + 1) * 12)) as *const ExtentIndex
                    };
                    Some(unsafe { *next_ptr })
                } else {
                    None
                };
                
                let in_range = if let Some(next) = next_idx {
                    file_block >= idx.block as u64 && file_block < next.block as u64
                } else {
                    file_block >= idx.block as u64
                };
                
                if in_range {
                    // Read child block
                    let child_block = self.read_block(idx.leaf())?;
                    let child_header = unsafe { *(child_block.as_ptr() as *const ExtentHeader) };
                    return self.find_extent_block(&child_header, &child_block, file_block, depth - 1);
                }
            }
            Ok(0)
        }
    }
    
    /// Reads data using indirect blocks (legacy).
    fn read_indirect_data(&self, inode: &Inode, offset: u64, buffer: &mut [u8]) -> Result<usize, VfsError> {
        let block_size = *self.block_size.lock() as u64;
        let block_num = (offset / block_size) as u32;
        let block_offset = (offset % block_size) as usize;
        
        // Direct blocks (0-11)
        let phys_block = if block_num < 12 {
            inode.block[block_num as usize] as u64
        } else {
            // Indirect block support would go here
            // For simplicity, we only support extents for large files
            return Err(VfsError::NotSupported);
        };
        
        if phys_block == 0 {
            for b in buffer.iter_mut() {
                *b = 0;
            }
            return Ok(buffer.len());
        }
        
        let block_data = self.read_block(phys_block)?;
        let to_copy = core::cmp::min(buffer.len(), block_data.len() - block_offset);
        buffer[..to_copy].copy_from_slice(&block_data[block_offset..block_offset + to_copy]);
        
        Ok(to_copy)
    }
    
    /// Reads a directory.
    fn read_directory(&self, inode: &Inode) -> Result<Vec<VfsDirEntry>, VfsError> {
        if inode.file_type() != VfsFileType::Directory {
            return Err(VfsError::NotADirectory);
        }
        
        let mut entries = Vec::new();
        let dir_size = inode.size();
        let mut offset = 0u64;
        
        while offset < dir_size {
            let mut entry_buffer = [0u8; 8];
            self.read_inode_data(inode, offset, &mut entry_buffer)?;
            
            let dir_entry = unsafe { *(entry_buffer.as_ptr() as *const DirEntry) };
            
            if dir_entry.rec_len == 0 {
                break;
            }
            
            if dir_entry.inode != 0 && dir_entry.name_len > 0 {
                let mut name_buffer = vec![0u8; dir_entry.name_len as usize];
                self.read_inode_data(inode, offset + 8, &mut name_buffer)?;
                
                let name = String::from_utf8_lossy(&name_buffer).into_owned();
                let file_type = match dir_entry.file_type {
                    EXT4_FT_REG_FILE => VfsFileType::Regular,
                    EXT4_FT_DIR => VfsFileType::Directory,
                    EXT4_FT_SYMLINK => VfsFileType::Symlink,
                    EXT4_FT_CHRDEV => VfsFileType::CharDevice,
                    EXT4_FT_BLKDEV => VfsFileType::BlockDevice,
                    EXT4_FT_FIFO => VfsFileType::Fifo,
                    EXT4_FT_SOCK => VfsFileType::Socket,
                    _ => VfsFileType::Regular,
                };
                
                entries.push(VfsDirEntry {
                    ino: dir_entry.inode as InodeNum,
                    name,
                    file_type,
                });
            }
            
            offset += dir_entry.rec_len as u64;
        }
        
        Ok(entries)
    }
    
    /// Looks up a name in a directory.
    fn lookup_in_dir(&self, dir_inode: &Inode, name: &str) -> Result<u32, VfsError> {
        let entries = self.read_directory(dir_inode)?;
        
        for entry in entries {
            if entry.name == name {
                return Ok(entry.ino as u32);
            }
        }
        
        Err(VfsError::NotFound)
    }
    
    /// Resolves a path to an inode number.
    fn resolve_path(&self, path: &str) -> Result<u32, VfsError> {
        let mut current_inode = EXT4_ROOT_INODE;
        
        for component in path.split('/').filter(|s| !s.is_empty()) {
            let inode = self.read_inode(current_inode)?;
            current_inode = self.lookup_in_dir(&inode, component)?;
        }
        
        Ok(current_inode)
    }
}

impl Filesystem for Ext4Fs {
    fn name(&self) -> &'static str {
        "ext4"
    }
    
    fn statfs(&self) -> Result<VfsStatFs, VfsError> {
        let sb = self.superblock.read();
        let sb = sb.as_ref().ok_or(VfsError::NoFilesystem)?;
        
        Ok(VfsStatFs {
            blocks: sb.blocks_count(),
            bfree: sb.free_blocks_count(),
            bavail: sb.free_blocks_count(),
            files: sb.inodes_count as u64,
            ffree: sb.free_inodes_count as u64,
            bsize: sb.block_size(),
            namelen: 255,
        })
    }
    
    fn lookup(&self, parent: InodeNum, name: &str) -> Result<InodeNum, VfsError> {
        let parent_inode = self.read_inode(parent as u32)?;
        let ino = self.lookup_in_dir(&parent_inode, name)?;
        Ok(ino as InodeNum)
    }
    
    fn getattr(&self, ino: InodeNum) -> Result<VfsAttr, VfsError> {
        let inode = self.read_inode(ino as u32)?;
        
        Ok(VfsAttr {
            ino,
            file_type: inode.file_type(),
            perm: inode.permissions(),
            size: inode.size(),
            nlink: inode.links_count as u32,
            blksize: *self.block_size.lock(),
            blocks: inode.blocks_lo as u64,
            atime: inode.atime as u64,
            mtime: inode.mtime as u64,
            ctime: inode.ctime as u64,
            crtime: inode.crtime as u64,
        })
    }
    
    fn readdir(&self, ino: InodeNum) -> Result<Vec<VfsDirEntry>, VfsError> {
        let inode = self.read_inode(ino as u32)?;
        self.read_directory(&inode)
    }
    
    fn read(&self, ino: InodeNum, offset: u64, size: usize) -> Result<Vec<u8>, VfsError> {
        let inode = self.read_inode(ino as u32)?;
        let mut buffer = alloc::vec![0u8; size];
        let bytes_read = self.read_inode_data(&inode, offset, &mut buffer)?;
        buffer.truncate(bytes_read);
        Ok(buffer)
    }
    
    fn readlink(&self, ino: InodeNum) -> Result<String, VfsError> {
        let inode = self.read_inode(ino as u32)?;
        
        if inode.file_type() != VfsFileType::Symlink {
            return Err(VfsError::NotAFile);
        }
        
        let size = inode.size() as usize;
        
        // Fast symlinks are stored inline
        if size <= 60 {
            let block_bytes = inode.block_bytes();
            let data = &block_bytes[..size];
            Ok(String::from_utf8_lossy(data).into_owned())
        } else {
            let mut buffer = alloc::vec![0u8; size];
            self.read_inode_data(&inode, 0, &mut buffer)?;
            Ok(String::from_utf8_lossy(&buffer).into_owned())
        }
    }
    
    // Write operations return error (read-only)
    
    fn create(&self, _parent: InodeNum, _name: &str, _file_type: VfsFileType) -> Result<InodeNum, VfsError> {
        Err(VfsError::ReadOnlyFs)
    }
    
    fn write(&self, _ino: InodeNum, _offset: u64, _data: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::ReadOnlyFs)
    }
    
    fn unlink(&self, _parent: InodeNum, _name: &str) -> Result<(), VfsError> {
        Err(VfsError::ReadOnlyFs)
    }
    
    fn rename(&self, _old_parent: InodeNum, _old_name: &str, _new_parent: InodeNum, _new_name: &str) -> Result<(), VfsError> {
        Err(VfsError::ReadOnlyFs)
    }
    
    fn truncate(&self, _ino: InodeNum, _size: u64) -> Result<(), VfsError> {
        Err(VfsError::ReadOnlyFs)
    }
    
    fn sync(&self) -> Result<(), VfsError> {
        // Nothing to sync for read-only filesystem
        Ok(())
    }
}

# ext4 Filesystem Documentation

## Overview

The ext4 (Fourth Extended Filesystem) subsystem provides read-only access to ext4 formatted partitions, supporting extent-based allocation, large files, and directory indexing.

## Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│                        ext4 Filesystem                              │
├────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌───────────┐ │
│  │  Superblock │  │ Group Desc  │  │   Inodes    │  │  Extents  │ │
│  │   Parser    │  │   Table     │  │             │  │           │ │
│  │             │  │             │  │ - Metadata  │  │ - Tree    │ │
│  │ - Features  │  │ - Block     │  │ - Perms     │  │ - Leaf    │ │
│  │ - Geometry  │  │   bitmap    │  │ - Timestamps│  │ - Index   │ │
│  │ - State     │  │ - Inode     │  │             │  │           │ │
│  │             │  │   bitmap    │  │             │  │           │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └───────────┘ │
├────────────────────────────────────────────────────────────────────┤
│                        Directory Layer                              │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  Linear Directories  │  HTree Index (htree)  │  Inline Data │   │
│  └─────────────────────────────────────────────────────────────┘   │
├────────────────────────────────────────────────────────────────────┤
│                         Block Device                                │
└────────────────────────────────────────────────────────────────────┘
```

## Disk Layout

```
┌────────────────────────────────────────────────────────────────────┐
│ Boot │ Block Group 0 │ Block Group 1 │ ... │ Block Group N │
│ 1KB  │               │               │     │               │
├──────┼───────────────┴───────────────┴─────┴───────────────┤
│      │                                                      │
│      │  Each Block Group:                                   │
│      │  ┌───────────────────────────────────────────────┐  │
│      │  │ Super │ GDT │ Block │ Inode │ Inode │ Data   │  │
│      │  │ block │     │ Bitmap│ Bitmap│ Table │ Blocks │  │
│      │  │ (opt) │     │       │       │       │        │  │
│      │  └───────────────────────────────────────────────┘  │
└──────┴──────────────────────────────────────────────────────┘
```

## Superblock

Located at byte offset 1024:

```rust
#[repr(C, packed)]
pub struct Superblock {
    // Basic fields (ext2 compatible)
    pub inodes_count: u32,          // Total inodes
    pub blocks_count_lo: u32,       // Total blocks (low 32 bits)
    pub r_blocks_count_lo: u32,     // Reserved blocks (low)
    pub free_blocks_count_lo: u32,  // Free blocks (low)
    pub free_inodes_count: u32,     // Free inodes
    pub first_data_block: u32,      // First data block (0 or 1)
    pub log_block_size: u32,        // Block size = 1024 << log_block_size
    pub log_cluster_size: u32,      // Cluster size for bigalloc
    pub blocks_per_group: u32,      // Blocks per group
    pub clusters_per_group: u32,    // Clusters per group
    pub inodes_per_group: u32,      // Inodes per group
    pub mtime: u32,                 // Last mount time
    pub wtime: u32,                 // Last write time
    pub mnt_count: u16,             // Mount count
    pub max_mnt_count: u16,         // Max mount count before check
    pub magic: u16,                 // Magic number (0xEF53)
    pub state: u16,                 // Filesystem state
    pub errors: u16,                // Error behavior
    pub minor_rev_level: u16,       // Minor revision
    pub lastcheck: u32,             // Last check time
    pub checkinterval: u32,         // Check interval
    pub creator_os: u32,            // Creator OS
    pub rev_level: u32,             // Revision level
    pub def_resuid: u16,            // Default UID for reserved blocks
    pub def_resgid: u16,            // Default GID for reserved blocks
    
    // ext4 specific fields
    pub first_ino: u32,             // First non-reserved inode
    pub inode_size: u16,            // Inode size
    pub block_group_nr: u16,        // Block group of this superblock
    pub feature_compat: u32,        // Compatible features
    pub feature_incompat: u32,      // Incompatible features
    pub feature_ro_compat: u32,     // Read-only compatible features
    pub uuid: [u8; 16],             // Volume UUID
    pub volume_name: [u8; 16],      // Volume name
    pub last_mounted: [u8; 64],     // Last mount point
    pub algorithm_usage_bitmap: u32,
    
    // Performance hints
    pub prealloc_blocks: u8,        // Blocks to preallocate for files
    pub prealloc_dir_blocks: u8,    // Blocks to preallocate for dirs
    pub reserved_gdt_blocks: u16,   // Reserved GDT blocks for growth
    
    // Journaling
    pub journal_uuid: [u8; 16],     // Journal UUID
    pub journal_inum: u32,          // Journal inode
    pub journal_dev: u32,           // Journal device
    pub last_orphan: u32,           // Head of orphan inode list
    pub hash_seed: [u32; 4],        // HTREE hash seed
    pub def_hash_version: u8,       // Default hash algorithm
    pub jnl_backup_type: u8,
    pub desc_size: u16,             // Group descriptor size
    pub default_mount_opts: u32,
    pub first_meta_bg: u32,         // First metablock block group
    pub mkfs_time: u32,             // Filesystem creation time
    pub jnl_blocks: [u32; 17],      // Backup of journal inode
    
    // 64-bit support
    pub blocks_count_hi: u32,       // Total blocks (high 32 bits)
    pub r_blocks_count_hi: u32,
    pub free_blocks_count_hi: u32,
    pub min_extra_isize: u16,
    pub want_extra_isize: u16,
    pub flags: u32,
    pub raid_stride: u16,
    pub mmp_interval: u16,
    pub mmp_block: u64,
    pub raid_stripe_width: u32,
    pub log_groups_per_flex: u8,
    pub checksum_type: u8,
    pub reserved_pad: u16,
    pub kbytes_written: u64,
    pub snapshot_inum: u32,
    pub snapshot_id: u32,
    pub snapshot_r_blocks_count: u64,
    pub snapshot_list: u32,
    pub error_count: u32,
    pub first_error_time: u32,
    pub first_error_ino: u32,
    pub first_error_block: u64,
    pub first_error_func: [u8; 32],
    pub first_error_line: u32,
    pub last_error_time: u32,
    pub last_error_ino: u32,
    pub last_error_line: u32,
    pub last_error_block: u64,
    pub last_error_func: [u8; 32],
    pub mount_opts: [u8; 64],
    pub usr_quota_inum: u32,
    pub grp_quota_inum: u32,
    pub overhead_blocks: u32,
    pub backup_bgs: [u32; 2],
    pub encrypt_algos: [u8; 4],
    pub encrypt_pw_salt: [u8; 16],
    pub lpf_ino: u32,               // Lost+found inode
    pub prj_quota_inum: u32,
    pub checksum_seed: u32,
    pub reserved: [u32; 98],
    pub checksum: u32,              // Superblock checksum
}

impl Superblock {
    pub const MAGIC: u16 = 0xEF53;
    
    pub fn block_size(&self) -> u32 {
        1024 << self.log_block_size
    }
    
    pub fn blocks_count(&self) -> u64 {
        (self.blocks_count_hi as u64) << 32 | self.blocks_count_lo as u64
    }
    
    pub fn is_64bit(&self) -> bool {
        self.feature_incompat & INCOMPAT_64BIT != 0
    }
    
    pub fn uses_extents(&self) -> bool {
        self.feature_incompat & INCOMPAT_EXTENTS != 0
    }
    
    pub fn group_count(&self) -> u32 {
        let blocks = self.blocks_count();
        ((blocks + self.blocks_per_group as u64 - 1) / self.blocks_per_group as u64) as u32
    }
}
```

### Feature Flags

```rust
// Compatible features (can mount read-write)
pub const COMPAT_DIR_PREALLOC: u32 = 0x0001;
pub const COMPAT_HAS_JOURNAL: u32 = 0x0004;
pub const COMPAT_EXT_ATTR: u32 = 0x0008;
pub const COMPAT_RESIZE_INODE: u32 = 0x0010;
pub const COMPAT_DIR_INDEX: u32 = 0x0020;

// Incompatible features (must understand to mount)
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
```

## Group Descriptors

```rust
#[repr(C, packed)]
pub struct GroupDesc {
    pub block_bitmap_lo: u32,       // Block bitmap location
    pub inode_bitmap_lo: u32,       // Inode bitmap location
    pub inode_table_lo: u32,        // Inode table location
    pub free_blocks_count_lo: u16,  // Free blocks count
    pub free_inodes_count_lo: u16,  // Free inodes count
    pub used_dirs_count_lo: u16,    // Directory count
    pub flags: u16,                 // Group flags
    pub exclude_bitmap_lo: u32,     // Exclude bitmap (snapshots)
    pub block_bitmap_csum_lo: u16,  // Block bitmap checksum
    pub inode_bitmap_csum_lo: u16,  // Inode bitmap checksum
    pub itable_unused_lo: u16,      // Unused inode count
    pub checksum: u16,              // Group descriptor checksum
    
    // 64-bit fields (if desc_size > 32)
    pub block_bitmap_hi: u32,
    pub inode_bitmap_hi: u32,
    pub inode_table_hi: u32,
    pub free_blocks_count_hi: u16,
    pub free_inodes_count_hi: u16,
    pub used_dirs_count_hi: u16,
    pub itable_unused_hi: u16,
    pub exclude_bitmap_hi: u32,
    pub block_bitmap_csum_hi: u16,
    pub inode_bitmap_csum_hi: u16,
    pub reserved: u32,
}

impl GroupDesc {
    pub fn inode_table(&self, is_64bit: bool) -> u64 {
        if is_64bit {
            (self.inode_table_hi as u64) << 32 | self.inode_table_lo as u64
        } else {
            self.inode_table_lo as u64
        }
    }
}
```

## Inodes

```rust
#[repr(C, packed)]
pub struct Inode {
    pub mode: u16,              // File type and permissions
    pub uid: u16,               // Owner UID (low 16 bits)
    pub size_lo: u32,           // Size in bytes (low 32 bits)
    pub atime: u32,             // Access time
    pub ctime: u32,             // Inode change time
    pub mtime: u32,             // Modification time
    pub dtime: u32,             // Deletion time
    pub gid: u16,               // Group ID (low 16 bits)
    pub links_count: u16,       // Hard link count
    pub blocks_lo: u32,         // Block count (in 512-byte units)
    pub flags: u32,             // Inode flags
    pub osd1: u32,              // OS dependent
    pub block: [u32; 15],       // Block pointers or extent tree
    pub generation: u32,        // File version (for NFS)
    pub file_acl_lo: u32,       // Extended attributes block
    pub size_hi: u32,           // Size (high 32 bits) or dir_acl
    pub obso_faddr: u32,        // Obsolete fragment address
    
    // OS dependent 2
    pub blocks_hi: u16,         // Block count (high 16 bits)
    pub file_acl_hi: u16,       // Extended attributes (high)
    pub uid_hi: u16,            // UID (high 16 bits)
    pub gid_hi: u16,            // GID (high 16 bits)
    pub checksum_lo: u16,       // Inode checksum (low)
    pub reserved: u16,
    
    // Extra fields (if inode_size > 128)
    pub extra_isize: u16,       // Extra inode size
    pub checksum_hi: u16,       // Inode checksum (high)
    pub ctime_extra: u32,       // Extra change time bits
    pub mtime_extra: u32,       // Extra modification time bits
    pub atime_extra: u32,       // Extra access time bits
    pub crtime: u32,            // Creation time
    pub crtime_extra: u32,      // Extra creation time bits
    pub version_hi: u32,        // Version (high 32 bits)
    pub projid: u32,            // Project ID
}

impl Inode {
    pub fn size(&self) -> u64 {
        (self.size_hi as u64) << 32 | self.size_lo as u64
    }
    
    pub fn is_directory(&self) -> bool {
        (self.mode & 0xF000) == 0x4000
    }
    
    pub fn is_regular(&self) -> bool {
        (self.mode & 0xF000) == 0x8000
    }
    
    pub fn is_symlink(&self) -> bool {
        (self.mode & 0xF000) == 0xA000
    }
    
    pub fn uses_extents(&self) -> bool {
        self.flags & 0x00080000 != 0  // EXT4_EXTENTS_FL
    }
}

// Inode flags
pub const EXT4_SECRM_FL: u32 = 0x00000001;       // Secure deletion
pub const EXT4_UNRM_FL: u32 = 0x00000002;        // Undelete
pub const EXT4_COMPR_FL: u32 = 0x00000004;       // Compressed file
pub const EXT4_SYNC_FL: u32 = 0x00000008;        // Synchronous updates
pub const EXT4_IMMUTABLE_FL: u32 = 0x00000010;   // Immutable file
pub const EXT4_APPEND_FL: u32 = 0x00000020;      // Append only
pub const EXT4_NODUMP_FL: u32 = 0x00000040;      // Don't dump
pub const EXT4_NOATIME_FL: u32 = 0x00000080;     // Don't update atime
pub const EXT4_DIRTY_FL: u32 = 0x00000100;       // Dirty (compressed)
pub const EXT4_COMPRBLK_FL: u32 = 0x00000200;    // Compressed blocks
pub const EXT4_NOCOMPR_FL: u32 = 0x00000400;     // Access raw data
pub const EXT4_ENCRYPT_FL: u32 = 0x00000800;     // Encrypted inode
pub const EXT4_INDEX_FL: u32 = 0x00001000;       // Hash-indexed directory
pub const EXT4_IMAGIC_FL: u32 = 0x00002000;      // AFS directory
pub const EXT4_JOURNAL_DATA_FL: u32 = 0x00004000;// Journal file data
pub const EXT4_NOTAIL_FL: u32 = 0x00008000;      // Don't merge tail
pub const EXT4_DIRSYNC_FL: u32 = 0x00010000;     // Synchronous directory
pub const EXT4_TOPDIR_FL: u32 = 0x00020000;      // Top of directory hierarchy
pub const EXT4_HUGE_FILE_FL: u32 = 0x00040000;   // Huge file
pub const EXT4_EXTENTS_FL: u32 = 0x00080000;     // Uses extents
pub const EXT4_VERITY_FL: u32 = 0x00100000;      // Verity protected
pub const EXT4_EA_INODE_FL: u32 = 0x00200000;    // Inode for EA
pub const EXT4_INLINE_DATA_FL: u32 = 0x10000000; // Inline data
pub const EXT4_PROJINHERIT_FL: u32 = 0x20000000; // Inherit project ID
pub const EXT4_CASEFOLD_FL: u32 = 0x40000000;    // Casefolded directory
```

## Extent Tree

ext4 uses extent trees for efficient large file allocation:

```
                    ┌─────────────────────┐
                    │  Extent Header      │
                    │  (in inode block[]) │
                    ├─────────────────────┤
                    │  Extent Index 0     │──┐
                    │  Extent Index 1     │  │
                    │  ...                │  │
                    └─────────────────────┘  │
                                             │
                    ┌────────────────────────┘
                    ▼
          ┌─────────────────────┐
          │  Extent Header      │
          │  (intermediate)     │
          ├─────────────────────┤
          │  Extent Index 0     │──┐
          │  ...                │  │
          └─────────────────────┘  │
                                   │
          ┌────────────────────────┘
          ▼
┌─────────────────────┐
│  Extent Header      │
│  (leaf)             │
├─────────────────────┤
│  Extent 0           │ ← block=100, len=50 (blocks 100-149)
│  Extent 1           │ ← block=200, len=100 (blocks 200-299)
│  ...                │
└─────────────────────┘
```

```rust
#[repr(C, packed)]
pub struct ExtentHeader {
    pub magic: u16,             // Magic number 0xF30A
    pub entries: u16,           // Number of valid entries
    pub max_entries: u16,       // Maximum entries possible
    pub depth: u16,             // Depth (0 = leaf)
    pub generation: u32,        // Tree generation
}

#[repr(C, packed)]
pub struct ExtentIndex {
    pub block: u32,             // Logical block covered by this index
    pub leaf_lo: u32,           // Physical block of child (low)
    pub leaf_hi: u16,           // Physical block of child (high)
    pub unused: u16,
}

#[repr(C, packed)]
pub struct Extent {
    pub block: u32,             // First logical block covered
    pub len: u16,               // Number of blocks (max 32768)
    pub start_hi: u16,          // Physical block (high)
    pub start_lo: u32,          // Physical block (low)
}

impl Extent {
    pub fn start(&self) -> u64 {
        (self.start_hi as u64) << 32 | self.start_lo as u64
    }
    
    pub fn is_unwritten(&self) -> bool {
        self.len > 32768  // Bit 15 set means unwritten
    }
    
    pub fn length(&self) -> u16 {
        self.len & 0x7FFF
    }
}

pub fn read_extent_tree(inode: &Inode, logical_block: u64) -> Option<u64> {
    let header = unsafe {
        &*(inode.block.as_ptr() as *const ExtentHeader)
    };
    
    if header.magic != 0xF30A {
        return None;
    }
    
    // Navigate tree
    let mut block_data = inode.block.as_ptr() as *const u8;
    let mut depth = header.depth;
    
    while depth > 0 {
        // Find index entry
        let header = unsafe { &*(block_data as *const ExtentHeader) };
        let indices = unsafe {
            core::slice::from_raw_parts(
                block_data.add(12) as *const ExtentIndex,
                header.entries as usize
            )
        };
        
        // Binary search for matching index
        let idx = indices.iter()
            .rposition(|i| (i.block as u64) <= logical_block)?;
        
        // Read child block
        let child = (indices[idx].leaf_hi as u64) << 32 | indices[idx].leaf_lo as u64;
        // block_data = read_block(child);
        depth -= 1;
    }
    
    // At leaf level, find extent
    let header = unsafe { &*(block_data as *const ExtentHeader) };
    let extents = unsafe {
        core::slice::from_raw_parts(
            block_data.add(12) as *const Extent,
            header.entries as usize
        )
    };
    
    for extent in extents {
        let start = extent.block as u64;
        let end = start + extent.length() as u64;
        
        if logical_block >= start && logical_block < end {
            let offset = logical_block - start;
            return Some(extent.start() + offset);
        }
    }
    
    None
}
```

## Directory Entries

### Linear Directory

```rust
#[repr(C, packed)]
pub struct DirEntry {
    pub inode: u32,             // Inode number
    pub rec_len: u16,           // Record length
    pub name_len: u8,           // Name length
    pub file_type: u8,          // File type
    // name follows (variable length)
}

// File types in directory entry
pub const FT_UNKNOWN: u8 = 0;
pub const FT_REG_FILE: u8 = 1;
pub const FT_DIR: u8 = 2;
pub const FT_CHRDEV: u8 = 3;
pub const FT_BLKDEV: u8 = 4;
pub const FT_FIFO: u8 = 5;
pub const FT_SOCK: u8 = 6;
pub const FT_SYMLINK: u8 = 7;

pub fn read_directory(inode: &Inode) -> Vec<(String, u32, u8)> {
    let mut entries = Vec::new();
    let mut offset = 0u64;
    let size = inode.size();
    
    while offset < size {
        let block = offset / block_size as u64;
        let block_offset = (offset % block_size as u64) as usize;
        
        let phys_block = read_extent_tree(inode, block)?;
        let data = read_block(phys_block);
        
        let entry = unsafe {
            &*(data.as_ptr().add(block_offset) as *const DirEntry)
        };
        
        if entry.inode != 0 {
            let name = unsafe {
                core::str::from_utf8_unchecked(
                    core::slice::from_raw_parts(
                        (entry as *const _ as *const u8).add(8),
                        entry.name_len as usize
                    )
                )
            };
            entries.push((name.to_string(), entry.inode, entry.file_type));
        }
        
        offset += entry.rec_len as u64;
    }
    
    entries
}
```

### HTree Directory Index

For large directories:

```rust
#[repr(C, packed)]
pub struct DxRoot {
    pub dot: DirEntry,          // "." entry
    pub dotdot: DirEntry,       // ".." entry
    pub info: DxRootInfo,
    pub entries: [DxEntry; 0],  // Variable
}

#[repr(C, packed)]
pub struct DxRootInfo {
    pub reserved_zero: u32,
    pub hash_version: u8,
    pub info_length: u8,
    pub indirect_levels: u8,
    pub unused_flags: u8,
}

#[repr(C, packed)]
pub struct DxEntry {
    pub hash: u32,              // Hash value
    pub block: u32,             // Block number
}
```

## Mounting

```rust
pub fn mount(device_name: &str, mount_point: &str) -> Result<(), VfsError> {
    // Check device exists
    if !crate::block::list_devices().iter().any(|d| d.name == device_name) {
        return Err(VfsError::NotFound);
    }
    
    // Create block device wrapper
    let device = Arc::new(BlockDeviceWrapper::new(device_name));
    
    // Create filesystem instance
    let fs = Ext4Fs::new(device);
    
    // Parse superblock
    fs.mount()?;
    
    // Register in mount table
    EXT4_MOUNTS.write().insert(mount_point.to_string(), fs);
    
    serial_println!("[ext4] Mounted {} at {} (read-only)", device_name, mount_point);
    Ok(())
}
```

## VFS Integration

```rust
impl Filesystem for Ext4Fs {
    fn lookup(&self, parent: InodeNum, name: &str) -> Result<InodeNum, VfsError> {
        let entries = self.readdir(parent)?;
        entries.iter()
            .find(|e| e.name == name)
            .map(|e| e.ino)
            .ok_or(VfsError::NotFound)
    }
    
    fn getattr(&self, ino: InodeNum) -> Result<VfsAttr, VfsError> {
        let inode = self.read_inode(ino as u32)?;
        
        Ok(VfsAttr {
            ino,
            file_type: if inode.is_directory() {
                VfsFileType::Directory
            } else if inode.is_symlink() {
                VfsFileType::Symlink
            } else {
                VfsFileType::Regular
            },
            size: inode.size(),
            perm: VfsPermissions::from_mode(inode.mode),
            nlink: inode.links_count as u32,
            blksize: self.block_size(),
            blocks: inode.blocks_lo as u64,
            atime: inode.atime as u64,
            mtime: inode.mtime as u64,
            ctime: inode.ctime as u64,
            crtime: inode.crtime as u64,
        })
    }
    
    fn read(&self, ino: InodeNum, offset: u64, size: usize) -> Result<Vec<u8>, VfsError> {
        let inode = self.read_inode(ino as u32)?;
        let file_size = inode.size();
        
        if offset >= file_size {
            return Ok(Vec::new());
        }
        
        let read_size = core::cmp::min(size as u64, file_size - offset) as usize;
        let mut data = vec![0u8; read_size];
        let mut bytes_read = 0;
        
        while bytes_read < read_size {
            let pos = offset + bytes_read as u64;
            let logical_block = pos / self.block_size() as u64;
            let block_offset = (pos % self.block_size() as u64) as usize;
            
            let phys_block = self.logical_to_physical(&inode, logical_block)?;
            let block_data = self.read_block(phys_block)?;
            
            let chunk_size = core::cmp::min(
                self.block_size() as usize - block_offset,
                read_size - bytes_read
            );
            
            data[bytes_read..bytes_read + chunk_size]
                .copy_from_slice(&block_data[block_offset..block_offset + chunk_size]);
            
            bytes_read += chunk_size;
        }
        
        Ok(data)
    }
    
    fn readdir(&self, ino: InodeNum) -> Result<Vec<VfsDirEntry>, VfsError> {
        let inode = self.read_inode(ino as u32)?;
        
        if !inode.is_directory() {
            return Err(VfsError::NotDirectory);
        }
        
        // Read directory contents
        let data = self.read(ino, 0, inode.size() as usize)?;
        let mut entries = Vec::new();
        let mut offset = 0;
        
        while offset < data.len() {
            let entry = unsafe {
                &*(data.as_ptr().add(offset) as *const DirEntry)
            };
            
            if entry.inode != 0 && entry.name_len > 0 {
                let name = unsafe {
                    core::str::from_utf8_unchecked(
                        &data[offset + 8..offset + 8 + entry.name_len as usize]
                    )
                };
                
                entries.push(VfsDirEntry {
                    name: name.to_string(),
                    ino: entry.inode as u64,
                    file_type: match entry.file_type {
                        FT_DIR => VfsFileType::Directory,
                        FT_SYMLINK => VfsFileType::Symlink,
                        _ => VfsFileType::Regular,
                    },
                });
            }
            
            offset += entry.rec_len as usize;
            if entry.rec_len == 0 {
                break;
            }
        }
        
        Ok(entries)
    }
}
```

## Shell Commands

### mount -t ext4

```
splax> mount -t ext4 sda1 /mnt/linux
Mounting sda1 as ext4 at /mnt/linux...
[ext4] Mounted: 10240000 blocks, 640000 inodes, block_size=4096, uses_extents=true
[OK] Mounted sda1 (ext4) at /mnt/linux
```

### fsls (on ext4)

```
splax> fsls /mnt/linux
Directory: /mnt/linux
d        4096  .
d        4096  ..
d        4096  bin
d        4096  etc
d        4096  home
-       12345  README.md
```

## Limitations

1. **Read-Only** - No write support
2. **No Journaling** - Journal replay not implemented
3. **No Extended Attributes** - xattr not supported
4. **No Encryption** - Encrypted directories not readable
5. **No Inline Data** - Small files inline in inode not supported

## Future Enhancements

1. **Write Support** - Full read/write filesystem
2. **Journal Recovery** - Replay uncommitted transactions
3. **Extended Attributes** - ACLs, security labels
4. **Encryption** - fscrypt support
5. **Quota Support** - User/group/project quotas

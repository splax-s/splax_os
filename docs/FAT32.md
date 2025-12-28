# FAT32 Filesystem Documentation

## Overview

The FAT32 (File Allocation Table, 32-bit) subsystem provides read-only access to FAT32 formatted volumes, commonly used on USB drives, SD cards, and UEFI system partitions.

## Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│                        FAT32 Filesystem                             │
├────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌───────────┐ │
│  │ Boot Sector │  │    FAT      │  │  Directory  │  │   File    │ │
│  │   Parser    │  │   Table     │  │   Handler   │  │   Read    │ │
│  │             │  │             │  │             │  │           │ │
│  │ - BPB       │  │ - Cluster   │  │ - Short     │  │ - Cluster │ │
│  │ - Geometry  │  │   chains    │  │   names     │  │   chain   │ │
│  │ - FAT info  │  │ - Free      │  │ - LFN       │  │   follow  │ │
│  │             │  │   tracking  │  │ - Attrs     │  │           │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └───────────┘ │
├────────────────────────────────────────────────────────────────────┤
│                         Block Device                                │
└────────────────────────────────────────────────────────────────────┘
```

## Disk Layout

```
┌─────────────────────────────────────────────────────────────────────┐
│ Reserved    │    FAT #1    │    FAT #2    │    Data Region         │
│ Sectors     │              │   (backup)   │    (Clusters)          │
├─────────────┼──────────────┼──────────────┼────────────────────────┤
│ Boot Sector │              │              │ Cluster 2 (Root Dir)   │
│ FSInfo      │              │              │ Cluster 3              │
│ Backup Boot │              │              │ Cluster 4              │
│ ...         │              │              │ ...                    │
└─────────────┴──────────────┴──────────────┴────────────────────────┘
```

## Boot Sector (BPB)

BIOS Parameter Block at sector 0:

```rust
#[repr(C, packed)]
pub struct BootSector {
    // Common BPB (BIOS Parameter Block)
    pub jmp_boot: [u8; 3],          // Jump instruction
    pub oem_name: [u8; 8],          // OEM identifier
    pub bytes_per_sector: u16,      // Bytes per sector (usually 512)
    pub sectors_per_cluster: u8,    // Sectors per cluster (power of 2)
    pub reserved_sectors: u16,      // Reserved sector count
    pub num_fats: u8,               // Number of FATs (usually 2)
    pub root_entry_count: u16,      // Root entries (0 for FAT32)
    pub total_sectors_16: u16,      // Total sectors (0 if > 65535)
    pub media_type: u8,             // Media type (0xF8 for hard disk)
    pub fat_size_16: u16,           // FAT size (0 for FAT32)
    pub sectors_per_track: u16,     // Sectors per track
    pub num_heads: u16,             // Number of heads
    pub hidden_sectors: u32,        // Hidden sectors before partition
    pub total_sectors_32: u32,      // Total sectors if > 65535
    
    // FAT32 Extended BPB
    pub fat_size_32: u32,           // Sectors per FAT
    pub ext_flags: u16,             // FAT flags
    pub fs_version: u16,            // Version (0.0)
    pub root_cluster: u32,          // Root directory cluster
    pub fs_info: u16,               // FSInfo sector number
    pub backup_boot_sector: u16,    // Backup boot sector
    pub reserved: [u8; 12],         // Reserved
    pub drive_number: u8,           // BIOS drive number
    pub reserved1: u8,              // Reserved
    pub boot_signature: u8,         // Extended boot signature (0x29)
    pub volume_id: u32,             // Volume serial number
    pub volume_label: [u8; 11],     // Volume label
    pub fs_type: [u8; 8],           // "FAT32   "
    pub boot_code: [u8; 420],       // Boot code
    pub signature: u16,             // 0xAA55
}

impl BootSector {
    pub const SIGNATURE: u16 = 0xAA55;
    
    pub fn validate(&self) -> bool {
        self.signature == Self::SIGNATURE &&
        self.bytes_per_sector >= 512 &&
        self.bytes_per_sector <= 4096 &&
        self.sectors_per_cluster.is_power_of_two() &&
        self.num_fats >= 1 &&
        self.root_cluster >= 2
    }
    
    pub fn cluster_size(&self) -> u32 {
        self.bytes_per_sector as u32 * self.sectors_per_cluster as u32
    }
    
    pub fn fat_start_sector(&self) -> u32 {
        self.reserved_sectors as u32
    }
    
    pub fn data_start_sector(&self) -> u32 {
        self.reserved_sectors as u32 + 
        (self.num_fats as u32 * self.fat_size_32)
    }
    
    pub fn cluster_to_sector(&self, cluster: u32) -> u64 {
        let data_start = self.data_start_sector();
        (data_start + (cluster - 2) * self.sectors_per_cluster as u32) as u64
    }
    
    pub fn total_clusters(&self) -> u32 {
        let data_sectors = self.total_sectors_32 - self.data_start_sector();
        data_sectors / self.sectors_per_cluster as u32
    }
}
```

## FSInfo Structure

Located at sector indicated by `fs_info` field:

```rust
#[repr(C, packed)]
pub struct FsInfo {
    pub lead_sig: u32,              // Lead signature (0x41615252)
    pub reserved1: [u8; 480],       // Reserved
    pub struct_sig: u32,            // Structure signature (0x61417272)
    pub free_count: u32,            // Free cluster count (0xFFFFFFFF = unknown)
    pub next_free: u32,             // Next free cluster hint
    pub reserved2: [u8; 12],        // Reserved
    pub trail_sig: u32,             // Trail signature (0xAA550000)
}

impl FsInfo {
    pub const LEAD_SIG: u32 = 0x41615252;
    pub const STRUCT_SIG: u32 = 0x61417272;
    pub const TRAIL_SIG: u32 = 0xAA550000;
    
    pub fn validate(&self) -> bool {
        self.lead_sig == Self::LEAD_SIG &&
        self.struct_sig == Self::STRUCT_SIG &&
        self.trail_sig == Self::TRAIL_SIG
    }
}
```

## File Allocation Table (FAT)

The FAT is an array of 32-bit entries mapping cluster chains:

```rust
// FAT32 entry values
pub const FAT32_FREE: u32 = 0x00000000;          // Free cluster
pub const FAT32_RESERVED_START: u32 = 0x0FFFFFF0; // Reserved range start
pub const FAT32_BAD: u32 = 0x0FFFFFF7;           // Bad cluster
pub const FAT32_EOC: u32 = 0x0FFFFFF8;           // End of chain (and above)

pub struct Fat32 {
    /// Boot sector parameters
    boot_sector: BootSector,
    /// Cached FAT entries
    fat_cache: BTreeMap<u32, u32>,
    /// Device access
    device: Arc<dyn BlockDevice>,
}

impl Fat32 {
    /// Read FAT entry for a cluster
    pub fn read_fat_entry(&self, cluster: u32) -> Result<u32, VfsError> {
        // Check cache first
        if let Some(&entry) = self.fat_cache.get(&cluster) {
            return Ok(entry);
        }
        
        let fat_offset = cluster * 4;
        let fat_sector = self.boot_sector.fat_start_sector() + 
                        (fat_offset / self.boot_sector.bytes_per_sector as u32);
        let entry_offset = (fat_offset % self.boot_sector.bytes_per_sector as u32) as usize;
        
        let sector_data = self.read_sector(fat_sector as u64)?;
        let entry = u32::from_le_bytes([
            sector_data[entry_offset],
            sector_data[entry_offset + 1],
            sector_data[entry_offset + 2],
            sector_data[entry_offset + 3],
        ]) & 0x0FFFFFFF; // Mask upper 4 bits
        
        Ok(entry)
    }
    
    /// Check if cluster is end of chain
    pub fn is_eoc(&self, cluster: u32) -> bool {
        cluster >= FAT32_EOC
    }
    
    /// Follow cluster chain
    pub fn get_cluster_chain(&self, start: u32) -> Result<Vec<u32>, VfsError> {
        let mut chain = Vec::new();
        let mut current = start;
        
        while current >= 2 && current < FAT32_RESERVED_START {
            chain.push(current);
            current = self.read_fat_entry(current)?;
            
            // Safety limit
            if chain.len() > 10_000_000 {
                return Err(VfsError::Corrupted);
            }
        }
        
        Ok(chain)
    }
}
```

## Directory Entries

### Short Directory Entry (8.3 format)

```rust
#[repr(C, packed)]
pub struct DirEntry {
    pub name: [u8; 8],              // Short name (space padded)
    pub ext: [u8; 3],               // Extension (space padded)
    pub attr: u8,                   // Attributes
    pub nt_reserved: u8,            // Reserved for Windows NT
    pub create_time_tenth: u8,      // Creation time (tenths of second)
    pub create_time: u16,           // Creation time
    pub create_date: u16,           // Creation date
    pub access_date: u16,           // Last access date
    pub first_cluster_hi: u16,      // First cluster (high 16 bits)
    pub write_time: u16,            // Last write time
    pub write_date: u16,            // Last write date
    pub first_cluster_lo: u16,      // First cluster (low 16 bits)
    pub file_size: u32,             // File size in bytes
}

// Attribute flags
pub const ATTR_READ_ONLY: u8 = 0x01;
pub const ATTR_HIDDEN: u8 = 0x02;
pub const ATTR_SYSTEM: u8 = 0x04;
pub const ATTR_VOLUME_ID: u8 = 0x08;
pub const ATTR_DIRECTORY: u8 = 0x10;
pub const ATTR_ARCHIVE: u8 = 0x20;
pub const ATTR_LONG_NAME: u8 = 0x0F; // LFN entry marker

impl DirEntry {
    pub fn is_free(&self) -> bool {
        self.name[0] == 0xE5
    }
    
    pub fn is_end(&self) -> bool {
        self.name[0] == 0x00
    }
    
    pub fn is_long_name(&self) -> bool {
        self.attr == ATTR_LONG_NAME
    }
    
    pub fn is_directory(&self) -> bool {
        self.attr & ATTR_DIRECTORY != 0
    }
    
    pub fn is_volume_label(&self) -> bool {
        self.attr & ATTR_VOLUME_ID != 0
    }
    
    pub fn first_cluster(&self) -> u32 {
        ((self.first_cluster_hi as u32) << 16) | self.first_cluster_lo as u32
    }
    
    pub fn short_name(&self) -> String {
        let name = core::str::from_utf8(&self.name)
            .unwrap_or("")
            .trim_end();
        let ext = core::str::from_utf8(&self.ext)
            .unwrap_or("")
            .trim_end();
        
        if ext.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", name, ext)
        }
    }
}
```

### Long File Name (LFN) Entry

```rust
#[repr(C, packed)]
pub struct LfnEntry {
    pub order: u8,                  // Sequence number (1-20, 0x40 = last)
    pub name1: [u16; 5],            // Characters 1-5 (Unicode)
    pub attr: u8,                   // Always 0x0F
    pub entry_type: u8,             // Always 0
    pub checksum: u8,               // Short name checksum
    pub name2: [u16; 6],            // Characters 6-11
    pub first_cluster: u16,         // Always 0
    pub name3: [u16; 2],            // Characters 12-13
}

impl LfnEntry {
    pub const LAST_ENTRY: u8 = 0x40;
    
    pub fn sequence(&self) -> u8 {
        self.order & 0x3F
    }
    
    pub fn is_last(&self) -> bool {
        self.order & Self::LAST_ENTRY != 0
    }
    
    pub fn get_chars(&self) -> Vec<u16> {
        let mut chars = Vec::with_capacity(13);
        chars.extend_from_slice(&self.name1);
        chars.extend_from_slice(&self.name2);
        chars.extend_from_slice(&self.name3);
        
        // Trim at NUL terminator
        if let Some(pos) = chars.iter().position(|&c| c == 0x0000) {
            chars.truncate(pos);
        }
        
        chars
    }
}

/// Reassemble long filename from LFN entries
pub fn assemble_lfn(lfn_entries: &[LfnEntry]) -> String {
    let mut chars: Vec<u16> = Vec::new();
    
    // Process in reverse order (entries are stored backwards)
    for entry in lfn_entries.iter().rev() {
        chars.extend(entry.get_chars());
    }
    
    String::from_utf16_lossy(&chars)
}

/// Calculate LFN checksum from short name
pub fn lfn_checksum(short_name: &[u8; 11]) -> u8 {
    let mut sum: u8 = 0;
    for &b in short_name {
        sum = ((sum & 1) << 7).wrapping_add(sum >> 1).wrapping_add(b);
    }
    sum
}
```

## Date/Time Encoding

```rust
/// Parse FAT date (2-second resolution)
pub fn parse_fat_date(date: u16) -> (u16, u8, u8) {
    let day = (date & 0x1F) as u8;
    let month = ((date >> 5) & 0x0F) as u8;
    let year = ((date >> 9) & 0x7F) as u16 + 1980;
    (year, month, day)
}

/// Parse FAT time
pub fn parse_fat_time(time: u16) -> (u8, u8, u8) {
    let second = ((time & 0x1F) * 2) as u8;
    let minute = ((time >> 5) & 0x3F) as u8;
    let hour = ((time >> 11) & 0x1F) as u8;
    (hour, minute, second)
}

/// Convert FAT timestamp to Unix timestamp
pub fn fat_to_unix(date: u16, time: u16) -> u64 {
    let (year, month, day) = parse_fat_date(date);
    let (hour, minute, second) = parse_fat_time(time);
    
    // Simplified conversion (ignores leap seconds, etc.)
    let days_since_1970 = days_from_epoch(year, month, day);
    (days_since_1970 as u64 * 86400) + 
        (hour as u64 * 3600) + 
        (minute as u64 * 60) + 
        second as u64
}
```

## Reading Files

```rust
impl Fat32Fs {
    pub fn read_file(&self, cluster: u32, offset: u64, size: usize) -> Result<Vec<u8>, VfsError> {
        let cluster_size = self.boot_sector.cluster_size() as u64;
        let chain = self.get_cluster_chain(cluster)?;
        
        let mut data = Vec::with_capacity(size);
        let mut remaining = size;
        let mut file_offset = offset;
        
        for &cluster in &chain {
            if remaining == 0 {
                break;
            }
            
            // Skip clusters before our offset
            if file_offset >= cluster_size {
                file_offset -= cluster_size;
                continue;
            }
            
            // Read this cluster
            let cluster_data = self.read_cluster(cluster)?;
            let start = file_offset as usize;
            let chunk_size = core::cmp::min(
                cluster_size as usize - start,
                remaining
            );
            
            data.extend_from_slice(&cluster_data[start..start + chunk_size]);
            remaining -= chunk_size;
            file_offset = 0;
        }
        
        Ok(data)
    }
    
    fn read_cluster(&self, cluster: u32) -> Result<Vec<u8>, VfsError> {
        let sector = self.boot_sector.cluster_to_sector(cluster);
        let sectors = self.boot_sector.sectors_per_cluster as usize;
        let sector_size = self.boot_sector.bytes_per_sector as usize;
        
        let mut data = vec![0u8; sectors * sector_size];
        
        for i in 0..sectors {
            let sector_data = self.read_sector(sector + i as u64)?;
            data[i * sector_size..(i + 1) * sector_size]
                .copy_from_slice(&sector_data);
        }
        
        Ok(data)
    }
}
```

## Directory Traversal

```rust
impl Fat32Fs {
    pub fn read_directory(&self, cluster: u32) -> Result<Vec<FatDirEntry>, VfsError> {
        let cluster_size = self.boot_sector.cluster_size() as usize;
        let chain = self.get_cluster_chain(cluster)?;
        
        let mut entries = Vec::new();
        let mut lfn_entries: Vec<LfnEntry> = Vec::new();
        
        for &cluster in &chain {
            let data = self.read_cluster(cluster)?;
            let mut offset = 0;
            
            while offset < cluster_size {
                let entry = unsafe {
                    &*(data.as_ptr().add(offset) as *const DirEntry)
                };
                
                if entry.is_end() {
                    return Ok(entries);
                }
                
                if entry.is_free() {
                    lfn_entries.clear();
                    offset += 32;
                    continue;
                }
                
                if entry.is_long_name() {
                    let lfn = unsafe {
                        &*(data.as_ptr().add(offset) as *const LfnEntry)
                    };
                    lfn_entries.push(*lfn);
                } else if !entry.is_volume_label() {
                    let name = if !lfn_entries.is_empty() {
                        assemble_lfn(&lfn_entries)
                    } else {
                        entry.short_name()
                    };
                    
                    entries.push(FatDirEntry {
                        name,
                        cluster: entry.first_cluster(),
                        size: entry.file_size,
                        is_directory: entry.is_directory(),
                        attributes: entry.attr,
                        create_time: entry.create_time,
                        create_date: entry.create_date,
                        access_date: entry.access_date,
                        write_time: entry.write_time,
                        write_date: entry.write_date,
                    });
                    
                    lfn_entries.clear();
                }
                
                offset += 32;
            }
        }
        
        Ok(entries)
    }
    
    pub fn lookup(&self, parent_cluster: u32, name: &str) -> Result<u32, VfsError> {
        let entries = self.read_directory(parent_cluster)?;
        
        for entry in entries {
            if entry.name.eq_ignore_ascii_case(name) {
                return Ok(entry.cluster);
            }
        }
        
        Err(VfsError::NotFound)
    }
}
```

## Path Resolution

```rust
impl Fat32Fs {
    pub fn resolve_path(&self, path: &str) -> Result<(u32, u32), VfsError> {
        let mut current_cluster = self.boot_sector.root_cluster;
        let mut file_size = 0u32;
        let mut is_dir = true;
        
        for component in path.split('/').filter(|s| !s.is_empty()) {
            if !is_dir {
                return Err(VfsError::NotDirectory);
            }
            
            let entries = self.read_directory(current_cluster)?;
            let entry = entries.iter()
                .find(|e| e.name.eq_ignore_ascii_case(component))
                .ok_or(VfsError::NotFound)?;
            
            current_cluster = entry.cluster;
            file_size = entry.size;
            is_dir = entry.is_directory;
        }
        
        Ok((current_cluster, file_size))
    }
}
```

## VFS Integration

```rust
impl Filesystem for Fat32Fs {
    fn lookup(&self, parent: InodeNum, name: &str) -> Result<InodeNum, VfsError> {
        let parent_cluster = self.inode_to_cluster(parent)?;
        let cluster = self.lookup(parent_cluster, name)?;
        Ok(cluster as InodeNum)
    }
    
    fn getattr(&self, ino: InodeNum) -> Result<VfsAttr, VfsError> {
        let cluster = ino as u32;
        
        // For root directory
        if cluster == self.boot_sector.root_cluster {
            return Ok(VfsAttr {
                ino,
                file_type: VfsFileType::Directory,
                size: 0,
                perm: VfsPermissions::default(),
                nlink: 1,
                blksize: self.boot_sector.cluster_size(),
                blocks: 0,
                atime: 0,
                mtime: 0,
                ctime: 0,
                crtime: 0,
            });
        }
        
        // Look up in parent (cached)
        let info = self.get_entry_info(cluster)?;
        
        Ok(VfsAttr {
            ino,
            file_type: if info.is_directory {
                VfsFileType::Directory
            } else {
                VfsFileType::Regular
            },
            size: info.size as u64,
            perm: VfsPermissions::default(),
            nlink: 1,
            blksize: self.boot_sector.cluster_size(),
            blocks: (info.size as u64 + 511) / 512,
            atime: fat_to_unix(info.access_date, 0),
            mtime: fat_to_unix(info.write_date, info.write_time),
            ctime: fat_to_unix(info.write_date, info.write_time),
            crtime: fat_to_unix(info.create_date, info.create_time),
        })
    }
    
    fn read(&self, ino: InodeNum, offset: u64, size: usize) -> Result<Vec<u8>, VfsError> {
        let cluster = ino as u32;
        self.read_file(cluster, offset, size)
    }
    
    fn readdir(&self, ino: InodeNum) -> Result<Vec<VfsDirEntry>, VfsError> {
        let cluster = ino as u32;
        let entries = self.read_directory(cluster)?;
        
        Ok(entries.into_iter()
            .filter(|e| e.name != "." && e.name != "..")
            .map(|e| VfsDirEntry {
                name: e.name,
                ino: e.cluster as InodeNum,
                file_type: if e.is_directory {
                    VfsFileType::Directory
                } else {
                    VfsFileType::Regular
                },
            })
            .collect())
    }
}
```

## Shell Commands

### mount -t fat32

```
splax> mount -t fat32 sda1 /mnt/usb
Mounting sda1 as FAT32 at /mnt/usb...
[fat32] Mounted: 1024 MB, cluster_size=4096, FAT32
[OK] Mounted sda1 (fat32) at /mnt/usb
```

### fsls (on FAT32)

```
splax> fsls /mnt/usb
Directory: /mnt/usb
d        0  System Volume Information
d        0  Documents
-    12345  README.txt
-  1234567  backup.zip
```

## Case Sensitivity

FAT32 is case-insensitive but case-preserving:

```rust
impl Fat32Fs {
    fn name_matches(&self, stored: &str, search: &str) -> bool {
        stored.eq_ignore_ascii_case(search)
    }
}
```

## Special Handling

### "." and ".." Entries

```rust
// Skip meta-entries in directory listings
fn filter_meta_entries(entries: Vec<FatDirEntry>) -> Vec<FatDirEntry> {
    entries.into_iter()
        .filter(|e| e.name != "." && e.name != "..")
        .collect()
}
```

### Volume Label

```rust
pub fn get_volume_label(&self) -> Option<String> {
    let entries = self.read_directory(self.boot_sector.root_cluster).ok()?;
    
    // Look for volume label entry
    for entry in entries {
        if entry.attributes & ATTR_VOLUME_ID != 0 {
            return Some(entry.name.trim().to_string());
        }
    }
    
    // Fallback to boot sector label
    let label = core::str::from_utf8(&self.boot_sector.volume_label)
        .ok()?
        .trim();
    
    if label.is_empty() || label == "NO NAME" {
        None
    } else {
        Some(label.to_string())
    }
}
```

## Error Handling

```rust
pub enum Fat32Error {
    InvalidBootSector,
    InvalidFsInfo,
    InvalidCluster,
    ClusterChainLoop,
    CorruptedDirectory,
    PathTooLong,
    NameTooLong,
    DeviceError(BlockError),
}

impl From<Fat32Error> for VfsError {
    fn from(e: Fat32Error) -> Self {
        match e {
            Fat32Error::InvalidBootSector => VfsError::InvalidArgument,
            Fat32Error::InvalidCluster => VfsError::Corrupted,
            Fat32Error::ClusterChainLoop => VfsError::Corrupted,
            Fat32Error::CorruptedDirectory => VfsError::Corrupted,
            Fat32Error::DeviceError(_) => VfsError::IoError,
            _ => VfsError::Unknown,
        }
    }
}
```

## Limitations

1. **Read-Only** - No write support
2. **No Long Paths** - 255 character path limit
3. **No Permissions** - FAT32 has no Unix permissions
4. **No Symlinks** - Not supported by FAT32
5. **4GB File Limit** - FAT32 limitation

## Future Enhancements

1. **Write Support** - Full read/write filesystem
2. **exFAT Support** - For large files
3. **vFAT Extensions** - Full LFN support validation
4. **Bad Cluster Handling** - Mark and skip bad clusters
5. **Transaction Support** - Safe writes with FSInfo updates

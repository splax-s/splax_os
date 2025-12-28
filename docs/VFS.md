# Virtual File System (VFS)

> Comprehensive documentation for Splax OS Virtual File System architecture.

## Overview

The Splax OS VFS provides a unified interface for all filesystem operations, abstracting the underlying filesystem implementations. It supports multiple filesystem types including SplaxFS (native), RamFS, ProcFS, SysFS, DevFS, ext4, and FAT32.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      User Applications                          │
│                  open() read() write() close()                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      VFS Layer (kernel/src/fs/vfs.rs)          │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │  Mount Table     │  Open File Table   │  Path Resolution │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          ▼                   ▼                   ▼
┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐
│     SplaxFS      │ │      RamFS       │ │      ext4        │
│  (Native disk)   │ │  (In-memory)     │ │  (Read-only)     │
└──────────────────┘ └──────────────────┘ └──────────────────┘
          │                   │                   │
          ▼                   ▼                   ▼
┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐
│   Block Layer    │ │     Memory       │ │   Block Layer    │
└──────────────────┘ └──────────────────┘ └──────────────────┘
```

---

## Filesystem Trait

All filesystems implement the common `Filesystem` trait:

```rust
// kernel/src/fs/vfs.rs

pub trait Filesystem: Send + Sync {
    /// Get filesystem name
    fn name(&self) -> &str;
    
    /// Read file contents
    fn read(&self, path: &str, buf: &mut [u8]) -> Result<usize, VfsError>;
    
    /// Write to file
    fn write(&self, path: &str, buf: &[u8]) -> Result<usize, VfsError>;
    
    /// Open file with flags
    fn open(&self, path: &str, flags: OpenFlags) -> Result<FileHandle, VfsError>;
    
    /// Close file handle
    fn close(&self, handle: FileHandle) -> Result<(), VfsError>;
    
    /// Get file/directory metadata
    fn stat(&self, path: &str) -> Result<FileStat, VfsError>;
    
    /// List directory contents
    fn readdir(&self, path: &str) -> Result<Vec<DirEntry>, VfsError>;
    
    /// Create directory
    fn mkdir(&self, path: &str) -> Result<(), VfsError>;
    
    /// Remove directory
    fn rmdir(&self, path: &str) -> Result<(), VfsError>;
    
    /// Delete file
    fn unlink(&self, path: &str) -> Result<(), VfsError>;
    
    /// Create file
    fn create(&self, path: &str) -> Result<(), VfsError>;
    
    /// Rename file/directory
    fn rename(&self, old: &str, new: &str) -> Result<(), VfsError>;
    
    /// Sync filesystem to disk
    fn sync(&self) -> Result<(), VfsError>;
}
```

---

## Core Types

### File Descriptor

```rust
pub type Fd = u64;

pub struct FileDescriptor {
    /// Underlying file handle
    pub handle: FileHandle,
    /// Current read/write position
    pub position: u64,
    /// Open flags (read, write, append, etc.)
    pub flags: OpenFlags,
    /// Reference to mounted filesystem
    pub fs: Arc<dyn Filesystem>,
    /// Path within filesystem
    pub path: String,
}
```

### Open Flags

```rust
bitflags! {
    pub struct OpenFlags: u32 {
        const READ      = 0b0000_0001;
        const WRITE     = 0b0000_0010;
        const CREATE    = 0b0000_0100;
        const TRUNCATE  = 0b0000_1000;
        const APPEND    = 0b0001_0000;
        const EXCLUSIVE = 0b0010_0000;
        const DIRECTORY = 0b0100_0000;
        const NOFOLLOW  = 0b1000_0000;
    }
}
```

### File Statistics

```rust
pub struct FileStat {
    /// Inode number
    pub inode: u64,
    /// File type (regular, directory, symlink, etc.)
    pub file_type: FileType,
    /// File size in bytes
    pub size: u64,
    /// Number of hard links
    pub nlink: u64,
    /// File permissions (capability-based)
    pub mode: u32,
    /// Owner capability token
    pub owner_cap: CapabilityToken,
    /// Block size for I/O
    pub blksize: u64,
    /// Number of 512-byte blocks
    pub blocks: u64,
    /// Access time
    pub atime: Timestamp,
    /// Modification time
    pub mtime: Timestamp,
    /// Status change time
    pub ctime: Timestamp,
    /// Creation time
    pub btime: Timestamp,
}

pub enum FileType {
    Regular,
    Directory,
    Symlink,
    CharDevice,
    BlockDevice,
    Fifo,
    Socket,
}
```

### Directory Entry

```rust
pub struct DirEntry {
    /// Entry name
    pub name: String,
    /// Entry type
    pub file_type: FileType,
    /// Inode number
    pub inode: u64,
}
```

---

## Mount System

### Mount Table

```rust
pub struct MountTable {
    /// Mount points (path → filesystem)
    mounts: BTreeMap<String, MountEntry>,
}

pub struct MountEntry {
    /// Mounted filesystem
    pub fs: Arc<dyn Filesystem>,
    /// Mount flags
    pub flags: MountFlags,
    /// Source device (if any)
    pub source: Option<String>,
    /// Required capability to access
    pub capability: Option<CapabilityToken>,
}
```

### Mount Flags

```rust
bitflags! {
    pub struct MountFlags: u32 {
        const RDONLY     = 0b0000_0001;  // Read-only
        const NOSUID     = 0b0000_0010;  // Ignore setuid bits
        const NODEV      = 0b0000_0100;  // Disallow device files
        const NOEXEC     = 0b0000_1000;  // Disallow execution
        const SYNCHRONOUS = 0b0001_0000; // Sync writes
        const NOATIME    = 0b0010_0000;  // Don't update atime
    }
}
```

### Mount Operations

```rust
impl Vfs {
    /// Mount filesystem at path
    pub fn mount(
        &mut self,
        source: Option<&str>,
        target: &str,
        fs_type: &str,
        flags: MountFlags,
    ) -> Result<(), VfsError>;
    
    /// Unmount filesystem
    pub fn umount(&mut self, target: &str) -> Result<(), VfsError>;
    
    /// List all mounts
    pub fn mounts(&self) -> Vec<MountInfo>;
}
```

---

## Path Resolution

### Algorithm

```rust
fn resolve_path(&self, path: &str) -> Result<(Arc<dyn Filesystem>, String), VfsError> {
    // 1. Normalize path (remove .., ., //)
    let normalized = normalize_path(path);
    
    // 2. Find longest matching mount point
    let mut best_match = "/";
    let mut best_fs = self.root_fs.clone();
    
    for (mount_point, entry) in &self.mounts {
        if normalized.starts_with(mount_point) && mount_point.len() > best_match.len() {
            best_match = mount_point;
            best_fs = entry.fs.clone();
        }
    }
    
    // 3. Strip mount point prefix to get path within filesystem
    let relative_path = normalized.strip_prefix(best_match)
        .unwrap_or("/")
        .to_string();
    
    Ok((best_fs, relative_path))
}
```

### Symlink Resolution

```rust
const MAX_SYMLINK_DEPTH: u32 = 40;

fn resolve_symlinks(
    &self,
    path: &str,
    depth: u32,
) -> Result<String, VfsError> {
    if depth > MAX_SYMLINK_DEPTH {
        return Err(VfsError::SymlinkLoop);
    }
    
    let (fs, rel_path) = self.resolve_path(path)?;
    let stat = fs.stat(&rel_path)?;
    
    match stat.file_type {
        FileType::Symlink => {
            let target = fs.readlink(&rel_path)?;
            let new_path = if target.starts_with('/') {
                target
            } else {
                join_path(parent(path), &target)
            };
            self.resolve_symlinks(&new_path, depth + 1)
        }
        _ => Ok(path.to_string()),
    }
}
```

---

## VFS Operations

### File Operations

```rust
impl Vfs {
    /// Open file, return file descriptor
    pub fn open(&mut self, path: &str, flags: OpenFlags) -> Result<Fd, VfsError> {
        let (fs, rel_path) = self.resolve_path(path)?;
        let handle = fs.open(&rel_path, flags)?;
        
        let fd = self.next_fd;
        self.next_fd += 1;
        
        self.fd_table.insert(fd, FileDescriptor {
            handle,
            position: 0,
            flags,
            fs,
            path: rel_path,
        });
        
        Ok(fd)
    }
    
    /// Read from file descriptor
    pub fn read(&mut self, fd: Fd, buf: &mut [u8]) -> Result<usize, VfsError> {
        let desc = self.fd_table.get_mut(&fd)
            .ok_or(VfsError::BadFd)?;
        
        let bytes_read = desc.fs.read_at(&desc.path, desc.position, buf)?;
        desc.position += bytes_read as u64;
        
        Ok(bytes_read)
    }
    
    /// Write to file descriptor
    pub fn write(&mut self, fd: Fd, buf: &[u8]) -> Result<usize, VfsError> {
        let desc = self.fd_table.get_mut(&fd)
            .ok_or(VfsError::BadFd)?;
        
        if !desc.flags.contains(OpenFlags::WRITE) {
            return Err(VfsError::PermissionDenied);
        }
        
        let bytes_written = desc.fs.write_at(&desc.path, desc.position, buf)?;
        desc.position += bytes_written as u64;
        
        Ok(bytes_written)
    }
    
    /// Seek within file
    pub fn lseek(&mut self, fd: Fd, offset: i64, whence: Whence) -> Result<u64, VfsError> {
        let desc = self.fd_table.get_mut(&fd)
            .ok_or(VfsError::BadFd)?;
        
        let new_pos = match whence {
            Whence::Set => offset as u64,
            Whence::Cur => (desc.position as i64 + offset) as u64,
            Whence::End => {
                let stat = desc.fs.stat(&desc.path)?;
                (stat.size as i64 + offset) as u64
            }
        };
        
        desc.position = new_pos;
        Ok(new_pos)
    }
    
    /// Close file descriptor
    pub fn close(&mut self, fd: Fd) -> Result<(), VfsError> {
        let desc = self.fd_table.remove(&fd)
            .ok_or(VfsError::BadFd)?;
        desc.fs.close(desc.handle)
    }
}
```

### Directory Operations

```rust
impl Vfs {
    /// Read directory contents
    pub fn readdir(&self, path: &str) -> Result<Vec<DirEntry>, VfsError> {
        let (fs, rel_path) = self.resolve_path(path)?;
        fs.readdir(&rel_path)
    }
    
    /// Create directory
    pub fn mkdir(&mut self, path: &str) -> Result<(), VfsError> {
        let (fs, rel_path) = self.resolve_path(path)?;
        fs.mkdir(&rel_path)
    }
    
    /// Remove empty directory
    pub fn rmdir(&mut self, path: &str) -> Result<(), VfsError> {
        let (fs, rel_path) = self.resolve_path(path)?;
        fs.rmdir(&rel_path)
    }
}
```

---

## Filesystem Implementations

### RamFS

In-memory filesystem for temporary storage:

```rust
// kernel/src/fs/ramfs.rs

pub struct RamFs {
    root: Mutex<INode>,
    next_inode: AtomicU64,
}

struct INode {
    inode_num: u64,
    name: String,
    node_type: NodeType,
    data: Vec<u8>,       // File contents
    children: Vec<INode>, // Directory entries
    created: u64,
    modified: u64,
}
```

**Features:**
- No disk I/O, purely memory-based
- Fast for temporary files
- Used for `/tmp`, `/run`

### SplaxFS

Native journaling filesystem:

```rust
// kernel/src/fs/splaxfs.rs

pub struct SplaxFs {
    device: Arc<dyn BlockDevice>,
    superblock: SuperBlock,
    block_cache: BlockCache,
    journal: Option<Journal>,
}
```

**Features:**
- Copy-on-write design
- Write-ahead journaling
- Checksumming (XXHash3)
- Compression support (LZ4/Zstd)
- Capability-aware permissions

See [SPLAXFS.md](SPLAXFS.md) for detailed documentation.

### ext4 (Read-Only)

Support for reading existing ext4 partitions:

```rust
// kernel/src/fs/ext4.rs

pub struct Ext4Fs {
    device: Arc<dyn BlockDevice>,
    superblock: Ext4SuperBlock,
    block_size: u32,
    groups: Vec<BlockGroupDescriptor>,
}
```

**Features:**
- Read files and directories
- Support for extents
- Large file support (>2GB)

See [EXT4.md](EXT4.md) for detailed documentation.

### FAT32

Compatibility with USB drives and SD cards:

```rust
// kernel/src/fs/fat32.rs

pub struct Fat32Fs {
    device: Arc<dyn BlockDevice>,
    bpb: BiosParameterBlock,
    fat_start: u64,
    data_start: u64,
    root_cluster: u32,
}
```

**Features:**
- Long filename support
- FAT12/FAT16/FAT32 detection
- Case-insensitive matching

See [FAT32.md](FAT32.md) for detailed documentation.

### ProcFS

Virtual filesystem exposing process information:

```rust
// kernel/src/fs/procfs.rs

pub struct ProcFs {
    sched: Arc<Scheduler>,
}
```

**Files:**
- `/proc/[pid]/status` - Process status
- `/proc/[pid]/cmdline` - Command line
- `/proc/[pid]/maps` - Memory maps
- `/proc/[pid]/fd/` - Open file descriptors
- `/proc/cpuinfo` - CPU information
- `/proc/meminfo` - Memory statistics
- `/proc/uptime` - System uptime
- `/proc/mounts` - Mount table

### SysFS

Virtual filesystem for kernel objects:

```rust
// kernel/src/fs/sysfs.rs

pub struct SysFs {
    subsystems: Vec<Subsystem>,
}
```

**Directories:**
- `/sys/class/` - Device classes
- `/sys/block/` - Block devices
- `/sys/bus/` - Bus types
- `/sys/devices/` - Device hierarchy
- `/sys/kernel/` - Kernel parameters

### DevFS

Virtual filesystem for device nodes:

```rust
// kernel/src/fs/devfs.rs

pub struct DevFs {
    devices: BTreeMap<String, DeviceNode>,
}

pub struct DeviceNode {
    name: String,
    dev_type: DeviceType,
    major: u32,
    minor: u32,
    read: Option<fn(&mut [u8]) -> usize>,
    write: Option<fn(&[u8]) -> usize>,
}
```

**Devices:**
- `/dev/null` - Null device
- `/dev/zero` - Zero device
- `/dev/random` - Random number generator
- `/dev/console` - System console
- `/dev/tty` - Terminal
- `/dev/vda`, `/dev/nvme0n1` - Block devices

---

## VFS Stub (Hybrid Kernel)

For the hybrid kernel migration (Phase 11), the VFS can be migrated to userspace:

### Kernel VFS Stub

```rust
// kernel/src/fs/vfs_stub.rs

/// Thin kernel VFS layer that forwards to S-STORAGE service
pub fn open(path: &str, flags: OpenFlags) -> Result<Fd, VfsError> {
    let request = VfsRequest::Open {
        request_id: next_request_id(),
        path: path.to_string(),
        flags: flags.bits(),
    };
    
    let response = send_to_storage(request)?;
    
    match response {
        VfsResponse::Opened { fd, .. } => Ok(fd),
        VfsResponse::Error { code, .. } => Err(code.into()),
        _ => Err(VfsError::Protocol),
    }
}
```

### VFS Protocol

```rust
// services/storage/src/vfs_protocol.rs

pub enum VfsRequest {
    Open { request_id: u64, path: String, flags: u32 },
    Close { request_id: u64, fd: u64 },
    Read { request_id: u64, fd: u64, len: usize },
    Write { request_id: u64, fd: u64, data: Vec<u8> },
    Seek { request_id: u64, fd: u64, offset: i64, whence: u8 },
    Stat { request_id: u64, path: String },
    Readdir { request_id: u64, path: String },
    Mkdir { request_id: u64, path: String },
    Rmdir { request_id: u64, path: String },
    Unlink { request_id: u64, path: String },
    Mount { request_id: u64, source: Option<String>, target: String, fs_type: String },
    Umount { request_id: u64, target: String },
    // ... more operations
}

pub enum VfsResponse {
    Opened { request_id: u64, fd: u64 },
    Closed { request_id: u64 },
    Read { request_id: u64, data: Vec<u8> },
    Written { request_id: u64, bytes: usize },
    Seeked { request_id: u64, position: u64 },
    Stat { request_id: u64, stat: VfsAttr },
    Dirents { request_id: u64, entries: Vec<VfsDirEntry> },
    Ok { request_id: u64 },
    Error { request_id: u64, code: u32, message: String },
}
```

---

## Shell Commands

### File Operations

| Command | Description | Example |
|---------|-------------|---------|
| `ls [path]` | List directory | `ls /proc` |
| `cat <file>` | Display file contents | `cat /proc/cpuinfo` |
| `touch <file>` | Create empty file | `touch /tmp/test.txt` |
| `rm <file>` | Remove file | `rm /tmp/test.txt` |
| `cp <src> <dst>` | Copy file | `cp /bin/hello /tmp/` |
| `mv <src> <dst>` | Move/rename file | `mv old.txt new.txt` |
| `mkdir <dir>` | Create directory | `mkdir /tmp/subdir` |
| `rmdir <dir>` | Remove empty directory | `rmdir /tmp/subdir` |
| `stat <path>` | Show file statistics | `stat /bin/hello` |
| `hexdump <file>` | Hex dump of file | `hexdump /bin/hello` |

### Mount Operations

| Command | Description | Example |
|---------|-------------|---------|
| `mount` | List all mounts | `mount` |
| `mount -t <type> <src> <dst>` | Mount filesystem | `mount -t ext4 /dev/vda1 /mnt` |
| `umount <path>` | Unmount filesystem | `umount /mnt` |

### Filesystem Information

| Command | Description | Example |
|---------|-------------|---------|
| `df` | Disk usage | `df` |
| `du <path>` | Directory size | `du /bin` |

---

## Error Handling

```rust
#[derive(Debug, Clone, Copy)]
pub enum VfsError {
    NotFound,           // File/directory not found
    PermissionDenied,   // Capability check failed
    IsDirectory,        // Operation invalid for directory
    NotDirectory,       // Expected directory, got file
    NotEmpty,           // Directory not empty
    AlreadyExists,      // File/directory already exists
    BadFd,              // Invalid file descriptor
    IoError,            // Block device I/O error
    ReadOnly,           // Filesystem is read-only
    NoSpace,            // No space left on device
    TooManyOpenFiles,   // Open file limit reached
    SymlinkLoop,        // Too many symlink levels
    InvalidPath,        // Malformed path
    Protocol,           // IPC protocol error
    NotSupported,       // Operation not supported
    Busy,               // Resource busy
    Corruption,         // Filesystem corruption detected
}
```

---

## Performance Considerations

### Caching

- **Inode Cache**: LRU cache of recently accessed inodes
- **Dentry Cache**: Path-to-inode mappings
- **Block Cache**: Recently accessed disk blocks
- **Page Cache**: File data pages (planned)

### Optimization

- **Readahead**: Prefetch sequential file data
- **Write-back**: Batch writes to reduce I/O
- **Mount caching**: Fast mount point lookups

---

## File Structure

```
kernel/src/fs/
├── mod.rs          # Module exports
├── vfs.rs          # VFS core implementation
├── vfs_stub.rs     # Kernel VFS stub for hybrid kernel
├── ramfs.rs        # RAM filesystem
├── splaxfs.rs      # Native Splax filesystem
├── ext4.rs         # ext4 read-only support
├── fat32.rs        # FAT32 filesystem
├── procfs.rs       # Process filesystem
├── sysfs.rs        # System filesystem
└── devfs.rs        # Device filesystem

services/storage/src/
├── lib.rs          # S-STORAGE service
├── vfs_protocol.rs # VFS RPC protocol
└── vfs_server.rs   # Userspace VFS server
```

---

## Future Work

1. **Page Cache**: Unified page cache for file I/O
2. **Write-back caching**: Delayed writes for performance
3. **Quotas**: Per-capability storage quotas
4. **Extended attributes**: Custom metadata
5. **File locking**: POSIX-style advisory locks
6. **Async I/O**: Non-blocking file operations
7. **io_uring-style interface**: High-performance batch I/O

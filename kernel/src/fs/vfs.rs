//! # Virtual Filesystem Layer (VFS)
//!
//! Abstract interface for filesystems, similar to Linux's VFS.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │                   User Space                    │
//! ├─────────────────────────────────────────────────┤
//! │                  System Calls                   │
//! │   open(), read(), write(), close(), stat()     │
//! ├─────────────────────────────────────────────────┤
//! │                      VFS                        │
//! │  - Mount table                                  │
//! │  - Inode cache                                  │
//! │  - Dentry cache                                 │
//! │  - File descriptor table                        │
//! ├─────────────────────────────────────────────────┤
//! │              Filesystem Drivers                 │
//! │   RamFS │ SplaxFS │ ProcFS │ DevFS │ SysFS    │
//! └─────────────────────────────────────────────────┘
//! ```

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::{Mutex, RwLock};

/// Maximum number of open files per process
pub const MAX_OPEN_FILES: usize = 256;

/// Maximum number of mount points
pub const MAX_MOUNTS: usize = 64;

/// File descriptor
pub type Fd = u32;

/// Inode number
pub type InodeNum = u64;

/// VFS errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    /// File not found
    NotFound,
    /// Permission denied
    PermissionDenied,
    /// Already exists
    AlreadyExists,
    /// Not a directory
    NotADirectory,
    /// Not a file
    NotAFile,
    /// Is a directory
    IsADirectory,
    /// Directory not empty
    NotEmpty,
    /// Bad file descriptor
    BadFd,
    /// Too many open files
    TooManyOpenFiles,
    /// No space left
    NoSpace,
    /// Read-only filesystem
    ReadOnlyFs,
    /// Invalid argument
    InvalidArgument,
    /// I/O error
    IoError,
    /// Not supported
    NotSupported,
    /// Path too long
    PathTooLong,
    /// Cross-device link
    CrossDevice,
    /// No such filesystem
    NoFilesystem,
    /// Mount point busy
    Busy,
}

/// File type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsFileType {
    /// Regular file
    Regular,
    /// Directory
    Directory,
    /// Symbolic link
    Symlink,
    /// Character device
    CharDevice,
    /// Block device
    BlockDevice,
    /// Named pipe (FIFO)
    Fifo,
    /// Unix socket
    Socket,
}

/// File permissions (capability-based in Splax)
#[derive(Debug, Clone, Copy)]
pub struct VfsPermissions {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
}

impl Default for VfsPermissions {
    fn default() -> Self {
        Self {
            readable: true,
            writable: true,
            executable: false,
        }
    }
}

/// Inode attributes (stat data)
#[derive(Debug, Clone)]
pub struct VfsAttr {
    /// Inode number
    pub ino: InodeNum,
    /// File type
    pub file_type: VfsFileType,
    /// Permissions
    pub perm: VfsPermissions,
    /// Size in bytes
    pub size: u64,
    /// Number of hard links
    pub nlink: u32,
    /// Block size for I/O
    pub blksize: u32,
    /// Number of 512-byte blocks
    pub blocks: u64,
    /// Access time (ticks)
    pub atime: u64,
    /// Modification time (ticks)
    pub mtime: u64,
    /// Status change time (ticks)
    pub ctime: u64,
    /// Creation time (ticks)
    pub crtime: u64,
}

impl VfsAttr {
    pub fn new_file(ino: InodeNum, size: u64) -> Self {
        let now = crate::arch::x86_64::interrupts::get_ticks();
        Self {
            ino,
            file_type: VfsFileType::Regular,
            perm: VfsPermissions::default(),
            size,
            nlink: 1,
            blksize: 4096,
            blocks: (size + 511) / 512,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
        }
    }

    pub fn new_directory(ino: InodeNum) -> Self {
        let now = crate::arch::x86_64::interrupts::get_ticks();
        Self {
            ino,
            file_type: VfsFileType::Directory,
            perm: VfsPermissions {
                readable: true,
                writable: true,
                executable: true, // Directories need +x for traversal
            },
            size: 0,
            nlink: 2, // . and parent's entry
            blksize: 4096,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
        }
    }
}

/// Directory entry
#[derive(Debug, Clone)]
pub struct VfsDirEntry {
    /// Entry name
    pub name: String,
    /// Inode number
    pub ino: InodeNum,
    /// File type
    pub file_type: VfsFileType,
}

/// Open file flags
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenFlags {
    /// Read access
    pub read: bool,
    /// Write access
    pub write: bool,
    /// Append mode
    pub append: bool,
    /// Create if doesn't exist
    pub create: bool,
    /// Fail if exists (with create)
    pub exclusive: bool,
    /// Truncate to zero length
    pub truncate: bool,
    /// Directory
    pub directory: bool,
}

impl OpenFlags {
    pub const fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            append: false,
            create: false,
            exclusive: false,
            truncate: false,
            directory: false,
        }
    }

    pub const fn write_only() -> Self {
        Self {
            read: false,
            write: true,
            append: false,
            create: false,
            exclusive: false,
            truncate: false,
            directory: false,
        }
    }

    pub const fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            append: false,
            create: false,
            exclusive: false,
            truncate: false,
            directory: false,
        }
    }

    pub const fn create_write() -> Self {
        Self {
            read: false,
            write: true,
            append: false,
            create: true,
            exclusive: false,
            truncate: true,
            directory: false,
        }
    }
}

/// Seek position
#[derive(Debug, Clone, Copy)]
pub enum SeekFrom {
    /// From start of file
    Start(u64),
    /// From current position
    Current(i64),
    /// From end of file
    End(i64),
}

/// Filesystem operations trait
pub trait Filesystem: Send + Sync {
    /// Returns the filesystem name
    fn name(&self) -> &'static str;

    /// Lookup a name in a directory
    fn lookup(&self, parent: InodeNum, name: &str) -> Result<InodeNum, VfsError>;

    /// Get attributes of an inode
    fn getattr(&self, ino: InodeNum) -> Result<VfsAttr, VfsError>;

    /// Read directory entries
    fn readdir(&self, ino: InodeNum) -> Result<Vec<VfsDirEntry>, VfsError>;

    /// Read file content
    fn read(&self, ino: InodeNum, offset: u64, size: usize) -> Result<Vec<u8>, VfsError>;

    /// Write file content
    fn write(&self, ino: InodeNum, offset: u64, data: &[u8]) -> Result<usize, VfsError>;

    /// Create a file
    fn create(&self, parent: InodeNum, name: &str, file_type: VfsFileType) -> Result<InodeNum, VfsError>;

    /// Remove a file/directory
    fn unlink(&self, parent: InodeNum, name: &str) -> Result<(), VfsError>;

    /// Rename/move a file
    fn rename(&self, old_parent: InodeNum, old_name: &str, new_parent: InodeNum, new_name: &str) -> Result<(), VfsError>;

    /// Truncate a file
    fn truncate(&self, ino: InodeNum, size: u64) -> Result<(), VfsError>;

    /// Sync filesystem
    fn sync(&self) -> Result<(), VfsError>;

    /// Get filesystem statistics
    fn statfs(&self) -> Result<VfsStatFs, VfsError>;

    /// Read symbolic link target
    fn readlink(&self, ino: InodeNum) -> Result<String, VfsError> {
        let _ = ino;
        Err(VfsError::NotSupported)
    }

    /// Create symbolic link
    fn symlink(&self, parent: InodeNum, name: &str, target: &str) -> Result<InodeNum, VfsError> {
        let _ = (parent, name, target);
        Err(VfsError::NotSupported)
    }

    /// Create hard link
    fn link(&self, ino: InodeNum, new_parent: InodeNum, new_name: &str) -> Result<(), VfsError> {
        let _ = (ino, new_parent, new_name);
        Err(VfsError::NotSupported)
    }

    /// Set file attributes
    fn setattr(&self, ino: InodeNum, attr: &VfsAttr) -> Result<(), VfsError> {
        let _ = (ino, attr);
        Err(VfsError::NotSupported)
    }
}

/// Filesystem statistics
#[derive(Debug, Clone)]
pub struct VfsStatFs {
    /// Total blocks
    pub blocks: u64,
    /// Free blocks
    pub bfree: u64,
    /// Available blocks (for unprivileged users)
    pub bavail: u64,
    /// Total inodes
    pub files: u64,
    /// Free inodes
    pub ffree: u64,
    /// Block size
    pub bsize: u32,
    /// Maximum name length
    pub namelen: u32,
}

/// Mount point
pub struct MountPoint {
    /// Mount path
    pub path: String,
    /// Mounted filesystem
    pub fs: Arc<dyn Filesystem>,
    /// Root inode of this mount
    pub root_ino: InodeNum,
    /// Flags
    pub read_only: bool,
}

/// Open file handle
pub struct OpenFile {
    /// Mounted filesystem
    pub mount: Arc<MountPoint>,
    /// Inode number
    pub ino: InodeNum,
    /// Current offset
    pub offset: u64,
    /// Open flags
    pub flags: OpenFlags,
}

/// File descriptor table (per-process)
pub struct FdTable {
    /// Open files
    files: BTreeMap<Fd, OpenFile>,
    /// Next file descriptor
    next_fd: Fd,
}

impl FdTable {
    pub fn new() -> Self {
        Self {
            files: BTreeMap::new(),
            next_fd: 3, // 0, 1, 2 reserved for stdin/stdout/stderr
        }
    }

    /// Allocate a new file descriptor
    pub fn alloc(&mut self, file: OpenFile) -> Result<Fd, VfsError> {
        if self.files.len() >= MAX_OPEN_FILES {
            return Err(VfsError::TooManyOpenFiles);
        }
        let fd = self.next_fd;
        self.next_fd += 1;
        self.files.insert(fd, file);
        Ok(fd)
    }

    /// Get an open file by fd
    pub fn get(&self, fd: Fd) -> Option<&OpenFile> {
        self.files.get(&fd)
    }

    /// Get mutable open file by fd
    pub fn get_mut(&mut self, fd: Fd) -> Option<&mut OpenFile> {
        self.files.get_mut(&fd)
    }

    /// Close a file descriptor
    pub fn close(&mut self, fd: Fd) -> Result<(), VfsError> {
        self.files.remove(&fd).ok_or(VfsError::BadFd)?;
        Ok(())
    }
}

/// The Virtual Filesystem
pub struct Vfs {
    /// Mount table
    mounts: RwLock<Vec<Arc<MountPoint>>>,
    /// Global inode counter
    next_ino: AtomicU64,
    /// Per-process file descriptor tables
    fd_tables: Mutex<BTreeMap<u64, FdTable>>,
}

impl Vfs {
    /// Create a new VFS
    pub const fn new() -> Self {
        Self {
            mounts: RwLock::new(Vec::new()),
            next_ino: AtomicU64::new(1),
            fd_tables: Mutex::new(BTreeMap::new()),
        }
    }

    /// Allocate a new inode number
    pub fn alloc_ino(&self) -> InodeNum {
        self.next_ino.fetch_add(1, Ordering::SeqCst)
    }

    /// Mount a filesystem at the given path
    pub fn mount(&self, path: &str, fs: Arc<dyn Filesystem>, read_only: bool) -> Result<(), VfsError> {
        let mut mounts = self.mounts.write();
        
        if mounts.len() >= MAX_MOUNTS {
            return Err(VfsError::NoSpace);
        }

        // Check if already mounted
        for m in mounts.iter() {
            if m.path == path {
                return Err(VfsError::AlreadyExists);
            }
        }

        let mount = Arc::new(MountPoint {
            path: String::from(path),
            fs,
            root_ino: 1, // Root inode is 1 by convention
            read_only,
        });

        mounts.push(mount);
        
        // Sort by path length descending for longest-match lookup
        mounts.sort_by(|a, b| b.path.len().cmp(&a.path.len()));
        
        Ok(())
    }

    /// Unmount a filesystem
    pub fn unmount(&self, path: &str) -> Result<(), VfsError> {
        let mut mounts = self.mounts.write();
        
        let pos = mounts.iter().position(|m| m.path == path)
            .ok_or(VfsError::NotFound)?;
        
        // Check if mount is in use (has open files)
        // For now, just remove it
        mounts.remove(pos);
        
        Ok(())
    }

    /// Find the mount point for a path
    fn find_mount(&self, path: &str) -> Option<(Arc<MountPoint>, String)> {
        let mounts = self.mounts.read();
        
        for mount in mounts.iter() {
            if path == mount.path || path.starts_with(&format!("{}/", mount.path)) {
                let relative = if mount.path == "/" {
                    path.to_string()
                } else {
                    path[mount.path.len()..].to_string()
                };
                return Some((mount.clone(), relative));
            }
        }
        
        None
    }

    /// Resolve a path to (mount, inode)
    fn resolve_path(&self, path: &str) -> Result<(Arc<MountPoint>, InodeNum), VfsError> {
        let (mount, relative) = self.find_mount(path).ok_or(VfsError::NotFound)?;
        
        let path = if relative.is_empty() { "/" } else { &relative };
        let path = path.trim_start_matches('/');
        
        let mut current_ino = mount.root_ino;
        
        if path.is_empty() {
            return Ok((mount, current_ino));
        }
        
        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            if component == ".." {
                // For simplicity, stay at root
                continue;
            }
            
            current_ino = mount.fs.lookup(current_ino, component)?;
        }
        
        Ok((mount, current_ino))
    }

    /// Get or create fd table for a process
    fn get_fd_table(&self, pid: u64) -> FdTable {
        let mut tables = self.fd_tables.lock();
        tables.entry(pid).or_insert_with(FdTable::new);
        // Clone is not ideal but needed for lock management
        FdTable::new()
    }

    /// Open a file
    pub fn open(&self, pid: u64, path: &str, flags: OpenFlags) -> Result<Fd, VfsError> {
        let mut tables = self.fd_tables.lock();
        let table = tables.entry(pid).or_insert_with(FdTable::new);
        
        // Resolve path or create file
        let (mount, ino) = if flags.create {
            match self.resolve_path(path) {
                Ok((mount, ino)) => {
                    if flags.exclusive {
                        return Err(VfsError::AlreadyExists);
                    }
                    (mount, ino)
                }
                Err(VfsError::NotFound) => {
                    // Create the file
                    let parent_path = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("/");
                    let name = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
                    
                    let (mount, parent_ino) = self.resolve_path(parent_path)?;
                    let ino = mount.fs.create(parent_ino, name, VfsFileType::Regular)?;
                    (mount, ino)
                }
                Err(e) => return Err(e),
            }
        } else {
            self.resolve_path(path)?
        };
        
        // Check it's not a directory (unless O_DIRECTORY)
        let attr = mount.fs.getattr(ino)?;
        if attr.file_type == VfsFileType::Directory && !flags.directory {
            return Err(VfsError::IsADirectory);
        }
        
        // Truncate if requested
        if flags.truncate && flags.write {
            mount.fs.truncate(ino, 0)?;
        }
        
        let file = OpenFile {
            mount,
            ino,
            offset: 0,
            flags,
        };
        
        table.alloc(file)
    }

    /// Close a file
    pub fn close(&self, pid: u64, fd: Fd) -> Result<(), VfsError> {
        let mut tables = self.fd_tables.lock();
        let table = tables.get_mut(&pid).ok_or(VfsError::BadFd)?;
        table.close(fd)
    }

    /// Read from a file
    pub fn read(&self, pid: u64, fd: Fd, buf: &mut [u8]) -> Result<usize, VfsError> {
        let mut tables = self.fd_tables.lock();
        let table = tables.get_mut(&pid).ok_or(VfsError::BadFd)?;
        let file = table.get_mut(fd).ok_or(VfsError::BadFd)?;
        
        if !file.flags.read {
            return Err(VfsError::PermissionDenied);
        }
        
        let data = file.mount.fs.read(file.ino, file.offset, buf.len())?;
        let len = data.len().min(buf.len());
        buf[..len].copy_from_slice(&data[..len]);
        file.offset += len as u64;
        
        Ok(len)
    }

    /// Write to a file
    pub fn write(&self, pid: u64, fd: Fd, buf: &[u8]) -> Result<usize, VfsError> {
        let mut tables = self.fd_tables.lock();
        let table = tables.get_mut(&pid).ok_or(VfsError::BadFd)?;
        let file = table.get_mut(fd).ok_or(VfsError::BadFd)?;
        
        if !file.flags.write {
            return Err(VfsError::PermissionDenied);
        }
        
        if file.mount.read_only {
            return Err(VfsError::ReadOnlyFs);
        }
        
        let offset = if file.flags.append {
            let attr = file.mount.fs.getattr(file.ino)?;
            attr.size
        } else {
            file.offset
        };
        
        let written = file.mount.fs.write(file.ino, offset, buf)?;
        file.offset = offset + written as u64;
        
        Ok(written)
    }

    /// Seek in a file
    pub fn seek(&self, pid: u64, fd: Fd, pos: SeekFrom) -> Result<u64, VfsError> {
        let mut tables = self.fd_tables.lock();
        let table = tables.get_mut(&pid).ok_or(VfsError::BadFd)?;
        let file = table.get_mut(fd).ok_or(VfsError::BadFd)?;
        
        let size = file.mount.fs.getattr(file.ino)?.size;
        
        let new_offset = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => {
                if n >= 0 {
                    file.offset.saturating_add(n as u64)
                } else {
                    file.offset.saturating_sub((-n) as u64)
                }
            }
            SeekFrom::End(n) => {
                if n >= 0 {
                    size.saturating_add(n as u64)
                } else {
                    size.saturating_sub((-n) as u64)
                }
            }
        };
        
        file.offset = new_offset;
        Ok(new_offset)
    }

    /// Get file attributes by path
    pub fn stat(&self, path: &str) -> Result<VfsAttr, VfsError> {
        let (mount, ino) = self.resolve_path(path)?;
        mount.fs.getattr(ino)
    }

    /// Get file attributes by fd
    pub fn fstat(&self, pid: u64, fd: Fd) -> Result<VfsAttr, VfsError> {
        let tables = self.fd_tables.lock();
        let table = tables.get(&pid).ok_or(VfsError::BadFd)?;
        let file = table.get(fd).ok_or(VfsError::BadFd)?;
        file.mount.fs.getattr(file.ino)
    }

    /// List directory contents
    pub fn readdir(&self, path: &str) -> Result<Vec<VfsDirEntry>, VfsError> {
        let (mount, ino) = self.resolve_path(path)?;
        mount.fs.readdir(ino)
    }

    /// Create a directory
    pub fn mkdir(&self, path: &str) -> Result<(), VfsError> {
        let parent_path = path.rsplit_once('/').map(|(p, _)| if p.is_empty() { "/" } else { p }).unwrap_or("/");
        let name = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
        
        let (mount, parent_ino) = self.resolve_path(parent_path)?;
        mount.fs.create(parent_ino, name, VfsFileType::Directory)?;
        Ok(())
    }

    /// Remove a file
    pub fn unlink(&self, path: &str) -> Result<(), VfsError> {
        let parent_path = path.rsplit_once('/').map(|(p, _)| if p.is_empty() { "/" } else { p }).unwrap_or("/");
        let name = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
        
        let (mount, parent_ino) = self.resolve_path(parent_path)?;
        mount.fs.unlink(parent_ino, name)
    }

    /// Remove a directory
    pub fn rmdir(&self, path: &str) -> Result<(), VfsError> {
        // Same as unlink for now
        self.unlink(path)
    }

    /// Rename/move a file
    pub fn rename(&self, old_path: &str, new_path: &str) -> Result<(), VfsError> {
        let old_parent = old_path.rsplit_once('/').map(|(p, _)| if p.is_empty() { "/" } else { p }).unwrap_or("/");
        let old_name = old_path.rsplit_once('/').map(|(_, n)| n).unwrap_or(old_path);
        let new_parent = new_path.rsplit_once('/').map(|(p, _)| if p.is_empty() { "/" } else { p }).unwrap_or("/");
        let new_name = new_path.rsplit_once('/').map(|(_, n)| n).unwrap_or(new_path);
        
        let (old_mount, old_parent_ino) = self.resolve_path(old_parent)?;
        let (new_mount, new_parent_ino) = self.resolve_path(new_parent)?;
        
        // Check same filesystem
        if !Arc::ptr_eq(&old_mount.fs, &new_mount.fs) {
            return Err(VfsError::CrossDevice);
        }
        
        old_mount.fs.rename(old_parent_ino, old_name, new_parent_ino, new_name)
    }

    /// List mounted filesystems
    pub fn list_mounts(&self) -> Vec<String> {
        self.mounts.read().iter().map(|m| m.path.clone()).collect()
    }
}

/// Global VFS instance
pub static VFS: Vfs = Vfs::new();

/// Initialize VFS
pub fn init() {
    // VFS is initialized via Vfs::new() (const fn)
    // Actual mounting of root filesystem happens in fs::init()
}

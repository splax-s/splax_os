//! # VFS Server
//!
//! Userspace VFS service that handles filesystem operations via S-LINK IPC.
//! This is the userspace component of the Phase A hybrid kernel migration.
//!
//! ## Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────┐
//! │                      S-STORAGE Service                              │
//! ├────────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────────────────────────────────────────────────┐   │
//! │  │                      VfsServer                               │   │
//! │  │  ┌───────────────────────────────────────────────────────┐  │   │
//! │  │  │              Mount Table                               │  │   │
//! │  │  │  /         → RamFS                                    │  │   │
//! │  │  │  /mnt/ext  → Ext4Fs                                   │  │   │
//! │  │  │  /mnt/fat  → Fat32Fs                                  │  │   │
//! │  │  └───────────────────────────────────────────────────────┘  │   │
//! │  │  ┌───────────────────────────────────────────────────────┐  │   │
//! │  │  │              Open File Table                          │  │   │
//! │  │  │  Handle 1 → /etc/config (RamFS inode 5)               │  │   │
//! │  │  │  Handle 2 → /mnt/ext/data (ext4 inode 128)            │  │   │
//! │  │  └───────────────────────────────────────────────────────┘  │   │
//! │  └─────────────────────────────────────────────────────────────┘   │
//! ├────────────────────────────────────────────────────────────────────┤
//! │                         S-LINK IPC                                  │
//! └────────────────────────────────────────────────────────────────────┘
//!                              ↑
//!                              │ VfsRequest / VfsResponse
//!                              ↓
//! ┌────────────────────────────────────────────────────────────────────┐
//! │                      Kernel VFS Stub                                │
//! └────────────────────────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use spin::{Mutex, RwLock};

use crate::vfs_protocol::*;

/// Maximum number of open files
const MAX_OPEN_FILES: usize = 65536;

/// Maximum number of mounts
const MAX_MOUNTS: usize = 256;

/// Filesystem trait - implemented by each filesystem driver
pub trait Filesystem: Send + Sync {
    /// Get filesystem type name
    fn fs_type(&self) -> &str;

    /// Lookup file/directory by name in parent
    fn lookup(&self, parent: InodeNum, name: &str) -> Result<InodeNum, VfsError>;

    /// Get inode attributes
    fn getattr(&self, ino: InodeNum) -> Result<VfsAttr, VfsError>;

    /// Read directory entries
    fn readdir(&self, ino: InodeNum) -> Result<Vec<VfsDirEntry>, VfsError>;

    /// Read file data
    fn read(&self, ino: InodeNum, offset: u64, size: usize) -> Result<Vec<u8>, VfsError>;

    /// Write file data
    fn write(&self, _ino: InodeNum, _offset: u64, _data: &[u8]) -> Result<usize, VfsError> {
        Err(VfsError::ReadOnlyFs)
    }

    /// Create file
    fn create(
        &self,
        _parent: InodeNum,
        _name: &str,
        _mode: u32,
    ) -> Result<InodeNum, VfsError> {
        Err(VfsError::ReadOnlyFs)
    }

    /// Create directory
    fn mkdir(
        &self,
        _parent: InodeNum,
        _name: &str,
        _mode: u32,
    ) -> Result<InodeNum, VfsError> {
        Err(VfsError::ReadOnlyFs)
    }

    /// Remove file
    fn unlink(&self, _parent: InodeNum, _name: &str) -> Result<(), VfsError> {
        Err(VfsError::ReadOnlyFs)
    }

    /// Remove directory
    fn rmdir(&self, _parent: InodeNum, _name: &str) -> Result<(), VfsError> {
        Err(VfsError::ReadOnlyFs)
    }

    /// Rename file/directory
    fn rename(
        &self,
        _old_parent: InodeNum,
        _old_name: &str,
        _new_parent: InodeNum,
        _new_name: &str,
    ) -> Result<(), VfsError> {
        Err(VfsError::ReadOnlyFs)
    }

    /// Create symlink
    fn symlink(
        &self,
        _parent: InodeNum,
        _name: &str,
        _target: &str,
    ) -> Result<InodeNum, VfsError> {
        Err(VfsError::ReadOnlyFs)
    }

    /// Read symlink target
    fn readlink(&self, _ino: InodeNum) -> Result<String, VfsError> {
        Err(VfsError::NotSupported)
    }

    /// Truncate file
    fn truncate(&self, _ino: InodeNum, _size: u64) -> Result<(), VfsError> {
        Err(VfsError::ReadOnlyFs)
    }

    /// Sync filesystem
    fn sync(&self) -> Result<(), VfsError> {
        Ok(())
    }

    /// Get filesystem statistics
    fn statfs(&self) -> Result<StatFs, VfsError>;

    /// Get root inode number
    fn root_ino(&self) -> InodeNum;
}

/// Mount point information
struct MountPoint {
    /// Path where mounted
    path: String,
    /// Filesystem instance
    fs: Arc<dyn Filesystem>,
    /// Read-only flag
    read_only: bool,
    /// Device name (if any)
    device: Option<String>,
}

/// Open file handle information
struct OpenFile {
    /// Mount point index
    mount_idx: usize,
    /// Inode number within filesystem
    ino: InodeNum,
    /// Current position in file
    position: u64,
    /// Open flags
    flags: OpenFlags,
    /// File size (cached)
    size: u64,
}

/// VFS Server state
pub struct VfsServer {
    /// Mount points, sorted by path length (longest first for matching)
    mounts: RwLock<Vec<MountPoint>>,

    /// Open file table
    open_files: Mutex<BTreeMap<FileHandle, OpenFile>>,

    /// Next file handle
    next_handle: AtomicU64,

    /// Request counter for generating IDs
    request_counter: AtomicU64,
}

impl VfsServer {
    /// Create new VFS server
    pub fn new() -> Self {
        Self {
            mounts: RwLock::new(Vec::new()),
            open_files: Mutex::new(BTreeMap::new()),
            next_handle: AtomicU64::new(1),
            request_counter: AtomicU64::new(1),
        }
    }

    /// Mount a filesystem at the given path
    pub fn mount(
        &self,
        path: &str,
        fs: Arc<dyn Filesystem>,
        device: Option<&str>,
        read_only: bool,
    ) -> Result<(), VfsError> {
        let mut mounts = self.mounts.write();

        // Check mount limit
        if mounts.len() >= MAX_MOUNTS {
            return Err(VfsError::NoSpace);
        }

        // Check for existing mount at same path
        if mounts.iter().any(|m| m.path == path) {
            return Err(VfsError::AlreadyExists);
        }

        // Normalize path
        let path = Self::normalize_path(path);

        mounts.push(MountPoint {
            path,
            fs,
            read_only,
            device: device.map(|s| s.to_string()),
        });

        // Sort by path length (longest first for matching)
        mounts.sort_by(|a, b| b.path.len().cmp(&a.path.len()));

        Ok(())
    }

    /// Unmount a filesystem
    pub fn unmount(&self, path: &str) -> Result<(), VfsError> {
        let mut mounts = self.mounts.write();
        let path = Self::normalize_path(path);

        // Find mount
        let idx = mounts
            .iter()
            .position(|m| m.path == path)
            .ok_or(VfsError::NotFound)?;

        // Check for open files on this mount
        let open_files = self.open_files.lock();
        if open_files.values().any(|f| f.mount_idx == idx) {
            return Err(VfsError::Busy);
        }
        drop(open_files);

        mounts.remove(idx);
        Ok(())
    }

    /// Resolve path to mount point and relative path
    fn resolve_mount(&self, path: &str) -> Result<(usize, String), VfsError> {
        let path = Self::normalize_path(path);
        let mounts = self.mounts.read();

        for (idx, mount) in mounts.iter().enumerate() {
            if path.starts_with(&mount.path) {
                let relative = if path.len() == mount.path.len() {
                    String::from("/")
                } else if path.chars().nth(mount.path.len()) == Some('/') {
                    path[mount.path.len()..].to_string()
                } else if mount.path == "/" {
                    path.clone()
                } else {
                    continue;
                };
                return Ok((idx, relative));
            }
        }

        Err(VfsError::NotFound)
    }

    /// Resolve path to inode
    fn resolve_path(&self, path: &str) -> Result<(usize, InodeNum), VfsError> {
        let (mount_idx, relative) = self.resolve_mount(path)?;
        let mounts = self.mounts.read();
        let fs = &mounts[mount_idx].fs;

        let mut current_ino = fs.root_ino();

        for component in relative.split('/').filter(|s| !s.is_empty()) {
            current_ino = fs.lookup(current_ino, component)?;
        }

        Ok((mount_idx, current_ino))
    }

    /// Open a file
    pub fn open(&self, path: &str, flags: OpenFlags, mode: u32) -> Result<FileHandle, VfsError> {
        let (mount_idx, relative) = self.resolve_mount(path)?;
        let mounts = self.mounts.read();
        let mount = &mounts[mount_idx];

        // Check write permission
        if (flags.write || flags.truncate) && mount.read_only {
            return Err(VfsError::ReadOnlyFs);
        }

        let fs = &mount.fs;
        let ino;

        // Resolve path
        let parent_path = Self::parent_path(&relative);
        let name = Self::basename(&relative);

        if name.is_empty() {
            // Opening root
            ino = fs.root_ino();
        } else {
            // Try to find existing file
            let parent_ino = if parent_path.is_empty() || parent_path == "/" {
                fs.root_ino()
            } else {
                let mut curr = fs.root_ino();
                for comp in parent_path.split('/').filter(|s| !s.is_empty()) {
                    curr = fs.lookup(curr, comp)?;
                }
                curr
            };

            match fs.lookup(parent_ino, name) {
                Ok(found_ino) => {
                    if flags.create && flags.exclusive {
                        return Err(VfsError::AlreadyExists);
                    }
                    ino = found_ino;
                }
                Err(VfsError::NotFound) if flags.create => {
                    ino = fs.create(parent_ino, name, mode)?;
                }
                Err(e) => return Err(e),
            }
        }

        // Get file info
        let attr = fs.getattr(ino)?;

        // Can't open directory for writing
        if flags.write && attr.file_type == VfsFileType::Directory {
            return Err(VfsError::IsADirectory);
        }

        // Truncate if requested
        if flags.truncate {
            drop(mounts);
            let mounts = self.mounts.read();
            mounts[mount_idx].fs.truncate(ino, 0)?;
        }

        // Check open file limit
        let mut open_files = self.open_files.lock();
        if open_files.len() >= MAX_OPEN_FILES {
            return Err(VfsError::TooManyOpenFiles);
        }

        // Create handle
        let handle = FileHandle(self.next_handle.fetch_add(1, Ordering::Relaxed));

        open_files.insert(
            handle,
            OpenFile {
                mount_idx,
                ino,
                position: if flags.append { attr.size } else { 0 },
                flags,
                size: attr.size,
            },
        );

        Ok(handle)
    }

    /// Close a file
    pub fn close(&self, handle: FileHandle) -> Result<(), VfsError> {
        let mut open_files = self.open_files.lock();
        open_files
            .remove(&handle)
            .ok_or(VfsError::BadHandle)?;
        Ok(())
    }

    /// Read from file
    pub fn read(
        &self,
        handle: FileHandle,
        offset: Option<u64>,
        len: usize,
    ) -> Result<Vec<u8>, VfsError> {
        let mut open_files = self.open_files.lock();
        let file = open_files.get_mut(&handle).ok_or(VfsError::BadHandle)?;

        if !file.flags.read {
            return Err(VfsError::PermissionDenied);
        }

        let read_offset = offset.unwrap_or(file.position);
        let mounts = self.mounts.read();
        let fs = &mounts[file.mount_idx].fs;

        let data = fs.read(file.ino, read_offset, len)?;

        // Update position
        if offset.is_none() {
            file.position += data.len() as u64;
        }

        Ok(data)
    }

    /// Write to file
    pub fn write(&self, handle: FileHandle, offset: Option<u64>, data: &[u8]) -> Result<usize, VfsError> {
        let mut open_files = self.open_files.lock();
        let file = open_files.get_mut(&handle).ok_or(VfsError::BadHandle)?;

        if !file.flags.write {
            return Err(VfsError::PermissionDenied);
        }

        let write_offset = if file.flags.append {
            file.size
        } else {
            offset.unwrap_or(file.position)
        };

        let mounts = self.mounts.read();
        let mount = &mounts[file.mount_idx];

        if mount.read_only {
            return Err(VfsError::ReadOnlyFs);
        }

        let written = mount.fs.write(file.ino, write_offset, data)?;

        // Update position and size
        if offset.is_none() {
            file.position = write_offset + written as u64;
        }
        file.size = file.size.max(write_offset + written as u64);

        Ok(written)
    }

    /// Get file attributes
    pub fn stat(&self, path: &str) -> Result<VfsAttr, VfsError> {
        let (mount_idx, ino) = self.resolve_path(path)?;
        let mounts = self.mounts.read();
        mounts[mount_idx].fs.getattr(ino)
    }

    /// Get file attributes by handle
    pub fn fstat(&self, handle: FileHandle) -> Result<VfsAttr, VfsError> {
        let open_files = self.open_files.lock();
        let file = open_files.get(&handle).ok_or(VfsError::BadHandle)?;

        let mounts = self.mounts.read();
        mounts[file.mount_idx].fs.getattr(file.ino)
    }

    /// Read directory
    pub fn readdir(&self, path: &str) -> Result<Vec<VfsDirEntry>, VfsError> {
        let (mount_idx, ino) = self.resolve_path(path)?;
        let mounts = self.mounts.read();
        let fs = &mounts[mount_idx].fs;

        // Verify it's a directory
        let attr = fs.getattr(ino)?;
        if attr.file_type != VfsFileType::Directory {
            return Err(VfsError::NotADirectory);
        }

        fs.readdir(ino)
    }

    /// Create directory
    pub fn mkdir(&self, path: &str, mode: u32) -> Result<(), VfsError> {
        let (mount_idx, relative) = self.resolve_mount(path)?;
        let mounts = self.mounts.read();
        let mount = &mounts[mount_idx];

        if mount.read_only {
            return Err(VfsError::ReadOnlyFs);
        }

        let parent_path = Self::parent_path(&relative);
        let name = Self::basename(&relative);

        let parent_ino = if parent_path.is_empty() || parent_path == "/" {
            mount.fs.root_ino()
        } else {
            let mut curr = mount.fs.root_ino();
            for comp in parent_path.split('/').filter(|s| !s.is_empty()) {
                curr = mount.fs.lookup(curr, comp)?;
            }
            curr
        };

        mount.fs.mkdir(parent_ino, name, mode)?;
        Ok(())
    }

    /// Remove directory
    pub fn rmdir(&self, path: &str) -> Result<(), VfsError> {
        let (mount_idx, relative) = self.resolve_mount(path)?;
        let mounts = self.mounts.read();
        let mount = &mounts[mount_idx];

        if mount.read_only {
            return Err(VfsError::ReadOnlyFs);
        }

        let parent_path = Self::parent_path(&relative);
        let name = Self::basename(&relative);

        let parent_ino = if parent_path.is_empty() || parent_path == "/" {
            mount.fs.root_ino()
        } else {
            let mut curr = mount.fs.root_ino();
            for comp in parent_path.split('/').filter(|s| !s.is_empty()) {
                curr = mount.fs.lookup(curr, comp)?;
            }
            curr
        };

        mount.fs.rmdir(parent_ino, name)
    }

    /// Unlink file
    pub fn unlink(&self, path: &str) -> Result<(), VfsError> {
        let (mount_idx, relative) = self.resolve_mount(path)?;
        let mounts = self.mounts.read();
        let mount = &mounts[mount_idx];

        if mount.read_only {
            return Err(VfsError::ReadOnlyFs);
        }

        let parent_path = Self::parent_path(&relative);
        let name = Self::basename(&relative);

        let parent_ino = if parent_path.is_empty() || parent_path == "/" {
            mount.fs.root_ino()
        } else {
            let mut curr = mount.fs.root_ino();
            for comp in parent_path.split('/').filter(|s| !s.is_empty()) {
                curr = mount.fs.lookup(curr, comp)?;
            }
            curr
        };

        mount.fs.unlink(parent_ino, name)
    }

    /// Rename a file or directory
    pub fn rename(&self, old_path: &str, new_path: &str) -> Result<(), VfsError> {
        let (old_mount_idx, old_relative) = self.resolve_mount(old_path)?;
        let (new_mount_idx, new_relative) = self.resolve_mount(new_path)?;

        // Cannot rename across mount points
        if old_mount_idx != new_mount_idx {
            return Err(VfsError::CrossDevice);
        }

        let mounts = self.mounts.read();
        let mount = &mounts[old_mount_idx];

        if mount.read_only {
            return Err(VfsError::ReadOnlyFs);
        }

        let old_parent_path = Self::parent_path(&old_relative);
        let old_name = Self::basename(&old_relative);
        let new_parent_path = Self::parent_path(&new_relative);
        let new_name = Self::basename(&new_relative);

        let fs = &mount.fs;

        // Resolve old parent
        let old_parent_ino = if old_parent_path.is_empty() || old_parent_path == "/" {
            fs.root_ino()
        } else {
            let mut curr = fs.root_ino();
            for comp in old_parent_path.split('/').filter(|s| !s.is_empty()) {
                curr = fs.lookup(curr, comp)?;
            }
            curr
        };

        // Resolve new parent
        let new_parent_ino = if new_parent_path.is_empty() || new_parent_path == "/" {
            fs.root_ino()
        } else {
            let mut curr = fs.root_ino();
            for comp in new_parent_path.split('/').filter(|s| !s.is_empty()) {
                curr = fs.lookup(curr, comp)?;
            }
            curr
        };

        fs.rename(old_parent_ino, old_name, new_parent_ino, new_name)
    }

    /// Create a symbolic link
    pub fn symlink(&self, target: &str, link_path: &str) -> Result<(), VfsError> {
        let (mount_idx, relative) = self.resolve_mount(link_path)?;
        let mounts = self.mounts.read();
        let mount = &mounts[mount_idx];

        if mount.read_only {
            return Err(VfsError::ReadOnlyFs);
        }

        let parent_path = Self::parent_path(&relative);
        let name = Self::basename(&relative);

        let parent_ino = if parent_path.is_empty() || parent_path == "/" {
            mount.fs.root_ino()
        } else {
            let mut curr = mount.fs.root_ino();
            for comp in parent_path.split('/').filter(|s| !s.is_empty()) {
                curr = mount.fs.lookup(curr, comp)?;
            }
            curr
        };

        mount.fs.symlink(parent_ino, name, target)?;
        Ok(())
    }

    /// Read the target of a symbolic link
    pub fn readlink(&self, path: &str) -> Result<String, VfsError> {
        let (mount_idx, ino) = self.resolve_path(path)?;
        let mounts = self.mounts.read();
        let fs = &mounts[mount_idx].fs;

        // Verify it's a symlink
        let attr = fs.getattr(ino)?;
        if attr.file_type != VfsFileType::Symlink {
            return Err(VfsError::InvalidArgument);
        }

        fs.readlink(ino)
    }

    /// Truncate a file to a specified length
    pub fn truncate(&self, path: &str, length: u64) -> Result<(), VfsError> {
        let (mount_idx, ino) = self.resolve_path(path)?;
        let mounts = self.mounts.read();
        let mount = &mounts[mount_idx];

        if mount.read_only {
            return Err(VfsError::ReadOnlyFs);
        }

        // Verify it's a regular file
        let attr = mount.fs.getattr(ino)?;
        if attr.file_type != VfsFileType::Regular {
            return Err(VfsError::IsADirectory);
        }

        mount.fs.truncate(ino, length)
    }

    /// Sync a file handle to disk
    pub fn sync_handle(&self, handle: FileHandle) -> Result<(), VfsError> {
        let open_files = self.open_files.lock();
        let file = open_files.get(&handle).ok_or(VfsError::BadHandle)?;
        let mount_idx = file.mount_idx;
        drop(open_files);

        let mounts = self.mounts.read();
        mounts[mount_idx].fs.sync()
    }

    /// Seek in file
    pub fn seek(&self, handle: FileHandle, offset: i64, whence: SeekFrom) -> Result<u64, VfsError> {
        let mut open_files = self.open_files.lock();
        let file = open_files.get_mut(&handle).ok_or(VfsError::BadHandle)?;

        let new_pos = match whence {
            SeekFrom::Start => {
                if offset < 0 {
                    return Err(VfsError::InvalidArgument);
                }
                offset as u64
            }
            SeekFrom::Current => {
                if offset < 0 {
                    file.position.saturating_sub((-offset) as u64)
                } else {
                    file.position.saturating_add(offset as u64)
                }
            }
            SeekFrom::End => {
                if offset < 0 {
                    file.size.saturating_sub((-offset) as u64)
                } else {
                    file.size.saturating_add(offset as u64)
                }
            }
        };

        file.position = new_pos;
        Ok(new_pos)
    }

    /// Get filesystem statistics
    pub fn statfs(&self, path: &str) -> Result<StatFs, VfsError> {
        let (mount_idx, _) = self.resolve_mount(path)?;
        let mounts = self.mounts.read();
        mounts[mount_idx].fs.statfs()
    }

    /// Handle incoming VFS request
    pub fn handle_request(&self, request: VfsRequest) -> VfsResponse {
        match request {
            VfsRequest::Mount {
                request_id,
                device: _,
                mount_point: _,
                fs_type: _,
                flags: _,
            } => {
                // Mount requires filesystem factory - handled externally
                VfsResponse::error(request_id, VfsError::NotSupported)
            }

            VfsRequest::Unmount {
                request_id,
                mount_point,
            } => match self.unmount(&mount_point) {
                Ok(()) => VfsResponse::ok(request_id),
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Open {
                request_id,
                path,
                flags,
                mode,
            } => match self.open(&path, flags, mode) {
                Ok(handle) => VfsResponse::Handle { request_id, handle },
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Close { request_id, handle } => match self.close(handle) {
                Ok(()) => VfsResponse::ok(request_id),
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Read {
                request_id,
                handle,
                offset,
                len,
            } => match self.read(handle, Some(offset), len) {
                Ok(data) => VfsResponse::Data { request_id, data },
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Write {
                request_id,
                handle,
                offset,
                data,
            } => match self.write(handle, Some(offset), &data) {
                Ok(bytes_written) => VfsResponse::Written {
                    request_id,
                    bytes_written,
                },
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Stat { request_id, path } => match self.stat(&path) {
                Ok(attr) => VfsResponse::Attr { request_id, attr },
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Fstat { request_id, handle } => match self.fstat(handle) {
                Ok(attr) => VfsResponse::Attr { request_id, attr },
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Readdir { request_id, path } => match self.readdir(&path) {
                Ok(entries) => VfsResponse::DirEntries { request_id, entries },
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Mkdir {
                request_id,
                path,
                mode,
            } => match self.mkdir(&path, mode) {
                Ok(()) => VfsResponse::ok(request_id),
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Rmdir { request_id, path } => match self.rmdir(&path) {
                Ok(()) => VfsResponse::ok(request_id),
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Unlink { request_id, path } => match self.unlink(&path) {
                Ok(()) => VfsResponse::ok(request_id),
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Rename {
                request_id,
                old_path,
                new_path,
            } => match self.rename(&old_path, &new_path) {
                Ok(()) => VfsResponse::ok(request_id),
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Symlink {
                request_id,
                target,
                link_path,
            } => match self.symlink(&target, &link_path) {
                Ok(()) => VfsResponse::ok(request_id),
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Readlink { request_id, path } => match self.readlink(&path) {
                Ok(target) => VfsResponse::Link { request_id, target },
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Truncate {
                request_id,
                path,
                length,
            } => match self.truncate(&path, length) {
                Ok(()) => VfsResponse::ok(request_id),
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Sync {
                request_id,
                handle,
            } => match handle {
                Some(h) => match self.sync_handle(h) {
                    Ok(()) => VfsResponse::ok(request_id),
                    Err(e) => VfsResponse::error(request_id, e),
                },
                None => {
                    // Global sync - sync all mounted filesystems
                    let mounts = self.mounts.read();
                    for mount in mounts.iter() {
                        let _ = mount.fs.sync();
                    }
                    VfsResponse::ok(request_id)
                }
            },

            VfsRequest::Seek {
                request_id,
                handle,
                offset,
                whence,
            } => match self.seek(handle, offset, whence) {
                Ok(position) => VfsResponse::Position {
                    request_id,
                    position,
                },
                Err(e) => VfsResponse::error(request_id, e),
            },

            VfsRequest::Statfs { request_id, path } => match self.statfs(&path) {
                Ok(stats) => VfsResponse::FsStat {
                    request_id,
                    stats,
                },
                Err(e) => VfsResponse::error(request_id, e),
            },
        }
    }

    // === Utility Functions ===

    fn normalize_path(path: &str) -> String {
        let mut result = String::new();

        for component in path.split('/').filter(|s| !s.is_empty()) {
            if component == "." {
                continue;
            } else if component == ".." {
                // Remove last component
                if let Some(pos) = result.rfind('/') {
                    result.truncate(pos);
                }
            } else {
                result.push('/');
                result.push_str(component);
            }
        }

        if result.is_empty() {
            String::from("/")
        } else {
            result
        }
    }

    fn parent_path(path: &str) -> String {
        if let Some(pos) = path.rfind('/') {
            if pos == 0 {
                String::from("/")
            } else {
                path[..pos].to_string()
            }
        } else {
            String::from("/")
        }
    }

    fn basename(path: &str) -> &str {
        path.rsplit('/').find(|s| !s.is_empty()).unwrap_or("")
    }
}

impl Default for VfsServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(VfsServer::normalize_path("/"), "/");
        assert_eq!(VfsServer::normalize_path("/foo/bar"), "/foo/bar");
        assert_eq!(VfsServer::normalize_path("/foo//bar"), "/foo/bar");
        assert_eq!(VfsServer::normalize_path("/foo/./bar"), "/foo/bar");
        assert_eq!(VfsServer::normalize_path("/foo/../bar"), "/bar");
        assert_eq!(VfsServer::normalize_path("foo/bar"), "/foo/bar");
    }

    #[test]
    fn test_parent_path() {
        assert_eq!(VfsServer::parent_path("/foo/bar"), "/foo");
        assert_eq!(VfsServer::parent_path("/foo"), "/");
        assert_eq!(VfsServer::parent_path("/"), "/");
    }

    #[test]
    fn test_basename() {
        assert_eq!(VfsServer::basename("/foo/bar"), "bar");
        assert_eq!(VfsServer::basename("/foo"), "foo");
        assert_eq!(VfsServer::basename("/"), "");
    }
}

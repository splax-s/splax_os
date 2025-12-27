//! # RamFS - In-Memory Filesystem
//!
//! A VFS-compatible RAM filesystem implementation.
//!
//! ## Features
//!
//! - Full VFS trait implementation
//! - In-memory storage (volatile)
//! - Fast operations (no I/O)
//! - Configurable size limits

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::RwLock;

use super::vfs::{
    Filesystem, InodeNum, VfsAttr, VfsDirEntry, VfsError, VfsFileType, VfsPermissions, VfsStatFs,
};

/// Maximum file size (4 MB)
pub const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;

/// Maximum filename length
pub const MAX_NAME_LEN: usize = 255;

/// RamFS inode
#[derive(Debug, Clone)]
struct RamInode {
    /// Inode number
    ino: InodeNum,
    /// File type
    file_type: VfsFileType,
    /// File content (for regular files)
    content: Vec<u8>,
    /// Directory entries (for directories): name -> inode number
    children: BTreeMap<String, InodeNum>,
    /// Symlink target (for symlinks)
    link_target: Option<String>,
    /// Permissions
    perm: VfsPermissions,
    /// Hard link count
    nlink: u32,
    /// Access time
    atime: u64,
    /// Modification time
    mtime: u64,
    /// Status change time
    ctime: u64,
    /// Creation time
    crtime: u64,
}

impl RamInode {
    fn new_file(ino: InodeNum) -> Self {
        let now = crate::arch::x86_64::interrupts::get_ticks();
        Self {
            ino,
            file_type: VfsFileType::Regular,
            content: Vec::new(),
            children: BTreeMap::new(),
            link_target: None,
            perm: VfsPermissions::default(),
            nlink: 1,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
        }
    }

    fn new_directory(ino: InodeNum) -> Self {
        let now = crate::arch::x86_64::interrupts::get_ticks();
        Self {
            ino,
            file_type: VfsFileType::Directory,
            content: Vec::new(),
            children: BTreeMap::new(),
            link_target: None,
            perm: VfsPermissions {
                readable: true,
                writable: true,
                executable: true,
            },
            nlink: 2, // . and parent
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
        }
    }

    fn new_symlink(ino: InodeNum, target: &str) -> Self {
        let now = crate::arch::x86_64::interrupts::get_ticks();
        Self {
            ino,
            file_type: VfsFileType::Symlink,
            content: Vec::new(),
            children: BTreeMap::new(),
            link_target: Some(String::from(target)),
            perm: VfsPermissions {
                readable: true,
                writable: false,
                executable: false,
            },
            nlink: 1,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
        }
    }

    fn to_attr(&self) -> VfsAttr {
        VfsAttr {
            ino: self.ino,
            file_type: self.file_type,
            perm: self.perm,
            size: self.content.len() as u64,
            nlink: self.nlink,
            blksize: 4096,
            blocks: (self.content.len() as u64 + 511) / 512,
            atime: self.atime,
            mtime: self.mtime,
            ctime: self.ctime,
            crtime: self.crtime,
        }
    }

    fn touch_access(&mut self) {
        self.atime = crate::arch::x86_64::interrupts::get_ticks();
    }

    fn touch_modify(&mut self) {
        let now = crate::arch::x86_64::interrupts::get_ticks();
        self.mtime = now;
        self.ctime = now;
    }
}

/// RamFS filesystem
pub struct RamFs {
    /// All inodes by inode number
    inodes: RwLock<BTreeMap<InodeNum, RamInode>>,
    /// Next inode number
    next_ino: spin::Mutex<InodeNum>,
    /// Total bytes used
    bytes_used: spin::Mutex<usize>,
    /// Maximum size in bytes
    max_size: usize,
}

impl RamFs {
    /// Create a new RamFS with the given maximum size
    pub fn new(max_size: usize) -> Self {
        let mut inodes = BTreeMap::new();
        
        // Create root directory (inode 1)
        let root = RamInode::new_directory(1);
        inodes.insert(1, root);
        
        Self {
            inodes: RwLock::new(inodes),
            next_ino: spin::Mutex::new(2),
            bytes_used: spin::Mutex::new(0),
            max_size,
        }
    }

    /// Allocate a new inode number
    fn alloc_ino(&self) -> InodeNum {
        let mut next = self.next_ino.lock();
        let ino = *next;
        *next += 1;
        ino
    }

    /// Get total space used
    pub fn bytes_used(&self) -> usize {
        *self.bytes_used.lock()
    }

    /// Get maximum size
    pub fn max_size(&self) -> usize {
        self.max_size
    }

    /// Get inode count
    pub fn inode_count(&self) -> usize {
        self.inodes.read().len()
    }
}

impl Filesystem for RamFs {
    fn name(&self) -> &'static str {
        "ramfs"
    }

    fn lookup(&self, parent: InodeNum, name: &str) -> Result<InodeNum, VfsError> {
        let inodes = self.inodes.read();
        
        let parent_inode = inodes.get(&parent).ok_or(VfsError::NotFound)?;
        
        if parent_inode.file_type != VfsFileType::Directory {
            return Err(VfsError::NotADirectory);
        }
        
        parent_inode.children.get(name).copied().ok_or(VfsError::NotFound)
    }

    fn getattr(&self, ino: InodeNum) -> Result<VfsAttr, VfsError> {
        let inodes = self.inodes.read();
        let inode = inodes.get(&ino).ok_or(VfsError::NotFound)?;
        Ok(inode.to_attr())
    }

    fn readdir(&self, ino: InodeNum) -> Result<Vec<VfsDirEntry>, VfsError> {
        let inodes = self.inodes.read();
        let inode = inodes.get(&ino).ok_or(VfsError::NotFound)?;
        
        if inode.file_type != VfsFileType::Directory {
            return Err(VfsError::NotADirectory);
        }
        
        let mut entries = Vec::new();
        
        // Add . and ..
        entries.push(VfsDirEntry {
            name: String::from("."),
            ino,
            file_type: VfsFileType::Directory,
        });
        entries.push(VfsDirEntry {
            name: String::from(".."),
            ino, // For simplicity, point to self
            file_type: VfsFileType::Directory,
        });
        
        // Add children
        for (name, &child_ino) in &inode.children {
            if let Some(child) = inodes.get(&child_ino) {
                entries.push(VfsDirEntry {
                    name: name.clone(),
                    ino: child_ino,
                    file_type: child.file_type,
                });
            }
        }
        
        Ok(entries)
    }

    fn read(&self, ino: InodeNum, offset: u64, size: usize) -> Result<Vec<u8>, VfsError> {
        let mut inodes = self.inodes.write();
        let inode = inodes.get_mut(&ino).ok_or(VfsError::NotFound)?;
        
        if inode.file_type != VfsFileType::Regular {
            return Err(VfsError::NotAFile);
        }
        
        inode.touch_access();
        
        let offset = offset as usize;
        if offset >= inode.content.len() {
            return Ok(Vec::new());
        }
        
        let end = (offset + size).min(inode.content.len());
        Ok(inode.content[offset..end].to_vec())
    }

    fn write(&self, ino: InodeNum, offset: u64, data: &[u8]) -> Result<usize, VfsError> {
        let mut inodes = self.inodes.write();
        let inode = inodes.get_mut(&ino).ok_or(VfsError::NotFound)?;
        
        if inode.file_type != VfsFileType::Regular {
            return Err(VfsError::NotAFile);
        }
        
        let offset = offset as usize;
        let new_len = offset + data.len();
        
        if new_len > MAX_FILE_SIZE {
            return Err(VfsError::NoSpace);
        }
        
        // Check total space
        let old_len = inode.content.len();
        let additional = if new_len > old_len { new_len - old_len } else { 0 };
        
        {
            let mut bytes_used = self.bytes_used.lock();
            if *bytes_used + additional > self.max_size {
                return Err(VfsError::NoSpace);
            }
            *bytes_used += additional;
        }
        
        // Extend content if needed
        if offset > inode.content.len() {
            inode.content.resize(offset, 0);
        }
        
        if new_len > inode.content.len() {
            inode.content.resize(new_len, 0);
        }
        
        // Write data
        inode.content[offset..offset + data.len()].copy_from_slice(data);
        inode.touch_modify();
        
        Ok(data.len())
    }

    fn create(&self, parent: InodeNum, name: &str, file_type: VfsFileType) -> Result<InodeNum, VfsError> {
        if name.len() > MAX_NAME_LEN {
            return Err(VfsError::InvalidArgument);
        }
        
        let ino = self.alloc_ino();
        let mut inodes = self.inodes.write();
        
        // Check parent exists and is directory
        let parent_inode = inodes.get_mut(&parent).ok_or(VfsError::NotFound)?;
        if parent_inode.file_type != VfsFileType::Directory {
            return Err(VfsError::NotADirectory);
        }
        
        // Check name doesn't exist
        if parent_inode.children.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        
        // Create new inode
        let new_inode = match file_type {
            VfsFileType::Regular => RamInode::new_file(ino),
            VfsFileType::Directory => RamInode::new_directory(ino),
            _ => return Err(VfsError::NotSupported),
        };
        
        // Add to parent
        parent_inode.children.insert(String::from(name), ino);
        parent_inode.touch_modify();
        if file_type == VfsFileType::Directory {
            parent_inode.nlink += 1; // Subdirectory's ..
        }
        
        // Insert inode
        inodes.insert(ino, new_inode);
        
        Ok(ino)
    }

    fn unlink(&self, parent: InodeNum, name: &str) -> Result<(), VfsError> {
        let mut inodes = self.inodes.write();
        
        // Get parent and find child
        let parent_inode = inodes.get_mut(&parent).ok_or(VfsError::NotFound)?;
        if parent_inode.file_type != VfsFileType::Directory {
            return Err(VfsError::NotADirectory);
        }
        
        let child_ino = *parent_inode.children.get(name).ok_or(VfsError::NotFound)?;
        
        // Check if child is a directory and not empty
        if let Some(child) = inodes.get(&child_ino) {
            if child.file_type == VfsFileType::Directory && !child.children.is_empty() {
                return Err(VfsError::NotEmpty);
            }
        }
        
        // Remove from parent
        let parent_inode = inodes.get_mut(&parent).unwrap();
        parent_inode.children.remove(name);
        parent_inode.touch_modify();
        
        // Decrease link count and potentially remove
        if let Some(child) = inodes.get_mut(&child_ino) {
            let freed_bytes = child.content.len();
            let is_dir = child.file_type == VfsFileType::Directory;
            child.nlink -= 1;
            
            if child.nlink == 0 {
                // Free the inode
                *self.bytes_used.lock() -= freed_bytes;
                inodes.remove(&child_ino);
            }
            
            if is_dir {
                // Decrease parent's link count
                if let Some(parent) = inodes.get_mut(&parent) {
                    parent.nlink = parent.nlink.saturating_sub(1);
                }
            }
        }
        
        Ok(())
    }

    fn rename(&self, old_parent: InodeNum, old_name: &str, new_parent: InodeNum, new_name: &str) -> Result<(), VfsError> {
        if new_name.len() > MAX_NAME_LEN {
            return Err(VfsError::InvalidArgument);
        }
        
        let mut inodes = self.inodes.write();
        
        // Get the inode being moved
        let old_parent_inode = inodes.get(&old_parent).ok_or(VfsError::NotFound)?;
        let ino = *old_parent_inode.children.get(old_name).ok_or(VfsError::NotFound)?;
        
        // Check new parent is directory
        let new_parent_inode = inodes.get(&new_parent).ok_or(VfsError::NotFound)?;
        if new_parent_inode.file_type != VfsFileType::Directory {
            return Err(VfsError::NotADirectory);
        }
        
        // Remove from old parent
        let old_parent_inode = inodes.get_mut(&old_parent).unwrap();
        old_parent_inode.children.remove(old_name);
        old_parent_inode.touch_modify();
        
        // Remove existing entry at new location if any
        let new_parent_inode = inodes.get_mut(&new_parent).unwrap();
        if let Some(&existing) = new_parent_inode.children.get(new_name) {
            // Check if we can replace
            if let Some(existing_inode) = inodes.get(&existing) {
                if existing_inode.file_type == VfsFileType::Directory && !existing_inode.children.is_empty() {
                    // Restore old parent entry and fail
                    let old_parent_inode = inodes.get_mut(&old_parent).unwrap();
                    old_parent_inode.children.insert(String::from(old_name), ino);
                    return Err(VfsError::NotEmpty);
                }
            }
            // Remove existing
            inodes.remove(&existing);
        }
        
        // Add to new parent
        let new_parent_inode = inodes.get_mut(&new_parent).unwrap();
        new_parent_inode.children.insert(String::from(new_name), ino);
        new_parent_inode.touch_modify();
        
        Ok(())
    }

    fn truncate(&self, ino: InodeNum, size: u64) -> Result<(), VfsError> {
        let mut inodes = self.inodes.write();
        let inode = inodes.get_mut(&ino).ok_or(VfsError::NotFound)?;
        
        if inode.file_type != VfsFileType::Regular {
            return Err(VfsError::NotAFile);
        }
        
        let old_len = inode.content.len();
        let new_len = size as usize;
        
        if new_len > MAX_FILE_SIZE {
            return Err(VfsError::NoSpace);
        }
        
        // Update bytes used
        {
            let mut bytes_used = self.bytes_used.lock();
            if new_len > old_len {
                let additional = new_len - old_len;
                if *bytes_used + additional > self.max_size {
                    return Err(VfsError::NoSpace);
                }
                *bytes_used += additional;
            } else {
                *bytes_used -= old_len - new_len;
            }
        }
        
        inode.content.resize(new_len, 0);
        inode.touch_modify();
        
        Ok(())
    }

    fn sync(&self) -> Result<(), VfsError> {
        // RamFS is always in sync (no backing store)
        Ok(())
    }

    fn statfs(&self) -> Result<VfsStatFs, VfsError> {
        let inodes = self.inodes.read();
        let bytes_used = *self.bytes_used.lock();
        
        let block_size = 4096u32;
        let total_blocks = (self.max_size / block_size as usize) as u64;
        let used_blocks = (bytes_used / block_size as usize) as u64;
        
        Ok(VfsStatFs {
            blocks: total_blocks,
            bfree: total_blocks.saturating_sub(used_blocks),
            bavail: total_blocks.saturating_sub(used_blocks),
            files: inodes.len() as u64 + 1000, // Room for more
            ffree: 1000,
            bsize: block_size,
            namelen: MAX_NAME_LEN as u32,
        })
    }

    fn readlink(&self, ino: InodeNum) -> Result<String, VfsError> {
        let inodes = self.inodes.read();
        let inode = inodes.get(&ino).ok_or(VfsError::NotFound)?;
        
        if inode.file_type != VfsFileType::Symlink {
            return Err(VfsError::InvalidArgument);
        }
        
        inode.link_target.clone().ok_or(VfsError::IoError)
    }

    fn symlink(&self, parent: InodeNum, name: &str, target: &str) -> Result<InodeNum, VfsError> {
        if name.len() > MAX_NAME_LEN {
            return Err(VfsError::InvalidArgument);
        }
        
        let ino = self.alloc_ino();
        let mut inodes = self.inodes.write();
        
        let parent_inode = inodes.get_mut(&parent).ok_or(VfsError::NotFound)?;
        if parent_inode.file_type != VfsFileType::Directory {
            return Err(VfsError::NotADirectory);
        }
        
        if parent_inode.children.contains_key(name) {
            return Err(VfsError::AlreadyExists);
        }
        
        let new_inode = RamInode::new_symlink(ino, target);
        
        parent_inode.children.insert(String::from(name), ino);
        parent_inode.touch_modify();
        
        inodes.insert(ino, new_inode);
        
        Ok(ino)
    }

    fn link(&self, ino: InodeNum, new_parent: InodeNum, new_name: &str) -> Result<(), VfsError> {
        if new_name.len() > MAX_NAME_LEN {
            return Err(VfsError::InvalidArgument);
        }
        
        let mut inodes = self.inodes.write();
        
        // Check source exists and is not a directory
        let inode = inodes.get_mut(&ino).ok_or(VfsError::NotFound)?;
        if inode.file_type == VfsFileType::Directory {
            return Err(VfsError::NotSupported); // Hard links to directories not allowed
        }
        inode.nlink += 1;
        inode.ctime = crate::arch::x86_64::interrupts::get_ticks();
        
        // Add to new parent
        let parent_inode = inodes.get_mut(&new_parent).ok_or(VfsError::NotFound)?;
        if parent_inode.file_type != VfsFileType::Directory {
            return Err(VfsError::NotADirectory);
        }
        if parent_inode.children.contains_key(new_name) {
            // Rollback nlink
            if let Some(inode) = inodes.get_mut(&ino) {
                inode.nlink -= 1;
            }
            return Err(VfsError::AlreadyExists);
        }
        
        parent_inode.children.insert(String::from(new_name), ino);
        parent_inode.touch_modify();
        
        Ok(())
    }

    fn setattr(&self, ino: InodeNum, attr: &VfsAttr) -> Result<(), VfsError> {
        let mut inodes = self.inodes.write();
        let inode = inodes.get_mut(&ino).ok_or(VfsError::NotFound)?;
        
        // Update permissions
        inode.perm = attr.perm;
        
        // Update times
        inode.atime = attr.atime;
        inode.mtime = attr.mtime;
        inode.ctime = crate::arch::x86_64::interrupts::get_ticks();
        
        Ok(())
    }
}

/// Create a new RamFS instance
pub fn new(max_size: usize) -> Arc<RamFs> {
    Arc::new(RamFs::new(max_size))
}

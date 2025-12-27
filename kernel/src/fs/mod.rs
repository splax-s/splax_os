//! # Filesystem Subsystem
//!
//! Splax OS filesystem support:
//! - VFS: Virtual Filesystem layer
//! - RamFS: VFS-compatible in-memory filesystem
//! - ProcFS: Process/system information (/proc)
//! - DevFS: Device nodes (/dev)
//! - SysFS: Kernel objects (/sys)
//! - SplaxFS: On-disk persistent filesystem
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              VFS Layer                       │
//! │  (mount, open, read, write, close, etc.)    │
//! └─────────────┬───────────────────────────────┘
//!               │
//!     ┌─────────┼─────────┬─────────┬─────────┐
//!     ▼         ▼         ▼         ▼         ▼
//! ┌───────┐ ┌───────┐ ┌───────┐ ┌───────┐ ┌───────┐
//! │ RamFS │ │ProcFS │ │ DevFS │ │ SysFS │ │SplaxFS│
//! └───────┘ └───────┘ └───────┘ └───────┘ └───────┘
//! ```
//!
//! ## API
//!
//! The filesystem uses capability-gated access. Operations require
//! appropriate capabilities for the target path.

pub mod vfs;
pub mod ramfs;
pub mod splaxfs;
pub mod procfs;
pub mod devfs;
pub mod sysfs;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// Maximum file size (4 MB)
pub const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;

/// Maximum path length
pub const MAX_PATH_LEN: usize = 256;

/// Maximum filename length
pub const MAX_NAME_LEN: usize = 64;

/// Filesystem errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    /// File or directory not found
    NotFound,
    /// Permission denied
    PermissionDenied,
    /// File already exists
    AlreadyExists,
    /// Not a directory
    NotADirectory,
    /// Not a file
    NotAFile,
    /// Directory not empty
    NotEmpty,
    /// Path too long
    PathTooLong,
    /// Name too long
    NameTooLong,
    /// File too large
    FileTooLarge,
    /// Filesystem full
    NoSpace,
    /// Invalid path
    InvalidPath,
}

/// File type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// Regular file
    File,
    /// Directory
    Directory,
}

/// File metadata
#[derive(Debug, Clone)]
pub struct Metadata {
    /// File type
    pub file_type: FileType,
    /// Size in bytes (0 for directories)
    pub size: usize,
    /// Creation time (ticks since boot)
    pub created: u64,
    /// Last modification time (ticks since boot)
    pub modified: u64,
}

/// An inode in the filesystem
#[derive(Debug)]
pub struct Inode {
    /// File type
    pub file_type: FileType,
    /// File content (for regular files)
    pub content: Vec<u8>,
    /// Directory entries (for directories)
    pub children: BTreeMap<String, usize>, // name -> inode index
    /// Creation time
    pub created: u64,
    /// Last modification time
    pub modified: u64,
}

impl Inode {
    /// Creates a new file inode
    pub fn new_file() -> Self {
        let now = crate::arch::x86_64::interrupts::get_ticks();
        Self {
            file_type: FileType::File,
            content: Vec::new(),
            children: BTreeMap::new(),
            created: now,
            modified: now,
        }
    }

    /// Creates a new directory inode
    pub fn new_directory() -> Self {
        let now = crate::arch::x86_64::interrupts::get_ticks();
        Self {
            file_type: FileType::Directory,
            content: Vec::new(),
            children: BTreeMap::new(),
            created: now,
            modified: now,
        }
    }

    /// Gets metadata for this inode
    pub fn metadata(&self) -> Metadata {
        Metadata {
            file_type: self.file_type,
            size: self.content.len(),
            created: self.created,
            modified: self.modified,
        }
    }
}

/// The in-memory filesystem
pub struct RamFs {
    /// All inodes (index 0 is root)
    inodes: Vec<Inode>,
    /// Free inode indices
    free_inodes: Vec<usize>,
    /// Total bytes used
    bytes_used: usize,
    /// Maximum total size
    max_size: usize,
}

impl RamFs {
    /// Creates a new ramfs with the given maximum size
    pub fn new(max_size: usize) -> Self {
        let mut fs = Self {
            inodes: Vec::new(),
            free_inodes: Vec::new(),
            bytes_used: 0,
            max_size,
        };
        
        // Create root directory
        fs.inodes.push(Inode::new_directory());
        
        fs
    }

    /// Allocates a new inode
    fn alloc_inode(&mut self, inode: Inode) -> Option<usize> {
        if let Some(idx) = self.free_inodes.pop() {
            self.inodes[idx] = inode;
            Some(idx)
        } else {
            let idx = self.inodes.len();
            self.inodes.push(inode);
            Some(idx)
        }
    }

    /// Frees an inode
    fn free_inode(&mut self, idx: usize) {
        if idx > 0 && idx < self.inodes.len() {
            self.bytes_used = self.bytes_used.saturating_sub(self.inodes[idx].content.len());
            self.inodes[idx] = Inode::new_file(); // Reset
            self.free_inodes.push(idx);
        }
    }

    /// Resolves a path to an inode index
    fn resolve_path(&self, path: &str) -> Result<usize, FsError> {
        if path.len() > MAX_PATH_LEN {
            return Err(FsError::PathTooLong);
        }

        let path = path.trim_start_matches('/');
        if path.is_empty() {
            return Ok(0); // Root
        }

        let mut current = 0usize; // Start at root

        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            if component == ".." {
                // Go up (for simplicity, stay at root if already there)
                continue;
            }

            let inode = &self.inodes[current];
            if inode.file_type != FileType::Directory {
                return Err(FsError::NotADirectory);
            }

            current = *inode.children.get(component).ok_or(FsError::NotFound)?;
        }

        Ok(current)
    }

    /// Resolves the parent directory of a path and returns (parent_idx, name as owned String)
    fn resolve_parent(&self, path: &str) -> Result<(usize, String), FsError> {
        let path = path.trim_start_matches('/').trim_end_matches('/');
        if path.is_empty() {
            return Err(FsError::InvalidPath);
        }

        let (parent_path, name) = match path.rfind('/') {
            Some(pos) => (&path[..pos], &path[pos + 1..]),
            None => ("", path),
        };

        if name.len() > MAX_NAME_LEN {
            return Err(FsError::NameTooLong);
        }

        let parent_idx = self.resolve_path(if parent_path.is_empty() { "/" } else { parent_path })?;
        
        if self.inodes[parent_idx].file_type != FileType::Directory {
            return Err(FsError::NotADirectory);
        }

        Ok((parent_idx, String::from(name)))
    }

    /// Creates a file at the given path
    pub fn create_file(&mut self, path: &str) -> Result<(), FsError> {
        let (parent_idx, name) = self.resolve_parent(path)?;
        
        if self.inodes[parent_idx].children.contains_key(&name) {
            return Err(FsError::AlreadyExists);
        }

        let inode_idx = self.alloc_inode(Inode::new_file()).ok_or(FsError::NoSpace)?;
        self.inodes[parent_idx].children.insert(name, inode_idx);
        self.inodes[parent_idx].modified = crate::arch::x86_64::interrupts::get_ticks();

        Ok(())
    }

    /// Creates a directory at the given path
    pub fn create_dir(&mut self, path: &str) -> Result<(), FsError> {
        let (parent_idx, name) = self.resolve_parent(path)?;
        
        if self.inodes[parent_idx].children.contains_key(&name) {
            return Err(FsError::AlreadyExists);
        }

        let inode_idx = self.alloc_inode(Inode::new_directory()).ok_or(FsError::NoSpace)?;
        self.inodes[parent_idx].children.insert(name, inode_idx);
        self.inodes[parent_idx].modified = crate::arch::x86_64::interrupts::get_ticks();

        Ok(())
    }

    /// Reads a file's content
    pub fn read_file(&self, path: &str) -> Result<&[u8], FsError> {
        let idx = self.resolve_path(path)?;
        let inode = &self.inodes[idx];
        
        if inode.file_type != FileType::File {
            return Err(FsError::NotAFile);
        }

        Ok(&inode.content)
    }

    /// Writes data to a file (replaces content)
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        if data.len() > MAX_FILE_SIZE {
            return Err(FsError::FileTooLarge);
        }

        let idx = self.resolve_path(path)?;
        let inode = &mut self.inodes[idx];
        
        if inode.file_type != FileType::File {
            return Err(FsError::NotAFile);
        }

        // Check space
        let old_size = inode.content.len();
        let new_bytes = self.bytes_used - old_size + data.len();
        if new_bytes > self.max_size {
            return Err(FsError::NoSpace);
        }

        inode.content = data.to_vec();
        inode.modified = crate::arch::x86_64::interrupts::get_ticks();
        self.bytes_used = new_bytes;

        Ok(())
    }

    /// Appends data to a file
    pub fn append_file(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        let idx = self.resolve_path(path)?;
        let inode = &mut self.inodes[idx];
        
        if inode.file_type != FileType::File {
            return Err(FsError::NotAFile);
        }

        let new_size = inode.content.len() + data.len();
        if new_size > MAX_FILE_SIZE {
            return Err(FsError::FileTooLarge);
        }

        let new_bytes = self.bytes_used + data.len();
        if new_bytes > self.max_size {
            return Err(FsError::NoSpace);
        }

        inode.content.extend_from_slice(data);
        inode.modified = crate::arch::x86_64::interrupts::get_ticks();
        self.bytes_used = new_bytes;

        Ok(())
    }

    /// Lists directory contents
    pub fn list_dir(&self, path: &str) -> Result<Vec<(String, Metadata)>, FsError> {
        let idx = self.resolve_path(path)?;
        let inode = &self.inodes[idx];
        
        if inode.file_type != FileType::Directory {
            return Err(FsError::NotADirectory);
        }

        let mut entries = Vec::new();
        for (name, &child_idx) in &inode.children {
            let child_meta = self.inodes[child_idx].metadata();
            entries.push((name.clone(), child_meta));
        }

        Ok(entries)
    }

    /// Gets metadata for a path
    pub fn stat(&self, path: &str) -> Result<Metadata, FsError> {
        let idx = self.resolve_path(path)?;
        Ok(self.inodes[idx].metadata())
    }

    /// Removes a file
    pub fn remove_file(&mut self, path: &str) -> Result<(), FsError> {
        let (parent_idx, name) = self.resolve_parent(path)?;
        
        let child_idx = *self.inodes[parent_idx].children.get(&name).ok_or(FsError::NotFound)?;
        
        if self.inodes[child_idx].file_type != FileType::File {
            return Err(FsError::NotAFile);
        }

        self.inodes[parent_idx].children.remove(&name);
        self.inodes[parent_idx].modified = crate::arch::x86_64::interrupts::get_ticks();
        self.free_inode(child_idx);

        Ok(())
    }

    /// Removes an empty directory
    pub fn remove_dir(&mut self, path: &str) -> Result<(), FsError> {
        if path == "/" {
            return Err(FsError::PermissionDenied);
        }

        let (parent_idx, name) = self.resolve_parent(path)?;
        
        let child_idx = *self.inodes[parent_idx].children.get(&name).ok_or(FsError::NotFound)?;
        
        if self.inodes[child_idx].file_type != FileType::Directory {
            return Err(FsError::NotADirectory);
        }

        if !self.inodes[child_idx].children.is_empty() {
            return Err(FsError::NotEmpty);
        }

        self.inodes[parent_idx].children.remove(&name);
        self.inodes[parent_idx].modified = crate::arch::x86_64::interrupts::get_ticks();
        self.free_inode(child_idx);

        Ok(())
    }

    /// Returns filesystem statistics
    pub fn stats(&self) -> FsStats {
        FsStats {
            total_bytes: self.max_size,
            used_bytes: self.bytes_used,
            inode_count: self.inodes.len(),
            free_inodes: self.free_inodes.len(),
        }
    }
}

/// Filesystem statistics
#[derive(Debug, Clone)]
pub struct FsStats {
    /// Total capacity in bytes
    pub total_bytes: usize,
    /// Used bytes
    pub used_bytes: usize,
    /// Total inodes
    pub inode_count: usize,
    /// Free inodes
    pub free_inodes: usize,
}

/// Global filesystem instance (4 MB max)
static FILESYSTEM: Mutex<RamFs> = Mutex::new(RamFs {
    inodes: Vec::new(),
    free_inodes: Vec::new(),
    bytes_used: 0,
    max_size: 4 * 1024 * 1024,
});

/// Flag indicating if filesystem has been initialized
static FS_INITIALIZED: core::sync::atomic::AtomicBool = 
    core::sync::atomic::AtomicBool::new(false);

/// Initializes the filesystem
pub fn init() {
    if FS_INITIALIZED.swap(true, core::sync::atomic::Ordering::SeqCst) {
        return; // Already initialized
    }

    // Initialize legacy RamFs
    let mut fs = FILESYSTEM.lock();
    *fs = RamFs::new(4 * 1024 * 1024);

    // Create initial directory structure
    let _ = fs.create_dir("/bin");
    let _ = fs.create_dir("/etc");
    let _ = fs.create_dir("/tmp");
    let _ = fs.create_dir("/home");
    let _ = fs.create_dir("/var");
    let _ = fs.create_dir("/var/log");
    let _ = fs.create_dir("/proc");
    let _ = fs.create_dir("/dev");
    let _ = fs.create_dir("/sys");

    // Create some initial files
    let _ = fs.create_file("/etc/hostname");
    let _ = fs.write_file("/etc/hostname", b"splax");

    let _ = fs.create_file("/etc/version");
    let _ = fs.write_file("/etc/version", b"Splax OS v0.1.0\n");

    let _ = fs.create_file("/var/log/kernel.log");
    let _ = fs.write_file("/var/log/kernel.log", b"[kernel] Filesystem initialized\n");

    // Create a welcome message
    let _ = fs.create_file("/etc/motd");
    let _ = fs.write_file("/etc/motd", b"Welcome to Splax OS!\n\nA capability-secure, distributed-first operating system.\n\nType 'help' for available commands.\n");

    // Create a minimal test WASM module
    // This is a valid WASM 1.0 module that:
    //   - Has a function type: () -> i32
    //   - Has one function that returns 42
    //   - Exports the function as "main"
    let _ = fs.create_file("/bin/hello.wasm");
    let test_wasm: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, // Magic: \0asm
        0x01, 0x00, 0x00, 0x00, // Version: 1
        // Type section
        0x01, 0x05,             // Section ID=1, size=5
        0x01,                   // 1 type
        0x60, 0x00, 0x01, 0x7F, // func () -> i32
        // Function section
        0x03, 0x02,             // Section ID=3, size=2
        0x01, 0x00,             // 1 function, type index 0
        // Export section
        0x07, 0x08,             // Section ID=7, size=8
        0x01,                   // 1 export
        0x04, b'm', b'a', b'i', b'n', // Name: "main"
        0x00, 0x00,             // Export kind=func, index=0
        // Code section
        0x0A, 0x06,             // Section ID=10, size=6
        0x01,                   // 1 function body
        0x04,                   // Body size=4
        0x00,                   // 0 locals
        0x41, 0x2A,             // i32.const 42
        0x0B,                   // end
    ];
    let _ = fs.write_file("/bin/hello.wasm", test_wasm);

    // Create another test WASM: adds two numbers
    let _ = fs.create_file("/bin/add.wasm");
    let add_wasm: &[u8] = &[
        0x00, 0x61, 0x73, 0x6D, // Magic: \0asm
        0x01, 0x00, 0x00, 0x00, // Version: 1
        // Type section
        0x01, 0x07,             // Section ID=1, size=7
        0x01,                   // 1 type
        0x60, 0x02, 0x7F, 0x7F, // func (i32, i32)
        0x01, 0x7F,             // -> i32
        // Function section
        0x03, 0x02,             // Section ID=3, size=2
        0x01, 0x00,             // 1 function, type index 0
        // Export section
        0x07, 0x07,             // Section ID=7, size=7
        0x01,                   // 1 export
        0x03, b'a', b'd', b'd', // Name: "add"
        0x00, 0x00,             // Export kind=func, index=0
        // Code section
        0x0A, 0x09,             // Section ID=10, size=9
        0x01,                   // 1 function body
        0x07,                   // Body size=7
        0x00,                   // 0 locals
        0x20, 0x00,             // local.get 0
        0x20, 0x01,             // local.get 1
        0x6A,                   // i32.add
        0x0B,                   // end
    ];
    let _ = fs.write_file("/bin/add.wasm", add_wasm);

    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial, "[fs] Ramfs initialized (4 MB)");
        }
    }
}

/// Gets the filesystem instance
pub fn filesystem() -> &'static Mutex<RamFs> {
    &FILESYSTEM
}

/// Lists directory contents
pub fn ls(path: &str) -> Result<Vec<(String, Metadata)>, FsError> {
    FILESYSTEM.lock().list_dir(path)
}

/// Reads file content
pub fn cat(path: &str) -> Result<Vec<u8>, FsError> {
    let fs = FILESYSTEM.lock();
    fs.read_file(path).map(|s| s.to_vec())
}

/// Writes file content
pub fn write(path: &str, data: &[u8]) -> Result<(), FsError> {
    FILESYSTEM.lock().write_file(path, data)
}

/// Creates a file
pub fn touch(path: &str) -> Result<(), FsError> {
    FILESYSTEM.lock().create_file(path)
}

/// Creates a directory
pub fn mkdir(path: &str) -> Result<(), FsError> {
    FILESYSTEM.lock().create_dir(path)
}

/// Removes a file
pub fn rm(path: &str) -> Result<(), FsError> {
    FILESYSTEM.lock().remove_file(path)
}

/// Removes an empty directory
pub fn rmdir(path: &str) -> Result<(), FsError> {
    FILESYSTEM.lock().remove_dir(path)
}

/// Gets filesystem stats
pub fn stats() -> FsStats {
    FILESYSTEM.lock().stats()
}

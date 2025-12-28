//! # VFS RPC Protocol
//!
//! Defines the message format for VFS operations over S-LINK IPC.
//! This protocol enables the kernel VFS stub to communicate with the
//! S-STORAGE userspace service.
//!
//! ## Message Flow
//!
//! ```text
//! Kernel VFS Stub                     S-STORAGE Service
//!      │                                    │
//!      │──── VfsRequest::Open ─────────────>│
//!      │<─── VfsResponse::Handle ───────────│
//!      │                                    │
//!      │──── VfsRequest::Read ─────────────>│
//!      │<─── VfsResponse::Data ────────────-│
//! ```

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Request ID for matching responses
pub type RequestId = u64;

/// File handle for open files
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileHandle(pub u64);

/// Inode number
pub type InodeNum = u64;

/// VFS file types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VfsFileType {
    Regular = 1,
    Directory = 2,
    Symlink = 3,
    CharDevice = 4,
    BlockDevice = 5,
    Fifo = 6,
    Socket = 7,
}

/// Open flags
#[derive(Debug, Clone, Copy)]
pub struct OpenFlags {
    pub read: bool,
    pub write: bool,
    pub create: bool,
    pub truncate: bool,
    pub append: bool,
    pub exclusive: bool,
}

impl Default for OpenFlags {
    fn default() -> Self {
        Self {
            read: true,
            write: false,
            create: false,
            truncate: false,
            append: false,
            exclusive: false,
        }
    }
}

impl OpenFlags {
    pub fn read_only() -> Self {
        Self::default()
    }

    pub fn write_only() -> Self {
        Self {
            read: false,
            write: true,
            ..Self::default()
        }
    }

    pub fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            ..Self::default()
        }
    }

    pub fn create() -> Self {
        Self {
            read: true,
            write: true,
            create: true,
            ..Self::default()
        }
    }
}

/// File attributes (stat data)
#[derive(Debug, Clone)]
pub struct VfsAttr {
    pub ino: InodeNum,
    pub file_type: VfsFileType,
    pub size: u64,
    pub nlink: u32,
    pub blksize: u32,
    pub blocks: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub crtime: u64,
    pub perm_read: bool,
    pub perm_write: bool,
    pub perm_execute: bool,
}

/// Directory entry
#[derive(Debug, Clone)]
pub struct VfsDirEntry {
    pub name: String,
    pub ino: InodeNum,
    pub file_type: VfsFileType,
}

/// Seek origin
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SeekFrom {
    /// Seek from start of file
    Start = 0,
    /// Seek from current position
    Current = 1,
    /// Seek from end of file
    End = 2,
}

/// VFS request types (kernel -> storage service)
#[derive(Debug, Clone)]
pub enum VfsRequest {
    /// Mount a filesystem
    Mount {
        request_id: RequestId,
        device: String,
        mount_point: String,
        fs_type: String,
        flags: u32,
    },

    /// Unmount a filesystem
    Unmount {
        request_id: RequestId,
        mount_point: String,
    },

    /// Open a file
    Open {
        request_id: RequestId,
        path: String,
        flags: OpenFlags,
        mode: u32,
    },

    /// Close a file
    Close {
        request_id: RequestId,
        handle: FileHandle,
    },

    /// Read from file
    Read {
        request_id: RequestId,
        handle: FileHandle,
        offset: u64,
        len: usize,
    },

    /// Write to file
    Write {
        request_id: RequestId,
        handle: FileHandle,
        offset: u64,
        data: Vec<u8>,
    },

    /// Get file attributes
    Stat {
        request_id: RequestId,
        path: String,
    },

    /// Get file attributes by handle
    Fstat {
        request_id: RequestId,
        handle: FileHandle,
    },

    /// Read directory entries
    Readdir {
        request_id: RequestId,
        path: String,
    },

    /// Create directory
    Mkdir {
        request_id: RequestId,
        path: String,
        mode: u32,
    },

    /// Remove directory
    Rmdir {
        request_id: RequestId,
        path: String,
    },

    /// Unlink file
    Unlink {
        request_id: RequestId,
        path: String,
    },

    /// Rename file/directory
    Rename {
        request_id: RequestId,
        old_path: String,
        new_path: String,
    },

    /// Create symlink
    Symlink {
        request_id: RequestId,
        target: String,
        link_path: String,
    },

    /// Read symlink target
    Readlink {
        request_id: RequestId,
        path: String,
    },

    /// Truncate file
    Truncate {
        request_id: RequestId,
        path: String,
        length: u64,
    },

    /// Sync filesystem
    Sync {
        request_id: RequestId,
        handle: Option<FileHandle>,
    },

    /// Seek in file
    Seek {
        request_id: RequestId,
        handle: FileHandle,
        offset: i64,
        whence: SeekFrom,
    },

    /// Get filesystem statistics
    Statfs {
        request_id: RequestId,
        path: String,
    },
}

impl VfsRequest {
    pub fn request_id(&self) -> RequestId {
        match self {
            VfsRequest::Mount { request_id, .. } => *request_id,
            VfsRequest::Unmount { request_id, .. } => *request_id,
            VfsRequest::Open { request_id, .. } => *request_id,
            VfsRequest::Close { request_id, .. } => *request_id,
            VfsRequest::Read { request_id, .. } => *request_id,
            VfsRequest::Write { request_id, .. } => *request_id,
            VfsRequest::Stat { request_id, .. } => *request_id,
            VfsRequest::Fstat { request_id, .. } => *request_id,
            VfsRequest::Readdir { request_id, .. } => *request_id,
            VfsRequest::Mkdir { request_id, .. } => *request_id,
            VfsRequest::Rmdir { request_id, .. } => *request_id,
            VfsRequest::Unlink { request_id, .. } => *request_id,
            VfsRequest::Rename { request_id, .. } => *request_id,
            VfsRequest::Symlink { request_id, .. } => *request_id,
            VfsRequest::Readlink { request_id, .. } => *request_id,
            VfsRequest::Truncate { request_id, .. } => *request_id,
            VfsRequest::Sync { request_id, .. } => *request_id,
            VfsRequest::Seek { request_id, .. } => *request_id,
            VfsRequest::Statfs { request_id, .. } => *request_id,
        }
    }
}

/// VFS error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VfsError {
    /// Success
    Success = 0,
    /// File not found
    NotFound = 1,
    /// Permission denied
    PermissionDenied = 2,
    /// Already exists
    AlreadyExists = 3,
    /// Not a directory
    NotADirectory = 4,
    /// Is a directory
    IsADirectory = 5,
    /// Directory not empty
    NotEmpty = 6,
    /// Bad file handle
    BadHandle = 7,
    /// Too many open files
    TooManyOpenFiles = 8,
    /// No space left
    NoSpace = 9,
    /// Read-only filesystem
    ReadOnlyFs = 10,
    /// Invalid argument
    InvalidArgument = 11,
    /// I/O error
    IoError = 12,
    /// Not supported
    NotSupported = 13,
    /// Path too long
    PathTooLong = 14,
    /// No such filesystem
    NoFilesystem = 15,
    /// Mount point busy
    Busy = 16,
    /// Invalid handle
    InvalidHandle = 17,
    /// Name too long
    NameTooLong = 18,
}

/// Filesystem statistics
#[derive(Debug, Clone)]
pub struct StatFs {
    /// Total blocks
    pub total_blocks: u64,
    /// Free blocks
    pub free_blocks: u64,
    /// Available blocks (non-root)
    pub avail_blocks: u64,
    /// Total inodes
    pub total_inodes: u64,
    /// Free inodes
    pub free_inodes: u64,
    /// Block size
    pub block_size: u32,
    /// Maximum name length
    pub name_max: u32,
    /// Filesystem type
    pub fs_type: String,
}

/// VFS response types (storage service -> kernel)
#[derive(Debug, Clone)]
pub enum VfsResponse {
    /// Operation completed successfully
    Ok {
        request_id: RequestId,
    },

    /// Error occurred
    Error {
        request_id: RequestId,
        error: VfsError,
    },

    /// File opened, return handle
    Handle {
        request_id: RequestId,
        handle: FileHandle,
    },

    /// Read data
    Data {
        request_id: RequestId,
        data: Vec<u8>,
    },

    /// Write completed
    Written {
        request_id: RequestId,
        bytes_written: usize,
    },

    /// File attributes
    Attr {
        request_id: RequestId,
        attr: VfsAttr,
    },

    /// Directory entries
    DirEntries {
        request_id: RequestId,
        entries: Vec<VfsDirEntry>,
    },

    /// Symlink target
    Link {
        request_id: RequestId,
        target: String,
    },

    /// Seek result (new position)
    Position {
        request_id: RequestId,
        position: u64,
    },

    /// Filesystem statistics
    FsStat {
        request_id: RequestId,
        stats: StatFs,
    },
}

impl VfsResponse {
    pub fn request_id(&self) -> RequestId {
        match self {
            VfsResponse::Ok { request_id, .. } => *request_id,
            VfsResponse::Error { request_id, .. } => *request_id,
            VfsResponse::Handle { request_id, .. } => *request_id,
            VfsResponse::Data { request_id, .. } => *request_id,
            VfsResponse::Written { request_id, .. } => *request_id,
            VfsResponse::Attr { request_id, .. } => *request_id,
            VfsResponse::DirEntries { request_id, .. } => *request_id,
            VfsResponse::Link { request_id, .. } => *request_id,
            VfsResponse::Position { request_id, .. } => *request_id,
            VfsResponse::FsStat { request_id, .. } => *request_id,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, VfsResponse::Error { .. })
    }

    pub fn ok(request_id: RequestId) -> Self {
        VfsResponse::Ok { request_id }
    }

    pub fn error(request_id: RequestId, error: VfsError) -> Self {
        VfsResponse::Error { request_id, error }
    }
}

/// Message encoding for IPC
/// 
/// Format:
/// - u32: message type
/// - u64: request ID
/// - variable: payload
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MessageType {
    // Requests
    ReqMount = 1,
    ReqUnmount = 2,
    ReqOpen = 3,
    ReqClose = 4,
    ReqRead = 5,
    ReqWrite = 6,
    ReqStat = 7,
    ReqFstat = 8,
    ReqReaddir = 9,
    ReqMkdir = 10,
    ReqRmdir = 11,
    ReqUnlink = 12,
    ReqRename = 13,
    ReqSymlink = 14,
    ReqReadlink = 15,
    ReqTruncate = 16,
    ReqSync = 17,
    ReqSeek = 18,
    ReqStatfs = 19,

    // Responses
    RespOk = 100,
    RespError = 101,
    RespHandle = 102,
    RespData = 103,
    RespWritten = 104,
    RespAttr = 105,
    RespDirEntries = 106,
    RespLink = 107,
    RespPosition = 108,
    RespFsStat = 109,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_flags() {
        let flags = OpenFlags::read_only();
        assert!(flags.read);
        assert!(!flags.write);

        let flags = OpenFlags::create();
        assert!(flags.read);
        assert!(flags.write);
        assert!(flags.create);
    }

    #[test]
    fn test_request_id() {
        let req = VfsRequest::Open {
            request_id: 42,
            path: String::from("/test"),
            flags: OpenFlags::default(),
            mode: 0o644,
        };
        assert_eq!(req.request_id(), 42);
    }
}

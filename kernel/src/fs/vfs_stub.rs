//! # VFS Stub (Kernel Side)
//!
//! Thin VFS layer that forwards all filesystem operations to the S-STORAGE
//! userspace service via S-LINK IPC. This is the kernel component of the
//! Phase A hybrid kernel migration.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │                    System Call Layer                      │
//! │        open(), read(), write(), close(), stat()          │
//! ├──────────────────────────────────────────────────────────┤
//! │                      VFS Stub                             │
//! │  ┌────────────────────────────────────────────────────┐  │
//! │  │  - Converts syscalls to VfsRequest messages        │  │
//! │  │  - Sends via S-LINK to S-STORAGE                   │  │
//! │  │  - Converts VfsResponse back to syscall results    │  │
//! │  │  - Caches file descriptors locally                 │  │
//! │  └────────────────────────────────────────────────────┘  │
//! ├──────────────────────────────────────────────────────────┤
//! │                      S-LINK IPC                           │
//! └──────────────────────────────────────────────────────────┘
//!                              │
//!                              ↓
//! ┌──────────────────────────────────────────────────────────┐
//! │               S-STORAGE Userspace Service                 │
//! └──────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Initialize with S-LINK channel to S-STORAGE
//! vfs_stub::init(storage_channel);
//!
//! // Now VFS operations go through IPC
//! let fd = vfs_stub::open("/etc/config", O_RDONLY)?;
//! let data = vfs_stub::read(fd, &mut buffer)?;
//! vfs_stub::close(fd)?;
//! ```

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use spin::{Mutex, Once};

use crate::ipc::{ChannelId, IpcError, Message, MessageData};
use crate::sched::ProcessId;

/// Storage channel (initialized at boot)
static STORAGE_CHANNEL: Once<StorageChannel> = Once::new();

/// Whether stub is initialized
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Request ID counter
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

/// File descriptor type
pub type Fd = u32;

/// File handle from S-STORAGE
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct RemoteHandle(u64);

/// Local file descriptor mapping
struct FdMapping {
    /// Remote handle in S-STORAGE
    remote: RemoteHandle,
    /// Current position (for non-seeking reads)
    position: u64,
    /// Flags used to open
    flags: u32,
}

/// File descriptor table (per-process, simplified as global for now)
static FD_TABLE: Mutex<BTreeMap<Fd, FdMapping>> = Mutex::new(BTreeMap::new());

/// Next local FD
static NEXT_FD: AtomicU64 = AtomicU64::new(3); // 0,1,2 reserved for stdin/stdout/stderr

/// Storage channel wrapper
struct StorageChannel {
    channel_id: ChannelId,
    // In real implementation, this would use the IPC subsystem
}

/// VFS Error codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    NotInitialized,
    NotFound,
    PermissionDenied,
    AlreadyExists,
    NotADirectory,
    IsADirectory,
    NotEmpty,
    BadFd,
    TooManyOpenFiles,
    NoSpace,
    ReadOnlyFs,
    InvalidArgument,
    IoError,
    NotSupported,
    PathTooLong,
    IpcError,
    Timeout,
}

impl From<IpcError> for VfsError {
    fn from(_: IpcError) -> Self {
        VfsError::IpcError
    }
}

/// Open flags
pub const O_RDONLY: u32 = 0x0000;
pub const O_WRONLY: u32 = 0x0001;
pub const O_RDWR: u32 = 0x0002;
pub const O_CREAT: u32 = 0x0040;
pub const O_EXCL: u32 = 0x0080;
pub const O_TRUNC: u32 = 0x0200;
pub const O_APPEND: u32 = 0x0400;

/// Seek origin
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

/// File type from stat
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    Symlink,
    CharDevice,
    BlockDevice,
    Fifo,
    Socket,
}

/// Stat structure
#[derive(Debug, Clone)]
pub struct Stat {
    pub ino: u64,
    pub file_type: FileType,
    pub size: u64,
    pub nlink: u32,
    pub blksize: u32,
    pub blocks: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
}

/// Directory entry
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub ino: u64,
    pub file_type: FileType,
}

// ============================================================================
// VFS Request/Response Types (matches vfs_protocol.rs in S-STORAGE)
// ============================================================================

#[repr(u32)]
enum RequestType {
    Open = 3,
    Close = 4,
    Read = 5,
    Write = 6,
    Stat = 7,
    Readdir = 9,
    Mkdir = 10,
    Rmdir = 11,
    Unlink = 12,
    Seek = 18,
}

#[repr(u32)]
enum ResponseType {
    Ok = 100,
    Error = 101,
    Handle = 102,
    Data = 103,
    Written = 104,
    Attr = 105,
    DirEntries = 106,
    Position = 108,
}

// ============================================================================
// Public API
// ============================================================================

/// Initialize VFS stub with S-LINK channel to S-STORAGE
pub fn init(channel_id: ChannelId) {
    STORAGE_CHANNEL.call_once(|| StorageChannel { channel_id });
    INITIALIZED.store(true, Ordering::Release);
}

/// Check if stub is initialized
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Open a file
pub fn open(path: &str, flags: u32, mode: u32) -> Result<Fd, VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);

    // Build request message
    let request = build_open_request(request_id, path, flags, mode);

    // Send to S-STORAGE and wait for response
    let response = send_and_receive(request)?;

    // Parse response
    let remote_handle = parse_handle_response(&response)?;

    // Allocate local FD
    let fd = NEXT_FD.fetch_add(1, Ordering::Relaxed) as Fd;

    let mut fd_table = FD_TABLE.lock();
    fd_table.insert(
        fd,
        FdMapping {
            remote: RemoteHandle(remote_handle),
            position: 0,
            flags,
        },
    );

    Ok(fd)
}

/// Close a file
pub fn close(fd: Fd) -> Result<(), VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let mut fd_table = FD_TABLE.lock();
    let mapping = fd_table.remove(&fd).ok_or(VfsError::BadFd)?;
    drop(fd_table);

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let request = build_close_request(request_id, mapping.remote.0);

    let response = send_and_receive(request)?;
    parse_ok_response(&response)?;

    Ok(())
}

/// Read from a file
pub fn read(fd: Fd, buf: &mut [u8]) -> Result<usize, VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let mut fd_table = FD_TABLE.lock();
    let mapping = fd_table.get_mut(&fd).ok_or(VfsError::BadFd)?;
    let position = mapping.position;
    let remote = mapping.remote.0;
    drop(fd_table);

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let request = build_read_request(request_id, remote, position, buf.len());

    let response = send_and_receive(request)?;
    let data = parse_data_response(&response)?;

    let bytes_read = data.len().min(buf.len());
    buf[..bytes_read].copy_from_slice(&data[..bytes_read]);

    // Update position
    let mut fd_table = FD_TABLE.lock();
    if let Some(mapping) = fd_table.get_mut(&fd) {
        mapping.position += bytes_read as u64;
    }

    Ok(bytes_read)
}

/// Read at specific offset (pread)
pub fn pread(fd: Fd, buf: &mut [u8], offset: u64) -> Result<usize, VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let fd_table = FD_TABLE.lock();
    let mapping = fd_table.get(&fd).ok_or(VfsError::BadFd)?;
    let remote = mapping.remote.0;
    drop(fd_table);

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let request = build_read_request(request_id, remote, offset, buf.len());

    let response = send_and_receive(request)?;
    let data = parse_data_response(&response)?;

    let bytes_read = data.len().min(buf.len());
    buf[..bytes_read].copy_from_slice(&data[..bytes_read]);

    Ok(bytes_read)
}

/// Write to a file
pub fn write(fd: Fd, buf: &[u8]) -> Result<usize, VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let mut fd_table = FD_TABLE.lock();
    let mapping = fd_table.get_mut(&fd).ok_or(VfsError::BadFd)?;
    let position = mapping.position;
    let remote = mapping.remote.0;
    let is_append = (mapping.flags & O_APPEND) != 0;
    drop(fd_table);

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);

    // For append mode, seek to end first (simplified - real impl would be atomic)
    let write_offset = if is_append {
        // Get file size
        u64::MAX // Signal append mode
    } else {
        position
    };

    let request = build_write_request(request_id, remote, write_offset, buf);

    let response = send_and_receive(request)?;
    let bytes_written = parse_written_response(&response)?;

    // Update position
    let mut fd_table = FD_TABLE.lock();
    if let Some(mapping) = fd_table.get_mut(&fd) {
        mapping.position += bytes_written as u64;
    }

    Ok(bytes_written)
}

/// Seek in file
pub fn lseek(fd: Fd, offset: i64, whence: i32) -> Result<u64, VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let fd_table = FD_TABLE.lock();
    let mapping = fd_table.get(&fd).ok_or(VfsError::BadFd)?;
    let remote = mapping.remote.0;
    drop(fd_table);

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let request = build_seek_request(request_id, remote, offset, whence);

    let response = send_and_receive(request)?;
    let new_position = parse_position_response(&response)?;

    // Update local position cache
    let mut fd_table = FD_TABLE.lock();
    if let Some(mapping) = fd_table.get_mut(&fd) {
        mapping.position = new_position;
    }

    Ok(new_position)
}

/// Get file status
pub fn stat(path: &str) -> Result<Stat, VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let request = build_stat_request(request_id, path);

    let response = send_and_receive(request)?;
    parse_attr_response(&response)
}

/// Get file status by descriptor
pub fn fstat(fd: Fd) -> Result<Stat, VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let fd_table = FD_TABLE.lock();
    let mapping = fd_table.get(&fd).ok_or(VfsError::BadFd)?;
    let remote = mapping.remote.0;
    drop(fd_table);

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let request = build_fstat_request(request_id, remote);

    let response = send_and_receive(request)?;
    parse_attr_response(&response)
}

/// Read directory entries
pub fn readdir(path: &str) -> Result<Vec<DirEntry>, VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let request = build_readdir_request(request_id, path);

    let response = send_and_receive(request)?;
    parse_direntries_response(&response)
}

/// Create directory
pub fn mkdir(path: &str, mode: u32) -> Result<(), VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let request = build_mkdir_request(request_id, path, mode);

    let response = send_and_receive(request)?;
    parse_ok_response(&response)
}

/// Remove directory
pub fn rmdir(path: &str) -> Result<(), VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let request = build_rmdir_request(request_id, path);

    let response = send_and_receive(request)?;
    parse_ok_response(&response)
}

/// Unlink (delete) file
pub fn unlink(path: &str) -> Result<(), VfsError> {
    if !is_initialized() {
        return Err(VfsError::NotInitialized);
    }

    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let request = build_unlink_request(request_id, path);

    let response = send_and_receive(request)?;
    parse_ok_response(&response)
}

// ============================================================================
// Internal Functions (message building/parsing)
// ============================================================================

fn send_and_receive(request: Vec<u8>) -> Result<Vec<u8>, VfsError> {
    // In real implementation, this would:
    // 1. Send message via IPC to S-STORAGE channel
    // 2. Block waiting for response
    // 3. Return response data
    //
    // For now, this is a placeholder that shows the architecture
    
    let _channel = STORAGE_CHANNEL.get().ok_or(VfsError::NotInitialized)?;
    
    // Placeholder: In real implementation, use ipc::send() and ipc::receive()
    // let msg = Message::inline(ProcessId::kernel(), request);
    // ipc::send(channel.channel_id, msg)?;
    // let response = ipc::receive(channel.channel_id)?;
    // return Ok(response.data);
    
    // For testing without full IPC:
    let _ = request;
    Err(VfsError::NotSupported)
}

fn build_open_request(request_id: u64, path: &str, flags: u32, mode: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    
    // Type: Open (3)
    buf.extend_from_slice(&3u32.to_le_bytes());
    // Request ID
    buf.extend_from_slice(&request_id.to_le_bytes());
    // Path length + path
    buf.extend_from_slice(&(path.len() as u32).to_le_bytes());
    buf.extend_from_slice(path.as_bytes());
    // Flags
    buf.extend_from_slice(&flags.to_le_bytes());
    // Mode
    buf.extend_from_slice(&mode.to_le_bytes());
    
    buf
}

fn build_close_request(request_id: u64, handle: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&4u32.to_le_bytes()); // Close = 4
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&handle.to_le_bytes());
    buf
}

fn build_read_request(request_id: u64, handle: u64, offset: u64, len: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&5u32.to_le_bytes()); // Read = 5
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&handle.to_le_bytes());
    buf.extend_from_slice(&offset.to_le_bytes());
    buf.extend_from_slice(&(len as u64).to_le_bytes());
    buf
}

fn build_write_request(request_id: u64, handle: u64, offset: u64, data: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&6u32.to_le_bytes()); // Write = 6
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&handle.to_le_bytes());
    buf.extend_from_slice(&offset.to_le_bytes());
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(data);
    buf
}

fn build_seek_request(request_id: u64, handle: u64, offset: i64, whence: i32) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&18u32.to_le_bytes()); // Seek = 18
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&handle.to_le_bytes());
    buf.extend_from_slice(&offset.to_le_bytes());
    buf.extend_from_slice(&whence.to_le_bytes());
    buf
}

fn build_stat_request(request_id: u64, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&7u32.to_le_bytes()); // Stat = 7
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&(path.len() as u32).to_le_bytes());
    buf.extend_from_slice(path.as_bytes());
    buf
}

fn build_fstat_request(request_id: u64, handle: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&8u32.to_le_bytes()); // Fstat = 8
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&handle.to_le_bytes());
    buf
}

fn build_readdir_request(request_id: u64, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&9u32.to_le_bytes()); // Readdir = 9
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&(path.len() as u32).to_le_bytes());
    buf.extend_from_slice(path.as_bytes());
    buf
}

fn build_mkdir_request(request_id: u64, path: &str, mode: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&10u32.to_le_bytes()); // Mkdir = 10
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&(path.len() as u32).to_le_bytes());
    buf.extend_from_slice(path.as_bytes());
    buf.extend_from_slice(&mode.to_le_bytes());
    buf
}

fn build_rmdir_request(request_id: u64, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&11u32.to_le_bytes()); // Rmdir = 11
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&(path.len() as u32).to_le_bytes());
    buf.extend_from_slice(path.as_bytes());
    buf
}

fn build_unlink_request(request_id: u64, path: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&12u32.to_le_bytes()); // Unlink = 12
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&(path.len() as u32).to_le_bytes());
    buf.extend_from_slice(path.as_bytes());
    buf
}

// Response parsers

fn parse_ok_response(response: &[u8]) -> Result<(), VfsError> {
    if response.len() < 12 {
        return Err(VfsError::IoError);
    }
    
    let response_type = u32::from_le_bytes(response[0..4].try_into().unwrap());
    
    match response_type {
        100 => Ok(()), // Ok
        101 => {
            // Error
            let error_code = u32::from_le_bytes(response[12..16].try_into().unwrap_or([0; 4]));
            Err(error_code_to_vfs_error(error_code))
        }
        _ => Err(VfsError::IoError),
    }
}

fn parse_handle_response(response: &[u8]) -> Result<u64, VfsError> {
    if response.len() < 20 {
        return Err(VfsError::IoError);
    }
    
    let response_type = u32::from_le_bytes(response[0..4].try_into().unwrap());
    
    match response_type {
        102 => {
            // Handle response
            let handle = u64::from_le_bytes(response[12..20].try_into().unwrap());
            Ok(handle)
        }
        101 => {
            let error_code = u32::from_le_bytes(response[12..16].try_into().unwrap_or([0; 4]));
            Err(error_code_to_vfs_error(error_code))
        }
        _ => Err(VfsError::IoError),
    }
}

fn parse_data_response(response: &[u8]) -> Result<Vec<u8>, VfsError> {
    if response.len() < 16 {
        return Err(VfsError::IoError);
    }
    
    let response_type = u32::from_le_bytes(response[0..4].try_into().unwrap());
    
    match response_type {
        103 => {
            // Data response
            let len = u32::from_le_bytes(response[12..16].try_into().unwrap()) as usize;
            if response.len() < 16 + len {
                return Err(VfsError::IoError);
            }
            Ok(response[16..16 + len].to_vec())
        }
        101 => {
            let error_code = u32::from_le_bytes(response[12..16].try_into().unwrap_or([0; 4]));
            Err(error_code_to_vfs_error(error_code))
        }
        _ => Err(VfsError::IoError),
    }
}

fn parse_written_response(response: &[u8]) -> Result<usize, VfsError> {
    if response.len() < 20 {
        return Err(VfsError::IoError);
    }
    
    let response_type = u32::from_le_bytes(response[0..4].try_into().unwrap());
    
    match response_type {
        104 => {
            let bytes_written = u64::from_le_bytes(response[12..20].try_into().unwrap());
            Ok(bytes_written as usize)
        }
        101 => {
            let error_code = u32::from_le_bytes(response[12..16].try_into().unwrap_or([0; 4]));
            Err(error_code_to_vfs_error(error_code))
        }
        _ => Err(VfsError::IoError),
    }
}

fn parse_position_response(response: &[u8]) -> Result<u64, VfsError> {
    if response.len() < 20 {
        return Err(VfsError::IoError);
    }
    
    let response_type = u32::from_le_bytes(response[0..4].try_into().unwrap());
    
    match response_type {
        108 => {
            let position = u64::from_le_bytes(response[12..20].try_into().unwrap());
            Ok(position)
        }
        101 => {
            let error_code = u32::from_le_bytes(response[12..16].try_into().unwrap_or([0; 4]));
            Err(error_code_to_vfs_error(error_code))
        }
        _ => Err(VfsError::IoError),
    }
}

fn parse_attr_response(response: &[u8]) -> Result<Stat, VfsError> {
    if response.len() < 80 {
        return Err(VfsError::IoError);
    }
    
    let response_type = u32::from_le_bytes(response[0..4].try_into().unwrap());
    
    match response_type {
        105 => {
            // Parse attr (simplified)
            let ino = u64::from_le_bytes(response[12..20].try_into().unwrap());
            let file_type_u8 = response[20];
            let size = u64::from_le_bytes(response[24..32].try_into().unwrap());
            let nlink = u32::from_le_bytes(response[32..36].try_into().unwrap());
            let blksize = u32::from_le_bytes(response[36..40].try_into().unwrap());
            let blocks = u64::from_le_bytes(response[40..48].try_into().unwrap());
            let atime = u64::from_le_bytes(response[48..56].try_into().unwrap());
            let mtime = u64::from_le_bytes(response[56..64].try_into().unwrap());
            let ctime = u64::from_le_bytes(response[64..72].try_into().unwrap());
            
            let file_type = match file_type_u8 {
                1 => FileType::Regular,
                2 => FileType::Directory,
                3 => FileType::Symlink,
                4 => FileType::CharDevice,
                5 => FileType::BlockDevice,
                6 => FileType::Fifo,
                7 => FileType::Socket,
                _ => FileType::Regular,
            };
            
            Ok(Stat {
                ino,
                file_type,
                size,
                nlink,
                blksize,
                blocks,
                atime,
                mtime,
                ctime,
            })
        }
        101 => {
            let error_code = u32::from_le_bytes(response[12..16].try_into().unwrap_or([0; 4]));
            Err(error_code_to_vfs_error(error_code))
        }
        _ => Err(VfsError::IoError),
    }
}

fn parse_direntries_response(response: &[u8]) -> Result<Vec<DirEntry>, VfsError> {
    if response.len() < 16 {
        return Err(VfsError::IoError);
    }
    
    let response_type = u32::from_le_bytes(response[0..4].try_into().unwrap());
    
    match response_type {
        106 => {
            // Parse entries (simplified)
            let entry_count = u32::from_le_bytes(response[12..16].try_into().unwrap()) as usize;
            let mut entries = Vec::with_capacity(entry_count);
            let mut offset = 16;
            
            for _ in 0..entry_count {
                if offset + 12 > response.len() {
                    break;
                }
                
                let name_len = u32::from_le_bytes(response[offset..offset + 4].try_into().unwrap()) as usize;
                offset += 4;
                
                if offset + name_len + 9 > response.len() {
                    break;
                }
                
                let name = String::from_utf8_lossy(&response[offset..offset + name_len]).to_string();
                offset += name_len;
                
                let ino = u64::from_le_bytes(response[offset..offset + 8].try_into().unwrap());
                offset += 8;
                
                let file_type_u8 = response[offset];
                offset += 1;
                
                let file_type = match file_type_u8 {
                    1 => FileType::Regular,
                    2 => FileType::Directory,
                    3 => FileType::Symlink,
                    _ => FileType::Regular,
                };
                
                entries.push(DirEntry {
                    name,
                    ino,
                    file_type,
                });
            }
            
            Ok(entries)
        }
        101 => {
            let error_code = u32::from_le_bytes(response[12..16].try_into().unwrap_or([0; 4]));
            Err(error_code_to_vfs_error(error_code))
        }
        _ => Err(VfsError::IoError),
    }
}

fn error_code_to_vfs_error(code: u32) -> VfsError {
    match code {
        1 => VfsError::NotFound,
        2 => VfsError::PermissionDenied,
        3 => VfsError::AlreadyExists,
        4 => VfsError::NotADirectory,
        5 => VfsError::IsADirectory,
        6 => VfsError::NotEmpty,
        7 => VfsError::BadFd,
        8 => VfsError::TooManyOpenFiles,
        9 => VfsError::NoSpace,
        10 => VfsError::ReadOnlyFs,
        11 => VfsError::InvalidArgument,
        12 => VfsError::IoError,
        13 => VfsError::NotSupported,
        14 => VfsError::PathTooLong,
        _ => VfsError::IoError,
    }
}

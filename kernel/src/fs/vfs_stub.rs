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
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use spin::{Mutex, Once};

use crate::cap::CapabilityToken;
use crate::ipc::{ChannelId, IpcError, Message, MessageData, IPC_MANAGER};
use crate::ipc::fastpath::{FastEndpoint, FastMessage, IPC_STATS, tags};
use crate::sched::ProcessId;

/// Storage channel (initialized at boot)
static STORAGE_CHANNEL: Once<StorageChannel> = Once::new();

/// Whether stub is initialized
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Request ID counter
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

/// IPC timeout in spin iterations (approx 100ms at 1GHz)
const IPC_TIMEOUT_SPINS: u64 = 100_000_000;

/// Fast path timeout in iterations
#[allow(dead_code)]
const FASTPATH_TIMEOUT_SPINS: u64 = 1_000_000;

/// S-STORAGE service ID for fast path
#[allow(dead_code)]
const STORAGE_SERVICE_ID: u64 = 0x53544F52; // "STOR"

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

/// Pending requests waiting for responses
static PENDING_REQUESTS: Mutex<BTreeMap<u64, Option<Vec<u8>>>> = Mutex::new(BTreeMap::new());

/// Storage channel wrapper with full IPC support
struct StorageChannel {
    /// IPC channel identifier
    channel_id: ChannelId,
    /// Capability token for IPC authorization
    capability: CapabilityToken,
    /// Fast path endpoint for small messages (optional)
    fast_endpoint: Option<FastEndpoint>,
    /// Whether to use fast path when possible
    use_fast_path: bool,
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
/// 
/// # Arguments
/// * `channel_id` - The IPC channel ID connected to S-STORAGE service
/// * `capability` - Capability token authorizing IPC operations
pub fn init(channel_id: ChannelId, capability: CapabilityToken) {
    STORAGE_CHANNEL.call_once(|| StorageChannel { 
        channel_id,
        capability,
        fast_endpoint: None, // Fast path configured separately
        use_fast_path: false,
    });
    INITIALIZED.store(true, Ordering::Release);
}

/// Initialize VFS stub with fast path support
/// 
/// # Arguments
/// * `channel_id` - The IPC channel ID for fallback
/// * `capability` - Capability token authorizing IPC operations
/// * `fast_endpoint` - Fast path endpoint for optimized small messages
pub fn init_with_fast_path(
    channel_id: ChannelId,
    capability: CapabilityToken,
    fast_endpoint: FastEndpoint,
) {
    STORAGE_CHANNEL.call_once(|| StorageChannel {
        channel_id,
        capability,
        fast_endpoint: Some(fast_endpoint),
        use_fast_path: true,
    });
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
// Statistics and Diagnostics
// ============================================================================

/// VFS stub statistics
#[derive(Debug, Clone)]
pub struct VfsStubStats {
    /// Total requests sent
    pub total_requests: u64,
    /// Fast path messages sent
    pub fast_path_sends: u64,
    /// Fast path messages received
    pub fast_path_recvs: u64,
    /// Slow path fallbacks
    pub slow_path_count: u64,
    /// Buffer full events
    pub buffer_full_events: u64,
    /// Pending requests currently waiting
    pub pending_requests: usize,
    /// Open file descriptors
    pub open_fds: usize,
}

/// Get VFS stub statistics
pub fn get_stats() -> VfsStubStats {
    let pending = PENDING_REQUESTS.lock().len();
    let fds = FD_TABLE.lock().len();
    
    VfsStubStats {
        total_requests: REQUEST_COUNTER.load(Ordering::Relaxed) - 1,
        fast_path_sends: IPC_STATS.fast_sends.load(Ordering::Relaxed),
        fast_path_recvs: IPC_STATS.fast_recvs.load(Ordering::Relaxed),
        slow_path_count: IPC_STATS.slow_path.load(Ordering::Relaxed),
        buffer_full_events: IPC_STATS.buffer_full.load(Ordering::Relaxed),
        pending_requests: pending,
        open_fds: fds,
    }
}

/// Check if fast path is enabled
pub fn is_fast_path_enabled() -> bool {
    STORAGE_CHANNEL
        .get()
        .map(|ch| ch.use_fast_path)
        .unwrap_or(false)
}

/// Get the channel ID used for S-STORAGE communication
pub fn get_channel_id() -> Option<ChannelId> {
    STORAGE_CHANNEL.get().map(|ch| ch.channel_id)
}

// ============================================================================
// Internal Functions (message building/parsing)
// ============================================================================

/// Send a VFS request to S-STORAGE and receive the response.
/// 
/// This function handles the kernel-to-userspace IPC bridge:
/// 1. Attempts fast path for small messages if available
/// 2. Falls back to regular IPC channel
/// 3. Implements timeout handling
/// 4. Deserializes and returns the response
fn send_and_receive(request: Vec<u8>) -> Result<Vec<u8>, VfsError> {
    let channel = STORAGE_CHANNEL.get().ok_or(VfsError::NotInitialized)?;
    
    // Try fast path first for small messages
    if channel.use_fast_path && request.len() <= 56 {
        if let Some(ref endpoint) = channel.fast_endpoint {
            return send_and_receive_fast(endpoint, &request);
        }
    }
    
    // Fall back to regular IPC
    send_and_receive_standard(channel, request)
}

/// Fast path IPC for small messages (≤56 bytes)
/// 
/// Uses lock-free SPSC ring buffers for minimal latency.
fn send_and_receive_fast(endpoint: &FastEndpoint, request: &[u8]) -> Result<Vec<u8>, VfsError> {
    // Determine the message tag based on request type
    let request_type = if request.len() >= 4 {
        u32::from_le_bytes(request[0..4].try_into().unwrap())
    } else {
        return Err(VfsError::InvalidArgument);
    };
    
    let tag = match request_type {
        3 => tags::VFS_OPEN,
        4 => tags::VFS_CLOSE,
        5 => tags::VFS_READ,
        6 => tags::VFS_WRITE,
        7 => tags::VFS_STAT,
        9 => tags::VFS_READDIR,
        _ => {
            // Unsupported operation for fast path, fall back to standard
            IPC_STATS.record_slow_path();
            return Err(VfsError::NotSupported);
        }
    };
    
    // Create fast message
    let fast_msg = FastMessage::from_bytes(tag, request);
    
    // Send and wait for reply with timeout
    match endpoint.call(fast_msg) {
        Ok(reply) => {
            IPC_STATS.record_fast_send();
            IPC_STATS.record_fast_recv();
            
            // Check reply status
            if reply.tag == tags::REPLY_ERROR {
                let error_code = reply.data0 as u32;
                return Err(error_code_to_vfs_error(error_code));
            }
            
            // Extract response data
            let mut response = Vec::with_capacity(64);
            let mut buffer = [0u8; 56];
            reply.to_bytes(&mut buffer);
            response.extend_from_slice(&buffer);
            
            Ok(response)
        }
        Err(IpcError::Timeout) => Err(VfsError::Timeout),
        Err(IpcError::BufferFull) => {
            // Fast path buffer full, record and let caller retry via standard path
            IPC_STATS.buffer_full.fetch_add(1, Ordering::Relaxed);
            Err(VfsError::IpcError)
        }
        Err(_) => Err(VfsError::IpcError),
    }
}

/// Standard IPC path for larger messages or when fast path is unavailable
/// 
/// Uses the IPC manager's channel-based messaging with proper serialization.
fn send_and_receive_standard(channel: &StorageChannel, request: Vec<u8>) -> Result<Vec<u8>, VfsError> {
    // Extract request ID for matching response
    let request_id = if request.len() >= 12 {
        u64::from_le_bytes(request[4..12].try_into().unwrap())
    } else {
        return Err(VfsError::InvalidArgument);
    };
    
    // Register pending request before sending
    {
        let mut pending = PENDING_REQUESTS.lock();
        pending.insert(request_id, None);
    }
    
    // Create IPC message with kernel process ID as sender
    let sender = ProcessId::KERNEL;
    let ipc_msg = Message::inline(sender, request);
    
    // Send via IPC manager
    IPC_MANAGER.send(
        channel.channel_id,
        sender,
        ipc_msg,
        &channel.capability,
    ).map_err(|e| {
        // Clean up pending request on send failure
        let mut pending = PENDING_REQUESTS.lock();
        pending.remove(&request_id);
        ipc_error_to_vfs_error(e)
    })?;
    
    // Poll for response with timeout
    let mut spin_count: u64 = 0;
    loop {
        // Try to receive response
        match IPC_MANAGER.receive(
            channel.channel_id,
            sender, // Kernel receives responses
            &channel.capability,
        ) {
            Ok(response_msg) => {
                // Extract response data
                let response_data = match response_msg.data {
                    MessageData::Inline(data) => data,
                    MessageData::SharedRef { addr, size } => {
                        // Handle shared memory response (zero-copy path)
                        read_shared_memory_response(addr, size)?
                    }
                };
                
                // Verify this response matches our request
                if response_data.len() >= 12 {
                    let response_request_id = u64::from_le_bytes(
                        response_data[4..12].try_into().unwrap()
                    );
                    
                    if response_request_id == request_id {
                        // Clean up pending request
                        let mut pending = PENDING_REQUESTS.lock();
                        pending.remove(&request_id);
                        return Ok(response_data);
                    } else {
                        // Response for different request, store it
                        let mut pending = PENDING_REQUESTS.lock();
                        if pending.contains_key(&response_request_id) {
                            pending.insert(response_request_id, Some(response_data));
                        }
                        // Continue waiting for our response
                    }
                }
            }
            Err(IpcError::BufferEmpty) => {
                // Check if our response was stored by another receiver
                let mut pending = PENDING_REQUESTS.lock();
                if let Some(Some(data)) = pending.remove(&request_id) {
                    return Ok(data);
                }
                drop(pending);
                
                // No response yet, spin wait
                core::hint::spin_loop();
            }
            Err(IpcError::ChannelClosed) => {
                let mut pending = PENDING_REQUESTS.lock();
                pending.remove(&request_id);
                return Err(VfsError::IpcError);
            }
            Err(e) => {
                let mut pending = PENDING_REQUESTS.lock();
                pending.remove(&request_id);
                return Err(ipc_error_to_vfs_error(e));
            }
        }
        
        // Increment spin counter and check timeout
        spin_count += 1;
        if spin_count >= IPC_TIMEOUT_SPINS {
            let mut pending = PENDING_REQUESTS.lock();
            pending.remove(&request_id);
            return Err(VfsError::Timeout);
        }
        
        // Yield after spinning for a while to allow other work
        if spin_count % 10000 == 0 {
            core::hint::spin_loop();
        }
    }
}

/// Read response data from shared memory region (zero-copy path)
fn read_shared_memory_response(addr: u64, size: usize) -> Result<Vec<u8>, VfsError> {
    // Safety: This assumes the shared memory region is valid and accessible
    // In a real implementation, this would verify the memory mapping
    if size > 64 * 1024 {
        return Err(VfsError::InvalidArgument); // Max 64KB response
    }
    
    let mut data = Vec::with_capacity(size);
    
    // Safety: We trust that S-STORAGE has set up this shared memory correctly
    // The kernel has validated the address range during IPC setup
    unsafe {
        let ptr = addr as *const u8;
        for i in 0..size {
            data.push(ptr.add(i).read_volatile());
        }
    }
    
    Ok(data)
}

/// Convert IPC error to VFS error
fn ipc_error_to_vfs_error(error: IpcError) -> VfsError {
    match error {
        IpcError::ChannelNotFound => VfsError::NotInitialized,
        IpcError::NotAuthorized => VfsError::PermissionDenied,
        IpcError::ChannelClosed => VfsError::IpcError,
        IpcError::BufferFull => VfsError::IpcError,
        IpcError::BufferEmpty => VfsError::IpcError,
        IpcError::MessageTooLarge => VfsError::InvalidArgument,
        IpcError::TooManyChannels => VfsError::IpcError,
        IpcError::InvalidCapability => VfsError::PermissionDenied,
        IpcError::Timeout => VfsError::Timeout,
    }
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

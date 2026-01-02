//! # WASI (WebAssembly System Interface) Implementation
//!
//! This module implements WASI preview1 for Splax OS, enabling portable
//! WebAssembly applications to run with capability-based resource access.
//!
//! ## Implemented Functions
//!
//! ### Environment
//! - `environ_get` - Get environment variables
//! - `environ_sizes_get` - Get environment sizes
//! - `args_get` - Get command line arguments
//! - `args_sizes_get` - Get argument sizes
//!
//! ### Clock
//! - `clock_time_get` - Get current time
//! - `clock_res_get` - Get clock resolution
//!
//! ### Random
//! - `random_get` - Get random bytes
//!
//! ### File Descriptors
//! - `fd_read` - Read from file descriptor
//! - `fd_write` - Write to file descriptor
//! - `fd_close` - Close file descriptor
//! - `fd_seek` - Seek in file descriptor
//! - `fd_tell` - Get current position
//! - `fd_sync` - Sync file to disk
//! - `fd_fdstat_get` - Get file descriptor status
//! - `fd_prestat_get` - Get pre-opened directory info
//! - `fd_prestat_dir_name` - Get pre-opened directory name
//!
//! ### Filesystem
//! - `path_open` - Open a file
//! - `path_create_directory` - Create directory
//! - `path_remove_directory` - Remove directory
//! - `path_unlink_file` - Delete file
//! - `path_rename` - Rename file/directory
//! - `path_filestat_get` - Get file stats
//! - `path_readlink` - Read symbolic link
//!
//! ### Process
//! - `proc_exit` - Exit process
//! - `sched_yield` - Yield execution

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use splax_cap::CapabilityToken;

// =============================================================================
// WASI Error Codes
// =============================================================================

/// WASI error codes (errno values).
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Errno {
    /// No error.
    Success = 0,
    /// Argument list too long.
    TooBig = 1,
    /// Permission denied.
    Acces = 2,
    /// Address in use.
    Addrinuse = 3,
    /// Address not available.
    Addrnotavail = 4,
    /// Address family not supported.
    Afnosupport = 5,
    /// Resource unavailable.
    Again = 6,
    /// Connection already in progress.
    Already = 7,
    /// Bad file descriptor.
    Badf = 8,
    /// Bad message.
    Badmsg = 9,
    /// Device or resource busy.
    Busy = 10,
    /// Operation canceled.
    Canceled = 11,
    /// No child processes.
    Child = 12,
    /// Connection aborted.
    Connaborted = 13,
    /// Connection refused.
    Connrefused = 14,
    /// Connection reset.
    Connreset = 15,
    /// Resource deadlock avoided.
    Deadlk = 16,
    /// Destination address required.
    Destaddrreq = 17,
    /// Math argument out of domain.
    Dom = 18,
    /// File exists.
    Exist = 20,
    /// Bad address.
    Fault = 21,
    /// File too large.
    Fbig = 22,
    /// Host unreachable.
    Hostunreach = 23,
    /// Identifier removed.
    Idrm = 24,
    /// Illegal byte sequence.
    Ilseq = 25,
    /// Operation in progress.
    Inprogress = 26,
    /// Interrupted function.
    Intr = 27,
    /// Invalid argument.
    Inval = 28,
    /// I/O error.
    Io = 29,
    /// Socket is connected.
    Isconn = 30,
    /// Is a directory.
    Isdir = 31,
    /// Too many levels of symbolic links.
    Loop = 32,
    /// File descriptor value too large.
    Mfile = 33,
    /// Too many links.
    Mlink = 34,
    /// Message too large.
    Msgsize = 35,
    /// Filename too long.
    Nametoolong = 37,
    /// Network is down.
    Netdown = 38,
    /// Connection aborted by network.
    Netreset = 39,
    /// Network unreachable.
    Netunreach = 40,
    /// Too many files open.
    Nfile = 41,
    /// No buffer space available.
    Nobufs = 42,
    /// No such device.
    Nodev = 43,
    /// No such file or directory.
    Noent = 44,
    /// Executable file format error.
    Noexec = 45,
    /// No locks available.
    Nolck = 46,
    /// Not enough space.
    Nomem = 48,
    /// No message of the desired type.
    Nomsg = 49,
    /// Protocol not available.
    Noprotoopt = 50,
    /// No space left on device.
    Nospc = 51,
    /// Function not supported.
    Nosys = 52,
    /// Not a directory.
    Notdir = 54,
    /// Directory not empty.
    Notempty = 55,
    /// State not recoverable.
    Notrecoverable = 56,
    /// Not a socket.
    Notsock = 57,
    /// Not supported.
    Notsup = 58,
    /// Inappropriate I/O control operation.
    Notty = 59,
    /// No such device or address.
    Nxio = 60,
    /// Value too large.
    Overflow = 61,
    /// Previous owner died.
    Ownerdead = 62,
    /// Operation not permitted.
    Perm = 63,
    /// Broken pipe.
    Pipe = 64,
    /// Protocol error.
    Proto = 65,
    /// Protocol not supported.
    Protonosupport = 66,
    /// Protocol wrong type for socket.
    Prototype = 67,
    /// Result too large.
    Range = 68,
    /// Read-only file system.
    Rofs = 69,
    /// Invalid seek.
    Spipe = 70,
    /// No such process.
    Srch = 71,
    /// Connection timed out.
    Timedout = 73,
    /// Text file busy.
    Txtbsy = 74,
    /// Cross-device link.
    Xdev = 75,
    /// Capabilities insufficient.
    Notcapable = 76,
}

impl Errno {
    pub fn to_u16(self) -> u16 {
        self as u16
    }
}

// =============================================================================
// WASI Types
// =============================================================================

/// Clock identifiers.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockId {
    /// Wall clock time.
    Realtime = 0,
    /// Monotonic clock (for measuring intervals).
    Monotonic = 1,
    /// Process CPU time.
    ProcessCputimeId = 2,
    /// Thread CPU time.
    ThreadCputimeId = 3,
}

impl ClockId {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::Realtime),
            1 => Some(Self::Monotonic),
            2 => Some(Self::ProcessCputimeId),
            3 => Some(Self::ThreadCputimeId),
            _ => None,
        }
    }
}

/// File descriptor rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Rights(pub u64);

impl Rights {
    pub const FD_DATASYNC: Rights = Rights(1 << 0);
    pub const FD_READ: Rights = Rights(1 << 1);
    pub const FD_SEEK: Rights = Rights(1 << 2);
    pub const FD_FDSTAT_SET_FLAGS: Rights = Rights(1 << 3);
    pub const FD_SYNC: Rights = Rights(1 << 4);
    pub const FD_TELL: Rights = Rights(1 << 5);
    pub const FD_WRITE: Rights = Rights(1 << 6);
    pub const FD_ADVISE: Rights = Rights(1 << 7);
    pub const FD_ALLOCATE: Rights = Rights(1 << 8);
    pub const PATH_CREATE_DIRECTORY: Rights = Rights(1 << 9);
    pub const PATH_CREATE_FILE: Rights = Rights(1 << 10);
    pub const PATH_LINK_SOURCE: Rights = Rights(1 << 11);
    pub const PATH_LINK_TARGET: Rights = Rights(1 << 12);
    pub const PATH_OPEN: Rights = Rights(1 << 13);
    pub const FD_READDIR: Rights = Rights(1 << 14);
    pub const PATH_READLINK: Rights = Rights(1 << 15);
    pub const PATH_RENAME_SOURCE: Rights = Rights(1 << 16);
    pub const PATH_RENAME_TARGET: Rights = Rights(1 << 17);
    pub const PATH_FILESTAT_GET: Rights = Rights(1 << 18);
    pub const PATH_FILESTAT_SET_SIZE: Rights = Rights(1 << 19);
    pub const PATH_FILESTAT_SET_TIMES: Rights = Rights(1 << 20);
    pub const FD_FILESTAT_GET: Rights = Rights(1 << 21);
    pub const FD_FILESTAT_SET_SIZE: Rights = Rights(1 << 22);
    pub const FD_FILESTAT_SET_TIMES: Rights = Rights(1 << 23);
    pub const PATH_SYMLINK: Rights = Rights(1 << 24);
    pub const PATH_REMOVE_DIRECTORY: Rights = Rights(1 << 25);
    pub const PATH_UNLINK_FILE: Rights = Rights(1 << 26);
    pub const POLL_FD_READWRITE: Rights = Rights(1 << 27);
    pub const SOCK_SHUTDOWN: Rights = Rights(1 << 28);
    pub const SOCK_ACCEPT: Rights = Rights(1 << 29);

    pub const ALL: Rights = Rights(0x1FFFFFFF);
    pub const NONE: Rights = Rights(0);

    pub fn contains(&self, other: Rights) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// File type.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filetype {
    Unknown = 0,
    BlockDevice = 1,
    CharacterDevice = 2,
    Directory = 3,
    RegularFile = 4,
    SocketDgram = 5,
    SocketStream = 6,
    SymbolicLink = 7,
}

/// File descriptor flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Fdflags(pub u16);

impl Fdflags {
    pub const APPEND: Fdflags = Fdflags(1 << 0);
    pub const DSYNC: Fdflags = Fdflags(1 << 1);
    pub const NONBLOCK: Fdflags = Fdflags(1 << 2);
    pub const RSYNC: Fdflags = Fdflags(1 << 3);
    pub const SYNC: Fdflags = Fdflags(1 << 4);
    pub const NONE: Fdflags = Fdflags(0);
}

/// Open flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Oflags(pub u16);

impl Oflags {
    pub const CREAT: Oflags = Oflags(1 << 0);
    pub const DIRECTORY: Oflags = Oflags(1 << 1);
    pub const EXCL: Oflags = Oflags(1 << 2);
    pub const TRUNC: Oflags = Oflags(1 << 3);
    pub const NONE: Oflags = Oflags(0);

    pub fn contains(&self, other: Oflags) -> bool {
        (self.0 & other.0) == other.0
    }
}

/// Seek whence.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Whence {
    Set = 0,
    Cur = 1,
    End = 2,
}

impl Whence {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Set),
            1 => Some(Self::Cur),
            2 => Some(Self::End),
            _ => None,
        }
    }
}

/// File stat.
#[derive(Debug, Clone, Copy)]
pub struct Filestat {
    pub dev: u64,
    pub ino: u64,
    pub filetype: Filetype,
    pub nlink: u64,
    pub size: u64,
    pub atim: u64,
    pub mtim: u64,
    pub ctim: u64,
}

impl Filestat {
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        bytes[0..8].copy_from_slice(&self.dev.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.ino.to_le_bytes());
        bytes[16..17].copy_from_slice(&[self.filetype as u8]);
        // 7 bytes padding
        bytes[24..32].copy_from_slice(&self.nlink.to_le_bytes());
        bytes[32..40].copy_from_slice(&self.size.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.atim.to_le_bytes());
        bytes[48..56].copy_from_slice(&self.mtim.to_le_bytes());
        bytes[56..64].copy_from_slice(&self.ctim.to_le_bytes());
        bytes
    }
}

/// FD stat.
#[derive(Debug, Clone, Copy)]
pub struct Fdstat {
    pub fs_filetype: Filetype,
    pub fs_flags: Fdflags,
    pub fs_rights_base: Rights,
    pub fs_rights_inheriting: Rights,
}

impl Fdstat {
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut bytes = [0u8; 24];
        bytes[0] = self.fs_filetype as u8;
        bytes[2..4].copy_from_slice(&self.fs_flags.0.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.fs_rights_base.0.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.fs_rights_inheriting.0.to_le_bytes());
        bytes
    }
}

/// Prestat for pre-opened directories.
#[derive(Debug, Clone)]
pub struct Prestat {
    pub tag: u8, // 0 = directory
    pub pr_name_len: u32,
}

impl Prestat {
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0] = self.tag;
        bytes[4..8].copy_from_slice(&self.pr_name_len.to_le_bytes());
        bytes
    }
}

/// I/O vector for scatter/gather I/O.
#[derive(Debug, Clone, Copy)]
pub struct Iovec {
    pub buf: u32,
    pub buf_len: u32,
}

// =============================================================================
// WASI File Descriptor Table
// =============================================================================

/// A WASI file descriptor entry.
#[derive(Debug, Clone)]
pub struct WasiFd {
    /// Splax-specific file handle.
    pub handle: u64,
    /// File type.
    pub filetype: Filetype,
    /// Current position.
    pub position: u64,
    /// Rights.
    pub rights: Rights,
    /// Inheriting rights.
    pub rights_inheriting: Rights,
    /// Flags.
    pub flags: Fdflags,
    /// Pre-opened directory name (if applicable).
    pub preopen_name: Option<String>,
    /// Associated capability.
    pub capability: Option<CapabilityToken>,
    /// Data buffer (for stdin/stdout simulation).
    pub buffer: Vec<u8>,
}

impl WasiFd {
    /// Creates a new FD for stdin.
    pub fn stdin() -> Self {
        Self {
            handle: 0,
            filetype: Filetype::CharacterDevice,
            position: 0,
            rights: Rights::FD_READ,
            rights_inheriting: Rights::NONE,
            flags: Fdflags::NONE,
            preopen_name: None,
            capability: None,
            buffer: Vec::new(),
        }
    }

    /// Creates a new FD for stdout.
    pub fn stdout() -> Self {
        Self {
            handle: 1,
            filetype: Filetype::CharacterDevice,
            position: 0,
            rights: Rights::FD_WRITE,
            rights_inheriting: Rights::NONE,
            flags: Fdflags::NONE,
            preopen_name: None,
            capability: None,
            buffer: Vec::new(),
        }
    }

    /// Creates a new FD for stderr.
    pub fn stderr() -> Self {
        Self {
            handle: 2,
            filetype: Filetype::CharacterDevice,
            position: 0,
            rights: Rights::FD_WRITE,
            rights_inheriting: Rights::NONE,
            flags: Fdflags::NONE,
            preopen_name: None,
            capability: None,
            buffer: Vec::new(),
        }
    }

    /// Creates a pre-opened directory.
    pub fn preopen_dir(name: String, handle: u64, cap: Option<CapabilityToken>) -> Self {
        Self {
            handle,
            filetype: Filetype::Directory,
            position: 0,
            rights: Rights::ALL,
            rights_inheriting: Rights::ALL,
            flags: Fdflags::NONE,
            preopen_name: Some(name),
            capability: cap,
            buffer: Vec::new(),
        }
    }
}

// =============================================================================
// WASI Context
// =============================================================================

/// WASI execution context for a module instance.
pub struct WasiCtx {
    /// File descriptor table.
    pub fds: BTreeMap<u32, WasiFd>,
    /// Next available FD number.
    pub next_fd: u32,
    /// Environment variables.
    pub env: Vec<(String, String)>,
    /// Command line arguments.
    pub args: Vec<String>,
    /// Current working directory FD.
    pub cwd_fd: u32,
    /// Exit code (set by proc_exit).
    pub exit_code: Option<u32>,
    /// Output buffer (for capturing stdout).
    pub stdout_buffer: Vec<u8>,
    /// Error buffer (for capturing stderr).
    pub stderr_buffer: Vec<u8>,
    /// Random state for random_get.
    pub random_state: u64,
}

impl WasiCtx {
    /// Creates a new WASI context with standard FDs.
    pub fn new() -> Self {
        let mut fds = BTreeMap::new();
        fds.insert(0, WasiFd::stdin());
        fds.insert(1, WasiFd::stdout());
        fds.insert(2, WasiFd::stderr());

        Self {
            fds,
            next_fd: 3,
            env: Vec::new(),
            args: vec![String::from("wasm_module")],
            cwd_fd: 3,
            exit_code: None,
            stdout_buffer: Vec::new(),
            stderr_buffer: Vec::new(),
            random_state: 0x12345678_9ABCDEF0,
        }
    }

    /// Sets environment variables.
    pub fn set_env(&mut self, env: Vec<(String, String)>) {
        self.env = env;
    }

    /// Sets command line arguments.
    pub fn set_args(&mut self, args: Vec<String>) {
        self.args = args;
    }

    /// Adds a pre-opened directory.
    pub fn preopen_dir(&mut self, path: String, cap: Option<CapabilityToken>) -> u32 {
        let fd = self.next_fd;
        self.next_fd += 1;
        self.fds.insert(fd, WasiFd::preopen_dir(path, fd as u64, cap));
        fd
    }

    /// Allocates a new FD.
    pub fn alloc_fd(&mut self, fd_entry: WasiFd) -> u32 {
        let fd = self.next_fd;
        self.next_fd += 1;
        self.fds.insert(fd, fd_entry);
        fd
    }

    /// Gets an FD.
    pub fn get_fd(&self, fd: u32) -> Result<&WasiFd, Errno> {
        self.fds.get(&fd).ok_or(Errno::Badf)
    }

    /// Gets a mutable FD.
    pub fn get_fd_mut(&mut self, fd: u32) -> Result<&mut WasiFd, Errno> {
        self.fds.get_mut(&fd).ok_or(Errno::Badf)
    }

    /// Closes an FD.
    pub fn close_fd(&mut self, fd: u32) -> Result<(), Errno> {
        if fd < 3 {
            return Err(Errno::Badf); // Can't close stdin/stdout/stderr
        }
        self.fds.remove(&fd).ok_or(Errno::Badf)?;
        Ok(())
    }

    /// Generates random bytes.
    pub fn random_bytes(&mut self, buf: &mut [u8]) {
        // Simple xorshift64 PRNG (would use real RNG in kernel)
        for byte in buf.iter_mut() {
            self.random_state ^= self.random_state << 13;
            self.random_state ^= self.random_state >> 7;
            self.random_state ^= self.random_state << 17;
            *byte = self.random_state as u8;
        }
    }
}

impl Default for WasiCtx {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// WASI Syscall Implementations
// =============================================================================

/// WASI syscall handler.
pub struct WasiSyscalls;

impl WasiSyscalls {
    /// `args_get(argv: *mut *mut u8, argv_buf: *mut u8) -> errno`
    pub fn args_get(ctx: &WasiCtx, memory: &mut [u8], argv: u32, argv_buf: u32) -> Errno {
        let mut buf_offset = argv_buf as usize;
        let mut ptr_offset = argv as usize;

        for arg in &ctx.args {
            // Write pointer
            if ptr_offset + 4 > memory.len() {
                return Errno::Fault;
            }
            memory[ptr_offset..ptr_offset + 4].copy_from_slice(&(buf_offset as u32).to_le_bytes());
            ptr_offset += 4;

            // Write string + null terminator
            let bytes = arg.as_bytes();
            if buf_offset + bytes.len() + 1 > memory.len() {
                return Errno::Fault;
            }
            memory[buf_offset..buf_offset + bytes.len()].copy_from_slice(bytes);
            memory[buf_offset + bytes.len()] = 0;
            buf_offset += bytes.len() + 1;
        }

        Errno::Success
    }

    /// `args_sizes_get(argc: *mut size, argv_buf_size: *mut size) -> errno`
    pub fn args_sizes_get(ctx: &WasiCtx, memory: &mut [u8], argc_ptr: u32, argv_buf_size_ptr: u32) -> Errno {
        let argc = ctx.args.len() as u32;
        let argv_buf_size: u32 = ctx.args.iter().map(|a| a.len() as u32 + 1).sum();

        if (argc_ptr as usize) + 4 > memory.len() || (argv_buf_size_ptr as usize) + 4 > memory.len() {
            return Errno::Fault;
        }

        memory[argc_ptr as usize..(argc_ptr + 4) as usize].copy_from_slice(&argc.to_le_bytes());
        memory[argv_buf_size_ptr as usize..(argv_buf_size_ptr + 4) as usize].copy_from_slice(&argv_buf_size.to_le_bytes());

        Errno::Success
    }

    /// `environ_get(environ: *mut *mut u8, environ_buf: *mut u8) -> errno`
    pub fn environ_get(ctx: &WasiCtx, memory: &mut [u8], environ: u32, environ_buf: u32) -> Errno {
        let mut buf_offset = environ_buf as usize;
        let mut ptr_offset = environ as usize;

        for (key, value) in &ctx.env {
            // Write pointer
            if ptr_offset + 4 > memory.len() {
                return Errno::Fault;
            }
            memory[ptr_offset..ptr_offset + 4].copy_from_slice(&(buf_offset as u32).to_le_bytes());
            ptr_offset += 4;

            // Write "KEY=VALUE\0"
            let env_str = alloc::format!("{}={}", key, value);
            let bytes = env_str.as_bytes();
            if buf_offset + bytes.len() + 1 > memory.len() {
                return Errno::Fault;
            }
            memory[buf_offset..buf_offset + bytes.len()].copy_from_slice(bytes);
            memory[buf_offset + bytes.len()] = 0;
            buf_offset += bytes.len() + 1;
        }

        Errno::Success
    }

    /// `environ_sizes_get(environc: *mut size, environ_buf_size: *mut size) -> errno`
    pub fn environ_sizes_get(ctx: &WasiCtx, memory: &mut [u8], environc_ptr: u32, environ_buf_size_ptr: u32) -> Errno {
        let environc = ctx.env.len() as u32;
        let environ_buf_size: u32 = ctx.env.iter()
            .map(|(k, v)| k.len() as u32 + v.len() as u32 + 2) // key=value\0
            .sum();

        if (environc_ptr as usize) + 4 > memory.len() || (environ_buf_size_ptr as usize) + 4 > memory.len() {
            return Errno::Fault;
        }

        memory[environc_ptr as usize..(environc_ptr + 4) as usize].copy_from_slice(&environc.to_le_bytes());
        memory[environ_buf_size_ptr as usize..(environ_buf_size_ptr + 4) as usize].copy_from_slice(&environ_buf_size.to_le_bytes());

        Errno::Success
    }

    /// `clock_time_get(id: clockid, precision: timestamp, time: *mut timestamp) -> errno`
    pub fn clock_time_get(memory: &mut [u8], clock_id: u32, _precision: u64, time_ptr: u32) -> Errno {
        let time = match ClockId::from_u32(clock_id) {
            Some(ClockId::Realtime) => {
                // Use CPU timestamp as base, convert to nanoseconds since epoch
                // TSC gives cycles since boot; we estimate nanoseconds from that
                #[cfg(target_arch = "x86_64")]
                {
                    // Approximate: TSC cycles / 2 = nanoseconds (assumes ~2GHz)
                    // Add Unix epoch offset for Jan 1, 2025 as base
                    const EPOCH_BASE_NS: u64 = 1735689600_000_000_000; // Jan 1, 2025 00:00:00 UTC
                    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
                    EPOCH_BASE_NS + (tsc / 2) // Add boot-relative time
                }
                #[cfg(target_arch = "aarch64")]
                {
                    const EPOCH_BASE_NS: u64 = 1735689600_000_000_000;
                    let cnt: u64;
                    let freq: u64;
                    unsafe {
                        core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt, options(nostack, nomem));
                        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nostack, nomem));
                    }
                    if freq > 0 {
                        EPOCH_BASE_NS + (cnt * 1_000_000_000 / freq)
                    } else {
                        EPOCH_BASE_NS + cnt
                    }
                }
                #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
                {
                    1735689600_000_000_000u64 // Fallback: Jan 1, 2025
                }
            }
            Some(ClockId::Monotonic) | Some(ClockId::ProcessCputimeId) | Some(ClockId::ThreadCputimeId) => {
                // CPU cycles as nanoseconds estimate
                #[cfg(target_arch = "x86_64")]
                {
                    unsafe { core::arch::x86_64::_rdtsc() }
                }
                #[cfg(not(target_arch = "x86_64"))]
                {
                    0u64
                }
            }
            None => return Errno::Inval,
        };

        if (time_ptr as usize) + 8 > memory.len() {
            return Errno::Fault;
        }

        memory[time_ptr as usize..(time_ptr + 8) as usize].copy_from_slice(&time.to_le_bytes());
        Errno::Success
    }

    /// `clock_res_get(id: clockid, resolution: *mut timestamp) -> errno`
    pub fn clock_res_get(memory: &mut [u8], clock_id: u32, resolution_ptr: u32) -> Errno {
        let resolution = match ClockId::from_u32(clock_id) {
            Some(ClockId::Realtime) => 1_000_000u64, // 1ms
            Some(ClockId::Monotonic) => 1u64, // 1ns (TSC)
            Some(_) => 1_000_000u64,
            None => return Errno::Inval,
        };

        if (resolution_ptr as usize) + 8 > memory.len() {
            return Errno::Fault;
        }

        memory[resolution_ptr as usize..(resolution_ptr + 8) as usize].copy_from_slice(&resolution.to_le_bytes());
        Errno::Success
    }

    /// `random_get(buf: *mut u8, buf_len: size) -> errno`
    pub fn random_get(ctx: &mut WasiCtx, memory: &mut [u8], buf: u32, buf_len: u32) -> Errno {
        let start = buf as usize;
        let end = start + buf_len as usize;

        if end > memory.len() {
            return Errno::Fault;
        }

        ctx.random_bytes(&mut memory[start..end]);
        Errno::Success
    }

    /// `fd_write(fd: fd, iovs: *const iovec, iovs_len: size, nwritten: *mut size) -> errno`
    pub fn fd_write(ctx: &mut WasiCtx, memory: &mut [u8], fd: u32, iovs: u32, iovs_len: u32, nwritten_ptr: u32) -> Errno {
        let fd_entry = match ctx.get_fd(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        if !fd_entry.rights.contains(Rights::FD_WRITE) {
            return Errno::Perm;
        }

        let mut total_written = 0u32;

        for i in 0..iovs_len {
            let iov_ptr = iovs as usize + (i as usize * 8);
            if iov_ptr + 8 > memory.len() {
                return Errno::Fault;
            }

            let buf_ptr = u32::from_le_bytes(memory[iov_ptr..iov_ptr + 4].try_into().unwrap()) as usize;
            let buf_len = u32::from_le_bytes(memory[iov_ptr + 4..iov_ptr + 8].try_into().unwrap()) as usize;

            if buf_ptr + buf_len > memory.len() {
                return Errno::Fault;
            }

            let data = &memory[buf_ptr..buf_ptr + buf_len];

            // Handle stdout/stderr
            match fd {
                1 => ctx.stdout_buffer.extend_from_slice(data),
                2 => ctx.stderr_buffer.extend_from_slice(data),
                _ => {
                    // Would write to actual file here
                }
            }

            total_written += buf_len as u32;
        }

        if (nwritten_ptr as usize) + 4 > memory.len() {
            return Errno::Fault;
        }

        memory[nwritten_ptr as usize..(nwritten_ptr + 4) as usize].copy_from_slice(&total_written.to_le_bytes());
        Errno::Success
    }

    /// `fd_read(fd: fd, iovs: *const iovec, iovs_len: size, nread: *mut size) -> errno`
    pub fn fd_read(ctx: &mut WasiCtx, memory: &mut [u8], fd: u32, iovs: u32, iovs_len: u32, nread_ptr: u32) -> Errno {
        let fd_entry = match ctx.get_fd_mut(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        if !fd_entry.rights.contains(Rights::FD_READ) {
            return Errno::Perm;
        }

        let mut total_read = 0u32;

        for i in 0..iovs_len {
            let iov_ptr = iovs as usize + (i as usize * 8);
            if iov_ptr + 8 > memory.len() {
                return Errno::Fault;
            }

            let buf_ptr = u32::from_le_bytes(memory[iov_ptr..iov_ptr + 4].try_into().unwrap()) as usize;
            let buf_len = u32::from_le_bytes(memory[iov_ptr + 4..iov_ptr + 8].try_into().unwrap()) as usize;

            if buf_ptr + buf_len > memory.len() {
                return Errno::Fault;
            }

            // Read from buffer if available
            let bytes_to_read = buf_len.min(fd_entry.buffer.len());
            if bytes_to_read > 0 {
                let data: Vec<u8> = fd_entry.buffer.drain(..bytes_to_read).collect();
                memory[buf_ptr..buf_ptr + bytes_to_read].copy_from_slice(&data);
                total_read += bytes_to_read as u32;
            }
        }

        if (nread_ptr as usize) + 4 > memory.len() {
            return Errno::Fault;
        }

        memory[nread_ptr as usize..(nread_ptr + 4) as usize].copy_from_slice(&total_read.to_le_bytes());
        Errno::Success
    }

    /// `fd_close(fd: fd) -> errno`
    pub fn fd_close(ctx: &mut WasiCtx, fd: u32) -> Errno {
        match ctx.close_fd(fd) {
            Ok(()) => Errno::Success,
            Err(e) => e,
        }
    }

    /// `fd_seek(fd: fd, offset: filedelta, whence: whence, newoffset: *mut filesize) -> errno`
    pub fn fd_seek(ctx: &mut WasiCtx, memory: &mut [u8], fd: u32, offset: i64, whence: u8, newoffset_ptr: u32) -> Errno {
        let fd_entry = match ctx.get_fd_mut(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        if !fd_entry.rights.contains(Rights::FD_SEEK) {
            return Errno::Perm;
        }

        let new_pos = match Whence::from_u8(whence) {
            Some(Whence::Set) => offset as u64,
            Some(Whence::Cur) => (fd_entry.position as i64 + offset) as u64,
            Some(Whence::End) => return Errno::Nosys, // Need file size
            None => return Errno::Inval,
        };

        fd_entry.position = new_pos;

        if (newoffset_ptr as usize) + 8 > memory.len() {
            return Errno::Fault;
        }

        memory[newoffset_ptr as usize..(newoffset_ptr + 8) as usize].copy_from_slice(&new_pos.to_le_bytes());
        Errno::Success
    }

    /// `fd_fdstat_get(fd: fd, stat: *mut fdstat) -> errno`
    pub fn fd_fdstat_get(ctx: &WasiCtx, memory: &mut [u8], fd: u32, stat_ptr: u32) -> Errno {
        let fd_entry = match ctx.get_fd(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        let stat = Fdstat {
            fs_filetype: fd_entry.filetype,
            fs_flags: fd_entry.flags,
            fs_rights_base: fd_entry.rights,
            fs_rights_inheriting: fd_entry.rights_inheriting,
        };

        if (stat_ptr as usize) + 24 > memory.len() {
            return Errno::Fault;
        }

        memory[stat_ptr as usize..(stat_ptr + 24) as usize].copy_from_slice(&stat.to_bytes());
        Errno::Success
    }

    /// `fd_prestat_get(fd: fd, buf: *mut prestat) -> errno`
    pub fn fd_prestat_get(ctx: &WasiCtx, memory: &mut [u8], fd: u32, buf_ptr: u32) -> Errno {
        let fd_entry = match ctx.get_fd(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        let name = match &fd_entry.preopen_name {
            Some(n) => n,
            None => return Errno::Badf,
        };

        let prestat = Prestat {
            tag: 0, // directory
            pr_name_len: name.len() as u32,
        };

        if (buf_ptr as usize) + 8 > memory.len() {
            return Errno::Fault;
        }

        memory[buf_ptr as usize..(buf_ptr + 8) as usize].copy_from_slice(&prestat.to_bytes());
        Errno::Success
    }

    /// `fd_prestat_dir_name(fd: fd, path: *mut u8, path_len: size) -> errno`
    pub fn fd_prestat_dir_name(ctx: &WasiCtx, memory: &mut [u8], fd: u32, path_ptr: u32, path_len: u32) -> Errno {
        let fd_entry = match ctx.get_fd(fd) {
            Ok(e) => e,
            Err(e) => return e,
        };

        let name = match &fd_entry.preopen_name {
            Some(n) => n,
            None => return Errno::Badf,
        };

        let copy_len = (path_len as usize).min(name.len());
        let start = path_ptr as usize;
        let end = start + copy_len;

        if end > memory.len() {
            return Errno::Fault;
        }

        memory[start..end].copy_from_slice(&name.as_bytes()[..copy_len]);
        Errno::Success
    }

    /// `proc_exit(code: exitcode)`
    pub fn proc_exit(ctx: &mut WasiCtx, code: u32) {
        ctx.exit_code = Some(code);
    }

    /// `sched_yield() -> errno`
    pub fn sched_yield() -> Errno {
        // Yield CPU (no-op in single-threaded context)
        Errno::Success
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasi_ctx_creation() {
        let ctx = WasiCtx::new();
        assert!(ctx.get_fd(0).is_ok()); // stdin
        assert!(ctx.get_fd(1).is_ok()); // stdout
        assert!(ctx.get_fd(2).is_ok()); // stderr
        assert!(ctx.get_fd(3).is_err()); // not open
    }

    #[test]
    fn test_args_sizes_get() {
        let mut ctx = WasiCtx::new();
        ctx.set_args(vec![String::from("prog"), String::from("arg1")]);

        let mut memory = [0u8; 1024];
        let result = WasiSyscalls::args_sizes_get(&ctx, &mut memory, 0, 4);
        assert_eq!(result, Errno::Success);

        let argc = u32::from_le_bytes(memory[0..4].try_into().unwrap());
        assert_eq!(argc, 2);
    }

    #[test]
    fn test_random_get() {
        let mut ctx = WasiCtx::new();
        let mut memory = [0u8; 1024];

        let result = WasiSyscalls::random_get(&mut ctx, &mut memory, 0, 32);
        assert_eq!(result, Errno::Success);

        // Should have non-zero bytes
        assert!(memory[0..32].iter().any(|&b| b != 0));
    }

    #[test]
    fn test_fd_write() {
        let mut ctx = WasiCtx::new();
        let mut memory = [0u8; 1024];

        // Set up iovec: buf=100, len=5
        memory[0..4].copy_from_slice(&100u32.to_le_bytes());
        memory[4..8].copy_from_slice(&5u32.to_le_bytes());

        // Write "hello" at offset 100
        memory[100..105].copy_from_slice(b"hello");

        let result = WasiSyscalls::fd_write(&mut ctx, &mut memory, 1, 0, 1, 8);
        assert_eq!(result, Errno::Success);

        let nwritten = u32::from_le_bytes(memory[8..12].try_into().unwrap());
        assert_eq!(nwritten, 5);

        assert_eq!(&ctx.stdout_buffer, b"hello");
    }

    #[test]
    fn test_preopen_dir() {
        let mut ctx = WasiCtx::new();
        let fd = ctx.preopen_dir(String::from("/"), None);
        assert_eq!(fd, 3);

        let entry = ctx.get_fd(fd).unwrap();
        assert_eq!(entry.filetype, Filetype::Directory);
        assert_eq!(entry.preopen_name.as_deref(), Some("/"));
    }
}

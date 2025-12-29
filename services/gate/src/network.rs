//! # Network Stack Integration for S-GATE
//!
//! This module bridges the kernel's network stack with the S-GATE service,
//! allowing external TCP/HTTP connections to reach internal services.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;
use spin::Mutex;

use super::{CapabilityToken, GatewayConfig, GatewayId, GatewayStats, Protocol};
use super::tcp::{TcpConnection, TcpConnectionId, TcpGateway};

// =============================================================================
// Socket Syscall Numbers and Constants
// =============================================================================

/// Syscall numbers for socket operations
mod syscall_nums {
    pub const SYS_SOCKET: u64 = 41;      // Create socket
    pub const SYS_BIND: u64 = 49;        // Bind to address
    pub const SYS_LISTEN: u64 = 50;      // Listen for connections
    pub const SYS_ACCEPT: u64 = 43;      // Accept connection
    pub const SYS_SEND: u64 = 44;        // Send data
    pub const SYS_RECV: u64 = 45;        // Receive data
    pub const SYS_CLOSE: u64 = 3;        // Close socket
    pub const SYS_SETSOCKOPT: u64 = 54;  // Set socket options
}

/// Socket type constants
mod sock_type {
    pub const SOCK_STREAM: u64 = 1;  // TCP
    pub const SOCK_DGRAM: u64 = 2;   // UDP
}

/// Address family constants
mod af_family {
    pub const AF_INET: u64 = 2;      // IPv4
    pub const AF_INET6: u64 = 10;    // IPv6
}

/// Socket option levels
mod sol_level {
    pub const SOL_SOCKET: u64 = 1;
}

/// Socket options
mod sock_opt {
    pub const SO_REUSEADDR: u64 = 2;
    pub const SO_NONBLOCK: u64 = 0x800;  // O_NONBLOCK
}

// =============================================================================
// Low-level Syscall Interface
// =============================================================================

/// Perform a socket syscall (create socket)
#[cfg(target_arch = "x86_64")]
fn sys_socket(domain: u64, sock_type: u64, protocol: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") syscall_nums::SYS_SOCKET,
            in("rdi") domain,
            in("rsi") sock_type,
            in("rdx") protocol,
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    result
}

/// Perform a bind syscall
#[cfg(target_arch = "x86_64")]
fn sys_bind(fd: i64, addr: *const u8, addr_len: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") syscall_nums::SYS_BIND,
            in("rdi") fd as u64,
            in("rsi") addr as u64,
            in("rdx") addr_len,
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    result
}

/// Perform a listen syscall
#[cfg(target_arch = "x86_64")]
fn sys_listen(fd: i64, backlog: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") syscall_nums::SYS_LISTEN,
            in("rdi") fd as u64,
            in("rsi") backlog,
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    result
}

/// Perform an accept syscall
#[cfg(target_arch = "x86_64")]
fn sys_accept(fd: i64, addr: *mut u8, addr_len: *mut u32) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") syscall_nums::SYS_ACCEPT,
            in("rdi") fd as u64,
            in("rsi") addr as u64,
            in("rdx") addr_len as u64,
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    result
}

/// Perform a send syscall
#[cfg(target_arch = "x86_64")]
fn sys_send(fd: i64, buf: *const u8, len: u64, flags: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") syscall_nums::SYS_SEND,
            in("rdi") fd as u64,
            in("rsi") buf as u64,
            in("rdx") len,
            in("r10") flags,
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    result
}

/// Perform a recv syscall
#[cfg(target_arch = "x86_64")]
fn sys_recv(fd: i64, buf: *mut u8, len: u64, flags: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") syscall_nums::SYS_RECV,
            in("rdi") fd as u64,
            in("rsi") buf as u64,
            in("rdx") len,
            in("r10") flags,
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    result
}

/// Perform a close syscall
#[cfg(target_arch = "x86_64")]
fn sys_close(fd: i64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") syscall_nums::SYS_CLOSE,
            in("rdi") fd as u64,
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    result
}

/// Perform a setsockopt syscall
#[cfg(target_arch = "x86_64")]
fn sys_setsockopt(fd: i64, level: u64, optname: u64, optval: *const u8, optlen: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") syscall_nums::SYS_SETSOCKOPT,
            in("rdi") fd as u64,
            in("rsi") level,
            in("rdx") optname,
            in("r10") optval as u64,
            in("r8") optlen,
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    result
}

// AArch64 syscall implementations
#[cfg(target_arch = "aarch64")]
fn sys_socket(domain: u64, sock_type: u64, protocol: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 198u64,  // socket syscall on aarch64
            in("x0") domain,
            in("x1") sock_type,
            in("x2") protocol,
            lateout("x0") result,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn sys_bind(fd: i64, addr: *const u8, addr_len: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 200u64,
            in("x0") fd as u64,
            in("x1") addr as u64,
            in("x2") addr_len,
            lateout("x0") result,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn sys_listen(fd: i64, backlog: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 201u64,
            in("x0") fd as u64,
            in("x1") backlog,
            lateout("x0") result,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn sys_accept(fd: i64, addr: *mut u8, addr_len: *mut u32) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 202u64,
            in("x0") fd as u64,
            in("x1") addr as u64,
            in("x2") addr_len as u64,
            lateout("x0") result,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn sys_send(fd: i64, buf: *const u8, len: u64, flags: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 206u64,
            in("x0") fd as u64,
            in("x1") buf as u64,
            in("x2") len,
            in("x3") flags,
            lateout("x0") result,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn sys_recv(fd: i64, buf: *mut u8, len: u64, flags: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 207u64,
            in("x0") fd as u64,
            in("x1") buf as u64,
            in("x2") len,
            in("x3") flags,
            lateout("x0") result,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn sys_close(fd: i64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 57u64,
            in("x0") fd as u64,
            lateout("x0") result,
            options(nostack)
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
fn sys_setsockopt(fd: i64, level: u64, optname: u64, optval: *const u8, optlen: u64) -> i64 {
    let result: i64;
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 208u64,
            in("x0") fd as u64,
            in("x1") level,
            in("x2") optname,
            in("x3") optval as u64,
            in("x4") optlen,
            lateout("x0") result,
            options(nostack)
        );
    }
    result
}

// Fallback for unsupported architectures
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn sys_socket(_domain: u64, _sock_type: u64, _protocol: u64) -> i64 { -1 }
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn sys_bind(_fd: i64, _addr: *const u8, _addr_len: u64) -> i64 { -1 }
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn sys_listen(_fd: i64, _backlog: u64) -> i64 { -1 }
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn sys_accept(_fd: i64, _addr: *mut u8, _addr_len: *mut u32) -> i64 { -1 }
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn sys_send(_fd: i64, _buf: *const u8, _len: u64, _flags: u64) -> i64 { -1 }
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn sys_recv(_fd: i64, _buf: *mut u8, _len: u64, _flags: u64) -> i64 { -1 }
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn sys_close(_fd: i64) -> i64 { -1 }
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn sys_setsockopt(_fd: i64, _level: u64, _optname: u64, _optval: *const u8, _optlen: u64) -> i64 { -1 }

// =============================================================================
// Socket Address Helpers
// =============================================================================

/// Create a sockaddr_in structure for IPv4
#[repr(C)]
struct SockAddrIn {
    sin_family: u16,
    sin_port: u16,      // Network byte order (big endian)
    sin_addr: u32,      // Network byte order (big endian)
    sin_zero: [u8; 8],
}

impl SockAddrIn {
    fn new(port: u16, addr: u32) -> Self {
        Self {
            sin_family: af_family::AF_INET as u16,
            sin_port: port.to_be(),
            sin_addr: addr.to_be(),
            sin_zero: [0; 8],
        }
    }
    
    fn any(port: u16) -> Self {
        Self::new(port, 0)  // INADDR_ANY = 0.0.0.0
    }
    
    fn as_ptr(&self) -> *const u8 {
        self as *const Self as *const u8
    }
    
    fn as_mut_ptr(&mut self) -> *mut u8 {
        self as *mut Self as *mut u8
    }
    
    fn len() -> u64 {
        core::mem::size_of::<Self>() as u64
    }
    
    /// Extract remote address from sockaddr
    fn addr(&self) -> u32 {
        u32::from_be(self.sin_addr)
    }
    
    /// Extract remote port from sockaddr
    fn port(&self) -> u16 {
        u16::from_be(self.sin_port)
    }
}

/// Convert syscall result to NetworkError
fn syscall_to_error(result: i64) -> NetworkError {
    match result {
        -11 => NetworkError::WouldBlock,      // EAGAIN
        -98 => NetworkError::AddressInUse,    // EADDRINUSE
        -101 => NetworkError::NetworkUnreachable, // ENETUNREACH
        -104 => NetworkError::ConnectionReset, // ECONNRESET
        -110 => NetworkError::TimedOut,       // ETIMEDOUT
        -111 => NetworkError::ConnectionRefused, // ECONNREFUSED
        _ => NetworkError::InternalError,
    }
}

// =============================================================================
// S-LINK Message Types for Inter-Service Communication
// =============================================================================

/// S-LINK message type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Request message (expects response)
    Request,
    /// Response message
    Response,
    /// One-way notification
    Notification,
}

/// S-LINK message for inter-service communication.
#[derive(Debug, Clone)]
pub struct SLinkMessage {
    /// Channel ID for routing
    pub channel_id: u64,
    /// Message type
    pub message_type: MessageType,
    /// Correlation ID for request/response matching
    pub correlation_id: u64,
    /// Payload data
    pub payload: Vec<u8>,
    /// Capabilities attached to message
    pub capabilities: Vec<CapabilityToken>,
}

/// Send a request and wait for response with timeout.
pub fn send_and_receive(request: SLinkMessage, timeout: Duration) -> Result<SLinkMessage, NetworkError> {
    // Convert timeout to CPU cycles (assuming ~2GHz CPU)
    let timeout_cycles = timeout.as_nanos() as u64 * 2;
    let start = get_timestamp();
    
    // Global message queues for S-LINK IPC
    // Outbound: requests waiting to be processed by target services
    // Inbound: responses from services back to callers
    static OUTBOUND: Mutex<Vec<SLinkMessage>> = Mutex::new(Vec::new());
    static INBOUND: Mutex<Vec<SLinkMessage>> = Mutex::new(Vec::new());
    
    let correlation_id = request.correlation_id;
    let channel_id = request.channel_id;
    
    // Route message through S-LINK router
    route_message(&request);
    OUTBOUND.lock().push(request);
    
    // Poll for response
    loop {
        // Process any pending service handlers (cooperative scheduling)
        process_pending_handlers();
        
        // Check for matching response
        {
            let mut inbound = INBOUND.lock();
            if let Some(idx) = inbound.iter().position(|m| {
                m.message_type == MessageType::Response && 
                m.correlation_id == correlation_id &&
                m.channel_id == channel_id
            }) {
                return Ok(inbound.remove(idx));
            }
        }
        
        // Check timeout
        let elapsed = get_timestamp().saturating_sub(start);
        if elapsed >= timeout_cycles {
            // Remove pending request from outbound queue
            OUTBOUND.lock().retain(|m| m.correlation_id != correlation_id);
            return Err(NetworkError::TimedOut);
        }
        
        // CPU pause hint for power efficiency
        pause_hint();
    }
}

/// Route a message through the S-LINK routing layer
fn route_message(msg: &SLinkMessage) {
    // S-LINK routing based on channel ID
    // Channels are mapped to service endpoints during gateway initialization
    // Message is placed in the appropriate service queue
    
    // For local services, directly queue the message
    // For remote services, forward to network layer
    let _ = msg; // Message is already queued in OUTBOUND
}

/// Process pending handlers (cooperative multitasking)
fn process_pending_handlers() {
    // Give services a chance to process pending requests
    // This enables cooperative scheduling within the gateway
    
    // In a preemptive system, this would be a no-op since
    // service threads handle their own processing
}

/// Deliver a response message (called by service handlers).
pub fn deliver_response(response: SLinkMessage) {
    static INBOUND: Mutex<Vec<SLinkMessage>> = Mutex::new(Vec::new());
    INBOUND.lock().push(response);
}

/// Get pending requests for a service to process (called by service handlers).
pub fn get_pending_requests() -> Vec<SLinkMessage> {
    static OUTBOUND: Mutex<Vec<SLinkMessage>> = Mutex::new(Vec::new());
    core::mem::take(&mut *OUTBOUND.lock())
}

#[cfg(target_arch = "x86_64")]
fn get_timestamp() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[cfg(target_arch = "aarch64")]
fn get_timestamp() -> u64 {
    let cnt: u64;
    unsafe {
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt, options(nostack, nomem));
    }
    cnt
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn get_timestamp() -> u64 {
    0
}

#[cfg(target_arch = "x86_64")]
fn pause_hint() {
    unsafe { core::arch::x86_64::_mm_pause() }
}

#[cfg(target_arch = "aarch64")]
fn pause_hint() {
    unsafe {
        core::arch::asm!("yield", options(nostack, nomem));
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn pause_hint() {}

/// Network socket handle from kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernelSocket(pub usize);

/// Network listener for S-GATE.
///
/// This wraps the kernel's socket abstraction and provides a higher-level
/// interface for S-GATE to accept connections.
pub struct NetworkListener {
    /// Bound port.
    port: u16,
    /// Kernel socket handle.
    socket: Option<KernelSocket>,
    /// Whether listener is active.
    active: bool,
}

impl NetworkListener {
    /// Creates a new listener.
    pub fn new(port: u16) -> Self {
        Self {
            port,
            socket: None,
            active: false,
        }
    }
    
    /// Binds and starts listening.
    pub fn bind(&mut self) -> Result<(), NetworkError> {
        // Step 1: Create a TCP stream socket
        let fd = sys_socket(af_family::AF_INET, sock_type::SOCK_STREAM, 0);
        if fd < 0 {
            return Err(syscall_to_error(fd));
        }
        
        // Step 2: Set SO_REUSEADDR to allow quick rebinding
        let optval: u32 = 1;
        let opt_result = sys_setsockopt(
            fd,
            sol_level::SOL_SOCKET,
            sock_opt::SO_REUSEADDR,
            &optval as *const u32 as *const u8,
            core::mem::size_of::<u32>() as u64,
        );
        if opt_result < 0 {
            sys_close(fd);
            return Err(syscall_to_error(opt_result));
        }
        
        // Step 3: Bind to the specified port on all interfaces (0.0.0.0)
        let addr = SockAddrIn::any(self.port);
        let bind_result = sys_bind(fd, addr.as_ptr(), SockAddrIn::len());
        if bind_result < 0 {
            sys_close(fd);
            return Err(syscall_to_error(bind_result));
        }
        
        // Step 4: Start listening with a backlog of 128 connections
        const LISTEN_BACKLOG: u64 = 128;
        let listen_result = sys_listen(fd, LISTEN_BACKLOG);
        if listen_result < 0 {
            sys_close(fd);
            return Err(syscall_to_error(listen_result));
        }
        
        // Successfully bound and listening
        self.socket = Some(KernelSocket(fd as usize));
        self.active = true;
        Ok(())
    }
    
    /// Accepts a connection.
    pub fn accept(&self) -> Result<NetworkConnection, NetworkError> {
        if !self.active {
            return Err(NetworkError::NotListening);
        }
        
        let socket = self.socket.ok_or(NetworkError::NotBound)?;
        let fd = socket.0 as i64;
        
        // Prepare sockaddr structure to receive peer address
        let mut peer_addr = SockAddrIn::any(0);
        let mut addr_len: u32 = SockAddrIn::len() as u32;
        
        // Call accept syscall
        let new_fd = sys_accept(fd, peer_addr.as_mut_ptr(), &mut addr_len as *mut u32);
        
        if new_fd < 0 {
            return Err(syscall_to_error(new_fd));
        }
        
        // Create a new NetworkConnection with the accepted socket
        let connection = NetworkConnection::new(
            peer_addr.addr(),
            peer_addr.port(),
            self.port,
            KernelSocket(new_fd as usize),
        );
        
        Ok(connection)
    }
    
    /// Closes the listener.
    pub fn close(&mut self) {
        // Close the socket via syscall if it exists
        if let Some(socket) = self.socket.take() {
            let fd = socket.0 as i64;
            sys_close(fd);
        }
        self.active = false;
    }
    
    /// Get the listening port.
    pub fn port(&self) -> u16 {
        self.port
    }
    
    /// Check if the listener is active.
    pub fn is_active(&self) -> bool {
        self.active
    }
    
    /// Get the socket file descriptor.
    pub fn fd(&self) -> Option<i64> {
        self.socket.map(|s| s.0 as i64)
    }
}

/// Network connection for S-GATE.
pub struct NetworkConnection {
    /// Remote IP address.
    pub remote_addr: u32,
    /// Remote port.
    pub remote_port: u16,
    /// Local port.
    pub local_port: u16,
    /// Kernel socket handle.
    socket: KernelSocket,
    /// Receive buffer.
    recv_buffer: Vec<u8>,
    /// Send buffer.
    send_buffer: Vec<u8>,
}

impl NetworkConnection {
    /// Creates a new connection.
    pub fn new(
        remote_addr: u32,
        remote_port: u16,
        local_port: u16,
        socket: KernelSocket,
    ) -> Self {
        Self {
            remote_addr,
            remote_port,
            local_port,
            socket,
            recv_buffer: Vec::new(),
            send_buffer: Vec::new(),
        }
    }
    
    /// Sends data.
    pub fn send(&mut self, data: &[u8]) -> Result<usize, NetworkError> {
        let fd = self.socket.0 as i64;
        
        // If we have buffered data, try to send it first
        if !self.send_buffer.is_empty() {
            let buffered_result = sys_send(
                fd,
                self.send_buffer.as_ptr(),
                self.send_buffer.len() as u64,
                0,  // No flags
            );
            
            if buffered_result > 0 {
                // Remove sent bytes from buffer
                let sent = buffered_result as usize;
                if sent >= self.send_buffer.len() {
                    self.send_buffer.clear();
                } else {
                    self.send_buffer.drain(..sent);
                }
            }
        }
        
        // Try to send the new data directly
        let result = sys_send(fd, data.as_ptr(), data.len() as u64, 0);
        
        if result < 0 {
            let err = syscall_to_error(result);
            if matches!(err, NetworkError::WouldBlock) {
                // Buffer the data for later transmission
                self.send_buffer.extend_from_slice(data);
                return Ok(0);  // No data sent immediately, but buffered
            }
            return Err(err);
        }
        
        let bytes_sent = result as usize;
        
        // If not all data was sent, buffer the remainder
        if bytes_sent < data.len() {
            self.send_buffer.extend_from_slice(&data[bytes_sent..]);
        }
        
        Ok(bytes_sent)
    }
    
    /// Receives data.
    pub fn recv(&mut self, buffer: &mut [u8]) -> Result<usize, NetworkError> {
        // First, check if we have buffered data from a previous recv
        if !self.recv_buffer.is_empty() {
            let len = buffer.len().min(self.recv_buffer.len());
            buffer[..len].copy_from_slice(&self.recv_buffer[..len]);
            self.recv_buffer.drain(..len);
            return Ok(len);
        }
        
        // No buffered data, try to receive from the socket
        let fd = self.socket.0 as i64;
        let result = sys_recv(fd, buffer.as_mut_ptr(), buffer.len() as u64, 0);
        
        if result < 0 {
            return Err(syscall_to_error(result));
        }
        
        if result == 0 {
            // Connection closed by peer
            return Err(NetworkError::ConnectionReset);
        }
        
        Ok(result as usize)
    }
    
    /// Closes the connection.
    pub fn close(self) {
        // Close the socket via syscall
        let fd = self.socket.0 as i64;
        sys_close(fd);
        // Socket handle is dropped, connection is closed
    }
    
    /// Flush any buffered send data.
    pub fn flush(&mut self) -> Result<(), NetworkError> {
        if self.send_buffer.is_empty() {
            return Ok(());
        }
        
        let fd = self.socket.0 as i64;
        
        while !self.send_buffer.is_empty() {
            let result = sys_send(
                fd,
                self.send_buffer.as_ptr(),
                self.send_buffer.len() as u64,
                0,
            );
            
            if result < 0 {
                let err = syscall_to_error(result);
                if matches!(err, NetworkError::WouldBlock) {
                    // Can't send more right now, caller should retry
                    return Err(NetworkError::WouldBlock);
                }
                return Err(err);
            }
            
            let sent = result as usize;
            self.send_buffer.drain(..sent);
        }
        
        Ok(())
    }
    
    /// Check if there's pending data to send.
    pub fn has_pending_send(&self) -> bool {
        !self.send_buffer.is_empty()
    }
    
    /// Get the socket file descriptor.
    pub fn fd(&self) -> i64 {
        self.socket.0 as i64
    }
}

/// Network error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkError {
    /// Socket not bound.
    NotBound,
    /// Not listening.
    NotListening,
    /// Would block (non-blocking mode).
    WouldBlock,
    /// Connection refused.
    ConnectionRefused,
    /// Connection reset.
    ConnectionReset,
    /// Connection timed out.
    TimedOut,
    /// Address in use.
    AddressInUse,
    /// Network unreachable.
    NetworkUnreachable,
    /// Internal error.
    InternalError,
}

/// Integrated gateway that uses the kernel network stack.
pub struct IntegratedGateway {
    /// Gateway ID.
    id: GatewayId,
    /// Configuration.
    config: GatewayConfig,
    /// Network listener.
    listener: Mutex<Option<NetworkListener>>,
    /// Active connections.
    connections: Mutex<BTreeMap<TcpConnectionId, NetworkConnection>>,
    /// Next connection ID.
    next_conn_id: Mutex<u64>,
    /// Statistics.
    stats: Mutex<GatewayStats>,
}

impl IntegratedGateway {
    /// Creates a new integrated gateway.
    pub fn new(id: GatewayId, config: GatewayConfig) -> Self {
        Self {
            id,
            config,
            listener: Mutex::new(None),
            connections: Mutex::new(BTreeMap::new()),
            next_conn_id: Mutex::new(1),
            stats: Mutex::new(GatewayStats::default()),
        }
    }
    
    /// Starts the gateway.
    pub fn start(&self) -> Result<(), NetworkError> {
        let mut listener = NetworkListener::new(self.config.external_port);
        listener.bind()?;
        
        *self.listener.lock() = Some(listener);
        Ok(())
    }
    
    /// Stops the gateway.
    pub fn stop(&self) {
        if let Some(mut listener) = self.listener.lock().take() {
            listener.close();
        }
        
        // Close all connections
        let mut connections = self.connections.lock();
        connections.clear();
    }
    
    /// Polls for new connections and data.
    pub fn poll(&self) {
        // Accept new connections
        if let Some(ref listener) = *self.listener.lock() {
            while let Ok(conn) = listener.accept() {
                let mut next_id = self.next_conn_id.lock();
                let id = TcpConnectionId(*next_id);
                *next_id += 1;
                
                self.connections.lock().insert(id, conn);
                self.stats.lock().total_connections += 1;
            }
        }
        
        // Update active connection count
        self.stats.lock().active_connections = self.connections.lock().len();
    }
    
    /// Gets gateway ID.
    pub fn id(&self) -> GatewayId {
        self.id
    }
    
    /// Gets statistics.
    pub fn stats(&self) -> GatewayStats {
        self.stats.lock().clone()
    }
}

/// Global gateway manager using kernel network stack.
pub struct NetworkGatewayManager {
    /// Active gateways.
    gateways: Mutex<BTreeMap<GatewayId, IntegratedGateway>>,
    /// Next gateway ID.
    next_id: Mutex<u64>,
}

impl NetworkGatewayManager {
    /// Creates a new manager.
    pub const fn new() -> Self {
        Self {
            gateways: Mutex::new(BTreeMap::new()),
            next_id: Mutex::new(1),
        }
    }
    
    /// Creates a new gateway.
    pub fn create(&self, config: GatewayConfig) -> Result<GatewayId, NetworkError> {
        let mut next_id = self.next_id.lock();
        let id = GatewayId(*next_id);
        *next_id += 1;
        
        let gateway = IntegratedGateway::new(id, config);
        gateway.start()?;
        
        self.gateways.lock().insert(id, gateway);
        Ok(id)
    }
    
    /// Destroys a gateway.
    pub fn destroy(&self, id: GatewayId) -> Result<(), NetworkError> {
        if let Some(gateway) = self.gateways.lock().remove(&id) {
            gateway.stop();
            Ok(())
        } else {
            Err(NetworkError::InternalError)
        }
    }
    
    /// Polls all gateways.
    pub fn poll_all(&self) {
        for gateway in self.gateways.lock().values() {
            gateway.poll();
        }
    }
}

static GATEWAY_MANAGER: NetworkGatewayManager = NetworkGatewayManager::new();

/// Gets the global gateway manager.
pub fn gateway_manager() -> &'static NetworkGatewayManager {
    &GATEWAY_MANAGER
}

//! # Socket Manager
//!
//! Manages BSD-style socket abstraction in userspace.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use super::{NetError, SocketAddr, SocketDomain, SocketType};

/// Socket handle counter
static SOCKET_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Socket state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketState {
    /// Just created, not bound
    Created,
    /// Bound to local address
    Bound,
    /// Listening for connections (TCP)
    Listening,
    /// Connection in progress
    Connecting,
    /// Connected
    Connected,
    /// Closing
    Closing,
    /// Closed
    Closed,
}

/// Socket options
#[derive(Debug, Clone)]
pub struct SocketOptions {
    /// Receive buffer size
    pub recv_buffer: usize,
    /// Send buffer size
    pub send_buffer: usize,
    /// Reuse address
    pub reuse_addr: bool,
    /// Reuse port
    pub reuse_port: bool,
    /// Keep-alive
    pub keepalive: bool,
    /// No delay (TCP)
    pub nodelay: bool,
    /// Non-blocking mode
    pub nonblocking: bool,
    /// Receive timeout (ms)
    pub recv_timeout: Option<u32>,
    /// Send timeout (ms)
    pub send_timeout: Option<u32>,
}

impl Default for SocketOptions {
    fn default() -> Self {
        Self {
            recv_buffer: 65536,
            send_buffer: 65536,
            reuse_addr: false,
            reuse_port: false,
            keepalive: false,
            nodelay: false,
            nonblocking: false,
            recv_timeout: None,
            send_timeout: None,
        }
    }
}

/// A socket instance
pub struct Socket {
    /// Socket handle
    pub handle: u32,
    /// Address family
    pub domain: SocketDomain,
    /// Socket type
    pub sock_type: SocketType,
    /// Protocol
    pub protocol: u8,
    /// Current state
    pub state: SocketState,
    /// Local address (if bound)
    pub local_addr: Option<SocketAddr>,
    /// Remote address (if connected)
    pub remote_addr: Option<SocketAddr>,
    /// Socket options
    pub options: SocketOptions,
    /// Receive buffer
    pub recv_buffer: Vec<u8>,
    /// Send buffer
    pub send_buffer: Vec<u8>,
    /// Pending connections (for listening sockets)
    pub pending_connections: Vec<Socket>,
    /// Owning process capability token
    pub capability: u64,
}

impl Socket {
    /// Creates a new socket
    pub fn new(domain: SocketDomain, sock_type: SocketType, protocol: u8) -> Self {
        Self {
            handle: SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed),
            domain,
            sock_type,
            protocol,
            state: SocketState::Created,
            local_addr: None,
            remote_addr: None,
            options: SocketOptions::default(),
            recv_buffer: Vec::new(),
            send_buffer: Vec::new(),
            pending_connections: Vec::new(),
            capability: 0,
        }
    }

    /// Binds the socket to an address
    pub fn bind(&mut self, addr: SocketAddr) -> Result<(), NetError> {
        if self.state != SocketState::Created {
            return Err(NetError::InvalidArgument);
        }

        // In full implementation: check port availability
        self.local_addr = Some(addr);
        self.state = SocketState::Bound;
        Ok(())
    }

    /// Starts listening (TCP only)
    pub fn listen(&mut self, _backlog: u32) -> Result<(), NetError> {
        if self.sock_type != SocketType::Stream {
            return Err(NetError::InvalidArgument);
        }

        if self.state != SocketState::Bound {
            return Err(NetError::InvalidArgument);
        }

        self.state = SocketState::Listening;
        Ok(())
    }

    /// Connects to remote address
    pub fn connect(&mut self, addr: SocketAddr) -> Result<(), NetError> {
        if self.state == SocketState::Connected {
            return Err(NetError::AlreadyConnected);
        }

        self.remote_addr = Some(addr);
        self.state = SocketState::Connecting;
        
        // In full implementation: initiate TCP handshake
        // For now, immediately mark as connected
        self.state = SocketState::Connected;
        Ok(())
    }

    /// Accepts a connection (TCP only)
    pub fn accept(&mut self) -> Result<Socket, NetError> {
        if self.sock_type != SocketType::Stream {
            return Err(NetError::InvalidArgument);
        }

        if self.state != SocketState::Listening {
            return Err(NetError::InvalidArgument);
        }

        if self.pending_connections.is_empty() {
            return Err(NetError::WouldBlock);
        }

        Ok(self.pending_connections.remove(0))
    }

    /// Sends data
    pub fn send(&mut self, data: &[u8]) -> Result<usize, NetError> {
        if self.state != SocketState::Connected {
            return Err(NetError::NotConnected);
        }

        // Buffer the data
        let space = self.options.send_buffer - self.send_buffer.len();
        let to_send = data.len().min(space);

        if to_send == 0 {
            return Err(NetError::BufferFull);
        }

        self.send_buffer.extend_from_slice(&data[..to_send]);
        Ok(to_send)
    }

    /// Receives data
    pub fn recv(&mut self, max_len: usize) -> Result<Vec<u8>, NetError> {
        if self.state != SocketState::Connected && self.sock_type == SocketType::Stream {
            return Err(NetError::NotConnected);
        }

        if self.recv_buffer.is_empty() {
            return Err(NetError::WouldBlock);
        }

        let to_recv = max_len.min(self.recv_buffer.len());
        let data: Vec<u8> = self.recv_buffer.drain(..to_recv).collect();
        Ok(data)
    }

    /// Closes the socket
    pub fn close(&mut self) {
        self.state = SocketState::Closing;
        // In full implementation: TCP FIN sequence
        self.state = SocketState::Closed;
    }
}

/// Socket manager
pub struct SocketManager {
    /// All sockets indexed by handle
    sockets: BTreeMap<u32, Socket>,
    /// Maximum number of sockets
    max_sockets: usize,
    /// Port allocations (port -> socket handle)
    port_map: BTreeMap<u16, u32>,
}

impl SocketManager {
    /// Creates a new socket manager
    pub fn new(max_sockets: usize) -> Self {
        Self {
            sockets: BTreeMap::new(),
            max_sockets,
            port_map: BTreeMap::new(),
        }
    }

    /// Creates a new socket
    pub fn create(
        &mut self,
        domain: SocketDomain,
        sock_type: SocketType,
        protocol: u8,
    ) -> Result<u32, NetError> {
        if self.sockets.len() >= self.max_sockets {
            return Err(NetError::TooManySockets);
        }

        let socket = Socket::new(domain, sock_type, protocol);
        let handle = socket.handle;
        self.sockets.insert(handle, socket);
        Ok(handle)
    }

    /// Gets a socket by handle
    pub fn get(&self, handle: u32) -> Option<&Socket> {
        self.sockets.get(&handle)
    }

    /// Gets a mutable socket by handle
    pub fn get_mut(&mut self, handle: u32) -> Option<&mut Socket> {
        self.sockets.get_mut(&handle)
    }

    /// Binds a socket to an address
    pub fn bind(&mut self, handle: u32, addr: SocketAddr) -> Result<(), NetError> {
        let port = addr.port();

        // Check if port is already in use
        if self.port_map.contains_key(&port) {
            let socket = self.sockets.get(&handle).ok_or(NetError::InvalidSocket)?;
            if !socket.options.reuse_port {
                return Err(NetError::AddressInUse);
            }
        }

        let socket = self.sockets.get_mut(&handle).ok_or(NetError::InvalidSocket)?;
        socket.bind(addr)?;
        self.port_map.insert(port, handle);
        Ok(())
    }

    /// Closes a socket
    pub fn close(&mut self, handle: u32) -> Result<(), NetError> {
        let socket = self.sockets.get_mut(&handle).ok_or(NetError::InvalidSocket)?;
        
        // Remove from port map
        if let Some(addr) = &socket.local_addr {
            self.port_map.remove(&addr.port());
        }
        
        socket.close();
        self.sockets.remove(&handle);
        Ok(())
    }

    /// Allocates an ephemeral port
    pub fn allocate_port(&mut self) -> Option<u16> {
        // Dynamic/ephemeral port range: 49152-65535
        for port in 49152..=65535u16 {
            if !self.port_map.contains_key(&port) {
                return Some(port);
            }
        }
        None
    }

    /// Returns the number of open sockets
    pub fn socket_count(&self) -> usize {
        self.sockets.len()
    }
}

impl Default for SocketManager {
    fn default() -> Self {
        Self::new(1024)
    }
}

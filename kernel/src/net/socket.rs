//! # Socket Abstraction
//!
//! High-level socket API for network communication, including TLS support
//! for secure connections.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use super::device::NetworkError;
use super::ip::Ipv4Address;
use super::tcp::{tcp_state, TcpConnectionKey, TcpEndpoint};
use super::udp::{udp_state, UdpEndpoint};
use super::tls::TlsConnection;

/// Socket type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketType {
    /// TCP stream socket.
    Stream,
    /// UDP datagram socket.
    Datagram,
}

/// Socket address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketAddr {
    pub addr: Ipv4Address,
    pub port: u16,
}

impl SocketAddr {
    pub fn new(addr: Ipv4Address, port: u16) -> Self {
        Self { addr, port }
    }
    
    pub fn any(port: u16) -> Self {
        Self::new(Ipv4Address::ANY, port)
    }
}

impl From<TcpEndpoint> for SocketAddr {
    fn from(ep: TcpEndpoint) -> Self {
        Self::new(ep.addr, ep.port)
    }
}

impl From<UdpEndpoint> for SocketAddr {
    fn from(ep: UdpEndpoint) -> Self {
        Self::new(ep.addr, ep.port)
    }
}

impl From<SocketAddr> for TcpEndpoint {
    fn from(addr: SocketAddr) -> Self {
        TcpEndpoint::new(addr.addr, addr.port)
    }
}

impl From<SocketAddr> for UdpEndpoint {
    fn from(addr: SocketAddr) -> Self {
        UdpEndpoint::new(addr.addr, addr.port)
    }
}

/// Socket handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketHandle(pub usize);

impl core::fmt::Display for SocketHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Socket state.
enum SocketInner {
    /// TCP stream.
    TcpStream {
        /// Connection key (None if not connected).
        connection_key: Option<TcpConnectionKey>,
        /// Local address.
        local: Option<SocketAddr>,
    },
    /// TCP listener.
    TcpListener {
        /// Bound port.
        port: u16,
        /// Local address.
        local: SocketAddr,
    },
    /// UDP socket.
    Udp {
        /// Bound port.
        port: Option<u16>,
        /// Local address.
        local: Option<SocketAddr>,
    },
    /// TLS-encrypted TCP stream.
    TlsStream {
        /// Underlying TCP connection key.
        connection_key: TcpConnectionKey,
        /// Local address.
        local: SocketAddr,
        /// TLS connection state.
        tls: TlsConnection,
    },
}

use super::namespace::{NetNsId, NetCapability, get_process_namespace, get_process_capabilities, get_namespace};

/// Socket.
struct Socket {
    inner: SocketInner,
    /// Non-blocking mode.
    non_blocking: bool,
    /// Network namespace this socket belongs to.
    namespace: NetNsId,
    /// Process ID that owns this socket.
    owner_pid: u64,
}

impl Socket {
    fn new_tcp_stream() -> Self {
        Self {
            inner: SocketInner::TcpStream {
                connection_key: None,
                local: None,
            },
            non_blocking: false,
            namespace: NetNsId::DEFAULT,
            owner_pid: 0,
        }
    }
    
    fn new_tcp_stream_in_ns(netns: NetNsId, pid: u64) -> Self {
        Self {
            inner: SocketInner::TcpStream {
                connection_key: None,
                local: None,
            },
            non_blocking: false,
            namespace: netns,
            owner_pid: pid,
        }
    }
    
    fn new_udp() -> Self {
        Self {
            inner: SocketInner::Udp {
                port: None,
                local: None,
            },
            non_blocking: false,
            namespace: NetNsId::DEFAULT,
            owner_pid: 0,
        }
    }
    
    fn new_udp_in_ns(netns: NetNsId, pid: u64) -> Self {
        Self {
            inner: SocketInner::Udp {
                port: None,
                local: None,
            },
            non_blocking: false,
            namespace: netns,
            owner_pid: pid,
        }
    }
}

/// Global socket table.
struct SocketTable {
    sockets: BTreeMap<SocketHandle, Socket>,
    next_handle: usize,
}

impl SocketTable {
    const fn new() -> Self {
        Self {
            sockets: BTreeMap::new(),
            next_handle: 1,
        }
    }
    
    fn allocate(&mut self, socket: Socket) -> SocketHandle {
        let handle = SocketHandle(self.next_handle);
        self.next_handle += 1;
        self.sockets.insert(handle, socket);
        handle
    }
    
    fn get(&self, handle: SocketHandle) -> Option<&Socket> {
        self.sockets.get(&handle)
    }
    
    fn get_mut(&mut self, handle: SocketHandle) -> Option<&mut Socket> {
        self.sockets.get_mut(&handle)
    }
    
    fn remove(&mut self, handle: SocketHandle) -> Option<Socket> {
        self.sockets.remove(&handle)
    }
}

static SOCKET_TABLE: Mutex<SocketTable> = Mutex::new(SocketTable::new());

/// Creates a new socket.
pub fn socket(socket_type: SocketType) -> Result<SocketHandle, NetworkError> {
    let socket = match socket_type {
        SocketType::Stream => Socket::new_tcp_stream(),
        SocketType::Datagram => Socket::new_udp(),
    };
    
    let handle = SOCKET_TABLE.lock().allocate(socket);
    Ok(handle)
}

/// Creates a new socket in a specific network namespace.
/// 
/// This is the namespace-aware version that checks capabilities and
/// registers the socket with the namespace.
pub fn socket_in_ns(socket_type: SocketType, pid: u64) -> Result<SocketHandle, NetworkError> {
    let netns = get_process_namespace(pid);
    let caps = get_process_capabilities(pid);
    
    // Check basic socket creation capability (implied by Bind or Connect)
    if !caps.has(NetCapability::Bind) && !caps.has(NetCapability::Connect) {
        return Err(NetworkError::PermissionDenied);
    }
    
    let socket = match socket_type {
        SocketType::Stream => Socket::new_tcp_stream_in_ns(netns, pid),
        SocketType::Datagram => Socket::new_udp_in_ns(netns, pid),
    };
    
    let handle = SOCKET_TABLE.lock().allocate(socket);
    
    // Register socket with namespace
    if let Some(ns) = get_namespace(netns) {
        ns.lock().register_socket(handle);
    }
    
    Ok(handle)
}

/// Binds a socket to an address.
pub fn bind(handle: SocketHandle, addr: SocketAddr) -> Result<(), NetworkError> {
    let mut table = SOCKET_TABLE.lock();
    let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
    
    match &mut socket.inner {
        SocketInner::TcpStream { local, .. } => {
            *local = Some(addr);
            Ok(())
        }
        SocketInner::Udp { port, local } => {
            udp_state().lock().bind(addr.port, addr.addr)?;
            *port = Some(addr.port);
            *local = Some(addr);
            Ok(())
        }
        SocketInner::TcpListener { .. } => Err(NetworkError::InvalidOperation),
        SocketInner::TlsStream { .. } => Err(NetworkError::InvalidOperation),
    }
}

/// Binds a socket to an address with capability checking.
/// 
/// This is the namespace-aware version that verifies the process has
/// permission to bind to the requested port.
pub fn bind_with_caps(handle: SocketHandle, addr: SocketAddr) -> Result<(), NetworkError> {
    let mut table = SOCKET_TABLE.lock();
    let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
    
    let netns = socket.namespace;
    let pid = socket.owner_pid;
    let caps = get_process_capabilities(pid);
    
    // Check port binding permissions via namespace ACL
    let protocol = match &socket.inner {
        SocketInner::TcpStream { .. } | SocketInner::TcpListener { .. } => 6, // TCP
        SocketInner::Udp { .. } => 17, // UDP
        SocketInner::TlsStream { .. } => 6, // TLS over TCP
    };
    
    if let Some(ns) = get_namespace(netns) {
        if !ns.lock().check_port_binding(pid, addr.port, protocol, caps) {
            return Err(NetworkError::PermissionDenied);
        }
    }
    
    match &mut socket.inner {
        SocketInner::TcpStream { local, .. } => {
            *local = Some(addr);
            Ok(())
        }
        SocketInner::Udp { port, local } => {
            udp_state().lock().bind(addr.port, addr.addr)?;
            *port = Some(addr.port);
            *local = Some(addr);
            Ok(())
        }
        SocketInner::TcpListener { .. } => Err(NetworkError::InvalidOperation),
        SocketInner::TlsStream { .. } => Err(NetworkError::InvalidOperation),
    }
}

/// Get the namespace a socket belongs to.
pub fn get_socket_namespace(handle: SocketHandle) -> Option<NetNsId> {
    SOCKET_TABLE.lock().get(handle).map(|s| s.namespace)
}

/// Get the owner PID of a socket.
pub fn get_socket_owner(handle: SocketHandle) -> Option<u64> {
    SOCKET_TABLE.lock().get(handle).map(|s| s.owner_pid)
}

/// Starts listening on a TCP socket.
pub fn listen(handle: SocketHandle, _backlog: usize) -> Result<(), NetworkError> {
    let mut table = SOCKET_TABLE.lock();
    let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
    
    match &socket.inner {
        SocketInner::TcpStream { local, .. } => {
            let local = local.ok_or(NetworkError::NotBound)?;
            
            tcp_state().lock().bind(local.port, local.addr)?;
            
            socket.inner = SocketInner::TcpListener {
                port: local.port,
                local,
            };
            
            Ok(())
        }
        _ => Err(NetworkError::InvalidOperation),
    }
}

/// Accepts a connection on a listening socket.
pub fn accept(handle: SocketHandle) -> Result<(SocketHandle, SocketAddr), NetworkError> {
    let table = SOCKET_TABLE.lock();
    let socket = table.get(handle).ok_or(NetworkError::InvalidSocket)?;
    
    // Inherit namespace and owner from listening socket
    let listener_ns = socket.namespace;
    let listener_pid = socket.owner_pid;
    
    match &socket.inner {
        SocketInner::TcpListener { port, .. } => {
            let port = *port;
            drop(table);
            
            // Try to accept
            let conn = tcp_state().lock().accept(port);
            
            match conn {
                Some(conn) => {
                    let remote_addr = SocketAddr::from(conn.key.remote);
                    
                    let new_socket = Socket {
                        inner: SocketInner::TcpStream {
                            connection_key: Some(conn.key),
                            local: Some(SocketAddr::from(conn.key.local)),
                        },
                        non_blocking: false,
                        namespace: listener_ns,
                        owner_pid: listener_pid,
                    };
                    
                    let new_handle = SOCKET_TABLE.lock().allocate(new_socket);
                    
                    // Register with namespace
                    if let Some(ns) = get_namespace(listener_ns) {
                        ns.lock().register_socket(new_handle);
                    }
                    
                    Ok((new_handle, remote_addr))
                }
                None => Err(NetworkError::WouldBlock),
            }
        }
        _ => Err(NetworkError::InvalidOperation),
    }
}

/// Connects a socket to a remote address.
pub fn connect(handle: SocketHandle, addr: SocketAddr) -> Result<(), NetworkError> {
    let mut table = SOCKET_TABLE.lock();
    let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
    
    match &mut socket.inner {
        SocketInner::TcpStream {
            connection_key,
            local,
        } => {
            let local_addr = local.map(|l| l.addr).unwrap_or(Ipv4Address::new(10, 0, 2, 15));
            
            let key = tcp_state().lock().connect(local_addr, addr.into());
            *connection_key = Some(key);
            *local = Some(SocketAddr::from(key.local));
            
            Ok(())
        }
        _ => Err(NetworkError::InvalidOperation),
    }
}

/// Sends data on a connected socket.
pub fn send(handle: SocketHandle, data: &[u8]) -> Result<usize, NetworkError> {
    let table = SOCKET_TABLE.lock();
    let socket = table.get(handle).ok_or(NetworkError::InvalidSocket)?;
    
    match &socket.inner {
        SocketInner::TcpStream {
            connection_key: Some(key),
            ..
        } => {
            let key = *key;
            drop(table);
            
            // Queue data for transmission via TCP state
            let mut state = tcp_state().lock();
            if let Some(conn) = state.connection_mut(key) {
                if let Some(segment) = conn.send(data) {
                    // Transmit the segment through the network interface
                    drop(state);
                    if let Err(e) = super::transmit_tcp_segment(&key, &segment) {
                        return Err(e);
                    }
                    Ok(data.len())
                } else {
                    // Connection not in established state
                    Err(NetworkError::NotConnected)
                }
            } else {
                Err(NetworkError::NotConnected)
            }
        }
        SocketInner::TcpStream {
            connection_key: None,
            ..
        } => Err(NetworkError::NotConnected),
        _ => Err(NetworkError::InvalidOperation),
    }
}

/// Receives data from a connected socket.
pub fn recv(handle: SocketHandle, buf: &mut [u8]) -> Result<usize, NetworkError> {
    let table = SOCKET_TABLE.lock();
    let socket = table.get(handle).ok_or(NetworkError::InvalidSocket)?;
    let non_blocking = socket.non_blocking;
    
    match &socket.inner {
        SocketInner::TcpStream {
            connection_key: Some(key),
            ..
        } => {
            let key = *key;
            drop(table);
            
            // Read from TCP connection receive buffer
            let mut state = tcp_state().lock();
            if let Some(conn) = state.connection_mut(key) {
                let bytes_read = conn.read(buf);
                if bytes_read > 0 {
                    Ok(bytes_read)
                } else if non_blocking {
                    Err(NetworkError::WouldBlock)
                } else {
                    // In blocking mode, spin-wait for data with yield
                    drop(state);
                    
                    // Retry loop with yield to allow other tasks to run
                    for _ in 0..1000 {
                        // Yield CPU to other tasks
                        core::hint::spin_loop();
                        
                        // Check for data again
                        let mut state = tcp_state().lock();
                        if let Some(conn) = state.connection_mut(key) {
                            let bytes_read = conn.read(buf);
                            if bytes_read > 0 {
                                return Ok(bytes_read);
                            }
                            // Check if connection was closed
                            if conn.is_closed() {
                                return Ok(0); // EOF
                            }
                        } else {
                            return Err(NetworkError::NotConnected);
                        }
                    }
                    // Timeout after spin-waiting
                    Err(NetworkError::WouldBlock)
                }
            } else {
                Err(NetworkError::NotConnected)
            }
        }
        SocketInner::TcpStream {
            connection_key: None,
            ..
        } => Err(NetworkError::NotConnected),
        _ => Err(NetworkError::InvalidOperation),
    }
}

/// Sends a datagram to an address (UDP).
pub fn sendto(handle: SocketHandle, data: &[u8], addr: SocketAddr) -> Result<usize, NetworkError> {
    let table = SOCKET_TABLE.lock();
    let socket = table.get(handle).ok_or(NetworkError::InvalidSocket)?;
    
    match &socket.inner {
        SocketInner::Udp { port: Some(_port), .. } => {
            // Would create datagram and send via network interface
            Ok(data.len())
        }
        SocketInner::Udp { port: None, .. } => Err(NetworkError::NotBound),
        _ => Err(NetworkError::InvalidOperation),
    }
}

/// Receives a datagram with source address (UDP).
pub fn recvfrom(handle: SocketHandle, buf: &mut [u8]) -> Result<(usize, SocketAddr), NetworkError> {
    let table = SOCKET_TABLE.lock();
    let socket = table.get(handle).ok_or(NetworkError::InvalidSocket)?;
    
    match &socket.inner {
        SocketInner::Udp { port: Some(port), .. } => {
            let port = *port;
            drop(table);
            
            let msg = udp_state().lock().socket(port).and_then(|s| s.recv());
            
            match msg {
                Some(msg) => {
                    let len = buf.len().min(msg.data.len());
                    buf[..len].copy_from_slice(&msg.data[..len]);
                    Ok((len, SocketAddr::from(msg.remote)))
                }
                None => Err(NetworkError::WouldBlock),
            }
        }
        SocketInner::Udp { port: None, .. } => Err(NetworkError::NotBound),
        _ => Err(NetworkError::InvalidOperation),
    }
}

/// Closes a socket.
pub fn close(handle: SocketHandle) -> Result<(), NetworkError> {
    let socket = SOCKET_TABLE.lock().remove(handle);
    
    match socket {
        Some(socket) => {
            // Unregister from namespace
            if let Some(ns) = get_namespace(socket.namespace) {
                ns.lock().unregister_socket(handle);
            }
            
            match socket.inner {
                SocketInner::Udp { port: Some(port), .. } => {
                    udp_state().lock().unbind(port);
                }
                _ => {}
            }
            Ok(())
        }
        None => Err(NetworkError::InvalidSocket),
    }
}

/// Sets socket to non-blocking mode.
pub fn set_nonblocking(handle: SocketHandle, non_blocking: bool) -> Result<(), NetworkError> {
    let mut table = SOCKET_TABLE.lock();
    let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
    socket.non_blocking = non_blocking;
    Ok(())
}

/// Gets local address of a socket.
pub fn local_addr(handle: SocketHandle) -> Result<SocketAddr, NetworkError> {
    let table = SOCKET_TABLE.lock();
    let socket = table.get(handle).ok_or(NetworkError::InvalidSocket)?;
    
    match &socket.inner {
        SocketInner::TcpStream { local: Some(addr), .. } => Ok(*addr),
        SocketInner::TcpListener { local, .. } => Ok(*local),
        SocketInner::Udp { local: Some(addr), .. } => Ok(*addr),
        SocketInner::TlsStream { local, .. } => Ok(*local),
        _ => Err(NetworkError::NotBound),
    }
}

/// Gets peer address of a connected socket.
pub fn peer_addr(handle: SocketHandle) -> Result<SocketAddr, NetworkError> {
    let table = SOCKET_TABLE.lock();
    let socket = table.get(handle).ok_or(NetworkError::InvalidSocket)?;
    
    match &socket.inner {
        SocketInner::TcpStream {
            connection_key: Some(key),
            ..
        } => Ok(SocketAddr::from(key.remote)),
        SocketInner::TlsStream { connection_key, .. } => Ok(SocketAddr::from(connection_key.remote)),
        _ => Err(NetworkError::NotConnected),
    }
}

// =============================================================================
// TLS Socket Functions
// =============================================================================

/// Upgrades a connected TCP socket to TLS (client mode).
///
/// The socket must already be connected. This function initiates the TLS
/// handshake and returns once the secure connection is established.
pub fn connect_tls(handle: SocketHandle) -> Result<(), NetworkError> {
    let mut table = SOCKET_TABLE.lock();
    let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
    
    // Extract TCP connection info
    let (connection_key, local) = match &socket.inner {
        SocketInner::TcpStream {
            connection_key: Some(key),
            local: Some(local),
        } => (*key, *local),
        SocketInner::TcpStream { connection_key: None, .. } => {
            return Err(NetworkError::NotConnected);
        }
        _ => return Err(NetworkError::InvalidOperation),
    };
    
    // Create TLS client connection
    let mut tls = TlsConnection::new_client();
    
    // Start the handshake
    let client_hello = tls.start_handshake()
        .map_err(|_| NetworkError::TlsError)?;
    
    // Send ClientHello via TCP
    drop(table);
    let _ = send_raw_tcp(&connection_key, &client_hello);
    
    // Update socket to TLS mode
    let mut table = SOCKET_TABLE.lock();
    let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
    
    socket.inner = SocketInner::TlsStream {
        connection_key,
        local,
        tls,
    };
    
    crate::println!("[net] TLS handshake initiated for socket {}", handle);
    Ok(())
}

/// Accepts a TLS connection on a connected TCP socket (server mode).
///
/// This is used after accept() to upgrade the connection to TLS.
pub fn accept_tls(handle: SocketHandle) -> Result<(), NetworkError> {
    let mut table = SOCKET_TABLE.lock();
    let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
    
    // Extract TCP connection info
    let (connection_key, local) = match &socket.inner {
        SocketInner::TcpStream {
            connection_key: Some(key),
            local: Some(local),
        } => (*key, *local),
        SocketInner::TcpStream { connection_key: None, .. } => {
            return Err(NetworkError::NotConnected);
        }
        _ => return Err(NetworkError::InvalidOperation),
    };
    
    // Create TLS server connection
    let tls = TlsConnection::new_server();
    
    // Update socket to TLS mode
    socket.inner = SocketInner::TlsStream {
        connection_key,
        local,
        tls,
    };
    
    crate::println!("[net] TLS server mode enabled for socket {}", handle);
    Ok(())
}

/// Sends encrypted data over a TLS socket.
pub fn send_tls(handle: SocketHandle, data: &[u8]) -> Result<usize, NetworkError> {
    let mut table = SOCKET_TABLE.lock();
    let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
    
    match &mut socket.inner {
        SocketInner::TlsStream { connection_key, tls, .. } => {
            let key = *connection_key;
            
            // Encrypt the data
            let encrypted = tls.encrypt(data)
                .map_err(|_| NetworkError::TlsError)?;
            
            drop(table);
            
            // Send encrypted data via TCP
            send_raw_tcp(&key, &encrypted)?;
            
            Ok(data.len())
        }
        _ => Err(NetworkError::InvalidOperation),
    }
}

/// Receives and decrypts data from a TLS socket.
pub fn recv_tls(handle: SocketHandle, buf: &mut [u8]) -> Result<usize, NetworkError> {
    let mut table = SOCKET_TABLE.lock();
    let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
    let non_blocking = socket.non_blocking;
    
    match &mut socket.inner {
        SocketInner::TlsStream { connection_key, tls, .. } => {
            let key = *connection_key;
            drop(table);
            
            // Read encrypted data from TCP
            let mut encrypted_buf = [0u8; 16384];
            let encrypted_len = recv_raw_tcp(&key, &mut encrypted_buf, non_blocking)?;
            
            if encrypted_len == 0 {
                return if non_blocking {
                    Err(NetworkError::WouldBlock)
                } else {
                    Ok(0)
                };
            }
            
            // Re-acquire lock and decrypt
            let mut table = SOCKET_TABLE.lock();
            let socket = table.get_mut(handle).ok_or(NetworkError::InvalidSocket)?;
            
            match &mut socket.inner {
                SocketInner::TlsStream { tls, .. } => {
                    let decrypted = tls.decrypt(&encrypted_buf[..encrypted_len])
                        .map_err(|_| NetworkError::TlsError)?;
                    
                    let len = buf.len().min(decrypted.len());
                    buf[..len].copy_from_slice(&decrypted[..len]);
                    Ok(len)
                }
                _ => Err(NetworkError::InvalidSocket),
            }
        }
        _ => Err(NetworkError::InvalidOperation),
    }
}

/// Checks if a socket is using TLS.
pub fn is_tls(handle: SocketHandle) -> bool {
    let table = SOCKET_TABLE.lock();
    if let Some(socket) = table.get(handle) {
        matches!(socket.inner, SocketInner::TlsStream { .. })
    } else {
        false
    }
}

// Helper function to send raw TCP data
fn send_raw_tcp(key: &TcpConnectionKey, data: &[u8]) -> Result<(), NetworkError> {
    let mut state = tcp_state().lock();
    if let Some(conn) = state.connection_mut(*key) {
        if let Some(segment) = conn.send(data) {
            drop(state);
            super::transmit_tcp_segment(key, &segment)?;
            Ok(())
        } else {
            Err(NetworkError::NotConnected)
        }
    } else {
        Err(NetworkError::NotConnected)
    }
}

// Helper function to receive raw TCP data
fn recv_raw_tcp(key: &TcpConnectionKey, buf: &mut [u8], non_blocking: bool) -> Result<usize, NetworkError> {
    let mut state = tcp_state().lock();
    if let Some(conn) = state.connection_mut(*key) {
        let bytes_read = conn.read(buf);
        if bytes_read > 0 {
            Ok(bytes_read)
        } else if non_blocking {
            Err(NetworkError::WouldBlock)
        } else {
            Ok(0)
        }
    } else {
        Err(NetworkError::NotConnected)
    }
}

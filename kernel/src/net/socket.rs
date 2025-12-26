//! # Socket Abstraction
//!
//! High-level socket API for network communication.

use alloc::collections::BTreeMap;
use spin::Mutex;

use super::device::NetworkError;
use super::ip::Ipv4Address;
use super::tcp::{tcp_state, TcpConnectionKey, TcpEndpoint};
use super::udp::{udp_state, UdpEndpoint};

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
}

/// Socket.
struct Socket {
    inner: SocketInner,
    /// Non-blocking mode.
    non_blocking: bool,
}

impl Socket {
    fn new_tcp_stream() -> Self {
        Self {
            inner: SocketInner::TcpStream {
                connection_key: None,
                local: None,
            },
            non_blocking: false,
        }
    }
    
    fn new_udp() -> Self {
        Self {
            inner: SocketInner::Udp {
                port: None,
                local: None,
            },
            non_blocking: false,
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
    }
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
                    };
                    
                    let new_handle = SOCKET_TABLE.lock().allocate(new_socket);
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
            connection_key: Some(_key),
            ..
        } => {
            // In a real implementation, this would queue data for transmission
            Ok(data.len())
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
    
    match &socket.inner {
        SocketInner::TcpStream {
            connection_key: Some(_key),
            ..
        } => {
            // In a real implementation, this would read from receive buffer
            if socket.non_blocking {
                Err(NetworkError::WouldBlock)
            } else {
                Ok(0) // No data available
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
        _ => Err(NetworkError::NotConnected),
    }
}

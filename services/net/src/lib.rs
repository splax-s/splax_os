//! # S-NET: Splax OS Network Service
//!
//! S-NET is the userspace network service that implements the full TCP/IP stack.
//! As part of Splax OS's microkernel architecture, network processing runs in
//! userspace while only packet DMA remains in the kernel.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      USERSPACE                              │
//! │ ┌─────────────────────────────────────────────────────────┐ │
//! │ │                     S-NET Service                       │ │
//! │ │  ┌─────────────────────────────────────────────────┐    │ │
//! │ │  │              Socket Manager                     │    │ │
//! │ │  │  - BSD socket API implementation                │    │ │
//! │ │  │  - Connection tracking                          │    │ │
//! │ │  │  - Port allocation                              │    │ │
//! │ │  └─────────────────────────────────────────────────┘    │ │
//! │ │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   │ │
//! │ │  │   TCP    │ │   UDP    │ │   ICMP   │ │   DNS    │   │ │
//! │ │  └──────────┘ └──────────┘ └──────────┘ └──────────┘   │ │
//! │ │  ┌─────────────────────────────────────────────────┐    │ │
//! │ │  │              IP Layer                           │    │ │
//! │ │  │  - Routing table                                │    │ │
//! │ │  │  - Fragmentation/reassembly                     │    │ │
//! │ │  └─────────────────────────────────────────────────┘    │ │
//! │ │  ┌─────────────────────────────────────────────────┐    │ │
//! │ │  │           Firewall (S-FIREWALL)                 │    │ │
//! │ │  └─────────────────────────────────────────────────┘    │ │
//! │ └─────────────────────────────────────────────────────────┘ │
//! ├─────────────────────────────────────────────────────────────┤
//! │                         S-LINK IPC                          │
//! ├─────────────────────────────────────────────────────────────┤
//! │                      KERNEL (S-CORE)                        │
//! │ ┌─────────────────────────────────────────────────────────┐ │
//! │ │              Packet DMA / Ring Buffers                  │ │
//! │ │  - Device driver (virtio-net, e1000)                    │ │
//! │ │  - Interrupt handling                                   │ │
//! │ │  - Zero-copy packet transfer                            │ │
//! │ └─────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## IPC Protocol
//!
//! S-NET communicates with the kernel and applications via S-LINK channels:
//!
//! - `net.socket.create` - Create a new socket
//! - `net.socket.bind` - Bind socket to address
//! - `net.socket.connect` - Connect to remote host
//! - `net.socket.listen` - Listen for connections
//! - `net.socket.accept` - Accept incoming connection
//! - `net.socket.send` - Send data
//! - `net.socket.recv` - Receive data
//! - `net.socket.close` - Close socket
//!
//! ## Security
//!
//! All network operations require appropriate S-CAP capabilities:
//!
//! - `cap:net:bind` - Permission to bind to ports
//! - `cap:net:connect` - Permission to make outgoing connections
//! - `cap:net:listen` - Permission to accept incoming connections
//! - `cap:net:raw` - Permission for raw socket access

#![no_std]

extern crate alloc;

pub mod socket;
pub mod tcp;
pub mod udp;
pub mod icmp;
pub mod ip;
pub mod firewall;
pub mod config;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use socket::Socket;
use core::sync::atomic::{AtomicU32, Ordering};

/// S-NET service version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// S-NET service name for IPC registration
pub const SERVICE_NAME: &str = "net";

/// Network service configuration
#[derive(Debug, Clone)]
pub struct NetConfig {
    /// Maximum number of sockets
    pub max_sockets: usize,
    /// Maximum connections per socket
    pub max_connections: usize,
    /// TCP receive buffer size
    pub tcp_recv_buffer: usize,
    /// TCP send buffer size
    pub tcp_send_buffer: usize,
    /// UDP buffer size
    pub udp_buffer: usize,
    /// Enable IPv6
    pub ipv6_enabled: bool,
    /// Enable firewall
    pub firewall_enabled: bool,
}

impl Default for NetConfig {
    fn default() -> Self {
        Self {
            max_sockets: 1024,
            max_connections: 128,
            tcp_recv_buffer: 65536,
            tcp_send_buffer: 65536,
            udp_buffer: 65536,
            ipv6_enabled: true,
            firewall_enabled: true,
        }
    }
}

/// Network interface information
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    /// Interface name
    pub name: String,
    /// MAC address
    pub mac: [u8; 6],
    /// IPv4 address
    pub ipv4: Option<[u8; 4]>,
    /// IPv6 addresses
    pub ipv6: Vec<[u8; 16]>,
    /// MTU
    pub mtu: usize,
    /// Interface is up
    pub up: bool,
}

/// Socket manager for tracking active sockets
pub struct SocketManager {
    /// Active sockets by handle
    sockets: BTreeMap<u32, Socket>,
    /// Next socket handle
    next_handle: AtomicU32,
    /// Maximum allowed sockets
    max_sockets: usize,
}

impl SocketManager {
    /// Create a new socket manager
    pub fn new(max_sockets: usize) -> Self {
        Self {
            sockets: BTreeMap::new(),
            next_handle: AtomicU32::new(1),
            max_sockets,
        }
    }

    /// Create a new socket
    pub fn create(&mut self, domain: SocketDomain, sock_type: SocketType, protocol: u8) -> Result<u32, NetError> {
        if self.sockets.len() >= self.max_sockets {
            return Err(NetError::TooManySockets);
        }
        let socket = Socket::new(domain, sock_type, protocol);
        let handle = socket.handle;
        self.sockets.insert(handle, socket);
        Ok(handle)
    }

    /// Get a socket by handle
    pub fn get(&self, handle: u32) -> Option<&Socket> {
        self.sockets.get(&handle)
    }

    /// Get a mutable socket by handle
    pub fn get_mut(&mut self, handle: u32) -> Option<&mut Socket> {
        self.sockets.get_mut(&handle)
    }

    /// Close and remove a socket
    pub fn close(&mut self, handle: u32) -> Result<(), NetError> {
        self.sockets.remove(&handle).map(|_| ()).ok_or(NetError::InvalidSocket)
    }

    /// Get the number of active sockets
    pub fn count(&self) -> usize {
        self.sockets.len()
    }
}

/// Network service state
pub struct NetService {
    /// Configuration
    config: NetConfig,
    /// Network interfaces
    interfaces: BTreeMap<String, InterfaceInfo>,
    /// Socket manager
    sockets: SocketManager,
}

impl NetService {
    /// Creates a new network service
    pub fn new(config: NetConfig) -> Self {
        let max_sockets = config.max_sockets;
        Self {
            config,
            interfaces: BTreeMap::new(),
            sockets: SocketManager::new(max_sockets),
        }
    }

    /// Initializes the network service
    pub fn init(&mut self) -> Result<(), NetError> {
        // Register with S-LINK for IPC
        // Request packet channels from kernel
        // Initialize protocol stacks
        Ok(())
    }

    /// Main service loop
    pub fn run(&mut self) -> ! {
        loop {
            // Process IPC messages
            // Handle incoming packets
            // Process timers (TCP retransmission, etc.)
            core::hint::spin_loop();
        }
    }

    /// Registers a network interface
    pub fn register_interface(&mut self, info: InterfaceInfo) {
        self.interfaces.insert(info.name.clone(), info);
    }

    /// Gets interface info
    pub fn get_interface(&self, name: &str) -> Option<&InterfaceInfo> {
        self.interfaces.get(name)
    }

    /// Lists all interfaces
    pub fn list_interfaces(&self) -> Vec<&InterfaceInfo> {
        self.interfaces.values().collect()
    }
}

/// Network service errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    /// Socket limit reached
    TooManySockets,
    /// Invalid socket handle
    InvalidSocket,
    /// Address already in use
    AddressInUse,
    /// Connection refused
    ConnectionRefused,
    /// Connection reset
    ConnectionReset,
    /// Network unreachable
    NetworkUnreachable,
    /// Host unreachable
    HostUnreachable,
    /// Operation timed out
    TimedOut,
    /// Permission denied (capability check failed)
    PermissionDenied,
    /// Invalid argument
    InvalidArgument,
    /// Operation would block
    WouldBlock,
    /// Not connected
    NotConnected,
    /// Already connected
    AlreadyConnected,
    /// Buffer full
    BufferFull,
    /// Interface not found
    InterfaceNotFound,
}

/// IPC message types for socket operations
#[derive(Debug, Clone)]
pub enum SocketMessage {
    /// Create a socket
    Create {
        domain: SocketDomain,
        sock_type: SocketType,
        protocol: u8,
    },
    /// Bind socket to address
    Bind {
        socket: u32,
        addr: SocketAddr,
    },
    /// Connect to remote address
    Connect {
        socket: u32,
        addr: SocketAddr,
    },
    /// Listen for connections
    Listen {
        socket: u32,
        backlog: u32,
    },
    /// Accept connection
    Accept {
        socket: u32,
    },
    /// Send data
    Send {
        socket: u32,
        data: Vec<u8>,
        flags: u32,
    },
    /// Receive data
    Recv {
        socket: u32,
        max_len: usize,
        flags: u32,
    },
    /// Close socket
    Close {
        socket: u32,
    },
    /// Set socket option
    SetOpt {
        socket: u32,
        level: u32,
        option: u32,
        value: Vec<u8>,
    },
    /// Get socket option
    GetOpt {
        socket: u32,
        level: u32,
        option: u32,
    },
}

/// Socket domain (address family)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SocketDomain {
    /// IPv4
    Inet,
    /// IPv6
    Inet6,
    /// Unix domain (local)
    Unix,
}

/// Socket type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketType {
    /// Stream (TCP)
    Stream,
    /// Datagram (UDP)
    Dgram,
    /// Raw socket
    Raw,
}

/// Socket address
#[derive(Debug, Clone)]
pub enum SocketAddr {
    /// IPv4 address
    V4 {
        addr: [u8; 4],
        port: u16,
    },
    /// IPv6 address
    V6 {
        addr: [u8; 16],
        port: u16,
        flowinfo: u32,
        scope_id: u32,
    },
}

impl SocketAddr {
    /// Creates an IPv4 address
    pub fn v4(a: u8, b: u8, c: u8, d: u8, port: u16) -> Self {
        SocketAddr::V4 {
            addr: [a, b, c, d],
            port,
        }
    }

    /// Creates an IPv6 address
    pub fn v6(addr: [u8; 16], port: u16) -> Self {
        SocketAddr::V6 {
            addr,
            port,
            flowinfo: 0,
            scope_id: 0,
        }
    }

    /// Returns the port
    pub fn port(&self) -> u16 {
        match self {
            SocketAddr::V4 { port, .. } => *port,
            SocketAddr::V6 { port, .. } => *port,
        }
    }
}

//! # UDP (User Datagram Protocol)
//!
//! UDP implementation for simple, connectionless data delivery.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use super::device::NetworkError;
use super::ip::{checksum_bytes, Ipv4Address, Ipv4Packet, PROTOCOL_UDP};

/// UDP header.
#[derive(Debug, Clone)]
pub struct UdpHeader {
    /// Source port.
    pub src_port: u16,
    /// Destination port.
    pub dest_port: u16,
    /// Total length (header + data).
    pub length: u16,
    /// Checksum.
    pub checksum: u16,
}

impl UdpHeader {
    /// Header size (8 bytes).
    pub const SIZE: usize = 8;
    
    /// Parses a UDP header from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        
        Some(Self {
            src_port: u16::from_be_bytes([data[0], data[1]]),
            dest_port: u16::from_be_bytes([data[2], data[3]]),
            length: u16::from_be_bytes([data[4], data[5]]),
            checksum: u16::from_be_bytes([data[6], data[7]]),
        })
    }
    
    /// Serializes the header to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::SIZE);
        bytes.extend_from_slice(&self.src_port.to_be_bytes());
        bytes.extend_from_slice(&self.dest_port.to_be_bytes());
        bytes.extend_from_slice(&self.length.to_be_bytes());
        bytes.extend_from_slice(&self.checksum.to_be_bytes());
        bytes
    }
    
    /// Creates a new header.
    pub fn new(src_port: u16, dest_port: u16, data_len: usize) -> Self {
        Self {
            src_port,
            dest_port,
            length: (Self::SIZE + data_len) as u16,
            checksum: 0,
        }
    }
}

/// UDP datagram.
#[derive(Debug, Clone)]
pub struct UdpDatagram {
    pub header: UdpHeader,
    pub data: Vec<u8>,
}

impl UdpDatagram {
    /// Parses from IP packet payload.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = UdpHeader::parse(data)?;
        
        let payload_start = UdpHeader::SIZE;
        let payload_end = header.length as usize;
        
        if data.len() < payload_end {
            return None;
        }
        
        let payload = data[payload_start..payload_end].to_vec();
        
        Some(Self {
            header,
            data: payload,
        })
    }
    
    /// Serializes to bytes.
    pub fn to_bytes(&self, src_addr: Ipv4Address, dest_addr: Ipv4Address) -> Vec<u8> {
        let mut header = self.header.clone();
        header.checksum = 0;
        
        let mut bytes = header.to_bytes();
        bytes.extend_from_slice(&self.data);
        
        // Compute checksum with pseudo-header
        header.checksum = udp_checksum(src_addr, dest_addr, &bytes);
        
        // Rebuild with correct checksum
        let mut result = header.to_bytes();
        result.extend_from_slice(&self.data);
        result
    }
    
    /// Creates a new datagram.
    pub fn new(src_port: u16, dest_port: u16, data: Vec<u8>) -> Self {
        Self {
            header: UdpHeader::new(src_port, dest_port, data.len()),
            data,
        }
    }
}

/// Computes UDP checksum with pseudo-header.
pub fn udp_checksum(src_addr: Ipv4Address, dest_addr: Ipv4Address, udp_data: &[u8]) -> u16 {
    // Build pseudo-header
    let mut pseudo_header = Vec::with_capacity(12 + udp_data.len());
    pseudo_header.extend_from_slice(&src_addr.0);
    pseudo_header.extend_from_slice(&dest_addr.0);
    pseudo_header.push(0); // Zero
    pseudo_header.push(PROTOCOL_UDP); // Protocol
    pseudo_header.extend_from_slice(&(udp_data.len() as u16).to_be_bytes());
    pseudo_header.extend_from_slice(udp_data);
    
    checksum_bytes(&pseudo_header)
}

/// UDP endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UdpEndpoint {
    pub addr: Ipv4Address,
    pub port: u16,
}

impl UdpEndpoint {
    pub fn new(addr: Ipv4Address, port: u16) -> Self {
        Self { addr, port }
    }
    
    /// Any address (0.0.0.0).
    pub fn any(port: u16) -> Self {
        Self::new(Ipv4Address::ANY, port)
    }
}

/// Received UDP message.
#[derive(Debug, Clone)]
pub struct UdpMessage {
    pub remote: UdpEndpoint,
    pub data: Vec<u8>,
}

/// UDP socket.
pub struct UdpSocket {
    /// Local endpoint.
    pub local: UdpEndpoint,
    /// Receive buffer.
    pub recv_buffer: Vec<UdpMessage>,
    /// Maximum buffer size.
    pub max_buffer: usize,
}

impl UdpSocket {
    /// Default buffer size.
    const DEFAULT_BUFFER_SIZE: usize = 64;
    
    /// Creates a new socket.
    pub fn new(local: UdpEndpoint) -> Self {
        Self {
            local,
            recv_buffer: Vec::new(),
            max_buffer: Self::DEFAULT_BUFFER_SIZE,
        }
    }
    
    /// Queues a received message.
    pub fn queue_message(&mut self, remote: UdpEndpoint, data: Vec<u8>) {
        if self.recv_buffer.len() < self.max_buffer {
            self.recv_buffer.push(UdpMessage { remote, data });
        }
        // Drop if buffer full
    }
    
    /// Receives a message.
    pub fn recv(&mut self) -> Option<UdpMessage> {
        if self.recv_buffer.is_empty() {
            None
        } else {
            Some(self.recv_buffer.remove(0))
        }
    }
    
    /// Creates a datagram to send.
    pub fn send_to(&self, remote: UdpEndpoint, data: Vec<u8>) -> UdpDatagram {
        UdpDatagram::new(self.local.port, remote.port, data)
    }
}

/// Global UDP state.
pub struct UdpState {
    /// Bound sockets by port.
    sockets: BTreeMap<u16, UdpSocket>,
    /// Next ephemeral port.
    next_ephemeral_port: u16,
}

impl UdpState {
    /// Ephemeral port range start.
    const EPHEMERAL_START: u16 = 49152;
    /// Ephemeral port range end.
    const EPHEMERAL_END: u16 = 65535;
    
    /// Creates new UDP state.
    pub const fn new() -> Self {
        Self {
            sockets: BTreeMap::new(),
            next_ephemeral_port: Self::EPHEMERAL_START,
        }
    }
    
    /// Allocates an ephemeral port.
    pub fn allocate_port(&mut self) -> u16 {
        let port = self.next_ephemeral_port;
        self.next_ephemeral_port = if self.next_ephemeral_port >= Self::EPHEMERAL_END {
            Self::EPHEMERAL_START
        } else {
            self.next_ephemeral_port + 1
        };
        port
    }
    
    /// Binds a socket to a port.
    pub fn bind(&mut self, port: u16, local_addr: Ipv4Address) -> Result<(), NetworkError> {
        if self.sockets.contains_key(&port) {
            return Err(NetworkError::AddressInUse);
        }
        
        let socket = UdpSocket::new(UdpEndpoint::new(local_addr, port));
        self.sockets.insert(port, socket);
        Ok(())
    }
    
    /// Unbinds a socket.
    pub fn unbind(&mut self, port: u16) {
        self.sockets.remove(&port);
    }
    
    /// Gets a socket by port.
    pub fn socket(&mut self, port: u16) -> Option<&mut UdpSocket> {
        self.sockets.get_mut(&port)
    }
    
    /// Delivers a datagram to the appropriate socket.
    pub fn deliver(&mut self, remote: UdpEndpoint, local_port: u16, data: Vec<u8>) {
        if let Some(socket) = self.sockets.get_mut(&local_port) {
            socket.queue_message(remote, data);
        }
    }
}

/// Static UDP state.
static UDP_STATE: Mutex<UdpState> = Mutex::new(UdpState::new());

/// Gets UDP state.
pub fn udp_state() -> &'static Mutex<UdpState> {
    &UDP_STATE
}

/// Handles an incoming UDP packet.
pub fn handle_packet(ip_packet: &Ipv4Packet) {
    let datagram = match UdpDatagram::parse(&ip_packet.payload) {
        Some(d) => d,
        None => return,
    };
    
    let remote = UdpEndpoint::new(ip_packet.src_addr, datagram.header.src_port);
    let local_port = datagram.header.dest_port;
    
    UDP_STATE.lock().deliver(remote, local_port, datagram.data);
}

/// DNS query port.
pub const DNS_PORT: u16 = 53;
/// DHCP client port.
pub const DHCP_CLIENT_PORT: u16 = 68;
/// DHCP server port.
pub const DHCP_SERVER_PORT: u16 = 67;

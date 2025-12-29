//! # UDP Implementation
//!
//! Userspace UDP implementation for the S-NET service.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;

/// UDP datagram
#[derive(Debug, Clone)]
pub struct UdpDatagram {
    /// Source port
    pub src_port: u16,
    /// Destination port
    pub dst_port: u16,
    /// Length (header + data)
    pub length: u16,
    /// Checksum
    pub checksum: u16,
    /// Payload data
    pub payload: Vec<u8>,
}

impl UdpDatagram {
    /// UDP header size
    pub const HEADER_SIZE: usize = 8;
    /// Maximum UDP payload (65535 - 20 IP - 8 UDP)
    pub const MAX_PAYLOAD: usize = 65507;

    /// Creates a new UDP datagram
    pub fn new(src_port: u16, dst_port: u16, payload: Vec<u8>) -> Self {
        let length = (Self::HEADER_SIZE + payload.len()) as u16;
        Self {
            src_port,
            dst_port,
            length,
            checksum: 0,
            payload,
        }
    }

    /// Parses a UDP datagram from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::HEADER_SIZE {
            return None;
        }

        let src_port = u16::from_be_bytes([data[0], data[1]]);
        let dst_port = u16::from_be_bytes([data[2], data[3]]);
        let length = u16::from_be_bytes([data[4], data[5]]);
        let checksum = u16::from_be_bytes([data[6], data[7]]);

        if data.len() < length as usize {
            return None;
        }

        let payload = data[Self::HEADER_SIZE..length as usize].to_vec();

        Some(Self {
            src_port,
            dst_port,
            length,
            checksum,
            payload,
        })
    }

    /// Serializes the datagram to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::HEADER_SIZE + self.payload.len());
        buf.extend_from_slice(&self.src_port.to_be_bytes());
        buf.extend_from_slice(&self.dst_port.to_be_bytes());
        buf.extend_from_slice(&self.length.to_be_bytes());
        buf.extend_from_slice(&self.checksum.to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Calculates UDP checksum (with IP pseudo-header)
    pub fn calculate_checksum(&mut self, src_ip: u32, dst_ip: u32) {
        let mut sum: u32 = 0;

        // Pseudo-header
        sum += (src_ip >> 16) as u32;
        sum += (src_ip & 0xFFFF) as u32;
        sum += (dst_ip >> 16) as u32;
        sum += (dst_ip & 0xFFFF) as u32;
        sum += 17; // Protocol: UDP
        sum += self.length as u32;

        // UDP header
        sum += self.src_port as u32;
        sum += self.dst_port as u32;
        sum += self.length as u32;

        // Payload
        let mut i = 0;
        while i + 1 < self.payload.len() {
            sum += u16::from_be_bytes([self.payload[i], self.payload[i + 1]]) as u32;
            i += 2;
        }
        if i < self.payload.len() {
            sum += (self.payload[i] as u32) << 8;
        }

        // Fold to 16 bits
        while sum > 0xFFFF {
            sum = (sum >> 16) + (sum & 0xFFFF);
        }

        self.checksum = !(sum as u16);
        if self.checksum == 0 {
            self.checksum = 0xFFFF;
        }
    }

    /// Verifies checksum
    pub fn verify_checksum(&self, src_ip: u32, dst_ip: u32) -> bool {
        let mut dgram = self.clone();
        let original = self.checksum;
        dgram.calculate_checksum(src_ip, dst_ip);
        dgram.checksum == original || self.checksum == 0
    }
}

/// Received datagram with source information
#[derive(Debug, Clone)]
pub struct ReceivedDatagram {
    /// Source IP address
    pub src_ip: u32,
    /// Source port
    pub src_port: u16,
    /// Payload data
    pub data: Vec<u8>,
}

/// UDP socket state
pub struct UdpSocket {
    /// Local port (0 if unbound)
    pub local_port: u16,
    /// Bound local IP (0 = any)
    pub local_ip: u32,
    /// Receive queue
    pub recv_queue: VecDeque<ReceivedDatagram>,
    /// Maximum receive queue size
    pub max_queue_size: usize,
    /// Broadcast enabled
    pub broadcast: bool,
}

impl UdpSocket {
    /// Creates a new UDP socket
    pub fn new() -> Self {
        Self {
            local_port: 0,
            local_ip: 0,
            recv_queue: VecDeque::new(),
            max_queue_size: 128,
            broadcast: false,
        }
    }

    /// Binds to a local address
    pub fn bind(&mut self, ip: u32, port: u16) -> Result<(), &'static str> {
        if self.local_port != 0 {
            return Err("Already bound");
        }
        self.local_ip = ip;
        self.local_port = port;
        Ok(())
    }

    /// Receives a datagram
    pub fn recv(&mut self) -> Option<ReceivedDatagram> {
        self.recv_queue.pop_front()
    }

    /// Delivers a datagram to this socket
    pub fn deliver(&mut self, src_ip: u32, src_port: u16, data: Vec<u8>) -> Result<(), &'static str> {
        if self.recv_queue.len() >= self.max_queue_size {
            return Err("Receive queue full");
        }

        self.recv_queue.push_back(ReceivedDatagram {
            src_ip,
            src_port,
            data,
        });
        Ok(())
    }

    /// Checks if socket has pending data
    pub fn has_data(&self) -> bool {
        !self.recv_queue.is_empty()
    }

    /// Returns number of pending datagrams
    pub fn pending_count(&self) -> usize {
        self.recv_queue.len()
    }
}

impl Default for UdpSocket {
    fn default() -> Self {
        Self::new()
    }
}

/// UDP manager
pub struct UdpManager {
    /// Bound sockets: port -> socket index
    bound_sockets: BTreeMap<u16, usize>,
    /// All sockets
    sockets: Vec<UdpSocket>,
    /// Outgoing datagram queue
    outgoing: VecDeque<(u32, UdpDatagram)>, // (dst_ip, datagram)
}

impl UdpManager {
    /// Creates a new UDP manager
    pub fn new() -> Self {
        Self {
            bound_sockets: BTreeMap::new(),
            sockets: Vec::new(),
            outgoing: VecDeque::new(),
        }
    }

    /// Creates a new socket
    pub fn create_socket(&mut self) -> usize {
        let idx = self.sockets.len();
        self.sockets.push(UdpSocket::new());
        idx
    }

    /// Binds a socket to a port
    pub fn bind(&mut self, socket_idx: usize, ip: u32, port: u16) -> Result<(), &'static str> {
        if self.bound_sockets.contains_key(&port) {
            return Err("Port already in use");
        }

        if socket_idx >= self.sockets.len() {
            return Err("Invalid socket");
        }

        self.sockets[socket_idx].bind(ip, port)?;
        self.bound_sockets.insert(port, socket_idx);
        Ok(())
    }

    /// Sends a datagram
    pub fn send(
        &mut self,
        socket_idx: usize,
        dst_ip: u32,
        dst_port: u16,
        data: Vec<u8>,
    ) -> Result<(), &'static str> {
        if socket_idx >= self.sockets.len() {
            return Err("Invalid socket");
        }

        if data.len() > UdpDatagram::MAX_PAYLOAD {
            return Err("Payload too large");
        }

        let socket = &self.sockets[socket_idx];
        let src_port = if socket.local_port == 0 {
            // Allocate ephemeral port
            self.allocate_ephemeral_port()
                .ok_or("No ephemeral ports available")?
        } else {
            socket.local_port
        };

        let mut datagram = UdpDatagram::new(src_port, dst_port, data);
        
        // Calculate checksum (assuming socket local_ip as source)
        datagram.calculate_checksum(socket.local_ip, dst_ip);

        self.outgoing.push_back((dst_ip, datagram));
        Ok(())
    }

    /// Receives incoming datagram for a socket
    pub fn recv(&mut self, socket_idx: usize) -> Option<ReceivedDatagram> {
        self.sockets.get_mut(socket_idx)?.recv()
    }

    /// Processes an incoming datagram
    pub fn process_incoming(&mut self, src_ip: u32, datagram: UdpDatagram) {
        if let Some(&socket_idx) = self.bound_sockets.get(&datagram.dst_port) {
            if let Some(socket) = self.sockets.get_mut(socket_idx) {
                let _ = socket.deliver(src_ip, datagram.src_port, datagram.payload);
            }
        }
        // Drop datagram if no socket bound to port
    }

    /// Gets next outgoing datagram
    pub fn poll_outgoing(&mut self) -> Option<(u32, UdpDatagram)> {
        self.outgoing.pop_front()
    }

    /// Allocates an ephemeral port
    fn allocate_ephemeral_port(&self) -> Option<u16> {
        for port in 49152..=65535u16 {
            if !self.bound_sockets.contains_key(&port) {
                return Some(port);
            }
        }
        None
    }

    /// Closes a socket
    pub fn close(&mut self, socket_idx: usize) {
        if socket_idx < self.sockets.len() {
            let port = self.sockets[socket_idx].local_port;
            self.bound_sockets.remove(&port);
            // Mark socket as closed (don't remove to preserve indices)
            self.sockets[socket_idx].local_port = 0;
        }
    }
}

impl Default for UdpManager {
    fn default() -> Self {
        Self::new()
    }
}

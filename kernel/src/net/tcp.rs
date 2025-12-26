//! # TCP (Transmission Control Protocol)
//!
//! TCP implementation for reliable, ordered data delivery.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use super::device::NetworkError;
use super::ip::{checksum_bytes, Ipv4Address, Ipv4Packet, PROTOCOL_TCP};

/// TCP flags.
pub mod flags {
    pub const FIN: u8 = 0x01;
    pub const SYN: u8 = 0x02;
    pub const RST: u8 = 0x04;
    pub const PSH: u8 = 0x08;
    pub const ACK: u8 = 0x10;
    pub const URG: u8 = 0x20;
}

/// TCP connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpConnectionState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

/// TCP connection endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TcpEndpoint {
    pub addr: Ipv4Address,
    pub port: u16,
}

impl TcpEndpoint {
    pub fn new(addr: Ipv4Address, port: u16) -> Self {
        Self { addr, port }
    }
}

/// TCP connection key (4-tuple).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TcpConnectionKey {
    pub local: TcpEndpoint,
    pub remote: TcpEndpoint,
}

/// TCP header.
#[derive(Debug, Clone)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dest_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset: u8,
    pub flags: u8,
    pub window: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
    pub options: Vec<u8>,
}

impl TcpHeader {
    /// Minimum header size (20 bytes).
    pub const MIN_SIZE: usize = 20;
    
    /// Parses a TCP header from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::MIN_SIZE {
            return None;
        }
        
        let src_port = u16::from_be_bytes([data[0], data[1]]);
        let dest_port = u16::from_be_bytes([data[2], data[3]]);
        let seq_num = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ack_num = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let data_offset = (data[12] >> 4) * 4;
        let flags = data[13];
        let window = u16::from_be_bytes([data[14], data[15]]);
        let checksum = u16::from_be_bytes([data[16], data[17]]);
        let urgent_ptr = u16::from_be_bytes([data[18], data[19]]);
        
        let options = if data_offset as usize > Self::MIN_SIZE {
            data[Self::MIN_SIZE..data_offset as usize].to_vec()
        } else {
            Vec::new()
        };
        
        Some(Self {
            src_port,
            dest_port,
            seq_num,
            ack_num,
            data_offset,
            flags,
            window,
            checksum,
            urgent_ptr,
            options,
        })
    }
    
    /// Serializes the header to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let data_offset = ((Self::MIN_SIZE + self.options.len() + 3) / 4) as u8;
        
        let mut bytes = Vec::with_capacity(Self::MIN_SIZE + self.options.len());
        
        bytes.extend_from_slice(&self.src_port.to_be_bytes());
        bytes.extend_from_slice(&self.dest_port.to_be_bytes());
        bytes.extend_from_slice(&self.seq_num.to_be_bytes());
        bytes.extend_from_slice(&self.ack_num.to_be_bytes());
        bytes.push((data_offset << 4) | 0);
        bytes.push(self.flags);
        bytes.extend_from_slice(&self.window.to_be_bytes());
        bytes.extend_from_slice(&self.checksum.to_be_bytes());
        bytes.extend_from_slice(&self.urgent_ptr.to_be_bytes());
        bytes.extend_from_slice(&self.options);
        
        // Pad to 4-byte boundary
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        
        bytes
    }
    
    /// Creates a new header.
    pub fn new(src_port: u16, dest_port: u16, seq_num: u32, ack_num: u32, flags: u8) -> Self {
        Self {
            src_port,
            dest_port,
            seq_num,
            ack_num,
            data_offset: Self::MIN_SIZE as u8,
            flags,
            window: 65535,
            checksum: 0,
            urgent_ptr: 0,
            options: Vec::new(),
        }
    }
}

/// TCP segment (header + data).
#[derive(Debug, Clone)]
pub struct TcpSegment {
    pub header: TcpHeader,
    pub data: Vec<u8>,
}

impl TcpSegment {
    /// Parses from IP packet payload.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = TcpHeader::parse(data)?;
        let payload_start = header.data_offset as usize;
        
        if data.len() < payload_start {
            return None;
        }
        
        let payload = data[payload_start..].to_vec();
        
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
        header.checksum = tcp_checksum(src_addr, dest_addr, &bytes);
        
        // Rebuild with correct checksum
        let mut result = header.to_bytes();
        result.extend_from_slice(&self.data);
        result
    }
    
    /// Creates a new segment.
    pub fn new(
        src_port: u16,
        dest_port: u16,
        seq_num: u32,
        ack_num: u32,
        flags: u8,
        data: Vec<u8>,
    ) -> Self {
        Self {
            header: TcpHeader::new(src_port, dest_port, seq_num, ack_num, flags),
            data,
        }
    }
}

/// Computes TCP checksum with pseudo-header.
pub fn tcp_checksum(src_addr: Ipv4Address, dest_addr: Ipv4Address, tcp_data: &[u8]) -> u16 {
    // Build pseudo-header
    let mut pseudo_header = Vec::with_capacity(12 + tcp_data.len());
    pseudo_header.extend_from_slice(&src_addr.0);
    pseudo_header.extend_from_slice(&dest_addr.0);
    pseudo_header.push(0); // Zero
    pseudo_header.push(PROTOCOL_TCP); // Protocol
    pseudo_header.extend_from_slice(&(tcp_data.len() as u16).to_be_bytes());
    pseudo_header.extend_from_slice(tcp_data);
    
    checksum_bytes(&pseudo_header)
}

/// TCP connection state machine.
pub struct TcpConnection {
    /// Connection key.
    pub key: TcpConnectionKey,
    /// Current state.
    pub state: TcpConnectionState,
    /// Send sequence number.
    pub send_next: u32,
    /// Send unacknowledged.
    pub send_unack: u32,
    /// Send window.
    pub send_window: u16,
    /// Receive next expected.
    pub recv_next: u32,
    /// Receive window.
    pub recv_window: u16,
    /// Receive buffer.
    pub recv_buffer: Vec<u8>,
    /// Send buffer.
    pub send_buffer: Vec<u8>,
    /// Initial sequence number.
    pub initial_seq: u32,
}

impl TcpConnection {
    /// Creates a new connection.
    pub fn new(key: TcpConnectionKey) -> Self {
        // Generate initial sequence number (simplified)
        let initial_seq = 1000u32;
        
        Self {
            key,
            state: TcpConnectionState::Closed,
            send_next: initial_seq,
            send_unack: initial_seq,
            send_window: 65535,
            recv_next: 0,
            recv_window: 65535,
            recv_buffer: Vec::new(),
            send_buffer: Vec::new(),
            initial_seq,
        }
    }
    
    /// Initiates a connection (active open).
    pub fn connect(&mut self) -> TcpSegment {
        self.state = TcpConnectionState::SynSent;
        
        TcpSegment::new(
            self.key.local.port,
            self.key.remote.port,
            self.send_next,
            0,
            flags::SYN,
            Vec::new(),
        )
    }
    
    /// Starts listening (passive open).
    pub fn listen(&mut self) {
        self.state = TcpConnectionState::Listen;
    }
    
    /// Processes an incoming segment.
    pub fn process(&mut self, segment: &TcpSegment) -> Option<TcpSegment> {
        match self.state {
            TcpConnectionState::Listen => {
                if segment.header.flags & flags::SYN != 0 {
                    // SYN received - send SYN-ACK
                    self.recv_next = segment.header.seq_num.wrapping_add(1);
                    self.state = TcpConnectionState::SynReceived;
                    
                    return Some(TcpSegment::new(
                        self.key.local.port,
                        self.key.remote.port,
                        self.send_next,
                        self.recv_next,
                        flags::SYN | flags::ACK,
                        Vec::new(),
                    ));
                }
            }
            
            TcpConnectionState::SynSent => {
                if segment.header.flags & (flags::SYN | flags::ACK) == (flags::SYN | flags::ACK) {
                    // SYN-ACK received - send ACK
                    self.send_unack = segment.header.ack_num;
                    self.recv_next = segment.header.seq_num.wrapping_add(1);
                    self.state = TcpConnectionState::Established;
                    
                    return Some(TcpSegment::new(
                        self.key.local.port,
                        self.key.remote.port,
                        self.send_next,
                        self.recv_next,
                        flags::ACK,
                        Vec::new(),
                    ));
                }
            }
            
            TcpConnectionState::SynReceived => {
                if segment.header.flags & flags::ACK != 0 {
                    // ACK of SYN-ACK received - connection established
                    self.send_unack = segment.header.ack_num;
                    self.state = TcpConnectionState::Established;
                }
            }
            
            TcpConnectionState::Established => {
                // Handle data and control segments
                if segment.header.flags & flags::FIN != 0 {
                    // FIN received - begin close
                    self.recv_next = segment.header.seq_num.wrapping_add(1);
                    self.state = TcpConnectionState::CloseWait;
                    
                    return Some(TcpSegment::new(
                        self.key.local.port,
                        self.key.remote.port,
                        self.send_next,
                        self.recv_next,
                        flags::ACK,
                        Vec::new(),
                    ));
                }
                
                // Accept data
                if !segment.data.is_empty() {
                    if segment.header.seq_num == self.recv_next {
                        self.recv_buffer.extend_from_slice(&segment.data);
                        self.recv_next = self.recv_next.wrapping_add(segment.data.len() as u32);
                        
                        return Some(TcpSegment::new(
                            self.key.local.port,
                            self.key.remote.port,
                            self.send_next,
                            self.recv_next,
                            flags::ACK,
                            Vec::new(),
                        ));
                    }
                }
            }
            
            TcpConnectionState::CloseWait => {
                // Application should call close()
            }
            
            TcpConnectionState::FinWait1 => {
                if segment.header.flags & flags::ACK != 0 {
                    self.state = TcpConnectionState::FinWait2;
                }
            }
            
            TcpConnectionState::FinWait2 => {
                if segment.header.flags & flags::FIN != 0 {
                    self.recv_next = segment.header.seq_num.wrapping_add(1);
                    self.state = TcpConnectionState::TimeWait;
                    
                    return Some(TcpSegment::new(
                        self.key.local.port,
                        self.key.remote.port,
                        self.send_next,
                        self.recv_next,
                        flags::ACK,
                        Vec::new(),
                    ));
                }
            }
            
            _ => {}
        }
        
        None
    }
    
    /// Sends data.
    pub fn send(&mut self, data: &[u8]) -> Option<TcpSegment> {
        if self.state != TcpConnectionState::Established {
            return None;
        }
        
        let segment = TcpSegment::new(
            self.key.local.port,
            self.key.remote.port,
            self.send_next,
            self.recv_next,
            flags::ACK | flags::PSH,
            data.to_vec(),
        );
        
        self.send_next = self.send_next.wrapping_add(data.len() as u32);
        
        Some(segment)
    }
    
    /// Initiates close.
    pub fn close(&mut self) -> Option<TcpSegment> {
        match self.state {
            TcpConnectionState::Established => {
                self.state = TcpConnectionState::FinWait1;
                
                Some(TcpSegment::new(
                    self.key.local.port,
                    self.key.remote.port,
                    self.send_next,
                    self.recv_next,
                    flags::FIN | flags::ACK,
                    Vec::new(),
                ))
            }
            TcpConnectionState::CloseWait => {
                self.state = TcpConnectionState::LastAck;
                
                Some(TcpSegment::new(
                    self.key.local.port,
                    self.key.remote.port,
                    self.send_next,
                    self.recv_next,
                    flags::FIN | flags::ACK,
                    Vec::new(),
                ))
            }
            _ => None,
        }
    }
    
    /// Reads received data.
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let len = buf.len().min(self.recv_buffer.len());
        buf[..len].copy_from_slice(&self.recv_buffer[..len]);
        self.recv_buffer.drain(..len);
        len
    }
}

/// TCP listening socket.
pub struct TcpListener {
    pub local: TcpEndpoint,
    pub backlog: Vec<TcpConnection>,
    pub max_backlog: usize,
}

impl TcpListener {
    /// Creates a new listener.
    pub fn new(local: TcpEndpoint, max_backlog: usize) -> Self {
        Self {
            local,
            backlog: Vec::new(),
            max_backlog,
        }
    }
    
    /// Accepts a pending connection.
    pub fn accept(&mut self) -> Option<TcpConnection> {
        self.backlog.iter().position(|c| c.state == TcpConnectionState::Established)
            .map(|i| self.backlog.remove(i))
    }
}

/// Global TCP state.
pub struct TcpState {
    /// Active connections.
    connections: BTreeMap<TcpConnectionKey, TcpConnection>,
    /// Listening sockets.
    listeners: BTreeMap<u16, TcpListener>,
    /// Next ephemeral port.
    next_ephemeral_port: u16,
}

impl TcpState {
    /// Ephemeral port range start.
    const EPHEMERAL_START: u16 = 49152;
    /// Ephemeral port range end.
    const EPHEMERAL_END: u16 = 65535;
    
    /// Creates new TCP state.
    pub const fn new() -> Self {
        Self {
            connections: BTreeMap::new(),
            listeners: BTreeMap::new(),
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
    
    /// Binds a listener to a port.
    pub fn bind(&mut self, port: u16, local_addr: Ipv4Address) -> Result<(), NetworkError> {
        if self.listeners.contains_key(&port) {
            return Err(NetworkError::AddressInUse);
        }
        
        let listener = TcpListener::new(TcpEndpoint::new(local_addr, port), 128);
        self.listeners.insert(port, listener);
        Ok(())
    }
    
    /// Accepts from a listener.
    pub fn accept(&mut self, port: u16) -> Option<TcpConnection> {
        self.listeners.get_mut(&port)?.accept()
    }
    
    /// Creates a connection.
    pub fn connect(&mut self, local_addr: Ipv4Address, remote: TcpEndpoint) -> TcpConnectionKey {
        let local_port = self.allocate_port();
        let key = TcpConnectionKey {
            local: TcpEndpoint::new(local_addr, local_port),
            remote,
        };
        
        let mut conn = TcpConnection::new(key);
        conn.connect();
        self.connections.insert(key, conn);
        
        key
    }
}

/// Static TCP state.
static TCP_STATE: Mutex<TcpState> = Mutex::new(TcpState::new());

/// Gets TCP state.
pub fn tcp_state() -> &'static Mutex<TcpState> {
    &TCP_STATE
}

/// Handles an incoming TCP packet.
pub fn handle_packet(ip_packet: &Ipv4Packet) {
    let segment = match TcpSegment::parse(&ip_packet.payload) {
        Some(s) => s,
        None => return,
    };
    
    let key = TcpConnectionKey {
        local: TcpEndpoint::new(ip_packet.dest_addr, segment.header.dest_port),
        remote: TcpEndpoint::new(ip_packet.src_addr, segment.header.src_port),
    };
    
    let mut state = TCP_STATE.lock();
    
    // Check for existing connection
    if let Some(conn) = state.connections.get_mut(&key) {
        if let Some(_response) = conn.process(&segment) {
            // Would send response via network interface
        }
        return;
    }
    
    // Check for listening socket
    let dest_port = segment.header.dest_port;
    if let Some(listener) = state.listeners.get_mut(&dest_port) {
        if segment.header.flags & flags::SYN != 0 {
            // New connection
            let mut conn = TcpConnection::new(key);
            conn.listen();
            if let Some(_response) = conn.process(&segment) {
                // Would send SYN-ACK
            }
            listener.backlog.push(conn);
        }
    }
}

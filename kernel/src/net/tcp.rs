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

    /// Gets a mutable reference to a connection.
    pub fn connection_mut(&mut self, key: TcpConnectionKey) -> Option<&mut TcpConnection> {
        self.connections.get_mut(&key)
    }

    /// Gets an immutable reference to a connection.
    pub fn connection(&self, key: TcpConnectionKey) -> Option<&TcpConnection> {
        self.connections.get(&key)
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

// =============================================================================
// Public Module-Level TCP API
// =============================================================================

use super::socket::{SocketAddr, SocketHandle};

/// Socket handle to connection key mapping.
static SOCKET_CONNECTIONS: Mutex<BTreeMap<usize, TcpConnectionKey>> = Mutex::new(BTreeMap::new());

/// Next socket handle ID.
static NEXT_SOCKET_ID: Mutex<usize> = Mutex::new(1);

/// Allocates a new socket handle.
fn allocate_socket_handle() -> SocketHandle {
    let mut next_id = NEXT_SOCKET_ID.lock();
    let handle = SocketHandle(*next_id);
    *next_id += 1;
    handle
}

/// Binds a TCP socket to a local address for listening.
///
/// # Arguments
/// * `addr` - The local address and port to bind to
///
/// # Returns
/// A socket handle on success, or a NetworkError on failure.
pub fn tcp_bind(addr: SocketAddr) -> Result<SocketHandle, NetworkError> {
    let mut state = TCP_STATE.lock();
    state.bind(addr.port, addr.addr)?;
    
    let handle = allocate_socket_handle();
    // For listening sockets, we store the port in the connection map
    // using a special key format
    let listen_key = TcpConnectionKey {
        local: TcpEndpoint::new(addr.addr, addr.port),
        remote: TcpEndpoint::new(Ipv4Address::ANY, 0),
    };
    
    SOCKET_CONNECTIONS.lock().insert(handle.0, listen_key);
    
    Ok(handle)
}

/// Connects to a remote TCP endpoint.
///
/// # Arguments
/// * `addr` - The remote address and port to connect to
///
/// # Returns
/// A socket handle on success, or a NetworkError on failure.
pub fn tcp_connect(addr: SocketAddr) -> Result<SocketHandle, NetworkError> {
    let mut state = TCP_STATE.lock();
    
    // Use default local address (would be configured interface address)
    let local_addr = Ipv4Address([0, 0, 0, 0]); // INADDR_ANY - kernel will select
    let remote = TcpEndpoint::new(addr.addr, addr.port);
    
    let key = state.connect(local_addr, remote);
    
    let handle = allocate_socket_handle();
    SOCKET_CONNECTIONS.lock().insert(handle.0, key);
    
    Ok(handle)
}

/// Sends data over a TCP connection.
///
/// # Arguments
/// * `handle` - The socket handle for the connection
/// * `data` - The data to send
///
/// # Returns
/// The number of bytes sent on success, or a NetworkError on failure.
pub fn tcp_send(handle: SocketHandle, data: &[u8]) -> Result<usize, NetworkError> {
    let connections = SOCKET_CONNECTIONS.lock();
    let key = connections.get(&handle.0)
        .ok_or(NetworkError::InvalidSocket)?;
    let key = *key; // Copy the key before dropping the lock
    drop(connections);
    
    let mut state = TCP_STATE.lock();
    let conn = state.connection_mut(key)
        .ok_or(NetworkError::NotConnected)?;
    
    if conn.state != TcpConnectionState::Established {
        return Err(NetworkError::NotConnected);
    }
    
    // Queue data for sending
    conn.send_buffer.extend_from_slice(data);
    
    // Create segment and transmit it via the network interface
    if let Some(segment) = conn.send(data) {
        // Send the segment through the network stack
        drop(state); // Release lock before calling transmit
        if let Err(e) = super::transmit_tcp_segment(&key, &segment) {
            crate::serial_println!("[tcp] Failed to transmit segment: {:?}", e);
            return Err(e);
        }
    }
    
    Ok(data.len())
}

/// Receives data from a TCP connection.
///
/// # Arguments
/// * `handle` - The socket handle for the connection
/// * `buffer` - The buffer to receive data into
///
/// # Returns
/// The number of bytes received on success, or a NetworkError on failure.
pub fn tcp_recv(handle: SocketHandle, buffer: &mut [u8]) -> Result<usize, NetworkError> {
    let connections = SOCKET_CONNECTIONS.lock();
    let key = connections.get(&handle.0)
        .ok_or(NetworkError::InvalidSocket)?;
    
    let mut state = TCP_STATE.lock();
    let conn = state.connection_mut(*key)
        .ok_or(NetworkError::NotConnected)?;
    
    // Check if connection is in a valid state for receiving
    match conn.state {
        TcpConnectionState::Established |
        TcpConnectionState::FinWait1 |
        TcpConnectionState::FinWait2 |
        TcpConnectionState::CloseWait => {}
        TcpConnectionState::Closed => return Err(NetworkError::NotConnected),
        _ => {}
    }
    
    let bytes_read = conn.read(buffer);
    
    if bytes_read == 0 && conn.state == TcpConnectionState::CloseWait {
        // Connection closed by peer
        return Err(NetworkError::ConnectionClosed);
    }
    
    Ok(bytes_read)
}

/// Closes a TCP connection.
///
/// # Arguments
/// * `handle` - The socket handle to close
///
/// # Returns
/// Ok(()) on success, or a NetworkError on failure.
pub fn tcp_close(handle: SocketHandle) -> Result<(), NetworkError> {
    let mut connections = SOCKET_CONNECTIONS.lock();
    let key = connections.remove(&handle.0)
        .ok_or(NetworkError::InvalidSocket)?;
    drop(connections);
    
    // Check if this is a listening socket (remote port == 0)
    if key.remote.port == 0 {
        // Remove listener
        let mut state = TCP_STATE.lock();
        state.listeners.remove(&key.local.port);
        return Ok(());
    }
    
    // Close the connection and send FIN segment
    let mut state = TCP_STATE.lock();
    if let Some(conn) = state.connection_mut(key) {
        if let Some(fin_segment) = conn.close() {
            // Release lock before transmitting
            drop(state);
            // Send the FIN segment through the network stack
            if let Err(e) = super::transmit_tcp_segment(&key, &fin_segment) {
                crate::serial_println!("[tcp] Failed to send FIN segment: {:?}", e);
                return Err(e);
            }
        }
    }
    
    Ok(())
}

// =============================================================================
// TCP Optimizations (v0.2.0)
// =============================================================================

/// TCP option kinds.
pub mod options {
    pub const END_OF_OPTIONS: u8 = 0;
    pub const NOP: u8 = 1;
    pub const MSS: u8 = 2;
    pub const WINDOW_SCALE: u8 = 3;
    pub const SACK_PERMITTED: u8 = 4;
    pub const SACK: u8 = 5;
    pub const TIMESTAMP: u8 = 8;
}

/// TCP optimization configuration.
#[derive(Debug, Clone)]
pub struct TcpConfig {
    /// Enable Nagle's algorithm (coalesce small segments).
    pub nagle_enabled: bool,
    /// Enable window scaling (RFC 7323).
    pub window_scaling: bool,
    /// Window scale factor (0-14).
    pub window_scale: u8,
    /// Enable SACK (Selective Acknowledgment).
    pub sack_enabled: bool,
    /// Enable timestamps (RFC 7323).
    pub timestamps_enabled: bool,
    /// Maximum Segment Size.
    pub mss: u16,
    /// Initial congestion window (segments).
    pub initial_cwnd: u16,
    /// Slow start threshold (segments).
    pub ssthresh: u32,
    /// Enable fast retransmit on 3 duplicate ACKs.
    pub fast_retransmit: bool,
    /// RTO minimum (ms).
    pub rto_min_ms: u32,
    /// RTO maximum (ms).
    pub rto_max_ms: u32,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            nagle_enabled: true,
            window_scaling: true,
            window_scale: 7, // Allows up to 8MB window
            sack_enabled: true,
            timestamps_enabled: true,
            mss: 1460, // Standard Ethernet MSS
            initial_cwnd: 10,
            ssthresh: 65535,
            fast_retransmit: true,
            rto_min_ms: 200,
            rto_max_ms: 120_000,
        }
    }
}

/// Congestion control state.
#[derive(Debug, Clone)]
pub struct CongestionControl {
    /// Congestion window (bytes).
    pub cwnd: u32,
    /// Slow start threshold (bytes).
    pub ssthresh: u32,
    /// Current state.
    pub state: CongestionState,
    /// Duplicate ACK count.
    pub dup_ack_count: u8,
    /// Bytes in flight (unacknowledged).
    pub bytes_in_flight: u32,
    /// Last RTT measurement (ms).
    pub rtt_ms: u32,
    /// Smoothed RTT (SRTT).
    pub srtt_ms: u32,
    /// RTT variance.
    pub rttvar_ms: u32,
    /// Retransmission timeout (ms).
    pub rto_ms: u32,
}

/// Congestion control states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CongestionState {
    /// Slow start - exponential window growth.
    SlowStart,
    /// Congestion avoidance - linear window growth.
    CongestionAvoidance,
    /// Fast recovery after 3 duplicate ACKs.
    FastRecovery,
}

impl CongestionControl {
    /// Creates new congestion control state.
    pub fn new(config: &TcpConfig) -> Self {
        Self {
            cwnd: config.initial_cwnd as u32 * config.mss as u32,
            ssthresh: config.ssthresh,
            state: CongestionState::SlowStart,
            dup_ack_count: 0,
            bytes_in_flight: 0,
            rtt_ms: 0,
            srtt_ms: 0,
            rttvar_ms: 0,
            rto_ms: 1000, // Initial 1 second RTO
        }
    }

    /// Handles ACK reception.
    pub fn on_ack(&mut self, bytes_acked: u32, mss: u16) {
        self.bytes_in_flight = self.bytes_in_flight.saturating_sub(bytes_acked);
        self.dup_ack_count = 0;

        match self.state {
            CongestionState::SlowStart => {
                // Exponential growth: cwnd += mss for each ACK
                self.cwnd += mss as u32;
                
                if self.cwnd >= self.ssthresh {
                    self.state = CongestionState::CongestionAvoidance;
                }
            }
            CongestionState::CongestionAvoidance => {
                // Linear growth: cwnd += mss * (mss / cwnd) per ACK
                // Simplified: cwnd += mss^2 / cwnd
                self.cwnd += (mss as u32 * mss as u32) / self.cwnd.max(1);
            }
            CongestionState::FastRecovery => {
                // Exit fast recovery
                self.cwnd = self.ssthresh;
                self.state = CongestionState::CongestionAvoidance;
            }
        }
    }

    /// Handles duplicate ACK.
    pub fn on_duplicate_ack(&mut self, mss: u16) -> bool {
        self.dup_ack_count += 1;

        if self.dup_ack_count == 3 && self.state != CongestionState::FastRecovery {
            // Enter fast recovery
            self.ssthresh = (self.cwnd / 2).max(2 * mss as u32);
            self.cwnd = self.ssthresh + 3 * mss as u32;
            self.state = CongestionState::FastRecovery;
            return true; // Trigger fast retransmit
        }

        if self.state == CongestionState::FastRecovery {
            // Inflate window
            self.cwnd += mss as u32;
        }

        false
    }

    /// Handles timeout.
    pub fn on_timeout(&mut self, mss: u16) {
        // Reduce ssthresh
        self.ssthresh = (self.cwnd / 2).max(2 * mss as u32);
        // Reset to slow start
        self.cwnd = mss as u32;
        self.state = CongestionState::SlowStart;
        self.dup_ack_count = 0;
    }

    /// Updates RTT estimate (Jacobson/Karels algorithm).
    pub fn update_rtt(&mut self, measured_rtt_ms: u32, config: &TcpConfig) {
        self.rtt_ms = measured_rtt_ms;

        if self.srtt_ms == 0 {
            // First measurement
            self.srtt_ms = measured_rtt_ms;
            self.rttvar_ms = measured_rtt_ms / 2;
        } else {
            // RTTVAR = (1 - beta) * RTTVAR + beta * |SRTT - R|
            // beta = 1/4
            let delta = if measured_rtt_ms > self.srtt_ms {
                measured_rtt_ms - self.srtt_ms
            } else {
                self.srtt_ms - measured_rtt_ms
            };
            self.rttvar_ms = (3 * self.rttvar_ms + delta) / 4;

            // SRTT = (1 - alpha) * SRTT + alpha * R
            // alpha = 1/8
            self.srtt_ms = (7 * self.srtt_ms + measured_rtt_ms) / 8;
        }

        // RTO = SRTT + max(G, K * RTTVAR)
        // K = 4, G = clock granularity (assume 1ms)
        self.rto_ms = self.srtt_ms + (4 * self.rttvar_ms).max(1);
        self.rto_ms = self.rto_ms.clamp(config.rto_min_ms, config.rto_max_ms);
    }

    /// Checks if can send more data.
    pub fn can_send(&self, bytes: u32) -> bool {
        self.bytes_in_flight + bytes <= self.cwnd
    }
}

/// SACK block (left and right edge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SackBlock {
    pub left: u32,
    pub right: u32,
}

/// SACK state for selective acknowledgments.
#[derive(Debug, Clone, Default)]
pub struct SackState {
    /// Received SACK blocks (from peer).
    pub blocks: Vec<SackBlock>,
    /// Maximum SACK blocks to track.
    pub max_blocks: usize,
}

impl SackState {
    /// Creates new SACK state.
    pub fn new(max_blocks: usize) -> Self {
        Self {
            blocks: Vec::new(),
            max_blocks,
        }
    }

    /// Adds a SACK block.
    pub fn add_block(&mut self, left: u32, right: u32) {
        // Remove overlapping blocks and merge
        self.blocks.retain(|b| {
            !(b.left >= left && b.right <= right) // Remove contained blocks
        });

        self.blocks.push(SackBlock { left, right });

        // Keep only max_blocks most recent
        if self.blocks.len() > self.max_blocks {
            self.blocks.remove(0);
        }

        // Sort by left edge
        self.blocks.sort_by_key(|b| b.left);
    }

    /// Checks if a sequence number is SACKed.
    pub fn is_sacked(&self, seq: u32) -> bool {
        self.blocks.iter().any(|b| seq >= b.left && seq < b.right)
    }

    /// Encodes SACK blocks as TCP option bytes.
    pub fn encode(&self) -> Vec<u8> {
        if self.blocks.is_empty() {
            return Vec::new();
        }

        let mut opts = Vec::with_capacity(2 + self.blocks.len() * 8);
        opts.push(options::SACK);
        opts.push((2 + self.blocks.len() * 8) as u8);

        for block in &self.blocks {
            opts.extend_from_slice(&block.left.to_be_bytes());
            opts.extend_from_slice(&block.right.to_be_bytes());
        }

        opts
    }
}

/// Nagle's algorithm buffer.
#[derive(Debug)]
pub struct NagleBuffer {
    /// Pending data.
    data: Vec<u8>,
    /// MSS for coalescing.
    mss: u16,
    /// Whether there's unacknowledged data.
    has_unacked: bool,
}

impl NagleBuffer {
    /// Creates new Nagle buffer.
    pub fn new(mss: u16) -> Self {
        Self {
            data: Vec::new(),
            mss,
            has_unacked: false,
        }
    }

    /// Adds data to the buffer.
    pub fn push(&mut self, data: &[u8]) {
        self.data.extend_from_slice(data);
    }

    /// Checks if should send now.
    pub fn should_send(&self) -> bool {
        // Send if:
        // 1. Buffer >= MSS (full segment)
        // 2. No unacknowledged data (no waiting for ACK)
        // 3. Buffer has data and PSH flag is set
        self.data.len() >= self.mss as usize || !self.has_unacked
    }

    /// Takes data to send (up to MSS).
    pub fn take(&mut self) -> Vec<u8> {
        let len = self.data.len().min(self.mss as usize);
        let result = self.data[..len].to_vec();
        self.data.drain(..len);
        self.has_unacked = true;
        result
    }

    /// Called when ACK received.
    pub fn on_ack(&mut self) {
        self.has_unacked = false;
    }

    /// Returns pending data length.
    pub fn pending(&self) -> usize {
        self.data.len()
    }
}

/// Parses TCP options from header.
pub fn parse_tcp_options(data: &[u8]) -> TcpOptions {
    let mut opts = TcpOptions::default();
    let mut i = 0;

    while i < data.len() {
        match data[i] {
            options::END_OF_OPTIONS => break,
            options::NOP => i += 1,
            options::MSS if i + 4 <= data.len() => {
                opts.mss = Some(u16::from_be_bytes([data[i + 2], data[i + 3]]));
                i += 4;
            }
            options::WINDOW_SCALE if i + 3 <= data.len() => {
                opts.window_scale = Some(data[i + 2]);
                i += 3;
            }
            options::SACK_PERMITTED if i + 2 <= data.len() => {
                opts.sack_permitted = true;
                i += 2;
            }
            options::SACK if i + 2 <= data.len() => {
                let len = data[i + 1] as usize;
                if i + len <= data.len() {
                    let mut blocks = Vec::new();
                    let mut j = i + 2;
                    while j + 8 <= i + len {
                        let left = u32::from_be_bytes([data[j], data[j + 1], data[j + 2], data[j + 3]]);
                        let right = u32::from_be_bytes([data[j + 4], data[j + 5], data[j + 6], data[j + 7]]);
                        blocks.push(SackBlock { left, right });
                        j += 8;
                    }
                    opts.sack_blocks = blocks;
                }
                i += len;
            }
            options::TIMESTAMP if i + 10 <= data.len() => {
                opts.timestamp = Some(u32::from_be_bytes([data[i + 2], data[i + 3], data[i + 4], data[i + 5]]));
                opts.timestamp_echo = Some(u32::from_be_bytes([data[i + 6], data[i + 7], data[i + 8], data[i + 9]]));
                i += 10;
            }
            _ => {
                // Unknown option - skip using length field
                if i + 1 < data.len() && data[i + 1] > 0 {
                    i += data[i + 1] as usize;
                } else {
                    break;
                }
            }
        }
    }

    opts
}

/// Parsed TCP options.
#[derive(Debug, Clone, Default)]
pub struct TcpOptions {
    pub mss: Option<u16>,
    pub window_scale: Option<u8>,
    pub sack_permitted: bool,
    pub sack_blocks: Vec<SackBlock>,
    pub timestamp: Option<u32>,
    pub timestamp_echo: Option<u32>,
}

/// Builds TCP options for SYN segment.
pub fn build_syn_options(config: &TcpConfig) -> Vec<u8> {
    let mut opts = Vec::with_capacity(20);

    // MSS
    opts.push(options::MSS);
    opts.push(4);
    opts.extend_from_slice(&config.mss.to_be_bytes());

    // Window scale
    if config.window_scaling {
        opts.push(options::NOP);
        opts.push(options::WINDOW_SCALE);
        opts.push(3);
        opts.push(config.window_scale);
    }

    // SACK permitted
    if config.sack_enabled {
        opts.push(options::NOP);
        opts.push(options::NOP);
        opts.push(options::SACK_PERMITTED);
        opts.push(2);
    }

    // Pad to 4-byte boundary
    while opts.len() % 4 != 0 {
        opts.push(options::NOP);
    }

    opts
}

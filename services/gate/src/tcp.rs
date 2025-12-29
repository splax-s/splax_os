//! # TCP Gateway
//!
//! Raw TCP gateway for S-GATE. Handles TCP connections and routes
//! them to internal services via S-LINK.

use alloc::vec::Vec;
use alloc::collections::VecDeque;

use super::{CapabilityToken, ConnectionState, GateError};

/// TCP connection handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TcpConnectionId(pub u64);

/// TCP connection information.
#[derive(Debug, Clone)]
pub struct TcpConnection {
    /// Connection ID
    pub id: TcpConnectionId,
    /// Remote IP address
    pub remote_ip: u32,
    /// Remote port
    pub remote_port: u16,
    /// Local port
    pub local_port: u16,
    /// Connection state
    pub state: ConnectionState,
    /// Bytes received
    pub bytes_received: u64,
    /// Bytes sent
    pub bytes_sent: u64,
    /// Receive buffer for incoming data
    pub rx_buffer: VecDeque<u8>,
    /// Transmit buffer for outgoing data
    pub tx_buffer: VecDeque<u8>,
}

impl TcpConnection {
    /// Create a new connection
    pub fn new(id: TcpConnectionId, remote_ip: u32, remote_port: u16, local_port: u16) -> Self {
        Self {
            id,
            remote_ip,
            remote_port,
            local_port,
            state: ConnectionState::Connected,
            bytes_received: 0,
            bytes_sent: 0,
            rx_buffer: VecDeque::with_capacity(65536), // 64KB receive buffer
            tx_buffer: VecDeque::with_capacity(65536), // 64KB transmit buffer
        }
    }
}

/// TCP gateway operations.
pub struct TcpGateway {
    /// Local port
    port: u16,
    /// Internal service to route to
    internal_service: alloc::string::String,
    /// Active connections
    connections: spin::Mutex<Vec<TcpConnection>>,
    /// Next connection ID
    next_conn_id: spin::Mutex<u64>,
    /// Maximum connections
    max_connections: usize,
}

impl TcpGateway {
    /// Creates a new TCP gateway.
    pub fn new(
        port: u16,
        internal_service: alloc::string::String,
        max_connections: usize,
    ) -> Self {
        Self {
            port,
            internal_service,
            connections: spin::Mutex::new(Vec::new()),
            next_conn_id: spin::Mutex::new(1),
            max_connections,
        }
    }

    /// Accepts a new connection (called by network driver).
    pub fn accept(
        &self,
        remote_ip: u32,
        remote_port: u16,
        _cap_token: &CapabilityToken,
    ) -> Result<TcpConnectionId, GateError> {
        let mut connections = self.connections.lock();

        if connections.len() >= self.max_connections {
            return Err(GateError::ConnectionLimit);
        }

        let mut next_id = self.next_conn_id.lock();
        let id = TcpConnectionId(*next_id);
        *next_id += 1;

        let conn = TcpConnection::new(id, remote_ip, remote_port, self.port);
        connections.push(conn);
        Ok(id)
    }

    /// Sends data on a connection.
    pub fn send(
        &self,
        conn_id: TcpConnectionId,
        data: &[u8],
        _cap_token: &CapabilityToken,
    ) -> Result<usize, GateError> {
        let mut connections = self.connections.lock();
        let conn = connections
            .iter_mut()
            .find(|c| c.id == conn_id)
            .ok_or(GateError::InternalError)?;

        // Check connection state
        if conn.state != ConnectionState::Connected && conn.state != ConnectionState::Active {
            return Err(GateError::InternalError);
        }

        // Copy data to transmit buffer
        let available = 65536 - conn.tx_buffer.len();
        let to_send = core::cmp::min(data.len(), available);
        
        conn.tx_buffer.extend(data[..to_send].iter().copied());
        conn.bytes_sent += to_send as u64;
        conn.state = ConnectionState::Active;
        
        // Trigger network layer to transmit buffered data via S-NET IPC
        flush_tx_buffer(conn);
        
        Ok(to_send)
    }

    /// Receives data from a connection.
    pub fn receive(
        &self,
        conn_id: TcpConnectionId,
        buffer: &mut [u8],
        _cap_token: &CapabilityToken,
    ) -> Result<usize, GateError> {
        let mut connections = self.connections.lock();
        let conn = connections
            .iter_mut()
            .find(|c| c.id == conn_id)
            .ok_or(GateError::InternalError)?;

        // Check connection state
        if conn.state == ConnectionState::Closed {
            return Err(GateError::InternalError);
        }

        // Read available data from receive buffer
        let available = conn.rx_buffer.len();
        let to_read = core::cmp::min(buffer.len(), available);
        
        for i in 0..to_read {
            if let Some(byte) = conn.rx_buffer.pop_front() {
                buffer[i] = byte;
            }
        }
        
        conn.bytes_received += to_read as u64;
        
        // Update state based on buffer
        if conn.rx_buffer.is_empty() && conn.tx_buffer.is_empty() {
            conn.state = ConnectionState::Idle;
        }
        
        Ok(to_read)
    }

    /// Queues incoming data for a connection (called by network driver).
    pub fn queue_received_data(&self, conn_id: TcpConnectionId, data: &[u8]) -> Result<(), GateError> {
        let mut connections = self.connections.lock();
        let conn = connections
            .iter_mut()
            .find(|c| c.id == conn_id)
            .ok_or(GateError::InternalError)?;
        
        conn.rx_buffer.extend(data.iter().copied());
        Ok(())
    }

    /// Closes a connection.
    pub fn close(
        &self,
        conn_id: TcpConnectionId,
        _cap_token: &CapabilityToken,
    ) -> Result<(), GateError> {
        let mut connections = self.connections.lock();
        
        // Mark as closing first, then remove
        if let Some(conn) = connections.iter_mut().find(|c| c.id == conn_id) {
            conn.state = ConnectionState::Closing;
            // Send FIN packet via network stack
            // ... network layer handles TCP teardown
        }
        
        connections.retain(|c| c.id != conn_id);
        Ok(())
    }

    /// Gets connection count.
    pub fn connection_count(&self) -> usize {
        self.connections.lock().len()
    }

    /// Lists active connections.
    pub fn list_connections(&self) -> Vec<TcpConnection> {
        self.connections.lock().clone()
    }
}

// =============================================================================
// LISTENER MANAGEMENT
// =============================================================================

use alloc::collections::BTreeMap;
use spin::Mutex;

/// Global listener registry
static LISTENERS: Mutex<BTreeMap<u16, ListenerInfo>> = Mutex::new(BTreeMap::new());

/// Listener information
struct ListenerInfo {
    addr: u32,
    port: u16,
    backlog: VecDeque<PendingConnection>,
}

/// Pending connection waiting to be accepted
struct PendingConnection {
    remote_ip: u32,
    remote_port: u16,
    syn_received: bool,
}

/// Register a TCP listener on a port
pub fn register_listener(addr: u32, port: u16) -> Result<(), GateError> {
    let mut listeners = LISTENERS.lock();
    
    if listeners.contains_key(&port) {
        return Err(GateError::InternalError); // Port already in use
    }
    
    listeners.insert(port, ListenerInfo {
        addr,
        port,
        backlog: VecDeque::with_capacity(128), // SYN backlog
    });
    
    Ok(())
}

/// Unregister a TCP listener
pub fn unregister_listener(port: u16) {
    let mut listeners = LISTENERS.lock();
    listeners.remove(&port);
}

/// Queue a pending connection (called by network stack on SYN)
pub fn queue_connection(port: u16, remote_ip: u32, remote_port: u16) -> bool {
    let mut listeners = LISTENERS.lock();
    
    if let Some(listener) = listeners.get_mut(&port) {
        if listener.backlog.len() < 128 {
            listener.backlog.push_back(PendingConnection {
                remote_ip,
                remote_port,
                syn_received: true,
            });
            return true;
        }
    }
    
    false // No listener or backlog full
}

/// Flush transmit buffer to network via network stack syscall
fn flush_tx_buffer(conn: &mut TcpConnection) {
    // Take all pending data from transmit buffer
    if conn.tx_buffer.is_empty() {
        return;
    }
    
    // Collect buffer data into contiguous slice for transmission
    let data: Vec<u8> = conn.tx_buffer.iter().copied().collect();
    
    // Build TCP segment header
    let segment = TcpSegment {
        src_port: conn.local_port,
        dst_port: conn.remote_port,
        seq_num: 0, // Would track actual sequence numbers in full impl
        ack_num: 0,
        flags: TCP_FLAG_ACK | TCP_FLAG_PSH,
        window: 65535,
        payload: &data,
    };
    
    // Send via network stack syscall
    let result = net_send_tcp(conn.remote_ip, &segment);
    
    if result >= 0 {
        // Successfully queued for transmission - clear sent data
        let sent = result as usize;
        for _ in 0..sent.min(conn.tx_buffer.len()) {
            conn.tx_buffer.pop_front();
        }
    }
    // On error, data remains in buffer for retry
}

// TCP flags
const TCP_FLAG_ACK: u8 = 0x10;
const TCP_FLAG_PSH: u8 = 0x08;

/// TCP segment for transmission
struct TcpSegment<'a> {
    src_port: u16,
    dst_port: u16,
    seq_num: u32,
    ack_num: u32,
    flags: u8,
    window: u16,
    payload: &'a [u8],
}

/// Send TCP segment via network stack syscall
fn net_send_tcp(remote_ip: u32, segment: &TcpSegment) -> i64 {
    // Build raw TCP packet
    let header_len = 20; // Minimum TCP header
    let total_len = header_len + segment.payload.len();
    let mut packet = Vec::with_capacity(total_len);
    
    // TCP header
    packet.extend_from_slice(&segment.src_port.to_be_bytes());
    packet.extend_from_slice(&segment.dst_port.to_be_bytes());
    packet.extend_from_slice(&segment.seq_num.to_be_bytes());
    packet.extend_from_slice(&segment.ack_num.to_be_bytes());
    packet.push((5 << 4) | 0); // Data offset (5 words) + reserved
    packet.push(segment.flags);
    packet.extend_from_slice(&segment.window.to_be_bytes());
    packet.extend_from_slice(&[0u8; 2]); // Checksum (calculated by kernel)
    packet.extend_from_slice(&[0u8; 2]); // Urgent pointer
    packet.extend_from_slice(segment.payload);
    
    // Network send syscall
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let result: i64;
        core::arch::asm!(
            "syscall",
            in("rax") 44u64,  // sendto syscall
            in("rdi") 0u64,   // socket fd (would be actual fd)
            in("rsi") packet.as_ptr() as u64,
            in("rdx") packet.len() as u64,
            in("r10") 0u64,   // flags
            in("r8") remote_ip as u64,
            in("r9") 0u64,    // addr len
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
        return result;
    }
    
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let result: i64;
        core::arch::asm!(
            "svc #0",
            in("x8") 206u64,  // sendto syscall
            in("x0") 0u64,    // socket fd
            in("x1") packet.as_ptr() as u64,
            in("x2") packet.len() as u64,
            in("x3") 0u64,    // flags
            in("x4") remote_ip as u64,
            in("x5") 0u64,    // addr len
            lateout("x0") result,
            options(nostack)
        );
        return result;
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        let _ = (remote_ip, packet);
        -1
    }
}

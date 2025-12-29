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
        
        // Trigger network layer to transmit buffered data
        // This would be done via network stack syscall in production
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

/// Flush transmit buffer to network (stub for network stack integration)
fn flush_tx_buffer(conn: &mut TcpConnection) {
    // In a complete implementation, this would:
    // 1. Take data from tx_buffer
    // 2. Build TCP segments with proper headers
    // 3. Send via network stack
    // 
    // For now, data stays in buffer until network driver polls
    let _ = conn;
}

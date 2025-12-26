//! # TCP Gateway
//!
//! Raw TCP gateway for S-GATE. Handles TCP connections and routes
//! them to internal services via S-LINK.

use alloc::vec::Vec;

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

        let conn = TcpConnection {
            id,
            remote_ip,
            remote_port,
            local_port: self.port,
            state: ConnectionState::Connected,
            bytes_received: 0,
            bytes_sent: 0,
        };

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

        // In a real implementation, this would write to the network
        conn.bytes_sent += data.len() as u64;
        Ok(data.len())
    }

    /// Receives data from a connection.
    pub fn receive(
        &self,
        conn_id: TcpConnectionId,
        _buffer: &mut [u8],
        _cap_token: &CapabilityToken,
    ) -> Result<usize, GateError> {
        let connections = self.connections.lock();
        let _conn = connections
            .iter()
            .find(|c| c.id == conn_id)
            .ok_or(GateError::InternalError)?;

        // In a real implementation, this would read from the network
        Ok(0)
    }

    /// Closes a connection.
    pub fn close(
        &self,
        conn_id: TcpConnectionId,
        _cap_token: &CapabilityToken,
    ) -> Result<(), GateError> {
        let mut connections = self.connections.lock();
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

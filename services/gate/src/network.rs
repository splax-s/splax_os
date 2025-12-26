//! # Network Stack Integration for S-GATE
//!
//! This module bridges the kernel's network stack with the S-GATE service,
//! allowing external TCP/HTTP connections to reach internal services.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use super::{CapabilityToken, GatewayConfig, GatewayId, GatewayStats, Protocol};
use super::tcp::{TcpConnection, TcpConnectionId, TcpGateway};

/// Network socket handle from kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KernelSocket(pub usize);

/// Network listener for S-GATE.
///
/// This wraps the kernel's socket abstraction and provides a higher-level
/// interface for S-GATE to accept connections.
pub struct NetworkListener {
    /// Bound port.
    port: u16,
    /// Kernel socket handle.
    socket: Option<KernelSocket>,
    /// Whether listener is active.
    active: bool,
}

impl NetworkListener {
    /// Creates a new listener.
    pub fn new(port: u16) -> Self {
        Self {
            port,
            socket: None,
            active: false,
        }
    }
    
    /// Binds and starts listening.
    pub fn bind(&mut self) -> Result<(), NetworkError> {
        // In a full implementation, this would:
        // 1. Call kernel net::socket::socket(SocketType::Stream)
        // 2. Call kernel net::socket::bind(handle, SocketAddr::any(port))
        // 3. Call kernel net::socket::listen(handle, backlog)
        
        self.socket = Some(KernelSocket(self.port as usize));
        self.active = true;
        Ok(())
    }
    
    /// Accepts a connection.
    pub fn accept(&self) -> Result<NetworkConnection, NetworkError> {
        if !self.active {
            return Err(NetworkError::NotListening);
        }
        
        // In a full implementation, this would call kernel net::socket::accept()
        Err(NetworkError::WouldBlock)
    }
    
    /// Closes the listener.
    pub fn close(&mut self) {
        self.socket = None;
        self.active = false;
    }
}

/// Network connection for S-GATE.
pub struct NetworkConnection {
    /// Remote IP address.
    pub remote_addr: u32,
    /// Remote port.
    pub remote_port: u16,
    /// Local port.
    pub local_port: u16,
    /// Kernel socket handle.
    socket: KernelSocket,
    /// Receive buffer.
    recv_buffer: Vec<u8>,
    /// Send buffer.
    send_buffer: Vec<u8>,
}

impl NetworkConnection {
    /// Creates a new connection.
    pub fn new(
        remote_addr: u32,
        remote_port: u16,
        local_port: u16,
        socket: KernelSocket,
    ) -> Self {
        Self {
            remote_addr,
            remote_port,
            local_port,
            socket,
            recv_buffer: Vec::new(),
            send_buffer: Vec::new(),
        }
    }
    
    /// Sends data.
    pub fn send(&mut self, data: &[u8]) -> Result<usize, NetworkError> {
        // Queue data for transmission
        self.send_buffer.extend_from_slice(data);
        
        // In a full implementation, this would call kernel net::socket::send()
        let sent = data.len();
        self.send_buffer.clear();
        Ok(sent)
    }
    
    /// Receives data.
    pub fn recv(&mut self, buffer: &mut [u8]) -> Result<usize, NetworkError> {
        // In a full implementation, this would call kernel net::socket::recv()
        if self.recv_buffer.is_empty() {
            Err(NetworkError::WouldBlock)
        } else {
            let len = buffer.len().min(self.recv_buffer.len());
            buffer[..len].copy_from_slice(&self.recv_buffer[..len]);
            self.recv_buffer.drain(..len);
            Ok(len)
        }
    }
    
    /// Closes the connection.
    pub fn close(self) {
        // In a full implementation, this would call kernel net::socket::close()
    }
}

/// Network error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkError {
    /// Socket not bound.
    NotBound,
    /// Not listening.
    NotListening,
    /// Would block (non-blocking mode).
    WouldBlock,
    /// Connection refused.
    ConnectionRefused,
    /// Connection reset.
    ConnectionReset,
    /// Connection timed out.
    TimedOut,
    /// Address in use.
    AddressInUse,
    /// Network unreachable.
    NetworkUnreachable,
    /// Internal error.
    InternalError,
}

/// Integrated gateway that uses the kernel network stack.
pub struct IntegratedGateway {
    /// Gateway ID.
    id: GatewayId,
    /// Configuration.
    config: GatewayConfig,
    /// Network listener.
    listener: Mutex<Option<NetworkListener>>,
    /// Active connections.
    connections: Mutex<BTreeMap<TcpConnectionId, NetworkConnection>>,
    /// Next connection ID.
    next_conn_id: Mutex<u64>,
    /// Statistics.
    stats: Mutex<GatewayStats>,
}

impl IntegratedGateway {
    /// Creates a new integrated gateway.
    pub fn new(id: GatewayId, config: GatewayConfig) -> Self {
        Self {
            id,
            config,
            listener: Mutex::new(None),
            connections: Mutex::new(BTreeMap::new()),
            next_conn_id: Mutex::new(1),
            stats: Mutex::new(GatewayStats::default()),
        }
    }
    
    /// Starts the gateway.
    pub fn start(&self) -> Result<(), NetworkError> {
        let mut listener = NetworkListener::new(self.config.external_port);
        listener.bind()?;
        
        *self.listener.lock() = Some(listener);
        Ok(())
    }
    
    /// Stops the gateway.
    pub fn stop(&self) {
        if let Some(mut listener) = self.listener.lock().take() {
            listener.close();
        }
        
        // Close all connections
        let mut connections = self.connections.lock();
        connections.clear();
    }
    
    /// Polls for new connections and data.
    pub fn poll(&self) {
        // Accept new connections
        if let Some(ref listener) = *self.listener.lock() {
            while let Ok(conn) = listener.accept() {
                let mut next_id = self.next_conn_id.lock();
                let id = TcpConnectionId(*next_id);
                *next_id += 1;
                
                self.connections.lock().insert(id, conn);
                self.stats.lock().total_connections += 1;
            }
        }
        
        // Update active connection count
        self.stats.lock().active_connections = self.connections.lock().len();
    }
    
    /// Gets gateway ID.
    pub fn id(&self) -> GatewayId {
        self.id
    }
    
    /// Gets statistics.
    pub fn stats(&self) -> GatewayStats {
        self.stats.lock().clone()
    }
}

/// Global gateway manager using kernel network stack.
pub struct NetworkGatewayManager {
    /// Active gateways.
    gateways: Mutex<BTreeMap<GatewayId, IntegratedGateway>>,
    /// Next gateway ID.
    next_id: Mutex<u64>,
}

impl NetworkGatewayManager {
    /// Creates a new manager.
    pub const fn new() -> Self {
        Self {
            gateways: Mutex::new(BTreeMap::new()),
            next_id: Mutex::new(1),
        }
    }
    
    /// Creates a new gateway.
    pub fn create(&self, config: GatewayConfig) -> Result<GatewayId, NetworkError> {
        let mut next_id = self.next_id.lock();
        let id = GatewayId(*next_id);
        *next_id += 1;
        
        let gateway = IntegratedGateway::new(id, config);
        gateway.start()?;
        
        self.gateways.lock().insert(id, gateway);
        Ok(id)
    }
    
    /// Destroys a gateway.
    pub fn destroy(&self, id: GatewayId) -> Result<(), NetworkError> {
        if let Some(gateway) = self.gateways.lock().remove(&id) {
            gateway.stop();
            Ok(())
        } else {
            Err(NetworkError::InternalError)
        }
    }
    
    /// Polls all gateways.
    pub fn poll_all(&self) {
        for gateway in self.gateways.lock().values() {
            gateway.poll();
        }
    }
}

static GATEWAY_MANAGER: NetworkGatewayManager = NetworkGatewayManager::new();

/// Gets the global gateway manager.
pub fn gateway_manager() -> &'static NetworkGatewayManager {
    &GATEWAY_MANAGER
}

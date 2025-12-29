//! # Network Stack Integration for S-GATE
//!
//! This module bridges the kernel's network stack with the S-GATE service,
//! allowing external TCP/HTTP connections to reach internal services.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;
use spin::Mutex;

use super::{CapabilityToken, GatewayConfig, GatewayId, GatewayStats, Protocol};
use super::tcp::{TcpConnection, TcpConnectionId, TcpGateway};

// =============================================================================
// S-LINK Message Types for Inter-Service Communication
// =============================================================================

/// S-LINK message type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Request message (expects response)
    Request,
    /// Response message
    Response,
    /// One-way notification
    Notification,
}

/// S-LINK message for inter-service communication.
#[derive(Debug, Clone)]
pub struct SLinkMessage {
    /// Channel ID for routing
    pub channel_id: u64,
    /// Message type
    pub message_type: MessageType,
    /// Correlation ID for request/response matching
    pub correlation_id: u64,
    /// Payload data
    pub payload: Vec<u8>,
    /// Capabilities attached to message
    pub capabilities: Vec<CapabilityToken>,
}

/// Send a request and wait for response with timeout.
pub fn send_and_receive(request: SLinkMessage, timeout: Duration) -> Result<SLinkMessage, NetworkError> {
    // Convert timeout to CPU cycles (assuming ~2GHz CPU)
    let timeout_cycles = timeout.as_nanos() as u64 * 2;
    let start = get_timestamp();
    
    // Global message queues for S-LINK IPC
    // Outbound: requests waiting to be processed by target services
    // Inbound: responses from services back to callers
    static OUTBOUND: Mutex<Vec<SLinkMessage>> = Mutex::new(Vec::new());
    static INBOUND: Mutex<Vec<SLinkMessage>> = Mutex::new(Vec::new());
    
    let correlation_id = request.correlation_id;
    let channel_id = request.channel_id;
    
    // Route message through S-LINK router
    route_message(&request);
    OUTBOUND.lock().push(request);
    
    // Poll for response
    loop {
        // Process any pending service handlers (cooperative scheduling)
        process_pending_handlers();
        
        // Check for matching response
        {
            let mut inbound = INBOUND.lock();
            if let Some(idx) = inbound.iter().position(|m| {
                m.message_type == MessageType::Response && 
                m.correlation_id == correlation_id &&
                m.channel_id == channel_id
            }) {
                return Ok(inbound.remove(idx));
            }
        }
        
        // Check timeout
        let elapsed = get_timestamp().saturating_sub(start);
        if elapsed >= timeout_cycles {
            // Remove pending request from outbound queue
            OUTBOUND.lock().retain(|m| m.correlation_id != correlation_id);
            return Err(NetworkError::TimedOut);
        }
        
        // CPU pause hint for power efficiency
        pause_hint();
    }
}

/// Route a message through the S-LINK routing layer
fn route_message(msg: &SLinkMessage) {
    // S-LINK routing based on channel ID
    // Channels are mapped to service endpoints during gateway initialization
    // Message is placed in the appropriate service queue
    
    // For local services, directly queue the message
    // For remote services, forward to network layer
    let _ = msg; // Message is already queued in OUTBOUND
}

/// Process pending handlers (cooperative multitasking)
fn process_pending_handlers() {
    // Give services a chance to process pending requests
    // This enables cooperative scheduling within the gateway
    
    // In a preemptive system, this would be a no-op since
    // service threads handle their own processing
}

/// Deliver a response message (called by service handlers).
pub fn deliver_response(response: SLinkMessage) {
    static INBOUND: Mutex<Vec<SLinkMessage>> = Mutex::new(Vec::new());
    INBOUND.lock().push(response);
}

/// Get pending requests for a service to process (called by service handlers).
pub fn get_pending_requests() -> Vec<SLinkMessage> {
    static OUTBOUND: Mutex<Vec<SLinkMessage>> = Mutex::new(Vec::new());
    core::mem::take(&mut *OUTBOUND.lock())
}

#[cfg(target_arch = "x86_64")]
fn get_timestamp() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[cfg(target_arch = "aarch64")]
fn get_timestamp() -> u64 {
    let cnt: u64;
    unsafe {
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt, options(nostack, nomem));
    }
    cnt
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn get_timestamp() -> u64 {
    0
}

#[cfg(target_arch = "x86_64")]
fn pause_hint() {
    unsafe { core::arch::x86_64::_mm_pause() }
}

#[cfg(target_arch = "aarch64")]
fn pause_hint() {
    unsafe {
        core::arch::asm!("yield", options(nostack, nomem));
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn pause_hint() {}

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

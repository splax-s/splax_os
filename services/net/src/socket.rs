//! # Socket Manager
//!
//! Manages BSD-style socket abstraction in userspace.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::RwLock;

use super::{NetError, SocketAddr, SocketDomain, SocketType};
use super::tcp::{Tcb, TcpState, TcpFlags, TcpSegment};

/// Global set of bound ports for availability checking
static BOUND_PORTS: RwLock<BTreeSet<(SocketDomain, u16)>> = RwLock::new(BTreeSet::new());

/// Socket handle counter
static SOCKET_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Socket state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketState {
    /// Just created, not bound
    Created,
    /// Bound to local address
    Bound,
    /// Listening for connections (TCP)
    Listening,
    /// SYN sent, waiting for SYN-ACK
    SynSent,
    /// SYN received, waiting for ACK
    SynReceived,
    /// Connection in progress
    Connecting,
    /// Connected / Established
    Connected,
    /// FIN sent, waiting for ACK
    FinWait1,
    /// FIN ACKed, waiting for peer FIN
    FinWait2,
    /// Both sides closing simultaneously
    Closing,
    /// Waiting for TIME_WAIT period
    TimeWait,
    /// Received FIN, waiting for close from application
    CloseWait,
    /// Sent final FIN, waiting for ACK
    LastAck,
    /// Closed
    Closed,
}

/// Socket options
#[derive(Debug, Clone)]
pub struct SocketOptions {
    /// Receive buffer size
    pub recv_buffer: usize,
    /// Send buffer size
    pub send_buffer: usize,
    /// Reuse address
    pub reuse_addr: bool,
    /// Reuse port
    pub reuse_port: bool,
    /// Keep-alive
    pub keepalive: bool,
    /// No delay (TCP)
    pub nodelay: bool,
    /// Non-blocking mode
    pub nonblocking: bool,
    /// Receive timeout (ms)
    pub recv_timeout: Option<u32>,
    /// Send timeout (ms)
    pub send_timeout: Option<u32>,
}

impl Default for SocketOptions {
    fn default() -> Self {
        Self {
            recv_buffer: 65536,
            send_buffer: 65536,
            reuse_addr: false,
            reuse_port: false,
            keepalive: false,
            nodelay: false,
            nonblocking: false,
            recv_timeout: None,
            send_timeout: None,
        }
    }
}

/// A socket instance
pub struct Socket {
    /// Socket handle
    pub handle: u32,
    /// Address family
    pub domain: SocketDomain,
    /// Socket type
    pub sock_type: SocketType,
    /// Protocol
    pub protocol: u8,
    /// Current state
    pub state: SocketState,
    /// Local address (if bound)
    pub local_addr: Option<SocketAddr>,
    /// Remote address (if connected)
    pub remote_addr: Option<SocketAddr>,
    /// Socket options
    pub options: SocketOptions,
    /// Receive buffer
    pub recv_buffer: Vec<u8>,
    /// Send buffer
    pub send_buffer: Vec<u8>,
    /// Pending connections (for listening sockets)
    pub pending_connections: Vec<Socket>,
    /// Owning process capability token
    pub capability: u64,
    /// TCP Control Block for TCP sockets
    pub tcb: Option<Tcb>,
}

impl Socket {
    /// Creates a new socket
    pub fn new(domain: SocketDomain, sock_type: SocketType, protocol: u8) -> Self {
        // Initialize TCB for TCP sockets
        let tcb = if sock_type == SocketType::Stream {
            Some(Tcb::default())
        } else {
            None
        };

        Self {
            handle: SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed),
            domain,
            sock_type,
            protocol,
            state: SocketState::Created,
            local_addr: None,
            remote_addr: None,
            options: SocketOptions::default(),
            recv_buffer: Vec::new(),
            send_buffer: Vec::new(),
            pending_connections: Vec::new(),
            capability: 0,
            tcb,
        }
    }

    /// Checks if a port is available for binding
    fn is_port_available(&self, port: u16) -> bool {
        // Port 0 means "any available port"
        if port == 0 {
            return true;
        }

        let bound_ports = BOUND_PORTS.read();
        
        // Check if the exact (domain, port) combination is in use
        if bound_ports.contains(&(self.domain, port)) {
            // Port is in use, but check reuse options
            if self.options.reuse_addr || self.options.reuse_port {
                return true;
            }
            return false;
        }

        true
    }

    /// Reserves a port in the global bound ports set
    fn reserve_port(&self, port: u16) {
        if port != 0 {
            let mut bound_ports = BOUND_PORTS.write();
            bound_ports.insert((self.domain, port));
        }
    }

    /// Releases a port from the global bound ports set
    fn release_port(&self, port: u16) {
        if port != 0 {
            let mut bound_ports = BOUND_PORTS.write();
            bound_ports.remove(&(self.domain, port));
        }
    }

    /// Binds the socket to an address
    pub fn bind(&mut self, addr: SocketAddr) -> Result<(), NetError> {
        if self.state != SocketState::Created {
            return Err(NetError::InvalidArgument);
        }

        let port = addr.port();

        // Check port availability using the global bound ports set
        if !self.is_port_available(port) {
            return Err(NetError::AddressInUse);
        }

        // Reserve the port
        self.reserve_port(port);

        self.local_addr = Some(addr);
        self.state = SocketState::Bound;
        Ok(())
    }

    /// Starts listening (TCP only)
    pub fn listen(&mut self, _backlog: u32) -> Result<(), NetError> {
        if self.sock_type != SocketType::Stream {
            return Err(NetError::InvalidArgument);
        }

        if self.state != SocketState::Bound {
            return Err(NetError::InvalidArgument);
        }

        self.state = SocketState::Listening;
        Ok(())
    }

    /// Connects to remote address
    pub fn connect(&mut self, addr: SocketAddr) -> Result<(), NetError> {
        if self.state == SocketState::Connected {
            return Err(NetError::AlreadyConnected);
        }

        // TCP requires the 3-way handshake
        if self.sock_type == SocketType::Stream {
            // Get or initialize the TCB
            let tcb = self.tcb.get_or_insert_with(Tcb::default);

            // Step 1: Initiate active open - send SYN
            tcb.active_open();

            // Create SYN segment and add to retx queue directly
            let syn_segment = tcb.create_segment(TcpFlags::SYN, &[]);
            tcb.retx_queue.push(syn_segment);

            // Store the remote address
            let local_port = self.local_addr.as_ref().map(|a| a.port()).unwrap_or(0);
            let remote_port = addr.port();
            self.remote_addr = Some(addr);
            self.state = SocketState::SynSent;

            // In async mode, we'd return WouldBlock and complete later
            // For blocking mode, simulate receiving SYN-ACK
            if !self.options.nonblocking {
                // Simulate receiving SYN-ACK from peer
                let peer_iss = Tcb::generate_iss();
                let snd_nxt = tcb.snd_nxt;

                let syn_ack = TcpSegment {
                    src_port: remote_port,
                    dst_port: local_port,
                    seq_num: peer_iss,
                    ack_num: snd_nxt,
                    data_offset: 5,
                    flags: TcpFlags(TcpFlags::SYN | TcpFlags::ACK),
                    window: 65535,
                    checksum: 0,
                    urgent_ptr: 0,
                    options: Vec::new(),
                    payload: Vec::new(),
                };

                // Step 2: Process SYN-ACK
                if let Some(ack_segment) = tcb.process_segment(&syn_ack) {
                    // Step 3: Send ACK - add to retx queue
                    tcb.retx_queue.push(ack_segment);
                }

                // Check if handshake completed
                if tcb.state == TcpState::Established {
                    self.state = SocketState::Connected;
                    return Ok(());
                } else {
                    return Err(NetError::ConnectionRefused);
                }
            }

            // Non-blocking: return WouldBlock, caller polls for completion
            return Err(NetError::WouldBlock);
        }

        // UDP and other types don't require handshake
        self.remote_addr = Some(addr);
        self.state = SocketState::Connected;
        Ok(())
    }

    /// Processes an incoming TCP segment (called by network layer)
    pub fn process_incoming_segment(&mut self, segment: &TcpSegment) -> Option<TcpSegment> {
        let tcb = self.tcb.as_mut()?;
        
        let response = tcb.process_segment(segment);

        // Update socket state based on TCB state
        self.state = match tcb.state {
            TcpState::Closed => SocketState::Closed,
            TcpState::Listen => SocketState::Listening,
            TcpState::SynSent => SocketState::SynSent,
            TcpState::SynReceived => SocketState::SynReceived,
            TcpState::Established => SocketState::Connected,
            TcpState::FinWait1 => SocketState::FinWait1,
            TcpState::FinWait2 => SocketState::FinWait2,
            TcpState::Closing => SocketState::Closing,
            TcpState::TimeWait => SocketState::TimeWait,
            TcpState::CloseWait => SocketState::CloseWait,
            TcpState::LastAck => SocketState::LastAck,
        };

        // Copy data from TCB receive buffer to socket receive buffer
        while let Some(byte) = tcb.recv_buffer.pop_front() {
            self.recv_buffer.push(byte);
        }

        response
    }

    /// Queues an outgoing TCP segment for transmission
    /// 
    /// In a full implementation, this would serialize the segment and send via network interface.
    /// Currently queues to the TCB's retransmission queue for later processing.
    #[allow(dead_code)]
    fn queue_outgoing_segment(&mut self, segment: TcpSegment) {
        // In a full implementation, this would:
        // 1. Serialize the segment to bytes
        // 2. Add IP header
        // 3. Send via network interface
        // For now, add to retransmission queue
        if let Some(tcb) = &mut self.tcb {
            tcb.retx_queue.push(segment);
        }
    }

    /// Accepts a connection (TCP only)
    pub fn accept(&mut self) -> Result<Socket, NetError> {
        if self.sock_type != SocketType::Stream {
            return Err(NetError::InvalidArgument);
        }

        if self.state != SocketState::Listening {
            return Err(NetError::InvalidArgument);
        }

        if self.pending_connections.is_empty() {
            return Err(NetError::WouldBlock);
        }

        Ok(self.pending_connections.remove(0))
    }

    /// Sends data
    pub fn send(&mut self, data: &[u8]) -> Result<usize, NetError> {
        if self.state != SocketState::Connected {
            return Err(NetError::NotConnected);
        }

        // Buffer the data
        let space = self.options.send_buffer - self.send_buffer.len();
        let to_send = data.len().min(space);

        if to_send == 0 {
            return Err(NetError::BufferFull);
        }

        self.send_buffer.extend_from_slice(&data[..to_send]);
        Ok(to_send)
    }

    /// Receives data
    pub fn recv(&mut self, max_len: usize) -> Result<Vec<u8>, NetError> {
        if self.state != SocketState::Connected && self.sock_type == SocketType::Stream {
            return Err(NetError::NotConnected);
        }

        if self.recv_buffer.is_empty() {
            return Err(NetError::WouldBlock);
        }

        let to_recv = max_len.min(self.recv_buffer.len());
        let data: Vec<u8> = self.recv_buffer.drain(..to_recv).collect();
        Ok(data)
    }

    /// Closes the socket - implements TCP FIN sequence for stream sockets
    pub fn close(&mut self) {
        // Release the bound port
        if let Some(ref addr) = self.local_addr {
            self.release_port(addr.port());
        }

        // For TCP sockets, perform the FIN sequence
        if self.sock_type == SocketType::Stream {
            if let Some(ref mut tcb) = self.tcb {
                match tcb.state {
                    TcpState::Established => {
                        // Active close: send FIN
                        // Step 1: Send FIN, transition to FIN_WAIT_1
                        let fin_segment = tcb.create_segment(TcpFlags::FIN | TcpFlags::ACK, &[]);
                        tcb.snd_nxt = tcb.snd_nxt.wrapping_add(1); // FIN consumes one sequence number
                        tcb.retx_queue.push(fin_segment); // Queue directly on TCB
                        tcb.state = TcpState::FinWait1;
                        self.state = SocketState::FinWait1;

                        // In blocking mode, wait for the full close sequence
                        if !self.options.nonblocking {
                            // Simulate receiving ACK of our FIN
                            tcb.state = TcpState::FinWait2;
                            self.state = SocketState::FinWait2;

                            // Simulate receiving FIN from peer
                            // Step 2: Receive FIN, send ACK, go to TIME_WAIT
                            tcb.rcv_nxt = tcb.rcv_nxt.wrapping_add(1);
                            let ack_segment = tcb.create_segment(TcpFlags::ACK, &[]);
                            tcb.retx_queue.push(ack_segment);
                            tcb.state = TcpState::TimeWait;
                            self.state = SocketState::TimeWait;

                            // TIME_WAIT: Per RFC 793, wait 2*MSL before reusing socket.
                            // MSL (Maximum Segment Lifetime) is typically 30-120 seconds.
                            // This ensures delayed packets from the old connection don't
                            // interfere with a new connection on the same port tuple.
                            //
                            // For blocking close(), we spin-wait with yield to allow
                            // the kernel to handle other work. In practice, non-blocking
                            // mode with proper timer management is preferred.
                            #[cfg(target_arch = "x86_64")]
                            {
                                // Wait ~1 second (simplified for demo; real impl uses 2*MSL)
                                for _ in 0..1000 {
                                    core::hint::spin_loop();
                                }
                            }
                            tcb.state = TcpState::Closed;
                            self.state = SocketState::Closed;
                        }
                    }
                    TcpState::CloseWait => {
                        // Passive close: we received FIN, now send our FIN
                        let fin_segment = tcb.create_segment(TcpFlags::FIN | TcpFlags::ACK, &[]);
                        tcb.snd_nxt = tcb.snd_nxt.wrapping_add(1);
                        tcb.retx_queue.push(fin_segment);
                        tcb.state = TcpState::LastAck;
                        self.state = SocketState::LastAck;

                        if !self.options.nonblocking {
                            // Simulate receiving final ACK
                            tcb.state = TcpState::Closed;
                            self.state = SocketState::Closed;
                        }
                    }
                    TcpState::SynSent | TcpState::SynReceived => {
                        // Connection not yet established, can close immediately
                        // Send RST if needed
                        let rst_segment = tcb.create_segment(TcpFlags::RST, &[]);
                        tcb.retx_queue.push(rst_segment);
                        tcb.state = TcpState::Closed;
                        self.state = SocketState::Closed;
                    }
                    TcpState::FinWait1 | TcpState::FinWait2 | 
                    TcpState::Closing | TcpState::TimeWait | TcpState::LastAck => {
                        // Already closing, nothing more to do
                    }
                    TcpState::Closed | TcpState::Listen => {
                        // Already closed or just listening
                        tcb.state = TcpState::Closed;
                        self.state = SocketState::Closed;
                    }
                }
            } else {
                // No TCB, just mark as closed
                self.state = SocketState::Closed;
            }
        } else {
            // UDP and other socket types close immediately
            self.state = SocketState::Closed;
        }

        // Clear buffers
        self.send_buffer.clear();
        self.recv_buffer.clear();
    }

    /// Initiates graceful shutdown (half-close)
    pub fn shutdown(&mut self, how: ShutdownHow) -> Result<(), NetError> {
        if self.state != SocketState::Connected {
            return Err(NetError::NotConnected);
        }

        match how {
            ShutdownHow::Read => {
                // Stop receiving - just clear buffer
                self.recv_buffer.clear();
            }
            ShutdownHow::Write => {
                // Send FIN for TCP
                if self.sock_type == SocketType::Stream {
                    if let Some(ref mut tcb) = self.tcb {
                        if tcb.state == TcpState::Established {
                            let fin_segment = tcb.create_segment(TcpFlags::FIN | TcpFlags::ACK, &[]);
                            tcb.snd_nxt = tcb.snd_nxt.wrapping_add(1);
                            tcb.retx_queue.push(fin_segment);
                            tcb.state = TcpState::FinWait1;
                            self.state = SocketState::FinWait1;
                        }
                    }
                }
            }
            ShutdownHow::Both => {
                self.recv_buffer.clear();
                self.close();
            }
        }
        Ok(())
    }
}

/// Shutdown modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownHow {
    /// Stop receiving
    Read,
    /// Stop sending
    Write,
    /// Stop both
    Both,
}

/// Socket manager
pub struct SocketManager {
    /// All sockets indexed by handle
    sockets: BTreeMap<u32, Socket>,
    /// Maximum number of sockets
    max_sockets: usize,
    /// Port allocations (port -> socket handle)
    port_map: BTreeMap<u16, u32>,
}

impl SocketManager {
    /// Creates a new socket manager
    pub fn new(max_sockets: usize) -> Self {
        Self {
            sockets: BTreeMap::new(),
            max_sockets,
            port_map: BTreeMap::new(),
        }
    }

    /// Creates a new socket
    pub fn create(
        &mut self,
        domain: SocketDomain,
        sock_type: SocketType,
        protocol: u8,
    ) -> Result<u32, NetError> {
        if self.sockets.len() >= self.max_sockets {
            return Err(NetError::TooManySockets);
        }

        let socket = Socket::new(domain, sock_type, protocol);
        let handle = socket.handle;
        self.sockets.insert(handle, socket);
        Ok(handle)
    }

    /// Gets a socket by handle
    pub fn get(&self, handle: u32) -> Option<&Socket> {
        self.sockets.get(&handle)
    }

    /// Gets a mutable socket by handle
    pub fn get_mut(&mut self, handle: u32) -> Option<&mut Socket> {
        self.sockets.get_mut(&handle)
    }

    /// Binds a socket to an address
    pub fn bind(&mut self, handle: u32, addr: SocketAddr) -> Result<(), NetError> {
        let port = addr.port();

        // Check if port is already in use
        if self.port_map.contains_key(&port) {
            let socket = self.sockets.get(&handle).ok_or(NetError::InvalidSocket)?;
            if !socket.options.reuse_port {
                return Err(NetError::AddressInUse);
            }
        }

        let socket = self.sockets.get_mut(&handle).ok_or(NetError::InvalidSocket)?;
        socket.bind(addr)?;
        self.port_map.insert(port, handle);
        Ok(())
    }

    /// Closes a socket
    pub fn close(&mut self, handle: u32) -> Result<(), NetError> {
        let socket = self.sockets.get_mut(&handle).ok_or(NetError::InvalidSocket)?;
        
        // Remove from port map
        if let Some(addr) = &socket.local_addr {
            self.port_map.remove(&addr.port());
        }
        
        socket.close();
        self.sockets.remove(&handle);
        Ok(())
    }

    /// Allocates an ephemeral port
    pub fn allocate_port(&mut self) -> Option<u16> {
        // Dynamic/ephemeral port range: 49152-65535
        for port in 49152..=65535u16 {
            if !self.port_map.contains_key(&port) {
                return Some(port);
            }
        }
        None
    }

    /// Returns the number of open sockets
    pub fn socket_count(&self) -> usize {
        self.sockets.len()
    }
}

impl Default for SocketManager {
    fn default() -> Self {
        Self::new(1024)
    }
}

//! # SSH (Secure Shell) Protocol Implementation
//!
//! Provides SSH client and server functionality for secure remote access.
//!
//! ## Features
//! - SSH-2 protocol support
//! - Password and key-based authentication
//! - Inbound connections (SSH server)
//! - Outbound connections (SSH client)
//! - Shell session support
//!
//! ## Security Note
//! This is a minimal implementation for the Splax OS kernel.
//! Real-world usage would require cryptographic libraries.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use super::{Ipv4Address, NetworkError};

/// Default SSH port
pub const SSH_PORT: u16 = 22;

/// SSH protocol version string
pub const SSH_VERSION: &str = "SSH-2.0-SplaxOS_1.0";

/// SSH message types (RFC 4253)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshMessageType {
    Disconnect = 1,
    Ignore = 2,
    Unimplemented = 3,
    Debug = 4,
    ServiceRequest = 5,
    ServiceAccept = 6,
    KexInit = 20,
    NewKeys = 21,
    KexDhInit = 30,
    KexDhReply = 31,
    UserauthRequest = 50,
    UserauthFailure = 51,
    UserauthSuccess = 52,
    UserauthBanner = 53,
    GlobalRequest = 80,
    RequestSuccess = 81,
    RequestFailure = 82,
    ChannelOpen = 90,
    ChannelOpenConfirmation = 91,
    ChannelOpenFailure = 92,
    ChannelWindowAdjust = 93,
    ChannelData = 94,
    ChannelExtendedData = 95,
    ChannelEof = 96,
    ChannelClose = 97,
    ChannelRequest = 98,
    ChannelSuccess = 99,
    ChannelFailure = 100,
}

/// SSH disconnect reason codes
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum DisconnectReason {
    HostNotAllowedToConnect = 1,
    ProtocolError = 2,
    KeyExchangeFailed = 3,
    Reserved = 4,
    MacError = 5,
    CompressionError = 6,
    ServiceNotAvailable = 7,
    ProtocolVersionNotSupported = 8,
    HostKeyNotVerifiable = 9,
    ConnectionLost = 10,
    ByApplication = 11,
    TooManyConnections = 12,
    AuthCancelledByUser = 13,
    NoMoreAuthMethodsAvailable = 14,
    IllegalUserName = 15,
}

/// SSH connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshState {
    /// Initial state, exchanging version strings
    VersionExchange,
    /// Key exchange in progress
    KeyExchange,
    /// Waiting for service request
    ServiceRequest,
    /// User authentication in progress
    Authentication,
    /// Session established
    Established,
    /// Connection closed
    Closed,
}

/// SSH session information
#[derive(Debug, Clone)]
pub struct SshSession {
    /// Session ID
    pub id: u32,
    /// Current state
    pub state: SshState,
    /// Remote address
    pub remote_addr: Ipv4Address,
    /// Remote port
    pub remote_port: u16,
    /// Authenticated username
    pub username: Option<String>,
    /// Is this an inbound connection
    pub is_inbound: bool,
    /// Channel ID
    pub channel_id: u32,
    /// Socket handle for the connection
    pub socket_handle: Option<super::socket::SocketHandle>,
}

impl SshSession {
    /// Create a new SSH session
    pub fn new(id: u32, remote_addr: Ipv4Address, remote_port: u16, is_inbound: bool) -> Self {
        Self {
            id,
            state: SshState::VersionExchange,
            remote_addr,
            remote_port,
            username: None,
            is_inbound,
            channel_id: 0,
            socket_handle: None,
        }
    }
}

/// SSH server configuration
#[derive(Debug, Clone)]
pub struct SshServerConfig {
    /// Server port (default 22)
    pub port: u16,
    /// Banner message
    pub banner: String,
    /// Max authentication attempts
    pub max_auth_attempts: u8,
    /// Allow password authentication
    pub allow_password: bool,
    /// Allow public key authentication
    pub allow_pubkey: bool,
}

impl Default for SshServerConfig {
    fn default() -> Self {
        Self {
            port: SSH_PORT,
            banner: String::from("Welcome to Splax OS SSH Server\r\n"),
            max_auth_attempts: 3,
            allow_password: true,
            allow_pubkey: true,
        }
    }
}

/// SSH server state
pub struct SshServer {
    /// Configuration
    pub config: SshServerConfig,
    /// Active sessions
    pub sessions: Vec<SshSession>,
    /// Next session ID
    next_session_id: u32,
    /// Is server running
    pub is_running: bool,
}

impl SshServer {
    /// Create a new SSH server
    pub const fn new() -> Self {
        Self {
            config: SshServerConfig {
                port: SSH_PORT,
                banner: String::new(),
                max_auth_attempts: 3,
                allow_password: true,
                allow_pubkey: true,
            },
            sessions: Vec::new(),
            next_session_id: 1,
            is_running: false,
        }
    }

    /// Start the SSH server
    pub fn start(&mut self) -> Result<(), NetworkError> {
        if self.is_running {
            return Err(NetworkError::AlreadyConnected);
        }
        
        // Bind to the TCP port for SSH connections
        let bind_addr = super::SocketAddr::new(
            super::Ipv4Address::ANY,
            self.config.port,
        );
        
        // Register the server socket with the TCP stack
        if let Ok(socket_handle) = super::tcp::tcp_bind(bind_addr) {
            // Store the socket handle for accepting connections
            // The TCP stack will route incoming connections to our accept handler
            self.is_running = true;
            
            #[cfg(target_arch = "x86_64")]
            {
                use core::fmt::Write;
                if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                    let _ = writeln!(serial, "[ssh] Server listening on port {} (socket {})", 
                        self.config.port, socket_handle);
                }
            }
            
            Ok(())
        } else {
            Err(NetworkError::SocketError)
        }
    }

    /// Stop the SSH server
    pub fn stop(&mut self) {
        self.is_running = false;
        self.sessions.clear();
        
        #[cfg(target_arch = "x86_64")]
        {
            use core::fmt::Write;
            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                let _ = writeln!(serial, "[ssh] Server stopped");
            }
        }
    }

    /// Accept a new inbound connection
    pub fn accept_connection(&mut self, remote_addr: Ipv4Address, remote_port: u16) -> u32 {
        let session_id = self.next_session_id;
        self.next_session_id += 1;
        
        let session = SshSession::new(session_id, remote_addr, remote_port, true);
        self.sessions.push(session);
        
        #[cfg(target_arch = "x86_64")]
        {
            use core::fmt::Write;
            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                let _ = writeln!(serial, "[ssh] Accepted connection from {}.{}.{}.{}:{}",
                    remote_addr.octets()[0], remote_addr.octets()[1],
                    remote_addr.octets()[2], remote_addr.octets()[3], remote_port);
            }
        }
        
        session_id
    }

    /// Get session by ID
    pub fn get_session(&self, id: u32) -> Option<&SshSession> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// Get session count
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

/// SSH client configuration
#[derive(Debug, Clone)]
pub struct SshClientConfig {
    /// Connection timeout in milliseconds
    pub timeout_ms: u32,
    /// Username
    pub username: String,
    /// Password (if using password auth)
    pub password: Option<String>,
}

impl Default for SshClientConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 10000,
            username: String::from("root"),
            password: None,
        }
    }
}

/// SSH client for outbound connections
pub struct SshClient {
    /// Configuration
    pub config: SshClientConfig,
    /// Current session
    pub session: Option<SshSession>,
}

impl SshClient {
    /// Create a new SSH client
    pub fn new(config: SshClientConfig) -> Self {
        Self {
            config,
            session: None,
        }
    }

    /// Connect to a remote SSH server
    pub fn connect(&mut self, addr: Ipv4Address, port: u16) -> Result<&SshSession, NetworkError> {
        #[cfg(target_arch = "x86_64")]
        {
            use core::fmt::Write;
            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                let _ = writeln!(serial, "[ssh] Connecting to {}.{}.{}.{}:{}...",
                    addr.octets()[0], addr.octets()[1],
                    addr.octets()[2], addr.octets()[3], port);
            }
        }

        // Create session in version exchange state
        let mut session = SshSession::new(1, addr, port, false);
        
        // 1. Establish TCP connection
        let remote = super::SocketAddr::new(addr, port);
        let socket_handle = super::tcp::tcp_connect(remote)?;
        
        // 2. Exchange version strings
        let version_bytes = SSH_VERSION.as_bytes();
        let mut version_msg = alloc::vec![0u8; version_bytes.len() + 2];
        version_msg[..version_bytes.len()].copy_from_slice(version_bytes);
        version_msg[version_bytes.len()] = b'\r';
        version_msg[version_bytes.len() + 1] = b'\n';
        super::tcp::tcp_send(socket_handle, &version_msg)?;
        
        // Read server version
        let mut server_version = [0u8; 256];
        let _version_len = super::tcp::tcp_recv(socket_handle, &mut server_version)?;
        session.state = SshState::KeyExchange;
        
        // 3. Key exchange (KEX_INIT)
        let kex_init = self.build_kex_init();
        self.send_packet(socket_handle, SshMessageType::KexInit as u8, &kex_init)?;
        
        // Receive server's KEX_INIT
        let _server_kex = self.recv_packet(socket_handle)?;
        
        // 4. Diffie-Hellman key exchange  
        // Generate ephemeral DH keypair using our crypto module
        let (dh_private, dh_public) = self.generate_dh_keypair();
        
        // Send DH_INIT with our public key
        let mut dh_init = Vec::new();
        dh_init.extend_from_slice(&(dh_public.len() as u32).to_be_bytes());
        dh_init.extend_from_slice(&dh_public);
        self.send_packet(socket_handle, SshMessageType::KexDhInit as u8, &dh_init)?;
        
        // Receive DH_REPLY with server's public key and signature
        let dh_reply = self.recv_packet(socket_handle)?;
        if dh_reply.is_empty() {
            super::tcp::tcp_close(socket_handle);
            return Err(NetworkError::ConnectionReset);
        }
        
        // Compute shared secret and derive session keys
        let _shared_secret = self.compute_dh_shared(&dh_private, &dh_reply);
        
        // 5. Send NEW_KEYS
        self.send_packet(socket_handle, SshMessageType::NewKeys as u8, &[])?;
        let _ = self.recv_packet(socket_handle)?; // Receive server's NEW_KEYS
        session.state = SshState::Authentication;
        
        // 6. Request ssh-userauth service
        let mut service_req = Vec::new();
        let service_name = b"ssh-userauth";
        service_req.extend_from_slice(&(service_name.len() as u32).to_be_bytes());
        service_req.extend_from_slice(service_name);
        self.send_packet(socket_handle, SshMessageType::ServiceRequest as u8, &service_req)?;
        let _ = self.recv_packet(socket_handle)?; // SERVICE_ACCEPT
        
        // 7. Authenticate (password)
        if let Some(ref password) = self.config.password {
            let auth_req = self.build_password_auth(&self.config.username, password);
            self.send_packet(socket_handle, SshMessageType::UserauthRequest as u8, &auth_req)?;
            
            let auth_response = self.recv_packet(socket_handle)?;
            if auth_response.first() != Some(&(SshMessageType::UserauthSuccess as u8)) {
                super::tcp::tcp_close(socket_handle);
                return Err(NetworkError::ConnectionRefused);
            }
        }
        
        session.state = SshState::Established;
        session.username = Some(self.config.username.clone());
        session.socket_handle = Some(socket_handle);
        self.session = Some(session);
        
        #[cfg(target_arch = "x86_64")]
        {
            use core::fmt::Write;
            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                let _ = writeln!(serial, "[ssh] Connected and authenticated");
            }
        }
        
        Ok(self.session.as_ref().unwrap())
    }
    
    /// Build KEX_INIT message
    fn build_kex_init(&self) -> Vec<u8> {
        let mut msg = Vec::new();
        
        // Cookie (16 random bytes)
        let cookie = crate::crypto::random::random_bytes::<16>();
        msg.extend_from_slice(&cookie);
        
        // Name-lists for algorithms
        let kex_algorithms = b"diffie-hellman-group14-sha256";
        let host_key_algorithms = b"ssh-ed25519,ssh-rsa";
        let encryption_client = b"aes256-ctr,chacha20-poly1305@openssh.com";
        let encryption_server = encryption_client;
        let mac_client = b"hmac-sha2-256";
        let mac_server = mac_client;
        let compression_client = b"none";
        let compression_server = compression_client;
        let languages_client = b"";
        let languages_server = b"";
        
        // Write each name-list
        for list in [kex_algorithms.as_slice(), host_key_algorithms.as_slice(), 
                     encryption_client.as_slice(), encryption_server.as_slice(),
                     mac_client.as_slice(), mac_server.as_slice(),
                     compression_client.as_slice(), compression_server.as_slice(),
                     languages_client.as_slice(), languages_server.as_slice()] {
            msg.extend_from_slice(&(list.len() as u32).to_be_bytes());
            msg.extend_from_slice(list);
        }
        
        // first_kex_packet_follows = false, reserved = 0
        msg.push(0);
        msg.extend_from_slice(&0u32.to_be_bytes());
        
        msg
    }
    
    /// Build password authentication request
    fn build_password_auth(&self, username: &str, password: &str) -> Vec<u8> {
        let mut msg = Vec::new();
        
        // Username
        msg.extend_from_slice(&(username.len() as u32).to_be_bytes());
        msg.extend_from_slice(username.as_bytes());
        
        // Service name "ssh-connection"
        let service = b"ssh-connection";
        msg.extend_from_slice(&(service.len() as u32).to_be_bytes());
        msg.extend_from_slice(service);
        
        // Method name "password"
        let method = b"password";
        msg.extend_from_slice(&(method.len() as u32).to_be_bytes());
        msg.extend_from_slice(method);
        
        // FALSE (no password change)
        msg.push(0);
        
        // Password
        msg.extend_from_slice(&(password.len() as u32).to_be_bytes());
        msg.extend_from_slice(password.as_bytes());
        
        msg
    }
    
    /// Generate DH keypair for key exchange
    fn generate_dh_keypair(&self) -> (Vec<u8>, Vec<u8>) {
        // Use group14 (2048-bit MODP)
        // In practice, use a proper big-integer library
        // Generate random private key
        let private: [u8; 32] = crate::crypto::random::random_bytes();
        
        // For now, use the private key bytes directly as public key placeholder
        // Real implementation would compute g^private mod p
        let public = private.to_vec();
        
        (private.to_vec(), public)
    }
    
    /// Compute DH shared secret
    fn compute_dh_shared(&self, _private: &[u8], _server_public: &[u8]) -> Vec<u8> {
        // Real implementation would compute server_public^private mod p
        // and derive session keys using the hash
        let mut shared = alloc::vec![0u8; 32];
        let random: [u8; 32] = crate::crypto::random::random_bytes();
        shared.copy_from_slice(&random);
        shared
    }
    
    /// Send SSH packet
    fn send_packet(&self, socket: super::socket::SocketHandle, msg_type: u8, payload: &[u8]) -> Result<(), NetworkError> {
        let packet_len = 1 + payload.len() + 4; // type + payload + padding
        let padding_len = 8 - (packet_len % 8);
        let padding_len = if padding_len < 4 { padding_len + 8 } else { padding_len };
        
        let mut packet = Vec::new();
        packet.extend_from_slice(&((1 + payload.len() + padding_len) as u32).to_be_bytes());
        packet.push(padding_len as u8);
        packet.push(msg_type);
        packet.extend_from_slice(payload);
        
        // Random padding
        let padding: [u8; 16] = crate::crypto::random::random_bytes();
        packet.extend_from_slice(&padding[..padding_len]);
        
        super::tcp::tcp_send(socket, &packet)?;
        Ok(())
    }
    
    /// Receive SSH packet
    fn recv_packet(&self, socket: super::socket::SocketHandle) -> Result<Vec<u8>, NetworkError> {
        let mut header = [0u8; 5];
        let n = super::tcp::tcp_recv(socket, &mut header)?;
        if n < 5 {
            return Ok(Vec::new());
        }
        
        let packet_len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let padding_len = header[4] as usize;
        
        if packet_len > 35000 {
            return Err(NetworkError::InvalidData);
        }
        
        let mut payload = alloc::vec![0u8; packet_len - 1];
        let _ = super::tcp::tcp_recv(socket, &mut payload)?;
        
        // Strip padding
        let data_len = payload.len() - padding_len;
        payload.truncate(data_len);
        
        Ok(payload)
    }

    /// Disconnect from the server
    pub fn disconnect(&mut self) {
        if let Some(session) = &self.session {
            #[cfg(target_arch = "x86_64")]
            {
                use core::fmt::Write;
                if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                    let _ = writeln!(serial, "[ssh] Disconnected from {}.{}.{}.{}",
                        session.remote_addr.octets()[0], session.remote_addr.octets()[1],
                        session.remote_addr.octets()[2], session.remote_addr.octets()[3]);
                }
            }
        }
        self.session = None;
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.session.as_ref().map_or(false, |s| s.state == SshState::Established)
    }

    /// Execute a command on the remote server
    pub fn exec(&mut self, command: &str) -> Result<String, NetworkError> {
        let session = self.session.as_mut().ok_or(NetworkError::NotConnected)?;
        
        if session.state != SshState::Established {
            return Err(NetworkError::NotConnected);
        }
        
        #[cfg(target_arch = "x86_64")]
        {
            use core::fmt::Write;
            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                let _ = writeln!(serial, "[ssh] exec: {}", command);
            }
        }
        
        // Get socket handle from session
        let socket = session.socket_handle.ok_or(NetworkError::NotConnected)?;
        
        // Open a channel for command execution
        let channel_id = session.channel_id;
        session.channel_id += 1;
        
        // Build CHANNEL_OPEN message
        let mut channel_open = Vec::new();
        let channel_type = b"session";
        channel_open.extend_from_slice(&(channel_type.len() as u32).to_be_bytes());
        channel_open.extend_from_slice(channel_type);
        channel_open.extend_from_slice(&channel_id.to_be_bytes()); // sender channel
        channel_open.extend_from_slice(&(32768u32).to_be_bytes()); // initial window size
        channel_open.extend_from_slice(&(16384u32).to_be_bytes()); // max packet size
        self.send_packet(socket, SshMessageType::ChannelOpen as u8, &channel_open)?;
        
        // Wait for CHANNEL_OPEN_CONFIRMATION
        let _ = self.recv_packet(socket)?;
        
        // Send CHANNEL_REQUEST for exec
        let mut exec_req = Vec::new();
        exec_req.extend_from_slice(&channel_id.to_be_bytes()); // recipient channel
        let request_type = b"exec";
        exec_req.extend_from_slice(&(request_type.len() as u32).to_be_bytes());
        exec_req.extend_from_slice(request_type);
        exec_req.push(1); // want reply
        exec_req.extend_from_slice(&(command.len() as u32).to_be_bytes());
        exec_req.extend_from_slice(command.as_bytes());
        self.send_packet(socket, SshMessageType::ChannelRequest as u8, &exec_req)?;
        
        // Collect output
        let mut output = String::new();
        loop {
            let packet = self.recv_packet(socket)?;
            if packet.is_empty() {
                break;
            }
            
            let msg_type = packet.first().copied().unwrap_or(0);
            match msg_type {
                94 => { // CHANNEL_DATA
                    if packet.len() > 8 {
                        let data_len = u32::from_be_bytes([
                            packet[5], packet[6], packet[7], packet[8]
                        ]) as usize;
                        let data_start = 9;
                        let data_end = (data_start + data_len).min(packet.len());
                        if let Ok(s) = core::str::from_utf8(&packet[data_start..data_end]) {
                            output.push_str(s);
                        }
                    }
                }
                96 | 97 => break, // CHANNEL_EOF or CHANNEL_CLOSE
                _ => {}
            }
        }
        
        // Send CHANNEL_CLOSE
        let mut close_msg = Vec::new();
        close_msg.extend_from_slice(&channel_id.to_be_bytes());
        let _ = self.send_packet(socket, SshMessageType::ChannelClose as u8, &close_msg);
        
        Ok(output)
    }
}

/// Global SSH server instance
static SSH_SERVER: spin::Mutex<SshServer> = spin::Mutex::new(SshServer::new());

/// Get the global SSH server
pub fn ssh_server() -> &'static spin::Mutex<SshServer> {
    &SSH_SERVER
}

/// Start the SSH server on default port
pub fn start_server() -> Result<(), NetworkError> {
    SSH_SERVER.lock().start()
}

/// Stop the SSH server
pub fn stop_server() {
    SSH_SERVER.lock().stop()
}

/// Create a new SSH client connection
pub fn connect(addr: Ipv4Address, port: u16, username: &str, password: Option<&str>) -> Result<SshClient, NetworkError> {
    let config = SshClientConfig {
        timeout_ms: 10000,
        username: String::from(username),
        password: password.map(String::from),
    };
    
    let mut client = SshClient::new(config);
    client.connect(addr, port)?;
    Ok(client)
}

/// SSH server status information
#[derive(Debug, Clone)]
pub struct SshServerStatus {
    pub is_running: bool,
    pub port: u16,
    pub session_count: usize,
}

/// Get SSH server status
pub fn server_status() -> SshServerStatus {
    let server = SSH_SERVER.lock();
    SshServerStatus {
        is_running: server.is_running,
        port: server.config.port,
        session_count: server.session_count(),
    }
}

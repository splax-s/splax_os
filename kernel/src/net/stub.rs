//! Network Stub - Forwards network operations to S-NET userspace service
//!
//! This module replaces the monolithic network stack with IPC calls to the
//! S-NET service. Only packet DMA and driver initialization remain in kernel.

use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;

use crate::ipc::{Channel, Message, IpcError};
use crate::cap::CapabilityToken;

/// S-NET service endpoint ID
const SNET_SERVICE_ID: u64 = 0x4E455400; // "NET\0"

/// Network stub error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetStubError {
    /// S-NET service not available
    ServiceUnavailable,
    /// IPC communication failed
    IpcError,
    /// Invalid response from service
    InvalidResponse,
    /// Operation timed out
    Timeout,
    /// Permission denied
    PermissionDenied,
    /// Connection refused
    ConnectionRefused,
    /// Address in use
    AddressInUse,
    /// Network unreachable
    NetworkUnreachable,
}

/// Message types for S-NET IPC protocol
#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum NetMessageType {
    // Socket operations
    SocketCreate = 0x0001,
    SocketBind = 0x0002,
    SocketListen = 0x0003,
    SocketAccept = 0x0004,
    SocketConnect = 0x0005,
    SocketSend = 0x0006,
    SocketRecv = 0x0007,
    SocketClose = 0x0008,
    SocketGetOpt = 0x0009,
    SocketSetOpt = 0x000A,

    // Interface operations
    InterfaceList = 0x0100,
    InterfaceConfig = 0x0101,
    InterfaceUp = 0x0102,
    InterfaceDown = 0x0103,

    // Routing
    RouteAdd = 0x0200,
    RouteDelete = 0x0201,
    RouteList = 0x0202,

    // Firewall
    FirewallAddRule = 0x0300,
    FirewallDeleteRule = 0x0301,
    FirewallListRules = 0x0302,
    FirewallFlush = 0x0303,

    // DHCP
    DhcpRequest = 0x0400,
    DhcpRelease = 0x0401,

    // Responses
    Success = 0x8000,
    Error = 0x8001,
}

/// Network IPC message
#[repr(C)]
pub struct NetMessage {
    pub msg_type: NetMessageType,
    pub sequence: u32,
    pub payload_len: u32,
    pub payload: [u8; 1024],
}

impl Default for NetMessage {
    fn default() -> Self {
        Self {
            msg_type: NetMessageType::Success,
            sequence: 0,
            payload_len: 0,
            payload: [0; 1024],
        }
    }
}

/// S-NET service client stub
pub struct NetStub {
    channel: Option<Channel>,
    capability: Option<CapabilityToken>,
    sequence: u32,
}

impl NetStub {
    /// Create a new network stub
    pub const fn new() -> Self {
        Self {
            channel: None,
            capability: None,
            sequence: 0,
        }
    }

    /// Connect to the S-NET service
    pub fn connect(&mut self) -> Result<(), NetStubError> {
        // Request channel to S-NET service from S-INIT
        // In real implementation, this would use service discovery
        
        // For now, placeholder - service discovery would happen here
        // let channel = s_atlas::discover("s-net", discovery_token)?;
        
        Ok(())
    }

    /// Create a socket
    pub fn socket_create(
        &mut self,
        domain: u32,
        sock_type: u32,
        protocol: u32,
    ) -> Result<u64, NetStubError> {
        let mut msg = NetMessage::default();
        msg.msg_type = NetMessageType::SocketCreate;
        msg.sequence = self.next_sequence();
        
        // Pack arguments
        msg.payload[0..4].copy_from_slice(&domain.to_le_bytes());
        msg.payload[4..8].copy_from_slice(&sock_type.to_le_bytes());
        msg.payload[8..12].copy_from_slice(&protocol.to_le_bytes());
        msg.payload_len = 12;

        let response = self.send_receive(&msg)?;
        
        if matches!(response.msg_type, NetMessageType::Success) {
            let fd = u64::from_le_bytes(response.payload[0..8].try_into().unwrap());
            Ok(fd)
        } else {
            Err(NetStubError::IpcError)
        }
    }

    /// Bind a socket to an address
    pub fn socket_bind(
        &mut self,
        socket_fd: u64,
        addr: &[u8],
    ) -> Result<(), NetStubError> {
        let mut msg = NetMessage::default();
        msg.msg_type = NetMessageType::SocketBind;
        msg.sequence = self.next_sequence();
        
        msg.payload[0..8].copy_from_slice(&socket_fd.to_le_bytes());
        let addr_len = addr.len().min(1016);
        msg.payload[8..8 + addr_len].copy_from_slice(&addr[..addr_len]);
        msg.payload_len = 8 + addr_len as u32;

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            NetMessageType::Success => Ok(()),
            NetMessageType::Error => {
                let error_code = u32::from_le_bytes(
                    response.payload[0..4].try_into().unwrap()
                );
                Err(error_from_code(error_code))
            }
            _ => Err(NetStubError::InvalidResponse),
        }
    }

    /// Connect a socket to a remote address
    pub fn socket_connect(
        &mut self,
        socket_fd: u64,
        addr: &[u8],
    ) -> Result<(), NetStubError> {
        let mut msg = NetMessage::default();
        msg.msg_type = NetMessageType::SocketConnect;
        msg.sequence = self.next_sequence();
        
        msg.payload[0..8].copy_from_slice(&socket_fd.to_le_bytes());
        let addr_len = addr.len().min(1016);
        msg.payload[8..8 + addr_len].copy_from_slice(&addr[..addr_len]);
        msg.payload_len = 8 + addr_len as u32;

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            NetMessageType::Success => Ok(()),
            NetMessageType::Error => {
                let error_code = u32::from_le_bytes(
                    response.payload[0..4].try_into().unwrap()
                );
                Err(error_from_code(error_code))
            }
            _ => Err(NetStubError::InvalidResponse),
        }
    }

    /// Send data on a socket
    pub fn socket_send(
        &mut self,
        socket_fd: u64,
        data: &[u8],
        flags: u32,
    ) -> Result<usize, NetStubError> {
        let mut msg = NetMessage::default();
        msg.msg_type = NetMessageType::SocketSend;
        msg.sequence = self.next_sequence();
        
        msg.payload[0..8].copy_from_slice(&socket_fd.to_le_bytes());
        msg.payload[8..12].copy_from_slice(&flags.to_le_bytes());
        
        let data_len = data.len().min(1012);
        msg.payload[12..12 + data_len].copy_from_slice(&data[..data_len]);
        msg.payload_len = 12 + data_len as u32;

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            NetMessageType::Success => {
                let sent = u64::from_le_bytes(
                    response.payload[0..8].try_into().unwrap()
                );
                Ok(sent as usize)
            }
            NetMessageType::Error => {
                let error_code = u32::from_le_bytes(
                    response.payload[0..4].try_into().unwrap()
                );
                Err(error_from_code(error_code))
            }
            _ => Err(NetStubError::InvalidResponse),
        }
    }

    /// Receive data from a socket
    pub fn socket_recv(
        &mut self,
        socket_fd: u64,
        buffer: &mut [u8],
        flags: u32,
    ) -> Result<usize, NetStubError> {
        let mut msg = NetMessage::default();
        msg.msg_type = NetMessageType::SocketRecv;
        msg.sequence = self.next_sequence();
        
        msg.payload[0..8].copy_from_slice(&socket_fd.to_le_bytes());
        msg.payload[8..12].copy_from_slice(&flags.to_le_bytes());
        msg.payload[12..16].copy_from_slice(&(buffer.len() as u32).to_le_bytes());
        msg.payload_len = 16;

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            NetMessageType::Success => {
                let received = response.payload_len as usize;
                let copy_len = received.min(buffer.len());
                buffer[..copy_len].copy_from_slice(&response.payload[..copy_len]);
                Ok(received)
            }
            NetMessageType::Error => {
                let error_code = u32::from_le_bytes(
                    response.payload[0..4].try_into().unwrap()
                );
                Err(error_from_code(error_code))
            }
            _ => Err(NetStubError::InvalidResponse),
        }
    }

    /// Close a socket
    pub fn socket_close(&mut self, socket_fd: u64) -> Result<(), NetStubError> {
        let mut msg = NetMessage::default();
        msg.msg_type = NetMessageType::SocketClose;
        msg.sequence = self.next_sequence();
        
        msg.payload[0..8].copy_from_slice(&socket_fd.to_le_bytes());
        msg.payload_len = 8;

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            NetMessageType::Success => Ok(()),
            _ => Err(NetStubError::IpcError),
        }
    }

    /// Request DHCP lease for an interface
    pub fn dhcp_request(&mut self, interface: &str) -> Result<(), NetStubError> {
        let mut msg = NetMessage::default();
        msg.msg_type = NetMessageType::DhcpRequest;
        msg.sequence = self.next_sequence();
        
        let if_bytes = interface.as_bytes();
        let len = if_bytes.len().min(1024);
        msg.payload[..len].copy_from_slice(&if_bytes[..len]);
        msg.payload_len = len as u32;

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            NetMessageType::Success => Ok(()),
            _ => Err(NetStubError::IpcError),
        }
    }

    /// Add a firewall rule
    pub fn firewall_add_rule(
        &mut self,
        chain: &str,
        rule_data: &[u8],
    ) -> Result<u32, NetStubError> {
        let mut msg = NetMessage::default();
        msg.msg_type = NetMessageType::FirewallAddRule;
        msg.sequence = self.next_sequence();
        
        let chain_bytes = chain.as_bytes();
        let chain_len = chain_bytes.len().min(64);
        msg.payload[0] = chain_len as u8;
        msg.payload[1..1 + chain_len].copy_from_slice(&chain_bytes[..chain_len]);
        
        let rule_len = rule_data.len().min(959);
        msg.payload[65..65 + rule_len].copy_from_slice(&rule_data[..rule_len]);
        msg.payload_len = 65 + rule_len as u32;

        let response = self.send_receive(&msg)?;
        
        match response.msg_type {
            NetMessageType::Success => {
                let rule_id = u32::from_le_bytes(
                    response.payload[0..4].try_into().unwrap()
                );
                Ok(rule_id)
            }
            _ => Err(NetStubError::IpcError),
        }
    }

    /// Get next sequence number
    fn next_sequence(&mut self) -> u32 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    /// Send message and wait for response
    fn send_receive(&mut self, _msg: &NetMessage) -> Result<NetMessage, NetStubError> {
        // In real implementation:
        // 1. Serialize message to IPC buffer
        // 2. Send via S-LINK channel
        // 3. Block waiting for response
        // 4. Deserialize response
        
        // Placeholder for now - would use actual IPC
        let channel = self.channel.as_ref()
            .ok_or(NetStubError::ServiceUnavailable)?;
        
        // channel.send(msg)?;
        // let response = channel.receive()?;
        
        Ok(NetMessage::default())
    }
}

/// Convert error code to NetStubError
fn error_from_code(code: u32) -> NetStubError {
    match code {
        1 => NetStubError::PermissionDenied,
        2 => NetStubError::ConnectionRefused,
        3 => NetStubError::AddressInUse,
        4 => NetStubError::NetworkUnreachable,
        5 => NetStubError::Timeout,
        _ => NetStubError::IpcError,
    }
}

/// Global network stub instance
pub static NET_STUB: Mutex<NetStub> = Mutex::new(NetStub::new());

/// Initialize the network stub (called during kernel init)
pub fn init() -> Result<(), NetStubError> {
    NET_STUB.lock().connect()
}

// ============================================================================
// Syscall wrappers - These are called from the syscall handler
// ============================================================================

/// sys_socket - Create a socket (forwards to S-NET)
pub fn sys_socket(domain: u32, sock_type: u32, protocol: u32) -> Result<u64, NetStubError> {
    NET_STUB.lock().socket_create(domain, sock_type, protocol)
}

/// sys_bind - Bind a socket to an address (forwards to S-NET)
pub fn sys_bind(fd: u64, addr: &[u8]) -> Result<(), NetStubError> {
    NET_STUB.lock().socket_bind(fd, addr)
}

/// sys_connect - Connect a socket (forwards to S-NET)
pub fn sys_connect(fd: u64, addr: &[u8]) -> Result<(), NetStubError> {
    NET_STUB.lock().socket_connect(fd, addr)
}

/// sys_send - Send data on a socket (forwards to S-NET)
pub fn sys_send(fd: u64, data: &[u8], flags: u32) -> Result<usize, NetStubError> {
    NET_STUB.lock().socket_send(fd, data, flags)
}

/// sys_recv - Receive data from a socket (forwards to S-NET)
pub fn sys_recv(fd: u64, buffer: &mut [u8], flags: u32) -> Result<usize, NetStubError> {
    NET_STUB.lock().socket_recv(fd, buffer, flags)
}

/// sys_close_socket - Close a socket (forwards to S-NET)
pub fn sys_close_socket(fd: u64) -> Result<(), NetStubError> {
    NET_STUB.lock().socket_close(fd)
}

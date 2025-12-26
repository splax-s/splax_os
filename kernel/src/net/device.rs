//! # Network Device Abstraction
//!
//! Traits and types for network device drivers.

use alloc::vec::Vec;

/// Network device error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkError {
    /// Device not ready
    NotReady,
    /// No buffer available
    NoBuffer,
    /// Transmission failed
    TxFailed,
    /// No route to destination
    NoRoute,
    /// ARP resolution timeout
    ArpTimeout,
    /// Connection refused
    ConnectionRefused,
    /// Connection reset
    ConnectionReset,
    /// Connection timeout
    Timeout,
    /// Address in use
    AddressInUse,
    /// Invalid state
    InvalidState,
    /// Buffer too small
    BufferTooSmall,
    /// Would block (non-blocking operation)
    WouldBlock,
    /// Link is down
    LinkDown,
    /// Invalid packet
    InvalidPacket,
    /// Invalid socket
    InvalidSocket,
    /// Invalid operation
    InvalidOperation,
    /// Not bound
    NotBound,
    /// Not connected
    NotConnected,
    /// No network interface available
    NoInterface,
    /// Operation not supported
    NotSupported,
    /// DNS resolution failed
    DnsError,
    /// Host unreachable
    HostUnreachable,
    /// Network unreachable
    NetworkUnreachable,
    /// Port unreachable
    PortUnreachable,
    /// Already connected/running
    AlreadyConnected,
}

/// Network device information.
#[derive(Debug, Clone)]
pub struct NetworkDeviceInfo {
    /// Device name
    pub name: &'static str,
    /// MAC address
    pub mac: super::MacAddress,
    /// Maximum transmission unit
    pub mtu: u16,
    /// Link speed in Mbps
    pub link_speed: u32,
    /// Link is up
    pub link_up: bool,
}

/// Network device trait.
pub trait NetworkDevice {
    /// Gets device info.
    fn info(&self) -> NetworkDeviceInfo;
    
    /// Sends a packet.
    fn send(&self, data: &[u8]) -> Result<(), NetworkError>;
    
    /// Receives a packet if available.
    fn receive(&self) -> Result<Vec<u8>, NetworkError>;
    
    /// Checks if device is ready.
    fn is_ready(&self) -> bool;
    
    /// Gets link status.
    fn link_up(&self) -> bool;
}

/// Loopback device for testing.
pub struct LoopbackDevice {
    queue: spin::Mutex<Vec<Vec<u8>>>,
    mac: super::MacAddress,
}

impl LoopbackDevice {
    /// Creates a new loopback device.
    pub fn new() -> Self {
        Self {
            queue: spin::Mutex::new(Vec::new()),
            mac: super::MacAddress::new([0x00, 0x00, 0x00, 0x00, 0x00, 0x01]),
        }
    }
}

impl Default for LoopbackDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkDevice for LoopbackDevice {
    fn info(&self) -> NetworkDeviceInfo {
        NetworkDeviceInfo {
            name: "lo",
            mac: self.mac,
            mtu: 65535,
            link_speed: 0,
            link_up: true,
        }
    }
    
    fn send(&self, data: &[u8]) -> Result<(), NetworkError> {
        self.queue.lock().push(data.to_vec());
        Ok(())
    }
    
    fn receive(&self) -> Result<Vec<u8>, NetworkError> {
        let mut queue = self.queue.lock();
        if queue.is_empty() {
            Err(NetworkError::WouldBlock)
        } else {
            Ok(queue.remove(0))
        }
    }
    
    fn is_ready(&self) -> bool {
        true
    }
    
    fn link_up(&self) -> bool {
        true
    }
}

//! # ARP (Address Resolution Protocol)
//!
//! Maps IPv4 addresses to MAC addresses.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use super::ethernet::MacAddress;
use super::ip::Ipv4Address;

/// ARP operation: Request.
pub const ARP_REQUEST: u16 = 1;
/// ARP operation: Reply.
pub const ARP_REPLY: u16 = 2;

/// Hardware type: Ethernet.
const HARDWARE_ETHERNET: u16 = 1;
/// Protocol type: IPv4.
const PROTOCOL_IPV4: u16 = 0x0800;

/// ARP packet.
#[derive(Debug, Clone)]
pub struct ArpPacket {
    /// Hardware type.
    pub hardware_type: u16,
    /// Protocol type.
    pub protocol_type: u16,
    /// Hardware address length.
    pub hw_addr_len: u8,
    /// Protocol address length.
    pub proto_addr_len: u8,
    /// Operation (request/reply).
    pub operation: u16,
    /// Sender hardware (MAC) address.
    pub sender_mac: MacAddress,
    /// Sender protocol (IP) address.
    pub sender_ip: Ipv4Address,
    /// Target hardware (MAC) address.
    pub target_mac: MacAddress,
    /// Target protocol (IP) address.
    pub target_ip: Ipv4Address,
}

impl ArpPacket {
    /// ARP packet size.
    pub const SIZE: usize = 28;
    
    /// Creates an ARP request.
    pub fn request(
        sender_mac: MacAddress,
        sender_ip: Ipv4Address,
        target_ip: Ipv4Address,
    ) -> Self {
        Self {
            hardware_type: HARDWARE_ETHERNET,
            protocol_type: PROTOCOL_IPV4,
            hw_addr_len: 6,
            proto_addr_len: 4,
            operation: ARP_REQUEST,
            sender_mac,
            sender_ip,
            target_mac: MacAddress::ZERO,
            target_ip,
        }
    }
    
    /// Creates an ARP reply.
    pub fn reply(
        sender_mac: MacAddress,
        sender_ip: Ipv4Address,
        target_mac: MacAddress,
        target_ip: Ipv4Address,
    ) -> Self {
        Self {
            hardware_type: HARDWARE_ETHERNET,
            protocol_type: PROTOCOL_IPV4,
            hw_addr_len: 6,
            proto_addr_len: 4,
            operation: ARP_REPLY,
            sender_mac,
            sender_ip,
            target_mac,
            target_ip,
        }
    }
    
    /// Parses an ARP packet from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        
        let hardware_type = u16::from_be_bytes([data[0], data[1]]);
        let protocol_type = u16::from_be_bytes([data[2], data[3]]);
        let hw_addr_len = data[4];
        let proto_addr_len = data[5];
        let operation = u16::from_be_bytes([data[6], data[7]]);
        
        // Validate for Ethernet/IPv4
        if hardware_type != HARDWARE_ETHERNET 
            || protocol_type != PROTOCOL_IPV4
            || hw_addr_len != 6 
            || proto_addr_len != 4 
        {
            return None;
        }
        
        let sender_mac = MacAddress::from_bytes(&data[8..14])?;
        let sender_ip = Ipv4Address::from_bytes(&data[14..18])?;
        let target_mac = MacAddress::from_bytes(&data[18..24])?;
        let target_ip = Ipv4Address::from_bytes(&data[24..28])?;
        
        Some(Self {
            hardware_type,
            protocol_type,
            hw_addr_len,
            proto_addr_len,
            operation,
            sender_mac,
            sender_ip,
            target_mac,
            target_ip,
        })
    }
    
    /// Serializes the packet to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::SIZE);
        
        bytes.extend_from_slice(&self.hardware_type.to_be_bytes());
        bytes.extend_from_slice(&self.protocol_type.to_be_bytes());
        bytes.push(self.hw_addr_len);
        bytes.push(self.proto_addr_len);
        bytes.extend_from_slice(&self.operation.to_be_bytes());
        bytes.extend_from_slice(&self.sender_mac.0);
        bytes.extend_from_slice(&self.sender_ip.0);
        bytes.extend_from_slice(&self.target_mac.0);
        bytes.extend_from_slice(&self.target_ip.0);
        
        bytes
    }
}

/// ARP cache entry.
#[derive(Debug, Clone)]
struct ArpEntry {
    mac: MacAddress,
    timestamp: u64,
}

/// ARP cache for IP to MAC mapping.
pub struct ArpCache {
    entries: BTreeMap<Ipv4Address, ArpEntry>,
    /// Cache timeout in ticks.
    timeout: u64,
}

impl ArpCache {
    /// Default cache timeout (5 minutes at ~1GHz).
    const DEFAULT_TIMEOUT: u64 = 5 * 60 * 1_000_000_000;
    
    /// Creates a new ARP cache.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            timeout: Self::DEFAULT_TIMEOUT,
        }
    }
    
    /// Inserts an entry.
    pub fn insert(&mut self, ip: Ipv4Address, mac: MacAddress) {
        #[cfg(target_arch = "x86_64")]
        let now = crate::arch::x86_64::interrupts::get_ticks();
        #[cfg(not(target_arch = "x86_64"))]
        let now = 0u64;
        self.entries.insert(ip, ArpEntry { mac, timestamp: now });
    }
    
    /// Looks up a MAC address.
    pub fn lookup(&self, ip: Ipv4Address) -> Option<MacAddress> {
        self.entries.get(&ip).map(|e| e.mac)
    }
    
    /// Returns an iterator over all entries in the cache.
    pub fn entries(&self) -> impl Iterator<Item = (&Ipv4Address, &MacAddress)> {
        self.entries.iter().map(|(ip, entry)| (ip, &entry.mac))
    }
    
    /// Returns the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    
    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    
    /// Removes expired entries.
    pub fn expire(&mut self, now: u64) {
        self.entries.retain(|_, entry| {
            now.saturating_sub(entry.timestamp) < self.timeout
        });
    }
    
    /// Clears the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for ArpCache {
    fn default() -> Self {
        Self::new()
    }
}

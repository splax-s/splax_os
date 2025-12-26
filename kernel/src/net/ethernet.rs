//! # Ethernet Frame Handling
//!
//! Ethernet frame parsing and construction.

use alloc::vec::Vec;

/// Ethernet type for IPv4.
pub const ETHERTYPE_IPV4: u16 = 0x0800;
/// Ethernet type for ARP.
pub const ETHERTYPE_ARP: u16 = 0x0806;
/// Ethernet type for IPv6.
pub const ETHERTYPE_IPV6: u16 = 0x86DD;

/// MAC address (6 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    /// Zero MAC address.
    pub const ZERO: Self = Self([0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    
    /// Broadcast MAC address.
    pub const BROADCAST: Self = Self([0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    
    /// Creates a new MAC address.
    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }
    
    /// Creates from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 6 {
            let mut arr = [0u8; 6];
            arr.copy_from_slice(&bytes[..6]);
            Some(Self(arr))
        } else {
            None
        }
    }
    
    /// Returns the bytes.
    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }
    
    /// Checks if this is a broadcast address.
    pub fn is_broadcast(&self) -> bool {
        *self == Self::BROADCAST
    }
    
    /// Checks if this is a multicast address.
    pub fn is_multicast(&self) -> bool {
        (self.0[0] & 0x01) != 0
    }
}

impl core::fmt::Display for MacAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

/// Ethernet frame.
#[derive(Debug, Clone)]
pub struct EthernetFrame {
    /// Destination MAC address.
    pub dest_mac: MacAddress,
    /// Source MAC address.
    pub src_mac: MacAddress,
    /// Ethertype (protocol).
    pub ethertype: u16,
    /// Payload.
    pub payload: Vec<u8>,
}

impl EthernetFrame {
    /// Minimum frame size (without FCS).
    pub const MIN_SIZE: usize = 14;
    /// Maximum frame size (without FCS).
    pub const MAX_SIZE: usize = 1518;
    /// Header size.
    pub const HEADER_SIZE: usize = 14;
    
    /// Parses an Ethernet frame from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::HEADER_SIZE {
            return None;
        }
        
        let dest_mac = MacAddress::from_bytes(&data[0..6])?;
        let src_mac = MacAddress::from_bytes(&data[6..12])?;
        let ethertype = u16::from_be_bytes([data[12], data[13]]);
        let payload = data[Self::HEADER_SIZE..].to_vec();
        
        Some(Self {
            dest_mac,
            src_mac,
            ethertype,
            payload,
        })
    }
    
    /// Serializes the frame to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::HEADER_SIZE + self.payload.len());
        
        bytes.extend_from_slice(&self.dest_mac.0);
        bytes.extend_from_slice(&self.src_mac.0);
        bytes.extend_from_slice(&self.ethertype.to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        
        // Pad to minimum size (46 bytes payload)
        while bytes.len() < 60 {
            bytes.push(0);
        }
        
        bytes
    }
    
    /// Creates a new frame with payload.
    pub fn new(dest_mac: MacAddress, src_mac: MacAddress, ethertype: u16, payload: Vec<u8>) -> Self {
        Self {
            dest_mac,
            src_mac,
            ethertype,
            payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use alloc::format;

    use super::*;
    
    #[test]
    fn test_mac_address_display() {
        let mac = MacAddress::new([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        assert_eq!(format!("{}", mac), "aa:bb:cc:dd:ee:ff");
    }
    
    #[test]
    fn test_frame_roundtrip() {
        let frame = EthernetFrame {
            dest_mac: MacAddress::BROADCAST,
            src_mac: MacAddress::new([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]),
            ethertype: ETHERTYPE_IPV4,
            payload: vec![1, 2, 3, 4],
        };
        
        let bytes = frame.to_bytes();
        let parsed = EthernetFrame::parse(&bytes).unwrap();
        
        assert_eq!(parsed.dest_mac, frame.dest_mac);
        assert_eq!(parsed.src_mac, frame.src_mac);
        assert_eq!(parsed.ethertype, frame.ethertype);
    }
}

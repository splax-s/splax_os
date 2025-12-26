//! # ICMP (Internet Control Message Protocol)
//!
//! ICMP packet handling for ping and network diagnostics.

use alloc::vec::Vec;
use super::ip::checksum_bytes;

/// ICMP type: Echo Reply.
pub const ICMP_ECHO_REPLY: u8 = 0;
/// ICMP type: Destination Unreachable.
pub const ICMP_DEST_UNREACHABLE: u8 = 3;
/// ICMP type: Echo Request.
pub const ICMP_ECHO_REQUEST: u8 = 8;
/// ICMP type: Time Exceeded.
pub const ICMP_TIME_EXCEEDED: u8 = 11;

/// ICMP packet.
#[derive(Debug, Clone)]
pub struct IcmpPacket {
    /// ICMP type.
    pub icmp_type: u8,
    /// ICMP code.
    pub code: u8,
    /// Checksum.
    pub checksum: u16,
    /// Identifier (for echo).
    pub identifier: u16,
    /// Sequence number (for echo).
    pub sequence: u16,
    /// Data.
    pub data: Vec<u8>,
}

impl IcmpPacket {
    /// Minimum header size.
    pub const HEADER_SIZE: usize = 8;
    
    /// Creates an echo request (ping).
    pub fn echo_request(identifier: u16, sequence: u16, data: Vec<u8>) -> Self {
        let mut packet = Self {
            icmp_type: ICMP_ECHO_REQUEST,
            code: 0,
            checksum: 0,
            identifier,
            sequence,
            data,
        };
        packet.checksum = packet.compute_checksum();
        packet
    }
    
    /// Creates an echo reply (pong).
    pub fn echo_reply(identifier: u16, sequence: u16, data: Vec<u8>) -> Self {
        let mut packet = Self {
            icmp_type: ICMP_ECHO_REPLY,
            code: 0,
            checksum: 0,
            identifier,
            sequence,
            data,
        };
        packet.checksum = packet.compute_checksum();
        packet
    }
    
    /// Creates a destination unreachable message.
    pub fn destination_unreachable(code: u8, original_header: Vec<u8>) -> Self {
        let mut packet = Self {
            icmp_type: ICMP_DEST_UNREACHABLE,
            code,
            checksum: 0,
            identifier: 0,
            sequence: 0,
            data: original_header,
        };
        packet.checksum = packet.compute_checksum();
        packet
    }
    
    /// Parses an ICMP packet from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::HEADER_SIZE {
            return None;
        }
        
        let icmp_type = data[0];
        let code = data[1];
        let checksum = u16::from_be_bytes([data[2], data[3]]);
        let identifier = u16::from_be_bytes([data[4], data[5]]);
        let sequence = u16::from_be_bytes([data[6], data[7]]);
        let payload = data[Self::HEADER_SIZE..].to_vec();
        
        Some(Self {
            icmp_type,
            code,
            checksum,
            identifier,
            sequence,
            data: payload,
        })
    }
    
    /// Serializes the packet to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::HEADER_SIZE + self.data.len());
        
        bytes.push(self.icmp_type);
        bytes.push(self.code);
        bytes.extend_from_slice(&self.checksum.to_be_bytes());
        bytes.extend_from_slice(&self.identifier.to_be_bytes());
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        bytes.extend_from_slice(&self.data);
        
        bytes
    }
    
    /// Computes the checksum.
    fn compute_checksum(&self) -> u16 {
        let mut bytes = Vec::with_capacity(Self::HEADER_SIZE + self.data.len());
        bytes.push(self.icmp_type);
        bytes.push(self.code);
        bytes.push(0); // Checksum placeholder
        bytes.push(0);
        bytes.extend_from_slice(&self.identifier.to_be_bytes());
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        bytes.extend_from_slice(&self.data);
        
        checksum_bytes(&bytes)
    }
    
    /// Verifies the checksum.
    pub fn verify_checksum(&self) -> bool {
        self.compute_checksum() == self.checksum
    }
}

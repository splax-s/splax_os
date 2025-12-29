//! # IPv4 Protocol
//!
//! IPv4 packet parsing and construction.

use alloc::vec::Vec;

/// Protocol number: ICMP.
pub const PROTOCOL_ICMP: u8 = 1;
/// Protocol number: TCP.
pub const PROTOCOL_TCP: u8 = 6;
/// Protocol number: UDP.
pub const PROTOCOL_UDP: u8 = 17;

/// IPv4 address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ipv4Address(pub [u8; 4]);

impl Ipv4Address {
    /// Any address (0.0.0.0).
    pub const ANY: Self = Self([0, 0, 0, 0]);
    /// Unspecified address (0.0.0.0).
    pub const UNSPECIFIED: Self = Self([0, 0, 0, 0]);
    /// Broadcast address (255.255.255.255).
    pub const BROADCAST: Self = Self([255, 255, 255, 255]);
    /// Localhost (127.0.0.1).
    pub const LOCALHOST: Self = Self([127, 0, 0, 1]);
    
    /// Creates a new IPv4 address.
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }
    
    /// Creates a new IPv4 address (const alias for static initialization).
    pub const fn new_const(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }
    
    /// Creates from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 4 {
            Some(Self([bytes[0], bytes[1], bytes[2], bytes[3]]))
        } else {
            None
        }
    }
    
    /// Returns the bytes.
    pub fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }
    
    /// Returns the four octets of the address (std::net compatible).
    pub fn octets(&self) -> [u8; 4] {
        self.0
    }
    
    /// Checks if this is a loopback address.
    pub fn is_loopback(&self) -> bool {
        self.0[0] == 127
    }
    
    /// Checks if this is a broadcast address.
    pub fn is_broadcast(&self) -> bool {
        *self == Self::BROADCAST
    }
    
    /// Checks if this is a multicast address.
    pub fn is_multicast(&self) -> bool {
        self.0[0] >= 224 && self.0[0] <= 239
    }
}

impl From<Ipv4Address> for u32 {
    fn from(addr: Ipv4Address) -> u32 {
        u32::from_be_bytes(addr.0)
    }
}

impl From<u32> for Ipv4Address {
    fn from(val: u32) -> Self {
        Self(val.to_be_bytes())
    }
}

impl core::fmt::Display for Ipv4Address {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

/// IPv4 packet header.
#[derive(Debug, Clone)]
pub struct Ipv4Packet {
    /// IP version (always 4 for IPv4).
    pub version: u8,
    /// Internet Header Length (in 32-bit words, minimum 5).
    pub ihl: u8,
    /// Differentiated Services Code Point (6 bits).
    pub dscp: u8,
    /// Explicit Congestion Notification (2 bits).
    pub ecn: u8,
    /// Total length of packet (header + payload).
    pub total_length: u16,
    /// Identification for fragment reassembly.
    pub identification: u16,
    /// Flags (3 bits: Reserved, Don't Fragment, More Fragments).
    pub flags: u8,
    /// Fragment offset (13 bits, in 8-byte units).
    pub fragment_offset: u16,
    /// Time to Live.
    pub ttl: u8,
    /// Protocol number (e.g., TCP=6, UDP=17, ICMP=1).
    pub protocol: u8,
    /// Header checksum.
    pub header_checksum: u16,
    /// Source address.
    pub src_addr: Ipv4Address,
    /// Destination address.
    pub dest_addr: Ipv4Address,
    /// IP options (variable length, up to 40 bytes).
    pub options: Vec<u8>,
    /// Payload data.
    pub payload: Vec<u8>,
}

impl Ipv4Packet {
    /// Minimum header size.
    pub const MIN_HEADER_SIZE: usize = 20;
    /// Default TTL.
    pub const DEFAULT_TTL: u8 = 64;
    
    /// Creates a new IPv4 packet.
    pub fn new(
        src_addr: Ipv4Address,
        dest_addr: Ipv4Address,
        protocol: u8,
        payload: Vec<u8>,
    ) -> Self {
        let total_length = (Self::MIN_HEADER_SIZE + payload.len()) as u16;
        
        let mut packet = Self {
            version: 4,
            ihl: 5, // 5 * 4 = 20 bytes (minimum header)
            dscp: 0,
            ecn: 0,
            total_length,
            identification: 0,
            flags: 0x02, // Don't Fragment flag
            fragment_offset: 0,
            ttl: Self::DEFAULT_TTL,
            protocol,
            header_checksum: 0,
            src_addr,
            dest_addr,
            options: Vec::new(),
            payload,
        };
        
        packet.header_checksum = packet.compute_checksum();
        packet
    }
    
    /// Parses an IPv4 packet from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::MIN_HEADER_SIZE {
            return None;
        }
        
        let version_ihl = data[0];
        let version = version_ihl >> 4;
        let ihl = version_ihl & 0x0F;
        let header_len = (ihl as usize) * 4;
        
        // Must be IPv4
        if version != 4 || header_len < Self::MIN_HEADER_SIZE {
            return None;
        }
        
        let total_length = u16::from_be_bytes([data[2], data[3]]) as usize;
        
        if data.len() < total_length || total_length < header_len {
            return None;
        }
        
        let tos = data[1];
        let dscp = tos >> 2;
        let ecn = tos & 0x03;
        let identification = u16::from_be_bytes([data[4], data[5]]);
        let flags_fragment = u16::from_be_bytes([data[6], data[7]]);
        let flags = ((flags_fragment >> 13) & 0x07) as u8;
        let fragment_offset = flags_fragment & 0x1FFF;
        let ttl = data[8];
        let protocol = data[9];
        let header_checksum = u16::from_be_bytes([data[10], data[11]]);
        let src_addr = Ipv4Address::from_bytes(&data[12..16])?;
        let dest_addr = Ipv4Address::from_bytes(&data[16..20])?;
        
        // Parse options if header is larger than minimum
        let options = if header_len > Self::MIN_HEADER_SIZE {
            data[Self::MIN_HEADER_SIZE..header_len].to_vec()
        } else {
            Vec::new()
        };
        
        let payload = data[header_len..total_length].to_vec();
        
        Some(Self {
            version,
            ihl,
            dscp,
            ecn,
            total_length: total_length as u16,
            identification,
            flags,
            fragment_offset,
            ttl,
            protocol,
            header_checksum,
            src_addr,
            dest_addr,
            options,
            payload,
        })
    }
    
    /// Serializes the packet to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let header_len = Self::MIN_HEADER_SIZE + self.options.len();
        let mut bytes = Vec::with_capacity(header_len + self.payload.len());
        
        // Version + IHL
        let version_ihl = (self.version << 4) | self.ihl;
        bytes.push(version_ihl);
        
        // DSCP + ECN (Type of Service)
        let tos = (self.dscp << 2) | (self.ecn & 0x03);
        bytes.push(tos);
        
        bytes.extend_from_slice(&self.total_length.to_be_bytes());
        bytes.extend_from_slice(&self.identification.to_be_bytes());
        
        // Flags + Fragment Offset
        let flags_fragment = ((self.flags as u16) << 13) | (self.fragment_offset & 0x1FFF);
        bytes.extend_from_slice(&flags_fragment.to_be_bytes());
        
        bytes.push(self.ttl);
        bytes.push(self.protocol);
        bytes.extend_from_slice(&self.header_checksum.to_be_bytes());
        bytes.extend_from_slice(&self.src_addr.0);
        bytes.extend_from_slice(&self.dest_addr.0);
        
        // Options (if any)
        bytes.extend_from_slice(&self.options);
        
        // Payload
        bytes.extend_from_slice(&self.payload);
        
        bytes
    }
    
    /// Computes the header checksum.
    fn compute_checksum(&self) -> u16 {
        let version_ihl = (self.version << 4) | self.ihl;
        let tos = (self.dscp << 2) | (self.ecn & 0x03);
        let flags_fragment = ((self.flags as u16) << 13) | (self.fragment_offset & 0x1FFF);
        
        let mut header = alloc::vec![
            ((version_ihl as u16) << 8) | (tos as u16),
            self.total_length,
            self.identification,
            flags_fragment,
            ((self.ttl as u16) << 8) | (self.protocol as u16),
            0, // Checksum field zeroed for calculation (RFC 1071)
            u16::from_be_bytes([self.src_addr.0[0], self.src_addr.0[1]]),
            u16::from_be_bytes([self.src_addr.0[2], self.src_addr.0[3]]),
            u16::from_be_bytes([self.dest_addr.0[0], self.dest_addr.0[1]]),
            u16::from_be_bytes([self.dest_addr.0[2], self.dest_addr.0[3]]),
        ];
        
        // Include options in checksum calculation
        let mut i = 0;
        while i + 1 < self.options.len() {
            header.push(u16::from_be_bytes([self.options[i], self.options[i + 1]]));
            i += 2;
        }
        if i < self.options.len() {
            header.push((self.options[i] as u16) << 8);
        }
        
        internet_checksum(&header)
    }
    
    /// Verifies the checksum.
    pub fn verify_checksum(&self) -> bool {
        self.compute_checksum() == self.header_checksum
    }
}

/// Computes internet checksum (RFC 1071).
pub fn internet_checksum(data: &[u16]) -> u16 {
    let mut sum: u32 = 0;
    
    for &word in data {
        sum += word as u32;
    }
    
    // Fold 32-bit sum to 16 bits
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    
    !(sum as u16)
}

/// Computes checksum over bytes.
pub fn checksum_bytes(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    
    // Handle odd byte
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    
    // Fold
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    
    !(sum as u16)
}

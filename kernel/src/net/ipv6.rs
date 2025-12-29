//! # IPv6 Protocol
//!
//! Full IPv6 packet parsing and construction.
//!
//! ## Features
//!
//! - IPv6 address handling (128-bit addresses)
//! - ICMPv6 for neighbor discovery
//! - Extension header support
//! - Multicast support
//! - Link-local and global addresses

use alloc::vec::Vec;
use alloc::string::String;
use core::fmt;

/// Protocol number: ICMPv6.
pub const PROTOCOL_ICMPV6: u8 = 58;
/// Protocol number: TCP.
pub const PROTOCOL_TCP: u8 = 6;
/// Protocol number: UDP.
pub const PROTOCOL_UDP: u8 = 17;
/// Protocol number: No next header.
pub const PROTOCOL_NONE: u8 = 59;
/// Protocol number: Fragment header.
pub const PROTOCOL_FRAGMENT: u8 = 44;
/// Protocol number: Hop-by-hop options.
pub const PROTOCOL_HOP_BY_HOP: u8 = 0;
/// Protocol number: Routing header.
pub const PROTOCOL_ROUTING: u8 = 43;
/// Protocol number: Destination options.
pub const PROTOCOL_DEST_OPTIONS: u8 = 60;

/// Ethernet type for IPv6.
pub const ETHERTYPE_IPV6: u16 = 0x86DD;

/// IPv6 address (128 bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ipv6Address(pub [u8; 16]);

impl Ipv6Address {
    /// Unspecified address (::)
    pub const UNSPECIFIED: Self = Self([0; 16]);
    
    /// Loopback address (::1)
    pub const LOOPBACK: Self = Self([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    
    /// All nodes multicast (ff02::1)
    pub const ALL_NODES: Self = Self([0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    
    /// All routers multicast (ff02::2)
    pub const ALL_ROUTERS: Self = Self([0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
    
    /// Solicited-node multicast prefix (ff02::1:ff00:0/104)
    pub const SOLICITED_NODE_PREFIX: [u8; 13] = [0xff, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01, 0xff];
    
    /// Creates a new IPv6 address from 16 bytes.
    pub const fn new(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
    
    /// Creates an IPv6 address from eight 16-bit segments.
    pub const fn from_segments(segments: [u16; 8]) -> Self {
        let mut bytes = [0u8; 16];
        let mut i = 0;
        while i < 8 {
            bytes[i * 2] = (segments[i] >> 8) as u8;
            bytes[i * 2 + 1] = (segments[i] & 0xFF) as u8;
            i += 1;
        }
        Self(bytes)
    }
    
    /// Creates a link-local address from a MAC address (EUI-64).
    pub fn from_mac(mac: [u8; 6]) -> Self {
        let mut bytes = [0u8; 16];
        // fe80::/10 prefix
        bytes[0] = 0xfe;
        bytes[1] = 0x80;
        // Interface ID from EUI-64
        bytes[8] = mac[0] ^ 0x02; // Flip universal/local bit
        bytes[9] = mac[1];
        bytes[10] = mac[2];
        bytes[11] = 0xff;
        bytes[12] = 0xfe;
        bytes[13] = mac[3];
        bytes[14] = mac[4];
        bytes[15] = mac[5];
        Self(bytes)
    }
    
    /// Creates a solicited-node multicast address.
    pub fn solicited_node(&self) -> Self {
        let mut bytes = [0u8; 16];
        bytes[..13].copy_from_slice(&Self::SOLICITED_NODE_PREFIX);
        bytes[13] = self.0[13];
        bytes[14] = self.0[14];
        bytes[15] = self.0[15];
        Self(bytes)
    }
    
    /// Creates from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() >= 16 {
            let mut arr = [0u8; 16];
            arr.copy_from_slice(&bytes[..16]);
            Some(Self(arr))
        } else {
            None
        }
    }
    
    /// Returns the bytes.
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
    
    /// Returns the eight 16-bit segments.
    pub fn segments(&self) -> [u16; 8] {
        let mut segs = [0u16; 8];
        for i in 0..8 {
            segs[i] = u16::from_be_bytes([self.0[i * 2], self.0[i * 2 + 1]]);
        }
        segs
    }
    
    /// Checks if this is the unspecified address.
    pub fn is_unspecified(&self) -> bool {
        *self == Self::UNSPECIFIED
    }
    
    /// Checks if this is a loopback address.
    pub fn is_loopback(&self) -> bool {
        *self == Self::LOOPBACK
    }
    
    /// Checks if this is a multicast address (ff00::/8).
    pub fn is_multicast(&self) -> bool {
        self.0[0] == 0xff
    }
    
    /// Checks if this is a link-local address (fe80::/10).
    pub fn is_link_local(&self) -> bool {
        self.0[0] == 0xfe && (self.0[1] & 0xc0) == 0x80
    }
    
    /// Checks if this is a unique local address (fc00::/7).
    pub fn is_unique_local(&self) -> bool {
        (self.0[0] & 0xfe) == 0xfc
    }
    
    /// Checks if this is a global unicast address (2000::/3).
    pub fn is_global(&self) -> bool {
        (self.0[0] & 0xe0) == 0x20
    }
    
    /// Checks if this is a documentation address (2001:db8::/32).
    pub fn is_documentation(&self) -> bool {
        self.0[0] == 0x20 && self.0[1] == 0x01 && self.0[2] == 0x0d && self.0[3] == 0xb8
    }
    
    /// Computes the multicast MAC address for this IPv6 multicast address.
    pub fn multicast_mac(&self) -> Option<[u8; 6]> {
        if !self.is_multicast() {
            return None;
        }
        Some([0x33, 0x33, self.0[12], self.0[13], self.0[14], self.0[15]])
    }
}

impl fmt::Display for Ipv6Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let segs = self.segments();
        
        // Find longest run of zeros for :: compression
        let mut best_start = 8;
        let mut best_len = 0;
        let mut cur_start = 0;
        let mut cur_len = 0;
        
        for i in 0..8 {
            if segs[i] == 0 {
                if cur_len == 0 {
                    cur_start = i;
                }
                cur_len += 1;
            } else {
                if cur_len > best_len {
                    best_start = cur_start;
                    best_len = cur_len;
                }
                cur_len = 0;
            }
        }
        if cur_len > best_len {
            best_start = cur_start;
            best_len = cur_len;
        }
        
        // Only compress if at least 2 consecutive zeros
        if best_len < 2 {
            best_start = 8;
            best_len = 0;
        }
        
        // Write address with compression
        let mut first = true;
        for i in 0..8 {
            if i == best_start {
                if first {
                    write!(f, ":")?;
                }
                write!(f, ":")?;
                first = false;
                continue;
            }
            if i > best_start && i < best_start + best_len {
                continue;
            }
            if !first {
                write!(f, ":")?;
            }
            write!(f, "{:x}", segs[i])?;
            first = false;
        }
        
        Ok(())
    }
}

/// IPv6 packet header.
#[derive(Debug, Clone)]
pub struct Ipv6Packet {
    /// Version (4 bits) + Traffic class (8 bits) + Flow label (20 bits)
    pub version_tc_flow: u32,
    /// Payload length (not including header)
    pub payload_length: u16,
    /// Next header type
    pub next_header: u8,
    /// Hop limit (like TTL)
    pub hop_limit: u8,
    /// Source address
    pub src_addr: Ipv6Address,
    /// Destination address
    pub dest_addr: Ipv6Address,
    /// Extension headers
    pub extension_headers: Vec<ExtensionHeader>,
    /// Payload
    pub payload: Vec<u8>,
}

/// IPv6 extension header.
#[derive(Debug, Clone)]
pub struct ExtensionHeader {
    /// Header type
    pub header_type: u8,
    /// Next header type
    pub next_header: u8,
    /// Header data (excluding type and next header)
    pub data: Vec<u8>,
}

impl Ipv6Packet {
    /// IPv6 header size (fixed 40 bytes)
    pub const HEADER_SIZE: usize = 40;
    /// Default hop limit
    pub const DEFAULT_HOP_LIMIT: u8 = 64;
    /// IPv6 version
    pub const VERSION: u8 = 6;
    
    /// Creates a new IPv6 packet.
    pub fn new(
        src_addr: Ipv6Address,
        dest_addr: Ipv6Address,
        next_header: u8,
        payload: Vec<u8>,
    ) -> Self {
        let version_tc_flow = (6u32 << 28); // Version 6
        
        Self {
            version_tc_flow,
            payload_length: payload.len() as u16,
            next_header,
            hop_limit: Self::DEFAULT_HOP_LIMIT,
            src_addr,
            dest_addr,
            extension_headers: Vec::new(),
            payload,
        }
    }
    
    /// Creates a new IPv6 packet with traffic class and flow label.
    pub fn with_flow(
        src_addr: Ipv6Address,
        dest_addr: Ipv6Address,
        next_header: u8,
        traffic_class: u8,
        flow_label: u32,
        payload: Vec<u8>,
    ) -> Self {
        let version_tc_flow = (6u32 << 28) | ((traffic_class as u32) << 20) | (flow_label & 0xFFFFF);
        
        Self {
            version_tc_flow,
            payload_length: payload.len() as u16,
            next_header,
            hop_limit: Self::DEFAULT_HOP_LIMIT,
            src_addr,
            dest_addr,
            extension_headers: Vec::new(),
            payload,
        }
    }
    
    /// Returns the IP version.
    pub fn version(&self) -> u8 {
        ((self.version_tc_flow >> 28) & 0xF) as u8
    }
    
    /// Returns the traffic class.
    pub fn traffic_class(&self) -> u8 {
        ((self.version_tc_flow >> 20) & 0xFF) as u8
    }
    
    /// Returns the flow label.
    pub fn flow_label(&self) -> u32 {
        self.version_tc_flow & 0xFFFFF
    }
    
    /// Parses an IPv6 packet from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::HEADER_SIZE {
            return None;
        }
        
        let version_tc_flow = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let version = (version_tc_flow >> 28) as u8;
        
        if version != 6 {
            return None;
        }
        
        let payload_length = u16::from_be_bytes([data[4], data[5]]);
        let next_header = data[6];
        let hop_limit = data[7];
        let src_addr = Ipv6Address::from_bytes(&data[8..24])?;
        let dest_addr = Ipv6Address::from_bytes(&data[24..40])?;
        
        let expected_total = Self::HEADER_SIZE + payload_length as usize;
        if data.len() < expected_total {
            return None;
        }
        
        // Parse extension headers
        let (extension_headers, final_next_header, payload_start) = 
            Self::parse_extension_headers(&data[40..expected_total], next_header)?;
        
        let payload = data[40 + payload_start..expected_total].to_vec();
        
        Some(Self {
            version_tc_flow,
            payload_length,
            next_header: final_next_header,
            hop_limit,
            src_addr,
            dest_addr,
            extension_headers,
            payload,
        })
    }
    
    /// Parses extension headers.
    fn parse_extension_headers(data: &[u8], first_next_header: u8) -> Option<(Vec<ExtensionHeader>, u8, usize)> {
        let mut headers = Vec::new();
        let mut offset = 0;
        let mut next_header = first_next_header;
        
        loop {
            // Check if this is an upper-layer protocol (not an extension header)
            match next_header {
                PROTOCOL_TCP | PROTOCOL_UDP | PROTOCOL_ICMPV6 | PROTOCOL_NONE => {
                    break;
                }
                PROTOCOL_HOP_BY_HOP | PROTOCOL_ROUTING | PROTOCOL_DEST_OPTIONS => {
                    // Standard extension header format
                    if offset + 2 > data.len() {
                        return None;
                    }
                    let nh = data[offset];
                    let hdr_len = ((data[offset + 1] as usize) + 1) * 8;
                    if offset + hdr_len > data.len() {
                        return None;
                    }
                    headers.push(ExtensionHeader {
                        header_type: next_header,
                        next_header: nh,
                        data: data[offset + 2..offset + hdr_len].to_vec(),
                    });
                    next_header = nh;
                    offset += hdr_len;
                }
                PROTOCOL_FRAGMENT => {
                    // Fragment header is always 8 bytes
                    if offset + 8 > data.len() {
                        return None;
                    }
                    let nh = data[offset];
                    headers.push(ExtensionHeader {
                        header_type: next_header,
                        next_header: nh,
                        data: data[offset + 2..offset + 8].to_vec(),
                    });
                    next_header = nh;
                    offset += 8;
                }
                _ => {
                    // Unknown extension header or upper-layer protocol
                    break;
                }
            }
        }
        
        Some((headers, next_header, offset))
    }
    
    /// Serializes the packet to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let total_size = Self::HEADER_SIZE + self.extension_headers_size() + self.payload.len();
        let mut bytes = Vec::with_capacity(total_size);
        
        // Fixed header
        bytes.extend_from_slice(&self.version_tc_flow.to_be_bytes());
        bytes.extend_from_slice(&self.payload_length.to_be_bytes());
        bytes.push(self.next_header);
        bytes.push(self.hop_limit);
        bytes.extend_from_slice(&self.src_addr.0);
        bytes.extend_from_slice(&self.dest_addr.0);
        
        // Extension headers
        for hdr in &self.extension_headers {
            bytes.push(hdr.next_header);
            bytes.push(((hdr.data.len() + 2) / 8 - 1) as u8);
            bytes.extend_from_slice(&hdr.data);
        }
        
        // Payload
        bytes.extend_from_slice(&self.payload);
        
        bytes
    }
    
    /// Returns the total size of extension headers.
    fn extension_headers_size(&self) -> usize {
        self.extension_headers.iter().map(|h| h.data.len() + 2).sum()
    }
    
    /// Decrements hop limit and returns true if still valid.
    pub fn decrement_hop_limit(&mut self) -> bool {
        if self.hop_limit > 0 {
            self.hop_limit -= 1;
            true
        } else {
            false
        }
    }
}

/// ICMPv6 message types.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icmpv6Type {
    /// Destination unreachable
    DestUnreachable = 1,
    /// Packet too big
    PacketTooBig = 2,
    /// Time exceeded
    TimeExceeded = 3,
    /// Parameter problem
    ParameterProblem = 4,
    /// Echo request
    EchoRequest = 128,
    /// Echo reply
    EchoReply = 129,
    /// Router solicitation
    RouterSolicitation = 133,
    /// Router advertisement
    RouterAdvertisement = 134,
    /// Neighbor solicitation
    NeighborSolicitation = 135,
    /// Neighbor advertisement
    NeighborAdvertisement = 136,
    /// Redirect
    Redirect = 137,
}

impl Icmpv6Type {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            1 => Some(Self::DestUnreachable),
            2 => Some(Self::PacketTooBig),
            3 => Some(Self::TimeExceeded),
            4 => Some(Self::ParameterProblem),
            128 => Some(Self::EchoRequest),
            129 => Some(Self::EchoReply),
            133 => Some(Self::RouterSolicitation),
            134 => Some(Self::RouterAdvertisement),
            135 => Some(Self::NeighborSolicitation),
            136 => Some(Self::NeighborAdvertisement),
            137 => Some(Self::Redirect),
            _ => None,
        }
    }
}

/// ICMPv6 packet.
#[derive(Debug, Clone)]
pub struct Icmpv6Packet {
    /// Message type
    pub msg_type: u8,
    /// Code
    pub code: u8,
    /// Checksum
    pub checksum: u16,
    /// Message body
    pub body: Vec<u8>,
}

impl Icmpv6Packet {
    /// Creates a new ICMPv6 packet.
    pub fn new(msg_type: u8, code: u8, body: Vec<u8>) -> Self {
        let mut packet = Self {
            msg_type,
            code,
            checksum: 0,
            body,
        };
        packet
    }
    
    /// Creates an echo request.
    pub fn echo_request(identifier: u16, sequence: u16, data: &[u8]) -> Self {
        let mut body = Vec::with_capacity(4 + data.len());
        body.extend_from_slice(&identifier.to_be_bytes());
        body.extend_from_slice(&sequence.to_be_bytes());
        body.extend_from_slice(data);
        Self::new(Icmpv6Type::EchoRequest as u8, 0, body)
    }
    
    /// Creates an echo reply.
    pub fn echo_reply(identifier: u16, sequence: u16, data: &[u8]) -> Self {
        let mut body = Vec::with_capacity(4 + data.len());
        body.extend_from_slice(&identifier.to_be_bytes());
        body.extend_from_slice(&sequence.to_be_bytes());
        body.extend_from_slice(data);
        Self::new(Icmpv6Type::EchoReply as u8, 0, body)
    }
    
    /// Creates a neighbor solicitation.
    pub fn neighbor_solicitation(target: Ipv6Address, source_mac: Option<[u8; 6]>) -> Self {
        let mut body = Vec::with_capacity(24);
        body.extend_from_slice(&[0u8; 4]); // Reserved
        body.extend_from_slice(&target.0);
        
        // Source link-layer address option
        if let Some(mac) = source_mac {
            body.push(1); // Type: Source Link-Layer Address
            body.push(1); // Length: 1 (8 bytes)
            body.extend_from_slice(&mac);
        }
        
        Self::new(Icmpv6Type::NeighborSolicitation as u8, 0, body)
    }
    
    /// Creates a neighbor advertisement.
    pub fn neighbor_advertisement(target: Ipv6Address, target_mac: [u8; 6], solicited: bool, override_flag: bool) -> Self {
        let mut body = Vec::with_capacity(32);
        
        // Flags: R=0, S=solicited, O=override
        let flags = ((solicited as u8) << 6) | ((override_flag as u8) << 5);
        body.push(flags);
        body.extend_from_slice(&[0u8; 3]); // Reserved
        body.extend_from_slice(&target.0);
        
        // Target link-layer address option
        body.push(2); // Type: Target Link-Layer Address
        body.push(1); // Length: 1 (8 bytes)
        body.extend_from_slice(&target_mac);
        
        Self::new(Icmpv6Type::NeighborAdvertisement as u8, 0, body)
    }
    
    /// Creates a router solicitation.
    pub fn router_solicitation(source_mac: Option<[u8; 6]>) -> Self {
        let mut body = Vec::with_capacity(16);
        body.extend_from_slice(&[0u8; 4]); // Reserved
        
        // Source link-layer address option
        if let Some(mac) = source_mac {
            body.push(1); // Type: Source Link-Layer Address
            body.push(1); // Length: 1 (8 bytes)
            body.extend_from_slice(&mac);
        }
        
        Self::new(Icmpv6Type::RouterSolicitation as u8, 0, body)
    }
    
    /// Parses an ICMPv6 packet.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }
        
        Some(Self {
            msg_type: data[0],
            code: data[1],
            checksum: u16::from_be_bytes([data[2], data[3]]),
            body: data[4..].to_vec(),
        })
    }
    
    /// Serializes the packet.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + self.body.len());
        bytes.push(self.msg_type);
        bytes.push(self.code);
        bytes.extend_from_slice(&self.checksum.to_be_bytes());
        bytes.extend_from_slice(&self.body);
        bytes
    }
    
    /// Computes the ICMPv6 checksum (requires pseudo-header).
    pub fn compute_checksum(&mut self, src: &Ipv6Address, dst: &Ipv6Address) {
        // Build pseudo-header
        let icmp_len = 4 + self.body.len();
        let mut pseudo = Vec::with_capacity(40 + icmp_len);
        pseudo.extend_from_slice(&src.0);
        pseudo.extend_from_slice(&dst.0);
        pseudo.extend_from_slice(&(icmp_len as u32).to_be_bytes());
        pseudo.extend_from_slice(&[0, 0, 0, PROTOCOL_ICMPV6]);
        
        // Add ICMPv6 message (with checksum = 0 for calculation per RFC 1071)
        pseudo.push(self.msg_type);
        pseudo.push(self.code);
        pseudo.extend_from_slice(&[0, 0]); // Checksum field zeroed for calculation
        pseudo.extend_from_slice(&self.body);
        
        // Pad to even length
        if pseudo.len() % 2 != 0 {
            pseudo.push(0);
        }
        
        // Compute checksum
        let mut sum: u32 = 0;
        for i in (0..pseudo.len()).step_by(2) {
            sum += u16::from_be_bytes([pseudo[i], pseudo[i + 1]]) as u32;
        }
        
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        
        self.checksum = !(sum as u16);
    }
}

/// Neighbor cache entry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborState {
    /// Incomplete: resolution in progress
    Incomplete,
    /// Reachable: recently confirmed reachable
    Reachable,
    /// Stale: may be unreachable
    Stale,
    /// Delay: waiting before probing
    Delay,
    /// Probe: actively probing
    Probe,
}

/// Neighbor cache entry.
#[derive(Debug, Clone)]
pub struct NeighborEntry {
    /// IPv6 address
    pub address: Ipv6Address,
    /// Link-layer address (MAC)
    pub link_addr: Option<[u8; 6]>,
    /// State
    pub state: NeighborState,
    /// Last update timestamp
    pub last_update: u64,
    /// Number of solicitation attempts
    pub probe_count: u8,
}

impl NeighborEntry {
    /// Creates a new incomplete entry.
    pub fn new_incomplete(address: Ipv6Address) -> Self {
        Self {
            address,
            link_addr: None,
            state: NeighborState::Incomplete,
            last_update: 0,
            probe_count: 0,
        }
    }
    
    /// Creates a new reachable entry.
    pub fn new_reachable(address: Ipv6Address, link_addr: [u8; 6]) -> Self {
        Self {
            address,
            link_addr: Some(link_addr),
            state: NeighborState::Reachable,
            last_update: 0,
            probe_count: 0,
        }
    }
}

/// Neighbor Discovery Protocol cache.
pub struct NeighborCache {
    entries: spin::Mutex<alloc::collections::BTreeMap<Ipv6Address, NeighborEntry>>,
}

impl NeighborCache {
    /// Creates a new neighbor cache.
    pub fn new() -> Self {
        Self {
            entries: spin::Mutex::new(alloc::collections::BTreeMap::new()),
        }
    }
    
    /// Looks up a neighbor entry.
    pub fn lookup(&self, addr: Ipv6Address) -> Option<NeighborEntry> {
        self.entries.lock().get(&addr).cloned()
    }
    
    /// Inserts or updates a neighbor entry.
    pub fn insert(&self, entry: NeighborEntry) {
        self.entries.lock().insert(entry.address, entry);
    }
    
    /// Updates an existing entry to reachable state.
    pub fn update_reachable(&self, addr: Ipv6Address, link_addr: [u8; 6]) {
        let mut entries = self.entries.lock();
        if let Some(entry) = entries.get_mut(&addr) {
            entry.link_addr = Some(link_addr);
            entry.state = NeighborState::Reachable;
            entry.probe_count = 0;
        } else {
            entries.insert(addr, NeighborEntry::new_reachable(addr, link_addr));
        }
    }
    
    /// Marks an entry as stale.
    pub fn mark_stale(&self, addr: Ipv6Address) {
        if let Some(entry) = self.entries.lock().get_mut(&addr) {
            if entry.state == NeighborState::Reachable {
                entry.state = NeighborState::Stale;
            }
        }
    }
    
    /// Removes an entry.
    pub fn remove(&self, addr: Ipv6Address) {
        self.entries.lock().remove(&addr);
    }
    
    /// Returns all entries.
    pub fn all_entries(&self) -> alloc::vec::Vec<NeighborEntry> {
        self.entries.lock().values().cloned().collect()
    }
}

impl Default for NeighborCache {
    fn default() -> Self {
        Self::new()
    }
}

/// IPv6 interface configuration.
#[derive(Debug, Clone)]
pub struct Ipv6Config {
    /// Link-local address
    pub link_local: Ipv6Address,
    /// Global addresses
    pub global_addrs: Vec<Ipv6Address>,
    /// Default gateway
    pub gateway: Option<Ipv6Address>,
    /// MTU
    pub mtu: u16,
}

impl Ipv6Config {
    /// Creates a new IPv6 configuration from MAC address.
    pub fn from_mac(mac: [u8; 6]) -> Self {
        Self {
            link_local: Ipv6Address::from_mac(mac),
            global_addrs: Vec::new(),
            gateway: None,
            mtu: 1500,
        }
    }
}

//! # IP Layer
//!
//! Internet Protocol handling, routing, and fragmentation.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

/// IP protocols
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IpProtocol {
    Icmp = 1,
    Tcp = 6,
    Udp = 17,
    Icmpv6 = 58,
}

impl TryFrom<u8> for IpProtocol {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(IpProtocol::Icmp),
            6 => Ok(IpProtocol::Tcp),
            17 => Ok(IpProtocol::Udp),
            58 => Ok(IpProtocol::Icmpv6),
            _ => Err(()),
        }
    }
}

/// IPv4 header
#[derive(Debug, Clone)]
pub struct Ipv4Header {
    /// Version (always 4)
    pub version: u8,
    /// Header length (in 32-bit words)
    pub ihl: u8,
    /// Type of Service / DSCP + ECN
    pub tos: u8,
    /// Total length (header + data)
    pub total_length: u16,
    /// Identification
    pub identification: u16,
    /// Flags (3 bits)
    pub flags: u8,
    /// Fragment offset (13 bits)
    pub fragment_offset: u16,
    /// Time to Live
    pub ttl: u8,
    /// Protocol
    pub protocol: u8,
    /// Header checksum
    pub checksum: u16,
    /// Source IP address
    pub src_addr: u32,
    /// Destination IP address
    pub dst_addr: u32,
    /// Options (if ihl > 5)
    pub options: Vec<u8>,
}

impl Ipv4Header {
    /// Minimum header size (20 bytes)
    pub const MIN_SIZE: usize = 20;
    /// Default TTL
    pub const DEFAULT_TTL: u8 = 64;

    /// Creates a new IPv4 header
    pub fn new(src: u32, dst: u32, protocol: u8, payload_len: usize) -> Self {
        Self {
            version: 4,
            ihl: 5,
            tos: 0,
            total_length: (Self::MIN_SIZE + payload_len) as u16,
            identification: 0,
            flags: 0x02, // Don't Fragment
            fragment_offset: 0,
            ttl: Self::DEFAULT_TTL,
            protocol,
            checksum: 0,
            src_addr: src,
            dst_addr: dst,
            options: Vec::new(),
        }
    }

    /// Parses an IPv4 header from bytes
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < Self::MIN_SIZE {
            return None;
        }

        let version = data[0] >> 4;
        let ihl = data[0] & 0x0F;
        
        if version != 4 || ihl < 5 {
            return None;
        }

        let header_len = (ihl as usize) * 4;
        if data.len() < header_len {
            return None;
        }

        let tos = data[1];
        let total_length = u16::from_be_bytes([data[2], data[3]]);
        let identification = u16::from_be_bytes([data[4], data[5]]);
        let flags_frag = u16::from_be_bytes([data[6], data[7]]);
        let flags = (flags_frag >> 13) as u8;
        let fragment_offset = flags_frag & 0x1FFF;
        let ttl = data[8];
        let protocol = data[9];
        let checksum = u16::from_be_bytes([data[10], data[11]]);
        let src_addr = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        let dst_addr = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        let options = if header_len > Self::MIN_SIZE {
            data[Self::MIN_SIZE..header_len].to_vec()
        } else {
            Vec::new()
        };

        Some((
            Self {
                version,
                ihl,
                tos,
                total_length,
                identification,
                flags,
                fragment_offset,
                ttl,
                protocol,
                checksum,
                src_addr,
                dst_addr,
                options,
            },
            header_len,
        ))
    }

    /// Serializes the header to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let header_len = (self.ihl as usize) * 4;
        let mut buf = Vec::with_capacity(header_len);

        buf.push((self.version << 4) | self.ihl);
        buf.push(self.tos);
        buf.extend_from_slice(&self.total_length.to_be_bytes());
        buf.extend_from_slice(&self.identification.to_be_bytes());

        let flags_frag = ((self.flags as u16) << 13) | self.fragment_offset;
        buf.extend_from_slice(&flags_frag.to_be_bytes());

        buf.push(self.ttl);
        buf.push(self.protocol);
        buf.extend_from_slice(&self.checksum.to_be_bytes());
        buf.extend_from_slice(&self.src_addr.to_be_bytes());
        buf.extend_from_slice(&self.dst_addr.to_be_bytes());
        buf.extend_from_slice(&self.options);

        // Pad to 32-bit boundary if needed
        while buf.len() < header_len {
            buf.push(0);
        }

        buf
    }

    /// Calculates and sets the header checksum
    pub fn calculate_checksum(&mut self) {
        self.checksum = 0;
        let data = self.serialize();
        self.checksum = Self::compute_checksum(&data);
    }

    /// Computes Internet checksum
    fn compute_checksum(data: &[u8]) -> u16 {
        let mut sum: u32 = 0;

        let mut i = 0;
        while i + 1 < data.len() {
            sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
            i += 2;
        }
        if i < data.len() {
            sum += (data[i] as u32) << 8;
        }

        while sum > 0xFFFF {
            sum = (sum >> 16) + (sum & 0xFFFF);
        }

        !(sum as u16)
    }

    /// Verifies header checksum
    pub fn verify_checksum(&self) -> bool {
        let data = self.serialize();
        Self::compute_checksum(&data) == 0
    }

    /// Returns payload length
    pub fn payload_len(&self) -> usize {
        self.total_length as usize - (self.ihl as usize * 4)
    }
}

/// IP packet (header + payload)
#[derive(Debug, Clone)]
pub struct IpPacket {
    /// Header
    pub header: Ipv4Header,
    /// Payload
    pub payload: Vec<u8>,
}

impl IpPacket {
    /// Creates a new IP packet
    pub fn new(src: u32, dst: u32, protocol: u8, payload: Vec<u8>) -> Self {
        let header = Ipv4Header::new(src, dst, protocol, payload.len());
        Self { header, payload }
    }

    /// Parses an IP packet from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        let (header, header_len) = Ipv4Header::parse(data)?;
        let payload_end = header.total_length as usize;
        
        if data.len() < payload_end {
            return None;
        }

        let payload = data[header_len..payload_end].to_vec();
        Some(Self { header, payload })
    }

    /// Serializes the packet to bytes
    pub fn serialize(&mut self) -> Vec<u8> {
        self.header.calculate_checksum();
        let mut buf = self.header.serialize();
        buf.extend_from_slice(&self.payload);
        buf
    }
}

/// Routing table entry
#[derive(Debug, Clone)]
pub struct RouteEntry {
    /// Destination network
    pub dest: u32,
    /// Netmask
    pub netmask: u32,
    /// Gateway (0 = direct)
    pub gateway: u32,
    /// Interface index
    pub interface: usize,
    /// Metric (lower = preferred)
    pub metric: u32,
}

impl RouteEntry {
    /// Checks if an IP matches this route
    pub fn matches(&self, ip: u32) -> bool {
        (ip & self.netmask) == (self.dest & self.netmask)
    }

    /// Returns prefix length (for comparison)
    pub fn prefix_len(&self) -> u32 {
        self.netmask.count_ones()
    }
}

/// Routing table
pub struct RoutingTable {
    /// Routes
    routes: Vec<RouteEntry>,
    /// Default gateway
    default_gateway: Option<u32>,
}

impl RoutingTable {
    /// Creates a new routing table
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            default_gateway: None,
        }
    }

    /// Adds a route
    pub fn add_route(&mut self, route: RouteEntry) {
        // Check for default route
        if route.dest == 0 && route.netmask == 0 {
            self.default_gateway = Some(route.gateway);
        }
        self.routes.push(route);
    }

    /// Removes a route
    pub fn remove_route(&mut self, dest: u32, netmask: u32) {
        self.routes.retain(|r| r.dest != dest || r.netmask != netmask);
    }

    /// Looks up the best route for a destination
    pub fn lookup(&self, dest: u32) -> Option<&RouteEntry> {
        // Longest prefix match
        let mut best: Option<&RouteEntry> = None;
        let mut best_prefix_len = 0;
        let mut best_metric = u32::MAX;

        for route in &self.routes {
            if route.matches(dest) {
                let prefix_len = route.prefix_len();
                if prefix_len > best_prefix_len
                    || (prefix_len == best_prefix_len && route.metric < best_metric)
                {
                    best = Some(route);
                    best_prefix_len = prefix_len;
                    best_metric = route.metric;
                }
            }
        }

        best
    }

    /// Gets the next hop for a destination
    pub fn get_next_hop(&self, dest: u32) -> Option<(u32, usize)> {
        if let Some(route) = self.lookup(dest) {
            let next_hop = if route.gateway == 0 {
                dest // Direct route
            } else {
                route.gateway
            };
            Some((next_hop, route.interface))
        } else if let Some(gw) = self.default_gateway {
            // Use default gateway (assume interface 0)
            Some((gw, 0))
        } else {
            None
        }
    }

    /// Sets default gateway
    pub fn set_default_gateway(&mut self, gateway: u32) {
        self.default_gateway = Some(gateway);
    }

    /// Lists all routes
    pub fn list_routes(&self) -> &[RouteEntry] {
        &self.routes
    }
}

impl Default for RoutingTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Fragment reassembly entry
struct FragmentEntry {
    /// Source IP
    src: u32,
    /// Identification
    id: u16,
    /// Protocol
    protocol: u8,
    /// Fragments received (offset -> data)
    fragments: BTreeMap<u16, Vec<u8>>,
    /// Total expected length (set when last fragment received)
    total_len: Option<usize>,
    /// First received timestamp
    timestamp: u64,
}

/// IP fragmentation/reassembly
pub struct FragmentManager {
    /// Pending reassembly entries
    entries: BTreeMap<(u32, u16), FragmentEntry>,
    /// Maximum fragment lifetime (ms)
    max_lifetime: u64,
    /// Current time
    current_time: u64,
    /// Next identification number
    next_id: u16,
}

impl FragmentManager {
    /// Creates a new fragment manager
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            max_lifetime: 30000, // 30 seconds
            current_time: 0,
            next_id: 1,
        }
    }

    /// Fragments a packet if needed
    pub fn fragment(&mut self, packet: IpPacket, mtu: usize) -> Vec<IpPacket> {
        let header_len = (packet.header.ihl as usize) * 4;
        let max_payload = ((mtu - header_len) / 8) * 8; // Must be multiple of 8

        if packet.payload.len() <= mtu - header_len {
            return vec![packet];
        }

        // Check if Don't Fragment flag is set
        if packet.header.flags & 0x02 != 0 {
            return vec![]; // Can't fragment
        }

        let mut fragments = Vec::new();
        let mut offset = 0usize;
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        while offset < packet.payload.len() {
            let remaining = packet.payload.len() - offset;
            let frag_len = remaining.min(max_payload);
            let more_fragments = offset + frag_len < packet.payload.len();

            let mut header = packet.header.clone();
            header.identification = id;
            header.flags = if more_fragments { 0x01 } else { 0x00 };
            header.fragment_offset = (offset / 8) as u16;
            header.total_length = (header_len + frag_len) as u16;

            fragments.push(IpPacket {
                header,
                payload: packet.payload[offset..offset + frag_len].to_vec(),
            });

            offset += frag_len;
        }

        fragments
    }

    /// Reassembles a fragment, returns complete packet if ready
    pub fn reassemble(&mut self, packet: IpPacket) -> Option<IpPacket> {
        let key = (packet.header.src_addr, packet.header.identification);
        let offset = packet.header.fragment_offset * 8;
        let more_fragments = packet.header.flags & 0x01 != 0;

        let entry = self.entries.entry(key).or_insert_with(|| FragmentEntry {
            src: packet.header.src_addr,
            id: packet.header.identification,
            protocol: packet.header.protocol,
            fragments: BTreeMap::new(),
            total_len: None,
            timestamp: self.current_time,
        });

        entry.fragments.insert(offset as u16, packet.payload.clone());

        // If this is the last fragment, calculate total length
        if !more_fragments {
            entry.total_len = Some(offset as usize + packet.payload.len());
        }

        // Try to reassemble
        if let Some(total_len) = entry.total_len {
            let mut current_offset = 0usize;
            let mut complete = true;
            let mut data = Vec::with_capacity(total_len);

            for (&frag_offset, frag_data) in &entry.fragments {
                if frag_offset as usize != current_offset {
                    complete = false;
                    break;
                }
                data.extend_from_slice(frag_data);
                current_offset += frag_data.len();
            }

            if complete && current_offset == total_len {
                let entry = self.entries.remove(&key).unwrap();
                let mut header = Ipv4Header::new(
                    entry.src,
                    packet.header.dst_addr,
                    entry.protocol,
                    data.len(),
                );
                header.identification = entry.id;
                return Some(IpPacket {
                    header,
                    payload: data,
                });
            }
        }

        None
    }

    /// Updates time and cleans up expired entries
    pub fn update_time(&mut self, time: u64) {
        self.current_time = time;
        self.entries
            .retain(|_, e| time - e.timestamp < self.max_lifetime);
    }
}

impl Default for FragmentManager {
    fn default() -> Self {
        Self::new()
    }
}

/// IP address utilities
pub mod addr {
    use alloc::vec::Vec;
    
    /// Converts an IP address string to u32
    pub fn parse_ipv4(s: &str) -> Option<u32> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 4 {
            return None;
        }

        let mut result: u32 = 0;
        for part in parts {
            let octet = part.parse::<u8>().ok()?;
            result = (result << 8) | (octet as u32);
        }
        Some(result)
    }

    /// Formats a u32 IP address as string
    pub fn format_ipv4(ip: u32) -> [u8; 4] {
        [
            ((ip >> 24) & 0xFF) as u8,
            ((ip >> 16) & 0xFF) as u8,
            ((ip >> 8) & 0xFF) as u8,
            (ip & 0xFF) as u8,
        ]
    }

    /// Checks if IP is private (RFC 1918)
    pub fn is_private(ip: u32) -> bool {
        let a = (ip >> 24) & 0xFF;
        let b = (ip >> 16) & 0xFF;

        // 10.0.0.0/8
        if a == 10 {
            return true;
        }
        // 172.16.0.0/12
        if a == 172 && (16..=31).contains(&b) {
            return true;
        }
        // 192.168.0.0/16
        if a == 192 && b == 168 {
            return true;
        }
        false
    }

    /// Checks if IP is loopback
    pub fn is_loopback(ip: u32) -> bool {
        (ip >> 24) == 127
    }

    /// Checks if IP is broadcast
    pub fn is_broadcast(ip: u32) -> bool {
        ip == 0xFFFFFFFF
    }

    /// Checks if IP is multicast (224.0.0.0 - 239.255.255.255)
    pub fn is_multicast(ip: u32) -> bool {
        let first = (ip >> 24) & 0xFF;
        (224..=239).contains(&first)
    }
}

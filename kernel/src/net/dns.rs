//! # DNS Resolver
//!
//! Provides DNS resolution functionality for SplaxOS.
//! Supports standard DNS queries (A, AAAA, MX, TXT, NS, CNAME, PTR).

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use crate::net::{Ipv4Address, NetworkError};

/// DNS server addresses
pub const DNS_GOOGLE_PRIMARY: Ipv4Address = Ipv4Address::new_const(8, 8, 8, 8);
pub const DNS_GOOGLE_SECONDARY: Ipv4Address = Ipv4Address::new_const(8, 8, 4, 4);
pub const DNS_CLOUDFLARE: Ipv4Address = Ipv4Address::new_const(1, 1, 1, 1);
pub const DNS_QUAD9: Ipv4Address = Ipv4Address::new_const(9, 9, 9, 9);

/// DNS port
pub const DNS_PORT: u16 = 53;

/// DNS record types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum RecordType {
    A = 1,      // IPv4 address
    NS = 2,     // Name server
    CNAME = 5,  // Canonical name
    SOA = 6,    // Start of authority
    PTR = 12,   // Pointer (reverse DNS)
    MX = 15,    // Mail exchange
    TXT = 16,   // Text record
    AAAA = 28,  // IPv6 address
    SRV = 33,   // Service record
    ANY = 255,  // Any record
}

impl RecordType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "A" => Some(Self::A),
            "NS" => Some(Self::NS),
            "CNAME" => Some(Self::CNAME),
            "SOA" => Some(Self::SOA),
            "PTR" => Some(Self::PTR),
            "MX" => Some(Self::MX),
            "TXT" => Some(Self::TXT),
            "AAAA" => Some(Self::AAAA),
            "SRV" => Some(Self::SRV),
            "ANY" | "*" => Some(Self::ANY),
            _ => None,
        }
    }
    
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::A => "A",
            Self::NS => "NS",
            Self::CNAME => "CNAME",
            Self::SOA => "SOA",
            Self::PTR => "PTR",
            Self::MX => "MX",
            Self::TXT => "TXT",
            Self::AAAA => "AAAA",
            Self::SRV => "SRV",
            Self::ANY => "ANY",
        }
    }
}

/// DNS query class
#[derive(Debug, Clone, Copy)]
#[repr(u16)]
pub enum QueryClass {
    IN = 1,   // Internet
    CS = 2,   // CSNET (obsolete)
    CH = 3,   // CHAOS
    HS = 4,   // Hesiod
    ANY = 255,
}

/// DNS header flags
#[derive(Debug, Clone, Copy)]
pub struct DnsFlags {
    pub qr: bool,        // Query (0) or Response (1)
    pub opcode: u8,      // 0 = standard query
    pub aa: bool,        // Authoritative answer
    pub tc: bool,        // Truncated
    pub rd: bool,        // Recursion desired
    pub ra: bool,        // Recursion available
    pub rcode: u8,       // Response code
}

impl DnsFlags {
    pub fn query() -> Self {
        Self {
            qr: false,
            opcode: 0,
            aa: false,
            tc: false,
            rd: true,  // Request recursion
            ra: false,
            rcode: 0,
        }
    }
    
    pub fn to_u16(&self) -> u16 {
        let mut flags: u16 = 0;
        if self.qr { flags |= 0x8000; }
        flags |= ((self.opcode as u16) & 0xF) << 11;
        if self.aa { flags |= 0x0400; }
        if self.tc { flags |= 0x0200; }
        if self.rd { flags |= 0x0100; }
        if self.ra { flags |= 0x0080; }
        flags |= (self.rcode as u16) & 0xF;
        flags
    }
    
    pub fn from_u16(value: u16) -> Self {
        Self {
            qr: (value & 0x8000) != 0,
            opcode: ((value >> 11) & 0xF) as u8,
            aa: (value & 0x0400) != 0,
            tc: (value & 0x0200) != 0,
            rd: (value & 0x0100) != 0,
            ra: (value & 0x0080) != 0,
            rcode: (value & 0xF) as u8,
        }
    }
}

/// DNS packet header
#[derive(Debug, Clone)]
pub struct DnsHeader {
    pub id: u16,
    pub flags: DnsFlags,
    pub qdcount: u16,  // Question count
    pub ancount: u16,  // Answer count
    pub nscount: u16,  // Authority count
    pub arcount: u16,  // Additional count
}

impl DnsHeader {
    pub fn new_query(id: u16) -> Self {
        Self {
            id,
            flags: DnsFlags::query(),
            qdcount: 1,
            ancount: 0,
            nscount: 0,
            arcount: 0,
        }
    }
    
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(12);
        bytes.extend_from_slice(&self.id.to_be_bytes());
        bytes.extend_from_slice(&self.flags.to_u16().to_be_bytes());
        bytes.extend_from_slice(&self.qdcount.to_be_bytes());
        bytes.extend_from_slice(&self.ancount.to_be_bytes());
        bytes.extend_from_slice(&self.nscount.to_be_bytes());
        bytes.extend_from_slice(&self.arcount.to_be_bytes());
        bytes
    }
    
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        Some(Self {
            id: u16::from_be_bytes([data[0], data[1]]),
            flags: DnsFlags::from_u16(u16::from_be_bytes([data[2], data[3]])),
            qdcount: u16::from_be_bytes([data[4], data[5]]),
            ancount: u16::from_be_bytes([data[6], data[7]]),
            nscount: u16::from_be_bytes([data[8], data[9]]),
            arcount: u16::from_be_bytes([data[10], data[11]]),
        })
    }
}

/// DNS question section
#[derive(Debug, Clone)]
pub struct DnsQuestion {
    pub name: String,
    pub qtype: RecordType,
    pub qclass: QueryClass,
}

impl DnsQuestion {
    pub fn new(name: &str, qtype: RecordType) -> Self {
        Self {
            name: String::from(name),
            qtype,
            qclass: QueryClass::IN,
        }
    }
    
    /// Encode domain name in DNS wire format
    fn encode_name(name: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        for label in name.split('.') {
            if label.is_empty() { continue; }
            bytes.push(label.len() as u8);
            bytes.extend_from_slice(label.as_bytes());
        }
        bytes.push(0); // Null terminator
        bytes
    }
    
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Self::encode_name(&self.name);
        bytes.extend_from_slice(&(self.qtype as u16).to_be_bytes());
        bytes.extend_from_slice(&(self.qclass as u16).to_be_bytes());
        bytes
    }
}

/// DNS resource record (answer)
#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub name: String,
    pub rtype: RecordType,
    pub rclass: u16,
    pub ttl: u32,
    pub rdata: Vec<u8>,
}

impl DnsRecord {
    /// Parse an IPv4 address from A record
    pub fn as_ipv4(&self) -> Option<Ipv4Address> {
        if self.rdata.len() >= 4 {
            Some(Ipv4Address::new(self.rdata[0], self.rdata[1], self.rdata[2], self.rdata[3]))
        } else {
            None
        }
    }
    
    /// Parse text from TXT record
    pub fn as_text(&self) -> Option<String> {
        if self.rdata.is_empty() {
            return None;
        }
        // TXT records have length-prefixed strings
        let mut text = String::new();
        let mut i = 0;
        while i < self.rdata.len() {
            let len = self.rdata[i] as usize;
            i += 1;
            if i + len <= self.rdata.len() {
                if let Ok(s) = core::str::from_utf8(&self.rdata[i..i+len]) {
                    text.push_str(s);
                }
            }
            i += len;
        }
        Some(text)
    }
}

/// DNS response
#[derive(Debug, Clone)]
pub struct DnsResponse {
    pub header: DnsHeader,
    pub questions: Vec<DnsQuestion>,
    pub answers: Vec<DnsRecord>,
    pub authorities: Vec<DnsRecord>,
    pub additional: Vec<DnsRecord>,
    pub query_time_ms: u32,
    pub server: Ipv4Address,
}

impl DnsResponse {
    /// Check if the response indicates an error
    pub fn is_error(&self) -> bool {
        self.header.flags.rcode != 0
    }
    
    /// Get error message
    pub fn error_message(&self) -> &'static str {
        match self.header.flags.rcode {
            0 => "No error",
            1 => "Format error",
            2 => "Server failure",
            3 => "Name error (NXDOMAIN)",
            4 => "Not implemented",
            5 => "Refused",
            _ => "Unknown error",
        }
    }
}

/// DNS query builder
pub struct DnsQuery {
    pub id: u16,
    pub name: String,
    pub qtype: RecordType,
    pub server: Ipv4Address,
}

impl DnsQuery {
    pub fn new(name: &str, qtype: RecordType, server: Ipv4Address) -> Self {
        // Simple ID based on name hash
        let id = name.bytes().fold(0u16, |acc, b| acc.wrapping_add(b as u16).wrapping_mul(31));
        Self {
            id,
            name: String::from(name),
            qtype,
            server,
        }
    }
    
    /// Build the DNS query packet
    pub fn build_packet(&self) -> Vec<u8> {
        let header = DnsHeader::new_query(self.id);
        let question = DnsQuestion::new(&self.name, self.qtype);
        
        let mut packet = header.to_bytes();
        packet.extend(question.to_bytes());
        packet
    }
}

/// DNS resolver state
pub struct DnsResolver {
    /// Primary DNS server
    pub primary: Ipv4Address,
    /// Secondary DNS server
    pub secondary: Ipv4Address,
    /// Timeout in milliseconds
    pub timeout_ms: u32,
}

impl Default for DnsResolver {
    fn default() -> Self {
        Self {
            primary: DNS_GOOGLE_PRIMARY,
            secondary: DNS_CLOUDFLARE,
            timeout_ms: 5000,
        }
    }
}

impl DnsResolver {
    pub fn new(primary: Ipv4Address, secondary: Ipv4Address) -> Self {
        Self {
            primary,
            secondary,
            timeout_ms: 5000,
        }
    }
    
    /// Resolve a hostname to IPv4 address
    pub fn resolve(&self, hostname: &str) -> Result<Ipv4Address, NetworkError> {
        // First check if it's already an IP address
        if let Some(ip) = parse_ipv4(hostname) {
            return Ok(ip);
        }
        
        // Build and send DNS query
        let query = DnsQuery::new(hostname, RecordType::A, self.primary);
        let response = self.send_query(&query)?;
        
        // Look for A record in answers
        for record in &response.answers {
            if let Some(ip) = record.as_ipv4() {
                return Ok(ip);
            }
        }
        
        Err(NetworkError::DnsError)
    }
    
    /// Send a DNS query and wait for response
    fn send_query(&self, query: &DnsQuery) -> Result<DnsResponse, NetworkError> {
        use crate::net::udp::{udp_state, UdpEndpoint, UdpDatagram};
        use crate::net::{NETWORK_STACK, ip};
        
        // Build the DNS query packet
        let packet = query.build_packet();
        
        // Allocate an ephemeral port for the response
        let local_port = udp_state().lock().allocate_port();
        let local_addr = {
            let stack = NETWORK_STACK.lock();
            stack.primary_interface()
                .map(|iface| iface.config.ipv4_addr)
                .unwrap_or(Ipv4Address::new(10, 0, 2, 15))
        };
        
        // Bind the UDP socket
        let _ = udp_state().lock().bind(local_port, local_addr);
        
        // Create UDP datagram
        let datagram = UdpDatagram::new(
            local_port,
            DNS_PORT,
            packet.clone(),
        );
        
        // Send via network stack
        {
            let stack = NETWORK_STACK.lock();
            if let Some(interface) = stack.primary_interface() {
                let udp_bytes = datagram.to_bytes(local_addr, query.server);
                let ip_packet = crate::net::Ipv4Packet {
                    version: 4,
                    ihl: 5,
                    dscp: 0,
                    ecn: 0,
                    total_length: (20 + udp_bytes.len()) as u16,
                    identification: query.id,
                    flags: 0,
                    fragment_offset: 0,
                    ttl: 64,
                    protocol: ip::PROTOCOL_UDP,
                    header_checksum: 0,
                    src_addr: local_addr,
                    dest_addr: query.server,
                    options: Vec::new(),
                    payload: udp_bytes,
                };
                interface.send_ipv4(&ip_packet)?;
            } else {
                return Err(NetworkError::NoInterface);
            }
        }
        
        // Wait for response with timeout
        // Approximate: 1 cycle ≈ 0.5ns on 2GHz CPU, so timeout in cycles
        let timeout_cycles = (self.timeout_ms as u64) * 2_000_000; // ~2GHz assumption
        let start = crate::arch::read_cycle_counter();
        
        loop {
            // Poll for incoming packets
            {
                let stack = NETWORK_STACK.lock();
                stack.poll();
            }
            
            // Check for response in UDP socket
            if let Some(response_data) = udp_state().lock().socket(local_port).and_then(|s| s.recv()) {
                // Parse DNS response
                if let Some(response) = self.parse_response(&response_data.data, query) {
                    // Unbind the socket
                    let _ = udp_state().lock().unbind(local_port);
                    return Ok(response);
                }
            }
            
            // Check timeout
            let elapsed = crate::arch::read_cycle_counter().saturating_sub(start);
            if elapsed >= timeout_cycles {
                // Unbind the socket
                let _ = udp_state().lock().unbind(local_port);
                return Err(NetworkError::TimedOut);
            }
            
            // Yield CPU briefly
            core::hint::spin_loop();
        }
    }
    
    /// Parse a DNS response packet
    fn parse_response(&self, data: &[u8], query: &DnsQuery) -> Option<DnsResponse> {
        if data.len() < 12 {
            return None;
        }
        
        let header = DnsHeader::parse(data)?;
        
        // Verify this is a response to our query
        if header.id != query.id || !header.flags.qr {
            return None;
        }
        
        let mut offset = 12;
        
        // Skip questions
        let mut questions = Vec::new();
        for _ in 0..header.qdcount {
            let (name, new_offset) = self.parse_name(data, offset)?;
            offset = new_offset;
            if offset + 4 > data.len() { return None; }
            let qtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
            let _qclass = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
            offset += 4;
            
            if let Some(rtype) = self.u16_to_record_type(qtype) {
                questions.push(DnsQuestion::new(&name, rtype));
            }
        }
        
        // Parse answers
        let mut answers = Vec::new();
        for _ in 0..header.ancount {
            if let Some((record, new_offset)) = self.parse_record(data, offset) {
                answers.push(record);
                offset = new_offset;
            } else {
                break;
            }
        }
        
        Some(DnsResponse {
            header,
            questions,
            answers,
            authorities: Vec::new(),
            additional: Vec::new(),
            query_time_ms: 0,
            server: query.server,
        })
    }
    
    /// Parse a domain name from DNS wire format
    fn parse_name(&self, data: &[u8], mut offset: usize) -> Option<(String, usize)> {
        let mut name = String::new();
        let mut jumped = false;
        let mut return_offset = 0;
        
        loop {
            if offset >= data.len() { return None; }
            
            let len = data[offset] as usize;
            
            if len == 0 {
                offset += 1;
                break;
            }
            
            // Compression pointer
            if (len & 0xC0) == 0xC0 {
                if offset + 1 >= data.len() { return None; }
                let pointer = ((len & 0x3F) as usize) << 8 | data[offset + 1] as usize;
                if !jumped {
                    return_offset = offset + 2;
                    jumped = true;
                }
                offset = pointer;
                continue;
            }
            
            offset += 1;
            if offset + len > data.len() { return None; }
            
            if !name.is_empty() {
                name.push('.');
            }
            if let Ok(label) = core::str::from_utf8(&data[offset..offset + len]) {
                name.push_str(label);
            }
            offset += len;
        }
        
        Some((name, if jumped { return_offset } else { offset }))
    }
    
    /// Parse a resource record
    fn parse_record(&self, data: &[u8], offset: usize) -> Option<(DnsRecord, usize)> {
        let (name, mut offset) = self.parse_name(data, offset)?;
        
        if offset + 10 > data.len() { return None; }
        
        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let rclass = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
        let ttl = u32::from_be_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]);
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]) as usize;
        offset += 10;
        
        if offset + rdlength > data.len() { return None; }
        let rdata = data[offset..offset + rdlength].to_vec();
        offset += rdlength;
        
        let record = DnsRecord {
            name,
            rtype: self.u16_to_record_type(rtype).unwrap_or(RecordType::A),
            rclass,
            ttl,
            rdata,
        };
        
        Some((record, offset))
    }
    
    /// Convert u16 to RecordType
    fn u16_to_record_type(&self, value: u16) -> Option<RecordType> {
        match value {
            1 => Some(RecordType::A),
            2 => Some(RecordType::NS),
            5 => Some(RecordType::CNAME),
            6 => Some(RecordType::SOA),
            12 => Some(RecordType::PTR),
            15 => Some(RecordType::MX),
            16 => Some(RecordType::TXT),
            28 => Some(RecordType::AAAA),
            33 => Some(RecordType::SRV),
            255 => Some(RecordType::ANY),
            _ => None,
        }
    }
}

/// Parse an IPv4 address string
pub fn parse_ipv4(s: &str) -> Option<Ipv4Address> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    
    let mut octets = [0u8; 4];
    for (i, part) in parts.iter().enumerate() {
        match part.parse::<u8>() {
            Ok(n) => octets[i] = n,
            Err(_) => return None,
        }
    }
    
    Some(Ipv4Address::new(octets[0], octets[1], octets[2], octets[3]))
}

/// Format an IPv4 address for reverse DNS lookup
pub fn reverse_dns_name(ip: Ipv4Address) -> String {
    let octets = ip.octets();
    alloc::format!("{}.{}.{}.{}.in-addr.arpa", 
        octets[3], octets[2], octets[1], octets[0])
}

/// Global DNS resolver instance
use spin::Mutex;
pub static DNS_RESOLVER: Mutex<DnsResolver> = Mutex::new(DnsResolver {
    primary: DNS_GOOGLE_PRIMARY,
    secondary: DNS_CLOUDFLARE,
    timeout_ms: 5000,
});

/// Convenience function to resolve a hostname
pub fn resolve(hostname: &str) -> Result<Ipv4Address, NetworkError> {
    DNS_RESOLVER.lock().resolve(hostname)
}

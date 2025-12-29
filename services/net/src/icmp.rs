//! # ICMP Implementation
//!
//! Internet Control Message Protocol for ping, errors, etc.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// ICMP message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IcmpType {
    /// Echo reply
    EchoReply = 0,
    /// Destination unreachable
    DestUnreachable = 3,
    /// Source quench (deprecated)
    SourceQuench = 4,
    /// Redirect
    Redirect = 5,
    /// Echo request (ping)
    EchoRequest = 8,
    /// Router advertisement
    RouterAdvertisement = 9,
    /// Router solicitation
    RouterSolicitation = 10,
    /// Time exceeded
    TimeExceeded = 11,
    /// Parameter problem
    ParameterProblem = 12,
    /// Timestamp request
    TimestampRequest = 13,
    /// Timestamp reply
    TimestampReply = 14,
}

impl TryFrom<u8> for IcmpType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(IcmpType::EchoReply),
            3 => Ok(IcmpType::DestUnreachable),
            4 => Ok(IcmpType::SourceQuench),
            5 => Ok(IcmpType::Redirect),
            8 => Ok(IcmpType::EchoRequest),
            9 => Ok(IcmpType::RouterAdvertisement),
            10 => Ok(IcmpType::RouterSolicitation),
            11 => Ok(IcmpType::TimeExceeded),
            12 => Ok(IcmpType::ParameterProblem),
            13 => Ok(IcmpType::TimestampRequest),
            14 => Ok(IcmpType::TimestampReply),
            _ => Err(()),
        }
    }
}

/// Destination unreachable codes
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum DestUnreachableCode {
    NetUnreachable = 0,
    HostUnreachable = 1,
    ProtocolUnreachable = 2,
    PortUnreachable = 3,
    FragmentationNeeded = 4,
    SourceRouteFailed = 5,
    DestNetUnknown = 6,
    DestHostUnknown = 7,
    SourceHostIsolated = 8,
    NetAdminProhibited = 9,
    HostAdminProhibited = 10,
    NetUnreachableTos = 11,
    HostUnreachableTos = 12,
    CommAdminProhibited = 13,
    HostPrecedenceViolation = 14,
    PrecedenceCutoff = 15,
}

/// ICMP message
#[derive(Debug, Clone)]
pub struct IcmpMessage {
    /// Message type
    pub msg_type: u8,
    /// Code
    pub code: u8,
    /// Checksum
    pub checksum: u16,
    /// Rest of header (type-specific)
    pub header_data: u32,
    /// Payload
    pub payload: Vec<u8>,
}

impl IcmpMessage {
    /// ICMP header size
    pub const HEADER_SIZE: usize = 8;

    /// Creates an echo request (ping)
    pub fn echo_request(identifier: u16, sequence: u16, data: Vec<u8>) -> Self {
        Self {
            msg_type: IcmpType::EchoRequest as u8,
            code: 0,
            checksum: 0,
            header_data: ((identifier as u32) << 16) | (sequence as u32),
            payload: data,
        }
    }

    /// Creates an echo reply
    pub fn echo_reply(identifier: u16, sequence: u16, data: Vec<u8>) -> Self {
        Self {
            msg_type: IcmpType::EchoReply as u8,
            code: 0,
            checksum: 0,
            header_data: ((identifier as u32) << 16) | (sequence as u32),
            payload: data,
        }
    }

    /// Creates a destination unreachable message
    pub fn dest_unreachable(code: DestUnreachableCode, original_packet: &[u8]) -> Self {
        // Include IP header + first 8 bytes of original datagram
        let payload_len = original_packet.len().min(28);
        Self {
            msg_type: IcmpType::DestUnreachable as u8,
            code: code as u8,
            checksum: 0,
            header_data: 0,
            payload: original_packet[..payload_len].to_vec(),
        }
    }

    /// Creates a time exceeded message
    pub fn time_exceeded(code: u8, original_packet: &[u8]) -> Self {
        let payload_len = original_packet.len().min(28);
        Self {
            msg_type: IcmpType::TimeExceeded as u8,
            code,
            checksum: 0,
            header_data: 0,
            payload: original_packet[..payload_len].to_vec(),
        }
    }

    /// Parses an ICMP message from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::HEADER_SIZE {
            return None;
        }

        let msg_type = data[0];
        let code = data[1];
        let checksum = u16::from_be_bytes([data[2], data[3]]);
        let header_data = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let payload = data[Self::HEADER_SIZE..].to_vec();

        Some(Self {
            msg_type,
            code,
            checksum,
            header_data,
            payload,
        })
    }

    /// Serializes the message to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::HEADER_SIZE + self.payload.len());
        buf.push(self.msg_type);
        buf.push(self.code);
        buf.extend_from_slice(&self.checksum.to_be_bytes());
        buf.extend_from_slice(&self.header_data.to_be_bytes());
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Calculates and sets checksum
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

    /// Verifies checksum
    pub fn verify_checksum(&self) -> bool {
        let data = self.serialize();
        Self::compute_checksum(&data) == 0
    }

    /// Gets echo identifier (for echo request/reply)
    pub fn identifier(&self) -> u16 {
        (self.header_data >> 16) as u16
    }

    /// Gets echo sequence number
    pub fn sequence(&self) -> u16 {
        self.header_data as u16
    }
}

/// Pending ping request
#[derive(Debug)]
pub struct PingRequest {
    /// Destination IP
    pub dest_ip: u32,
    /// Identifier
    pub identifier: u16,
    /// Sequence number
    pub sequence: u16,
    /// Time sent (in ticks/ms)
    pub sent_time: u64,
    /// Timeout (in ticks/ms)
    pub timeout: u64,
}

/// Ping result
#[derive(Debug)]
pub struct PingResult {
    /// Source IP of reply
    pub src_ip: u32,
    /// Round-trip time (ms)
    pub rtt: u64,
    /// Sequence number
    pub sequence: u16,
    /// TTL from reply
    pub ttl: u8,
}

/// ICMP manager
pub struct IcmpManager {
    /// Pending ping requests
    pending_pings: VecDeque<PingRequest>,
    /// Completed ping results
    ping_results: VecDeque<PingResult>,
    /// Outgoing messages
    outgoing: VecDeque<(u32, IcmpMessage)>, // (dst_ip, message)
    /// Next identifier
    next_identifier: u16,
    /// Current time (updated externally)
    current_time: u64,
}

impl IcmpManager {
    /// Creates a new ICMP manager
    pub fn new() -> Self {
        Self {
            pending_pings: VecDeque::new(),
            ping_results: VecDeque::new(),
            outgoing: VecDeque::new(),
            next_identifier: 1,
            current_time: 0,
        }
    }

    /// Sends a ping
    pub fn ping(&mut self, dest_ip: u32, data: &[u8], timeout: u64) -> (u16, u16) {
        let identifier = self.next_identifier;
        self.next_identifier = self.next_identifier.wrapping_add(1);

        let sequence = self.pending_pings.len() as u16;

        let mut message = IcmpMessage::echo_request(identifier, sequence, data.to_vec());
        message.calculate_checksum();

        self.outgoing.push_back((dest_ip, message));

        self.pending_pings.push_back(PingRequest {
            dest_ip,
            identifier,
            sequence,
            sent_time: self.current_time,
            timeout,
        });

        (identifier, sequence)
    }

    /// Processes an incoming ICMP message
    pub fn process_incoming(&mut self, src_ip: u32, ttl: u8, message: IcmpMessage) {
        if !message.verify_checksum() {
            return; // Invalid checksum
        }

        match IcmpType::try_from(message.msg_type) {
            Ok(IcmpType::EchoRequest) => {
                // Reply to ping
                let reply = IcmpMessage::echo_reply(
                    message.identifier(),
                    message.sequence(),
                    message.payload,
                );
                self.send(src_ip, reply);
            }
            Ok(IcmpType::EchoReply) => {
                // Match with pending ping
                let identifier = message.identifier();
                let sequence = message.sequence();

                if let Some(pos) = self.pending_pings.iter().position(|p| {
                    p.identifier == identifier && p.sequence == sequence && p.dest_ip == src_ip
                }) {
                    let request = self.pending_pings.remove(pos).unwrap();
                    let rtt = self.current_time.saturating_sub(request.sent_time);

                    self.ping_results.push_back(PingResult {
                        src_ip,
                        rtt,
                        sequence,
                        ttl,
                    });
                }
            }
            Ok(IcmpType::DestUnreachable) => {
                // Handle destination unreachable
                // Could notify upper layers
            }
            Ok(IcmpType::TimeExceeded) => {
                // Handle TTL exceeded (traceroute)
            }
            _ => {
                // Unknown or unhandled type
            }
        }
    }

    /// Sends an ICMP message
    pub fn send(&mut self, dest_ip: u32, mut message: IcmpMessage) {
        message.calculate_checksum();
        self.outgoing.push_back((dest_ip, message));
    }

    /// Sends a destination unreachable error
    pub fn send_dest_unreachable(
        &mut self,
        dest_ip: u32,
        code: DestUnreachableCode,
        original_packet: &[u8],
    ) {
        let message = IcmpMessage::dest_unreachable(code, original_packet);
        self.send(dest_ip, message);
    }

    /// Gets next outgoing message
    pub fn poll_outgoing(&mut self) -> Option<(u32, IcmpMessage)> {
        self.outgoing.pop_front()
    }

    /// Gets next ping result
    pub fn poll_result(&mut self) -> Option<PingResult> {
        self.ping_results.pop_front()
    }

    /// Updates current time (call periodically)
    pub fn update_time(&mut self, time: u64) {
        self.current_time = time;

        // Check for timeouts
        while let Some(front) = self.pending_pings.front() {
            if time > front.sent_time + front.timeout {
                self.pending_pings.pop_front();
            } else {
                break;
            }
        }
    }

    /// Returns number of pending pings
    pub fn pending_count(&self) -> usize {
        self.pending_pings.len()
    }
}

impl Default for IcmpManager {
    fn default() -> Self {
        Self::new()
    }
}

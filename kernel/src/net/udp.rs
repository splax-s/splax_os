//! # UDP (User Datagram Protocol)
//!
//! UDP implementation for simple, connectionless data delivery.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use super::device::NetworkError;
use super::ip::{checksum_bytes, Ipv4Address, Ipv4Packet, PROTOCOL_UDP};

/// UDP header.
#[derive(Debug, Clone)]
pub struct UdpHeader {
    /// Source port.
    pub src_port: u16,
    /// Destination port.
    pub dest_port: u16,
    /// Total length (header + data).
    pub length: u16,
    /// Checksum.
    pub checksum: u16,
}

impl UdpHeader {
    /// Header size (8 bytes).
    pub const SIZE: usize = 8;
    
    /// Parses a UDP header from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }
        
        Some(Self {
            src_port: u16::from_be_bytes([data[0], data[1]]),
            dest_port: u16::from_be_bytes([data[2], data[3]]),
            length: u16::from_be_bytes([data[4], data[5]]),
            checksum: u16::from_be_bytes([data[6], data[7]]),
        })
    }
    
    /// Serializes the header to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::SIZE);
        bytes.extend_from_slice(&self.src_port.to_be_bytes());
        bytes.extend_from_slice(&self.dest_port.to_be_bytes());
        bytes.extend_from_slice(&self.length.to_be_bytes());
        bytes.extend_from_slice(&self.checksum.to_be_bytes());
        bytes
    }
    
    /// Creates a new header.
    pub fn new(src_port: u16, dest_port: u16, data_len: usize) -> Self {
        Self {
            src_port,
            dest_port,
            length: (Self::SIZE + data_len) as u16,
            checksum: 0,
        }
    }
}

/// UDP datagram.
#[derive(Debug, Clone)]
pub struct UdpDatagram {
    pub header: UdpHeader,
    pub data: Vec<u8>,
}

impl UdpDatagram {
    /// Parses from IP packet payload.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = UdpHeader::parse(data)?;
        
        let payload_start = UdpHeader::SIZE;
        let payload_end = header.length as usize;
        
        if data.len() < payload_end {
            return None;
        }
        
        let payload = data[payload_start..payload_end].to_vec();
        
        Some(Self {
            header,
            data: payload,
        })
    }
    
    /// Serializes to bytes.
    pub fn to_bytes(&self, src_addr: Ipv4Address, dest_addr: Ipv4Address) -> Vec<u8> {
        let mut header = self.header.clone();
        header.checksum = 0;
        
        let mut bytes = header.to_bytes();
        bytes.extend_from_slice(&self.data);
        
        // Compute checksum with pseudo-header
        header.checksum = udp_checksum(src_addr, dest_addr, &bytes);
        
        // Rebuild with correct checksum
        let mut result = header.to_bytes();
        result.extend_from_slice(&self.data);
        result
    }
    
    /// Creates a new datagram.
    pub fn new(src_port: u16, dest_port: u16, data: Vec<u8>) -> Self {
        Self {
            header: UdpHeader::new(src_port, dest_port, data.len()),
            data,
        }
    }
}

/// Computes UDP checksum with pseudo-header.
pub fn udp_checksum(src_addr: Ipv4Address, dest_addr: Ipv4Address, udp_data: &[u8]) -> u16 {
    // Build pseudo-header
    let mut pseudo_header = Vec::with_capacity(12 + udp_data.len());
    pseudo_header.extend_from_slice(&src_addr.0);
    pseudo_header.extend_from_slice(&dest_addr.0);
    pseudo_header.push(0); // Zero
    pseudo_header.push(PROTOCOL_UDP); // Protocol
    pseudo_header.extend_from_slice(&(udp_data.len() as u16).to_be_bytes());
    pseudo_header.extend_from_slice(udp_data);
    
    checksum_bytes(&pseudo_header)
}

/// UDP endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UdpEndpoint {
    pub addr: Ipv4Address,
    pub port: u16,
}

impl UdpEndpoint {
    pub fn new(addr: Ipv4Address, port: u16) -> Self {
        Self { addr, port }
    }
    
    /// Any address (0.0.0.0).
    pub fn any(port: u16) -> Self {
        Self::new(Ipv4Address::ANY, port)
    }
}

/// Received UDP message.
#[derive(Debug, Clone)]
pub struct UdpMessage {
    pub remote: UdpEndpoint,
    pub data: Vec<u8>,
}

/// UDP socket.
pub struct UdpSocket {
    /// Local endpoint.
    pub local: UdpEndpoint,
    /// Receive buffer.
    pub recv_buffer: Vec<UdpMessage>,
    /// Maximum buffer size.
    pub max_buffer: usize,
}

impl UdpSocket {
    /// Default buffer size.
    const DEFAULT_BUFFER_SIZE: usize = 64;
    
    /// Creates a new socket.
    pub fn new(local: UdpEndpoint) -> Self {
        Self {
            local,
            recv_buffer: Vec::new(),
            max_buffer: Self::DEFAULT_BUFFER_SIZE,
        }
    }
    
    /// Queues a received message.
    pub fn queue_message(&mut self, remote: UdpEndpoint, data: Vec<u8>) {
        if self.recv_buffer.len() < self.max_buffer {
            self.recv_buffer.push(UdpMessage { remote, data });
        }
        // Drop if buffer full
    }
    
    /// Receives a message.
    pub fn recv(&mut self) -> Option<UdpMessage> {
        if self.recv_buffer.is_empty() {
            None
        } else {
            Some(self.recv_buffer.remove(0))
        }
    }
    
    /// Creates a datagram to send.
    pub fn send_to(&self, remote: UdpEndpoint, data: Vec<u8>) -> UdpDatagram {
        UdpDatagram::new(self.local.port, remote.port, data)
    }
}

/// Global UDP state.
pub struct UdpState {
    /// Bound sockets by port.
    sockets: BTreeMap<u16, UdpSocket>,
    /// Next ephemeral port.
    next_ephemeral_port: u16,
}

impl UdpState {
    /// Ephemeral port range start.
    const EPHEMERAL_START: u16 = 49152;
    /// Ephemeral port range end.
    const EPHEMERAL_END: u16 = 65535;
    
    /// Creates new UDP state.
    pub const fn new() -> Self {
        Self {
            sockets: BTreeMap::new(),
            next_ephemeral_port: Self::EPHEMERAL_START,
        }
    }
    
    /// Allocates an ephemeral port.
    pub fn allocate_port(&mut self) -> u16 {
        let port = self.next_ephemeral_port;
        self.next_ephemeral_port = if self.next_ephemeral_port >= Self::EPHEMERAL_END {
            Self::EPHEMERAL_START
        } else {
            self.next_ephemeral_port + 1
        };
        port
    }
    
    /// Binds a socket to a port.
    pub fn bind(&mut self, port: u16, local_addr: Ipv4Address) -> Result<(), NetworkError> {
        if self.sockets.contains_key(&port) {
            return Err(NetworkError::AddressInUse);
        }
        
        let socket = UdpSocket::new(UdpEndpoint::new(local_addr, port));
        self.sockets.insert(port, socket);
        Ok(())
    }
    
    /// Unbinds a socket.
    pub fn unbind(&mut self, port: u16) {
        self.sockets.remove(&port);
    }
    
    /// Gets a socket by port.
    pub fn socket(&mut self, port: u16) -> Option<&mut UdpSocket> {
        self.sockets.get_mut(&port)
    }
    
    /// Delivers a datagram to the appropriate socket.
    pub fn deliver(&mut self, remote: UdpEndpoint, local_port: u16, data: Vec<u8>) {
        if let Some(socket) = self.sockets.get_mut(&local_port) {
            socket.queue_message(remote, data);
        }
    }
    
    /// Returns an iterator over bound ports.
    pub fn bound_ports(&self) -> impl Iterator<Item = u16> + '_ {
        self.sockets.keys().copied()
    }
}

/// Static UDP state.
static UDP_STATE: Mutex<UdpState> = Mutex::new(UdpState::new());

/// Gets UDP state.
pub fn udp_state() -> &'static Mutex<UdpState> {
    &UDP_STATE
}

/// Handles an incoming UDP packet.
pub fn handle_packet(ip_packet: &Ipv4Packet) {
    let datagram = match UdpDatagram::parse(&ip_packet.payload) {
        Some(d) => d,
        None => return,
    };
    
    let remote = UdpEndpoint::new(ip_packet.src_addr, datagram.header.src_port);
    let local_port = datagram.header.dest_port;
    
    UDP_STATE.lock().deliver(remote, local_port, datagram.data);
}

/// DNS query port.
pub const DNS_PORT: u16 = 53;
/// DHCP client port.
pub const DHCP_CLIENT_PORT: u16 = 68;
/// DHCP server port.
pub const DHCP_SERVER_PORT: u16 = 67;

// =============================================================================
// UDP Multicast Support (v0.2.0)
// =============================================================================

/// Multicast group address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MulticastGroup(pub Ipv4Address);

impl MulticastGroup {
    /// Creates a new multicast group.
    pub fn new(addr: Ipv4Address) -> Option<Self> {
        if addr.is_multicast() {
            Some(Self(addr))
        } else {
            None
        }
    }

    /// Returns the multicast address.
    pub fn addr(&self) -> Ipv4Address {
        self.0
    }

    /// Computes the MAC address for this multicast group.
    ///
    /// Multicast MAC = 01:00:5E + lower 23 bits of IP
    pub fn mac_address(&self) -> [u8; 6] {
        let ip = self.0.0;
        [
            0x01,
            0x00,
            0x5E,
            ip[1] & 0x7F, // Lower 23 bits
            ip[2],
            ip[3],
        ]
    }
}

impl Ipv4Address {
    /// Checks if this is a multicast address (224.0.0.0 - 239.255.255.255).
    pub fn is_multicast(&self) -> bool {
        self.0[0] >= 224 && self.0[0] <= 239
    }

    /// Well-known multicast addresses.
    pub const ALL_HOSTS: Ipv4Address = Ipv4Address([224, 0, 0, 1]);
    pub const ALL_ROUTERS: Ipv4Address = Ipv4Address([224, 0, 0, 2]);
    pub const MDNS: Ipv4Address = Ipv4Address([224, 0, 0, 251]);
    pub const LLMNR: Ipv4Address = Ipv4Address([224, 0, 0, 252]);
}

/// IGMP message types.
pub mod igmp {
    pub const MEMBERSHIP_QUERY: u8 = 0x11;
    pub const MEMBERSHIP_REPORT_V1: u8 = 0x12;
    pub const MEMBERSHIP_REPORT_V2: u8 = 0x16;
    pub const MEMBERSHIP_REPORT_V3: u8 = 0x22;
    pub const LEAVE_GROUP: u8 = 0x17;
}

/// IGMP message for multicast group management.
#[derive(Debug, Clone)]
pub struct IgmpMessage {
    pub msg_type: u8,
    pub max_resp_time: u8,
    pub checksum: u16,
    pub group_addr: Ipv4Address,
}

impl IgmpMessage {
    /// Header size (8 bytes).
    pub const SIZE: usize = 8;

    /// Parses an IGMP message.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }

        Some(Self {
            msg_type: data[0],
            max_resp_time: data[1],
            checksum: u16::from_be_bytes([data[2], data[3]]),
            group_addr: Ipv4Address([data[4], data[5], data[6], data[7]]),
        })
    }

    /// Serializes to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::SIZE);
        bytes.push(self.msg_type);
        bytes.push(self.max_resp_time);
        bytes.extend_from_slice(&self.checksum.to_be_bytes());
        bytes.extend_from_slice(&self.group_addr.0);
        bytes
    }

    /// Creates a membership report.
    pub fn membership_report(group: MulticastGroup) -> Self {
        let mut msg = Self {
            msg_type: igmp::MEMBERSHIP_REPORT_V2,
            max_resp_time: 0,
            checksum: 0,
            group_addr: group.addr(),
        };
        msg.checksum = msg.compute_checksum();
        msg
    }

    /// Creates a leave group message.
    pub fn leave_group(group: MulticastGroup) -> Self {
        let mut msg = Self {
            msg_type: igmp::LEAVE_GROUP,
            max_resp_time: 0,
            checksum: 0,
            group_addr: group.addr(),
        };
        msg.checksum = msg.compute_checksum();
        msg
    }

    /// Computes the IGMP checksum.
    fn compute_checksum(&self) -> u16 {
        let mut sum: u32 = 0;
        sum += ((self.msg_type as u32) << 8) | self.max_resp_time as u32;
        sum += 0; // Checksum field is 0 during computation
        sum += ((self.group_addr.0[0] as u32) << 8) | self.group_addr.0[1] as u32;
        sum += ((self.group_addr.0[2] as u32) << 8) | self.group_addr.0[3] as u32;

        while sum >> 16 != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }

        !sum as u16
    }
}

/// Multicast group membership.
#[derive(Debug, Clone)]
pub struct MulticastMembership {
    /// Group address.
    pub group: MulticastGroup,
    /// Interface index (for multi-homed hosts).
    pub interface: u32,
    /// Local address to receive on.
    pub local_addr: Ipv4Address,
    /// Filter mode (include/exclude sources).
    pub filter_mode: SourceFilterMode,
    /// Source list for filtering.
    pub sources: Vec<Ipv4Address>,
    /// Timestamp of last report sent.
    pub last_report: u64,
}

/// Source filter mode for multicast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFilterMode {
    /// Include only listed sources.
    Include,
    /// Exclude listed sources.
    Exclude,
}

/// Multicast socket extension.
pub struct MulticastSocket {
    /// Base UDP socket.
    pub socket: UdpSocket,
    /// Joined multicast groups.
    pub memberships: Vec<MulticastMembership>,
    /// Loopback enabled (receive own multicast).
    pub loopback: bool,
    /// TTL for outgoing multicast.
    pub multicast_ttl: u8,
    /// Interface for outgoing multicast.
    pub multicast_interface: Option<Ipv4Address>,
}

impl MulticastSocket {
    /// Creates a new multicast socket.
    pub fn new(local: UdpEndpoint) -> Self {
        Self {
            socket: UdpSocket::new(local),
            memberships: Vec::new(),
            loopback: true,
            multicast_ttl: 1, // Default: local network only
            multicast_interface: None,
        }
    }

    /// Joins a multicast group.
    pub fn join_group(&mut self, group: MulticastGroup, interface: Ipv4Address) -> Result<(), NetworkError> {
        // Check if already a member
        if self.memberships.iter().any(|m| m.group == group && m.local_addr == interface) {
            return Err(NetworkError::AddressInUse);
        }

        let membership = MulticastMembership {
            group,
            interface: 0,
            local_addr: interface,
            filter_mode: SourceFilterMode::Exclude,
            sources: Vec::new(),
            last_report: 0,
        };

        self.memberships.push(membership);

        // Would send IGMP membership report here
        Ok(())
    }

    /// Leaves a multicast group.
    pub fn leave_group(&mut self, group: MulticastGroup, interface: Ipv4Address) -> Result<(), NetworkError> {
        let idx = self.memberships.iter()
            .position(|m| m.group == group && m.local_addr == interface)
            .ok_or(NetworkError::NotConnected)?;

        self.memberships.remove(idx);

        // Would send IGMP leave group here
        Ok(())
    }

    /// Sets source filter for a group.
    pub fn set_source_filter(
        &mut self,
        group: MulticastGroup,
        mode: SourceFilterMode,
        sources: Vec<Ipv4Address>,
    ) -> Result<(), NetworkError> {
        let membership = self.memberships.iter_mut()
            .find(|m| m.group == group)
            .ok_or(NetworkError::NotConnected)?;

        membership.filter_mode = mode;
        membership.sources = sources;

        Ok(())
    }

    /// Checks if should receive from source.
    pub fn should_receive(&self, group: MulticastGroup, source: Ipv4Address) -> bool {
        for membership in &self.memberships {
            if membership.group == group {
                match membership.filter_mode {
                    SourceFilterMode::Include => {
                        return membership.sources.is_empty() || membership.sources.contains(&source);
                    }
                    SourceFilterMode::Exclude => {
                        return !membership.sources.contains(&source);
                    }
                }
            }
        }
        false
    }

    /// Sets multicast TTL.
    pub fn set_multicast_ttl(&mut self, ttl: u8) {
        self.multicast_ttl = ttl;
    }

    /// Sets multicast loopback.
    pub fn set_loopback(&mut self, enabled: bool) {
        self.loopback = enabled;
    }

    /// Sets outgoing interface for multicast.
    pub fn set_multicast_interface(&mut self, addr: Ipv4Address) {
        self.multicast_interface = Some(addr);
    }

    /// Sends to a multicast group.
    pub fn send_multicast(&self, group: MulticastGroup, data: Vec<u8>) -> UdpDatagram {
        // Uses the socket's local port as source
        UdpDatagram::new(self.socket.local.port, 0, data)
    }
}

/// Global multicast state.
pub struct MulticastState {
    /// Multicast sockets by port.
    sockets: BTreeMap<u16, MulticastSocket>,
    /// Global group memberships (for IGMP).
    groups: BTreeMap<MulticastGroup, Vec<u16>>, // Group -> ports
}

impl MulticastState {
    /// Creates new multicast state.
    pub const fn new() -> Self {
        Self {
            sockets: BTreeMap::new(),
            groups: BTreeMap::new(),
        }
    }

    /// Registers a multicast socket.
    pub fn register(&mut self, port: u16, socket: MulticastSocket) {
        for membership in &socket.memberships {
            self.groups
                .entry(membership.group)
                .or_insert_with(Vec::new)
                .push(port);
        }
        self.sockets.insert(port, socket);
    }

    /// Delivers a multicast datagram.
    pub fn deliver(&mut self, group: MulticastGroup, source: Ipv4Address, data: &[u8]) {
        if let Some(ports) = self.groups.get(&group) {
            for &port in ports {
                if let Some(socket) = self.sockets.get_mut(&port) {
                    if socket.should_receive(group, source) {
                        socket.socket.queue_message(
                            UdpEndpoint::new(source, 0),
                            data.to_vec(),
                        );
                    }
                }
            }
        }
    }

    /// Lists all joined groups.
    pub fn joined_groups(&self) -> Vec<MulticastGroup> {
        self.groups.keys().copied().collect()
    }
}

/// Static multicast state.
static MULTICAST_STATE: Mutex<MulticastState> = Mutex::new(MulticastState::new());

/// Gets multicast state.
pub fn multicast_state() -> &'static Mutex<MulticastState> {
    &MULTICAST_STATE
}

/// Handles an incoming multicast packet.
pub fn handle_multicast_packet(src: Ipv4Address, dest: Ipv4Address, data: &[u8]) {
    if let Some(group) = MulticastGroup::new(dest) {
        MULTICAST_STATE.lock().deliver(group, src, data);
    }
}

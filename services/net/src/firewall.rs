//! # Userspace Firewall
//!
//! Packet filtering and NAT for the S-NET service.
//! Implements iptables-like functionality in userspace.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::ip::IpPacket;

/// Firewall action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Accept the packet
    Accept,
    /// Drop the packet silently
    Drop,
    /// Reject with ICMP error
    Reject,
    /// Log and continue
    Log,
    /// Jump to another chain
    Jump,
    /// Return from chain
    Return,
}

/// Protocol match
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Any,
    Icmp,
    Tcp,
    Udp,
}

impl Protocol {
    fn matches(&self, proto: u8) -> bool {
        match self {
            Protocol::Any => true,
            Protocol::Icmp => proto == 1,
            Protocol::Tcp => proto == 6,
            Protocol::Udp => proto == 17,
        }
    }
}

/// IP address match
#[derive(Debug, Clone, Copy)]
pub struct IpMatch {
    /// Address
    pub addr: u32,
    /// Netmask
    pub mask: u32,
}

impl IpMatch {
    /// Creates a match for any address
    pub fn any() -> Self {
        Self { addr: 0, mask: 0 }
    }

    /// Creates a match for a specific address
    pub fn exact(addr: u32) -> Self {
        Self {
            addr,
            mask: 0xFFFFFFFF,
        }
    }

    /// Creates a match for a subnet
    pub fn subnet(addr: u32, prefix_len: u8) -> Self {
        let mask = if prefix_len == 0 {
            0
        } else {
            0xFFFFFFFF << (32 - prefix_len)
        };
        Self {
            addr: addr & mask,
            mask,
        }
    }

    /// Checks if an IP matches
    fn matches(&self, ip: u32) -> bool {
        (ip & self.mask) == (self.addr & self.mask)
    }
}

/// Port match
#[derive(Debug, Clone, Copy)]
pub struct PortMatch {
    /// Start port (inclusive)
    pub start: u16,
    /// End port (inclusive)
    pub end: u16,
}

impl PortMatch {
    /// Creates a match for any port
    pub fn any() -> Self {
        Self { start: 0, end: 65535 }
    }

    /// Creates a match for a specific port
    pub fn exact(port: u16) -> Self {
        Self {
            start: port,
            end: port,
        }
    }

    /// Creates a match for a port range
    pub fn range(start: u16, end: u16) -> Self {
        Self { start, end }
    }

    /// Checks if a port matches
    fn matches(&self, port: u16) -> bool {
        port >= self.start && port <= self.end
    }
}

/// TCP flags match
#[derive(Debug, Clone, Copy)]
pub struct TcpFlagsMatch {
    /// Required flags (must be set)
    pub required: u8,
    /// Mask (which flags to check)
    pub mask: u8,
}

impl TcpFlagsMatch {
    /// Match any flags
    pub fn any() -> Self {
        Self {
            required: 0,
            mask: 0,
        }
    }

    /// Match SYN packets (new connections)
    pub fn syn() -> Self {
        Self {
            required: 0x02,
            mask: 0x12, // SYN + ACK mask
        }
    }

    /// Match established connections
    pub fn established() -> Self {
        Self {
            required: 0x10,
            mask: 0x10, // ACK must be set
        }
    }
}

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// New connection
    New,
    /// Established connection
    Established,
    /// Related connection (e.g., ICMP error)
    Related,
    /// Invalid packet
    Invalid,
}

/// Firewall rule
#[derive(Debug, Clone)]
pub struct Rule {
    /// Rule name/comment
    pub name: String,
    /// Source IP match
    pub src_ip: IpMatch,
    /// Destination IP match
    pub dst_ip: IpMatch,
    /// Protocol
    pub protocol: Protocol,
    /// Source port match (TCP/UDP)
    pub src_port: PortMatch,
    /// Destination port match (TCP/UDP)
    pub dst_port: PortMatch,
    /// TCP flags match
    pub tcp_flags: TcpFlagsMatch,
    /// Connection state match
    pub state: Option<ConnState>,
    /// Interface (None = any)
    pub interface: Option<usize>,
    /// Invert match
    pub negate: bool,
    /// Action to take
    pub action: Action,
    /// Jump target (if action is Jump)
    pub target_chain: Option<String>,
    /// Log prefix
    pub log_prefix: Option<String>,
    /// Packet counter
    pub packet_count: u64,
    /// Byte counter
    pub byte_count: u64,
}

impl Rule {
    /// Creates a new rule
    pub fn new(action: Action) -> Self {
        Self {
            name: String::new(),
            src_ip: IpMatch::any(),
            dst_ip: IpMatch::any(),
            protocol: Protocol::Any,
            src_port: PortMatch::any(),
            dst_port: PortMatch::any(),
            tcp_flags: TcpFlagsMatch::any(),
            state: None,
            interface: None,
            negate: false,
            action,
            target_chain: None,
            log_prefix: None,
            packet_count: 0,
            byte_count: 0,
        }
    }

    /// Sets source IP match
    pub fn with_src_ip(mut self, ip: IpMatch) -> Self {
        self.src_ip = ip;
        self
    }

    /// Sets destination IP match
    pub fn with_dst_ip(mut self, ip: IpMatch) -> Self {
        self.dst_ip = ip;
        self
    }

    /// Sets protocol
    pub fn with_protocol(mut self, proto: Protocol) -> Self {
        self.protocol = proto;
        self
    }

    /// Sets destination port
    pub fn with_dst_port(mut self, port: PortMatch) -> Self {
        self.dst_port = port;
        self
    }

    /// Sets source port
    pub fn with_src_port(mut self, port: PortMatch) -> Self {
        self.src_port = port;
        self
    }

    /// Checks if a packet matches this rule
    fn matches(&self, packet: &IpPacket, _ports: Option<(u16, u16)>) -> bool {
        // Protocol check
        if !self.protocol.matches(packet.header.protocol) {
            return self.negate;
        }

        // Source IP check
        if !self.src_ip.matches(packet.header.src_addr) {
            return self.negate;
        }

        // Destination IP check
        if !self.dst_ip.matches(packet.header.dst_addr) {
            return self.negate;
        }

        // Port checks for TCP/UDP
        if let Some((src_port, dst_port)) = _ports {
            if !self.src_port.matches(src_port) {
                return self.negate;
            }
            if !self.dst_port.matches(dst_port) {
                return self.negate;
            }
        }

        !self.negate
    }
}

/// Firewall chain
#[derive(Debug, Clone)]
pub struct Chain {
    /// Chain name
    pub name: String,
    /// Rules in this chain
    pub rules: Vec<Rule>,
    /// Default policy
    pub policy: Action,
    /// Is this a built-in chain?
    pub builtin: bool,
}

impl Chain {
    /// Creates a new chain
    pub fn new(name: &str, policy: Action, builtin: bool) -> Self {
        Self {
            name: String::from(name),
            rules: Vec::new(),
            policy,
            builtin,
        }
    }

    /// Appends a rule
    pub fn append(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// Inserts a rule at position
    pub fn insert(&mut self, index: usize, rule: Rule) {
        if index <= self.rules.len() {
            self.rules.insert(index, rule);
        }
    }

    /// Deletes a rule at position
    pub fn delete(&mut self, index: usize) -> Option<Rule> {
        if index < self.rules.len() {
            Some(self.rules.remove(index))
        } else {
            None
        }
    }

    /// Flushes all rules
    pub fn flush(&mut self) {
        self.rules.clear();
    }
}

/// Connection tracking entry
#[derive(Debug, Clone)]
pub struct ConnTrackEntry {
    /// Source IP
    pub src_ip: u32,
    /// Destination IP
    pub dst_ip: u32,
    /// Source port
    pub src_port: u16,
    /// Destination port
    pub dst_port: u16,
    /// Protocol
    pub protocol: u8,
    /// Connection state
    pub state: ConnState,
    /// Packet count
    pub packets: u64,
    /// Byte count
    pub bytes: u64,
    /// Last seen timestamp
    pub last_seen: u64,
    /// Timeout (ms)
    pub timeout: u64,
}

/// Connection tracker
pub struct ConnTrack {
    /// Connections indexed by (src_ip, dst_ip, src_port, dst_port, proto)
    connections: BTreeMap<(u32, u32, u16, u16, u8), ConnTrackEntry>,
    /// Current time
    current_time: u64,
    /// Default timeout for established connections (ms)
    pub established_timeout: u64,
    /// Default timeout for new connections (ms)
    pub new_timeout: u64,
}

impl ConnTrack {
    /// Creates a new connection tracker
    pub fn new() -> Self {
        Self {
            connections: BTreeMap::new(),
            current_time: 0,
            established_timeout: 432000000, // 5 days
            new_timeout: 30000,             // 30 seconds
        }
    }

    /// Looks up connection state for a packet
    pub fn lookup(&self, packet: &IpPacket, src_port: u16, dst_port: u16) -> ConnState {
        let key = (
            packet.header.src_addr,
            packet.header.dst_addr,
            src_port,
            dst_port,
            packet.header.protocol,
        );

        // Check forward direction
        if let Some(entry) = self.connections.get(&key) {
            return entry.state;
        }

        // Check reverse direction (reply packets)
        let reverse_key = (
            packet.header.dst_addr,
            packet.header.src_addr,
            dst_port,
            src_port,
            packet.header.protocol,
        );

        if let Some(entry) = self.connections.get(&reverse_key) {
            if entry.state == ConnState::Established {
                return ConnState::Established;
            }
        }

        ConnState::New
    }

    /// Tracks a packet
    pub fn track(&mut self, packet: &IpPacket, src_port: u16, dst_port: u16) {
        let key = (
            packet.header.src_addr,
            packet.header.dst_addr,
            src_port,
            dst_port,
            packet.header.protocol,
        );

        if let Some(entry) = self.connections.get_mut(&key) {
            entry.packets += 1;
            entry.bytes += packet.header.total_length as u64;
            entry.last_seen = self.current_time;
            entry.state = ConnState::Established;
        } else {
            // Check if this is a reply to existing connection
            let reverse_key = (
                packet.header.dst_addr,
                packet.header.src_addr,
                dst_port,
                src_port,
                packet.header.protocol,
            );

            if let Some(entry) = self.connections.get_mut(&reverse_key) {
                entry.packets += 1;
                entry.bytes += packet.header.total_length as u64;
                entry.last_seen = self.current_time;
                entry.state = ConnState::Established;
            } else {
                // New connection
                self.connections.insert(
                    key,
                    ConnTrackEntry {
                        src_ip: packet.header.src_addr,
                        dst_ip: packet.header.dst_addr,
                        src_port,
                        dst_port,
                        protocol: packet.header.protocol,
                        state: ConnState::New,
                        packets: 1,
                        bytes: packet.header.total_length as u64,
                        last_seen: self.current_time,
                        timeout: self.new_timeout,
                    },
                );
            }
        }
    }

    /// Updates time and cleans up expired entries
    pub fn update_time(&mut self, time: u64) {
        self.current_time = time;
        self.connections
            .retain(|_, e| time - e.last_seen < e.timeout);
    }

    /// Returns number of tracked connections
    pub fn count(&self) -> usize {
        self.connections.len()
    }
}

impl Default for ConnTrack {
    fn default() -> Self {
        Self::new()
    }
}

/// Firewall table (filter, nat, mangle)
pub struct Table {
    /// Table name
    pub name: String,
    /// Chains in this table
    pub chains: BTreeMap<String, Chain>,
}

impl Table {
    /// Creates a new table with default chains
    pub fn new_filter() -> Self {
        let mut chains = BTreeMap::new();
        chains.insert("INPUT".into(), Chain::new("INPUT", Action::Accept, true));
        chains.insert("OUTPUT".into(), Chain::new("OUTPUT", Action::Accept, true));
        chains.insert("FORWARD".into(), Chain::new("FORWARD", Action::Accept, true));

        Self {
            name: "filter".into(),
            chains,
        }
    }

    /// Creates a NAT table
    pub fn new_nat() -> Self {
        let mut chains = BTreeMap::new();
        chains.insert("PREROUTING".into(), Chain::new("PREROUTING", Action::Accept, true));
        chains.insert("POSTROUTING".into(), Chain::new("POSTROUTING", Action::Accept, true));
        chains.insert("OUTPUT".into(), Chain::new("OUTPUT", Action::Accept, true));

        Self {
            name: "nat".into(),
            chains,
        }
    }

    /// Gets a chain
    pub fn get_chain(&self, name: &str) -> Option<&Chain> {
        self.chains.get(name)
    }

    /// Gets a mutable chain
    pub fn get_chain_mut(&mut self, name: &str) -> Option<&mut Chain> {
        self.chains.get_mut(name)
    }

    /// Creates a user chain
    pub fn create_chain(&mut self, name: &str) -> Result<(), &'static str> {
        if self.chains.contains_key(name) {
            return Err("Chain already exists");
        }
        self.chains
            .insert(name.into(), Chain::new(name, Action::Return, false));
        Ok(())
    }

    /// Deletes a user chain
    pub fn delete_chain(&mut self, name: &str) -> Result<(), &'static str> {
        if let Some(chain) = self.chains.get(name) {
            if chain.builtin {
                return Err("Cannot delete built-in chain");
            }
        }
        self.chains.remove(name);
        Ok(())
    }
}

/// Firewall manager
pub struct Firewall {
    /// Filter table
    pub filter: Table,
    /// NAT table
    pub nat: Table,
    /// Connection tracker
    pub conntrack: ConnTrack,
    /// Enabled
    pub enabled: bool,
}

impl Firewall {
    /// Creates a new firewall
    pub fn new() -> Self {
        Self {
            filter: Table::new_filter(),
            nat: Table::new_nat(),
            conntrack: ConnTrack::new(),
            enabled: true,
        }
    }

    /// Processes an incoming packet through INPUT chain
    pub fn filter_input(&mut self, packet: &IpPacket) -> Action {
        if !self.enabled {
            return Action::Accept;
        }

        let ports = self.extract_ports(packet);
        self.conntrack.track(packet, ports.0, ports.1);

        self.process_chain(&self.filter, "INPUT", packet, Some(ports))
    }

    /// Processes an outgoing packet through OUTPUT chain
    pub fn filter_output(&mut self, packet: &IpPacket) -> Action {
        if !self.enabled {
            return Action::Accept;
        }

        let ports = self.extract_ports(packet);
        self.conntrack.track(packet, ports.0, ports.1);

        self.process_chain(&self.filter, "OUTPUT", packet, Some(ports))
    }

    /// Processes a forwarded packet through FORWARD chain
    pub fn filter_forward(&mut self, packet: &IpPacket) -> Action {
        if !self.enabled {
            return Action::Accept;
        }

        let ports = self.extract_ports(packet);
        self.conntrack.track(packet, ports.0, ports.1);

        self.process_chain(&self.filter, "FORWARD", packet, Some(ports))
    }

    /// Extracts ports from TCP/UDP packet
    fn extract_ports(&self, packet: &IpPacket) -> (u16, u16) {
        if packet.payload.len() >= 4 {
            if packet.header.protocol == 6 || packet.header.protocol == 17 {
                let src = u16::from_be_bytes([packet.payload[0], packet.payload[1]]);
                let dst = u16::from_be_bytes([packet.payload[2], packet.payload[3]]);
                return (src, dst);
            }
        }
        (0, 0)
    }

    /// Processes a chain
    fn process_chain(
        &self,
        table: &Table,
        chain_name: &str,
        packet: &IpPacket,
        ports: Option<(u16, u16)>,
    ) -> Action {
        let chain = match table.get_chain(chain_name) {
            Some(c) => c,
            None => return Action::Accept,
        };

        for rule in &chain.rules {
            if rule.matches(packet, ports) {
                match rule.action {
                    Action::Accept | Action::Drop | Action::Reject => {
                        return rule.action;
                    }
                    Action::Log => {
                        // Log and continue
                        continue;
                    }
                    Action::Jump => {
                        if let Some(target) = &rule.target_chain {
                            let result = self.process_chain(table, target, packet, ports);
                            if result != Action::Return {
                                return result;
                            }
                        }
                    }
                    Action::Return => {
                        return Action::Return;
                    }
                }
            }
        }

        chain.policy
    }

    /// Adds a rule to allow established connections
    pub fn allow_established(&mut self) {
        let mut rule = Rule::new(Action::Accept);
        rule.state = Some(ConnState::Established);
        rule.name = "Allow established".into();

        if let Some(chain) = self.filter.get_chain_mut("INPUT") {
            chain.insert(0, rule);
        }
    }

    /// Adds a rule to allow a specific port
    pub fn allow_port(&mut self, port: u16, protocol: Protocol) {
        let rule = Rule::new(Action::Accept)
            .with_protocol(protocol)
            .with_dst_port(PortMatch::exact(port));

        if let Some(chain) = self.filter.get_chain_mut("INPUT") {
            chain.append(rule);
        }
    }

    /// Adds a rule to block an IP
    pub fn block_ip(&mut self, ip: u32) {
        let rule = Rule::new(Action::Drop).with_src_ip(IpMatch::exact(ip));

        if let Some(chain) = self.filter.get_chain_mut("INPUT") {
            chain.insert(0, rule);
        }
    }

    /// Updates time (for connection tracking)
    pub fn update_time(&mut self, time: u64) {
        self.conntrack.update_time(time);
    }
}

impl Default for Firewall {
    fn default() -> Self {
        Self::new()
    }
}

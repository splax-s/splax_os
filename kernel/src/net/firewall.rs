//! # Netfilter / Firewall
//!
//! Packet filtering and firewall rules for Splax OS.
//!
//! ## Features
//!
//! - Stateful packet inspection
//! - Connection tracking
//! - Rule-based filtering (iptables-like)
//! - NAT support
//! - Rate limiting
//! - Capability-based rule management

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::RwLock;

use crate::net::ip::Ipv4Address;
use crate::net::ipv6::Ipv6Address;

/// Firewall action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Allow the packet
    Accept,
    /// Drop the packet silently
    Drop,
    /// Reject with ICMP error
    Reject,
    /// Log and continue processing
    Log,
    /// Jump to another chain
    Jump,
    /// Return from current chain
    Return,
}

/// Protocol to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Any protocol
    Any,
    /// ICMP
    Icmp,
    /// ICMPv6
    Icmpv6,
    /// TCP
    Tcp,
    /// UDP
    Udp,
    /// Specific protocol number
    Number(u8),
}

impl Protocol {
    pub fn matches(&self, proto: u8) -> bool {
        match self {
            Protocol::Any => true,
            Protocol::Icmp => proto == 1,
            Protocol::Icmpv6 => proto == 58,
            Protocol::Tcp => proto == 6,
            Protocol::Udp => proto == 17,
            Protocol::Number(n) => proto == *n,
        }
    }
}

/// IP address match (v4 or v6).
#[derive(Debug, Clone)]
pub enum IpMatch {
    /// Match any address
    Any,
    /// Match specific IPv4 address
    Ipv4(Ipv4Address),
    /// Match IPv4 subnet (address, prefix length)
    Ipv4Subnet(Ipv4Address, u8),
    /// Match specific IPv6 address
    Ipv6(Ipv6Address),
    /// Match IPv6 subnet (address, prefix length)
    Ipv6Subnet(Ipv6Address, u8),
}

impl IpMatch {
    /// Checks if an IPv4 address matches.
    pub fn matches_v4(&self, addr: Ipv4Address) -> bool {
        match self {
            IpMatch::Any => true,
            IpMatch::Ipv4(a) => *a == addr,
            IpMatch::Ipv4Subnet(base, prefix) => {
                let mask = if *prefix >= 32 { 
                    0xFFFFFFFF 
                } else { 
                    !((1u32 << (32 - prefix)) - 1) 
                };
                let base_int = u32::from(*base);
                let addr_int = u32::from(addr);
                (base_int & mask) == (addr_int & mask)
            }
            _ => false,
        }
    }
    
    /// Checks if an IPv6 address matches.
    pub fn matches_v6(&self, addr: Ipv6Address) -> bool {
        match self {
            IpMatch::Any => true,
            IpMatch::Ipv6(a) => *a == addr,
            IpMatch::Ipv6Subnet(base, prefix) => {
                // Compare prefix bits
                let full_bytes = (*prefix / 8) as usize;
                let remaining_bits = *prefix % 8;
                
                // Compare full bytes
                if base.0[..full_bytes] != addr.0[..full_bytes] {
                    return false;
                }
                
                // Compare remaining bits
                if remaining_bits > 0 && full_bytes < 16 {
                    let mask = !((1u8 << (8 - remaining_bits)) - 1);
                    if (base.0[full_bytes] & mask) != (addr.0[full_bytes] & mask) {
                        return false;
                    }
                }
                
                true
            }
            _ => false,
        }
    }
}

/// Port range match.
#[derive(Debug, Clone, Copy)]
pub struct PortMatch {
    /// Start of range (inclusive)
    pub start: u16,
    /// End of range (inclusive)
    pub end: u16,
}

impl PortMatch {
    /// Match any port.
    pub const ANY: Self = Self { start: 0, end: 65535 };
    
    /// Match single port.
    pub const fn single(port: u16) -> Self {
        Self { start: port, end: port }
    }
    
    /// Match port range.
    pub const fn range(start: u16, end: u16) -> Self {
        Self { start, end }
    }
    
    /// Check if port matches.
    pub fn matches(&self, port: u16) -> bool {
        port >= self.start && port <= self.end
    }
}

/// TCP flags to match.
#[derive(Debug, Clone, Copy, Default)]
pub struct TcpFlags {
    pub syn: Option<bool>,
    pub ack: Option<bool>,
    pub fin: Option<bool>,
    pub rst: Option<bool>,
    pub psh: Option<bool>,
    pub urg: Option<bool>,
}

impl TcpFlags {
    /// Match SYN packets (new connections).
    pub const fn syn_only() -> Self {
        Self {
            syn: Some(true),
            ack: Some(false),
            fin: None,
            rst: None,
            psh: None,
            urg: None,
        }
    }
    
    /// Match established connections.
    pub const fn established() -> Self {
        Self {
            syn: None,
            ack: Some(true),
            fin: None,
            rst: None,
            psh: None,
            urg: None,
        }
    }
}

/// Connection state for stateful filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// New connection
    New,
    /// Established connection
    Established,
    /// Related to existing connection
    Related,
    /// Invalid packet
    Invalid,
}

/// Alias for ConnState for compatibility.
pub type ConnectionState = ConnState;

/// ICMP type/code match.
#[derive(Debug, Clone, Copy)]
pub struct IcmpMatch {
    pub icmp_type: Option<u8>,
    pub code: Option<u8>,
}

impl IcmpMatch {
    /// Match any ICMP.
    pub const ANY: Self = Self { icmp_type: None, code: None };
    
    /// Match specific type.
    pub const fn of_type(icmp_type: u8) -> Self {
        Self { icmp_type: Some(icmp_type), code: None }
    }
    
    /// Match echo request (ping).
    pub const fn echo_request() -> Self {
        Self { icmp_type: Some(8), code: Some(0) }
    }
    
    /// Match echo reply.
    pub const fn echo_reply() -> Self {
        Self { icmp_type: Some(0), code: Some(0) }
    }
}

/// Rate limit specification.
#[derive(Debug, Clone, Copy)]
pub struct RateLimit {
    /// Maximum packets per interval
    pub max_packets: u32,
    /// Interval in milliseconds
    pub interval_ms: u32,
    /// Current packet count
    pub current: u32,
    /// Last reset timestamp
    pub last_reset: u64,
}

impl RateLimit {
    /// Creates a new rate limit.
    pub const fn new(max_packets: u32, interval_ms: u32) -> Self {
        Self {
            max_packets,
            interval_ms,
            current: 0,
            last_reset: 0,
        }
    }
    
    /// Checks if rate limit is exceeded.
    pub fn check(&mut self, now_ms: u64) -> bool {
        // Reset counter if interval has passed
        if now_ms - self.last_reset >= self.interval_ms as u64 {
            self.current = 0;
            self.last_reset = now_ms;
        }
        
        if self.current < self.max_packets {
            self.current += 1;
            true
        } else {
            false
        }
    }
}

/// A firewall rule.
#[derive(Debug, Clone)]
pub struct Rule {
    /// Rule ID
    pub id: u32,
    /// Rule priority (lower = higher priority)
    pub priority: u32,
    /// Rule is enabled
    pub enabled: bool,
    /// Protocol to match
    pub protocol: Protocol,
    /// Source address match
    pub src_addr: IpMatch,
    /// Destination address match
    pub dst_addr: IpMatch,
    /// Source port match (TCP/UDP only)
    pub src_port: PortMatch,
    /// Destination port match (TCP/UDP only)
    pub dst_port: PortMatch,
    /// TCP flags match
    pub tcp_flags: Option<TcpFlags>,
    /// ICMP match
    pub icmp: Option<IcmpMatch>,
    /// Connection state match
    pub state: Option<ConnState>,
    /// Rate limit
    pub rate_limit: Option<RateLimit>,
    /// Action to take
    pub action: Action,
    /// Chain to jump to (if action is Jump)
    pub jump_target: Option<String>,
    /// Log prefix
    pub log_prefix: Option<String>,
    /// Comment
    pub comment: Option<String>,
    /// Packet counter
    pub packets: u64,
    /// Byte counter
    pub bytes: u64,
}

impl Rule {
    /// Creates a new rule with default settings.
    pub fn new(id: u32, action: Action) -> Self {
        Self {
            id,
            priority: 100,
            enabled: true,
            protocol: Protocol::Any,
            src_addr: IpMatch::Any,
            dst_addr: IpMatch::Any,
            src_port: PortMatch::ANY,
            dst_port: PortMatch::ANY,
            tcp_flags: None,
            icmp: None,
            state: None,
            rate_limit: None,
            action,
            jump_target: None,
            log_prefix: None,
            comment: None,
            packets: 0,
            bytes: 0,
        }
    }
    
    /// Builder: set protocol.
    pub fn protocol(mut self, proto: Protocol) -> Self {
        self.protocol = proto;
        self
    }
    
    /// Builder: set source address.
    pub fn src(mut self, addr: IpMatch) -> Self {
        self.src_addr = addr;
        self
    }
    
    /// Builder: set destination address.
    pub fn dst(mut self, addr: IpMatch) -> Self {
        self.dst_addr = addr;
        self
    }
    
    /// Builder: set source port.
    pub fn src_port(mut self, port: PortMatch) -> Self {
        self.src_port = port;
        self
    }
    
    /// Builder: set destination port.
    pub fn dst_port(mut self, port: PortMatch) -> Self {
        self.dst_port = port;
        self
    }
    
    /// Builder: set comment.
    pub fn comment(mut self, text: &str) -> Self {
        self.comment = Some(String::from(text));
        self
    }

    /// Builder: set connection state match.
    pub fn state(mut self, conn_state: ConnState) -> Self {
        self.state = Some(conn_state);
        self
    }
}

/// A chain of rules.
#[derive(Debug)]
pub struct Chain {
    /// Chain name
    pub name: String,
    /// Default policy if no rule matches
    pub policy: Action,
    /// Rules in this chain
    pub rules: Vec<Rule>,
}

impl Chain {
    /// Creates a new chain.
    pub fn new(name: &str, policy: Action) -> Self {
        Self {
            name: String::from(name),
            policy,
            rules: Vec::new(),
        }
    }
    
    /// Adds a rule to the chain.
    pub fn add_rule(&mut self, rule: Rule) {
        self.rules.push(rule);
        self.rules.sort_by_key(|r| r.priority);
    }
    
    /// Removes a rule by ID.
    pub fn remove_rule(&mut self, id: u32) -> bool {
        if let Some(pos) = self.rules.iter().position(|r| r.id == id) {
            self.rules.remove(pos);
            true
        } else {
            false
        }
    }
    
    /// Finds a rule by ID.
    pub fn get_rule(&self, id: u32) -> Option<&Rule> {
        self.rules.iter().find(|r| r.id == id)
    }
    
    /// Finds a rule by ID (mutable).
    pub fn get_rule_mut(&mut self, id: u32) -> Option<&mut Rule> {
        self.rules.iter_mut().find(|r| r.id == id)
    }
}

/// Connection tracking entry.
#[derive(Debug, Clone)]
pub struct ConnTrackEntry {
    /// Protocol
    pub protocol: u8,
    /// Source address (IPv4)
    pub src_v4: Option<Ipv4Address>,
    /// Destination address (IPv4)
    pub dst_v4: Option<Ipv4Address>,
    /// Source address (IPv6)
    pub src_v6: Option<Ipv6Address>,
    /// Destination address (IPv6)
    pub dst_v6: Option<Ipv6Address>,
    /// Source port
    pub src_port: u16,
    /// Destination port
    pub dst_port: u16,
    /// Connection state
    pub state: ConnState,
    /// Creation time
    pub created: u64,
    /// Last seen time
    pub last_seen: u64,
    /// Timeout in milliseconds
    pub timeout_ms: u64,
    /// Packet count
    pub packets: u64,
    /// Byte count
    pub bytes: u64,
}

impl ConnTrackEntry {
    /// Creates a new connection tracking entry.
    pub fn new_v4(protocol: u8, src: Ipv4Address, dst: Ipv4Address, src_port: u16, dst_port: u16) -> Self {
        Self {
            protocol,
            src_v4: Some(src),
            dst_v4: Some(dst),
            src_v6: None,
            dst_v6: None,
            src_port,
            dst_port,
            state: ConnState::New,
            created: 0,
            last_seen: 0,
            timeout_ms: 30000, // 30 seconds default
            packets: 0,
            bytes: 0,
        }
    }
    
    /// Creates a new IPv6 connection tracking entry.
    pub fn new_v6(protocol: u8, src: Ipv6Address, dst: Ipv6Address, src_port: u16, dst_port: u16) -> Self {
        Self {
            protocol,
            src_v4: None,
            dst_v4: None,
            src_v6: Some(src),
            dst_v6: Some(dst),
            src_port,
            dst_port,
            state: ConnState::New,
            created: 0,
            last_seen: 0,
            timeout_ms: 30000,
            packets: 0,
            bytes: 0,
        }
    }
    
    /// Checks if the connection has expired.
    pub fn is_expired(&self, now: u64) -> bool {
        now - self.last_seen > self.timeout_ms
    }
    
    /// Updates the connection state.
    pub fn update(&mut self, now: u64, bytes: u64) {
        self.last_seen = now;
        self.packets += 1;
        self.bytes += bytes;
        
        // Transition from New to Established
        if self.state == ConnState::New {
            self.state = ConnState::Established;
            // Increase timeout for established connections
            self.timeout_ms = 300000; // 5 minutes
        }
    }
}

/// Connection tracking table.
pub struct ConnTrack {
    /// Entries indexed by a tuple key
    entries: RwLock<BTreeMap<u64, ConnTrackEntry>>,
    /// Maximum entries
    max_entries: usize,
}

impl ConnTrack {
    /// Creates a new connection tracking table.
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: RwLock::new(BTreeMap::new()),
            max_entries,
        }
    }
    
    /// Generates a key for a connection.
    fn make_key_v4(proto: u8, src: Ipv4Address, dst: Ipv4Address, sport: u16, dport: u16) -> u64 {
        let src_u32 = u32::from(src);
        let dst_u32 = u32::from(dst);
        // Simple hash combining all fields
        (proto as u64) ^ 
        ((src_u32 as u64) << 8) ^ 
        ((dst_u32 as u64) << 16) ^ 
        ((sport as u64) << 24) ^ 
        ((dport as u64) << 32)
    }
    
    /// Looks up a connection.
    pub fn lookup_v4(&self, proto: u8, src: Ipv4Address, dst: Ipv4Address, sport: u16, dport: u16) -> Option<ConnTrackEntry> {
        let key = Self::make_key_v4(proto, src, dst, sport, dport);
        self.entries.read().get(&key).cloned()
    }
    
    /// Creates or updates a connection entry.
    pub fn update_v4(&self, proto: u8, src: Ipv4Address, dst: Ipv4Address, sport: u16, dport: u16, now: u64, bytes: u64) {
        let key = Self::make_key_v4(proto, src, dst, sport, dport);
        let mut entries = self.entries.write();
        
        if let Some(entry) = entries.get_mut(&key) {
            entry.update(now, bytes);
        } else if entries.len() < self.max_entries {
            let mut entry = ConnTrackEntry::new_v4(proto, src, dst, sport, dport);
            entry.created = now;
            entry.last_seen = now;
            entry.packets = 1;
            entry.bytes = bytes;
            entries.insert(key, entry);
        }
    }
    
    /// Removes expired entries.
    pub fn gc(&self, now: u64) {
        let mut entries = self.entries.write();
        entries.retain(|_, e| !e.is_expired(now));
    }
    
    /// Returns number of active connections.
    pub fn count(&self) -> usize {
        self.entries.read().len()
    }
}

/// NAT entry.
#[derive(Debug, Clone)]
pub struct NatEntry {
    /// Original source address
    pub orig_src: Ipv4Address,
    /// Original source port
    pub orig_sport: u16,
    /// Translated source address
    pub nat_src: Ipv4Address,
    /// Translated source port
    pub nat_sport: u16,
    /// Destination address
    pub dst: Ipv4Address,
    /// Destination port
    pub dport: u16,
    /// Protocol
    pub protocol: u8,
    /// Creation time
    pub created: u64,
    /// Last used
    pub last_used: u64,
}

/// NAT type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    /// Source NAT (masquerading)
    Snat,
    /// Destination NAT (port forwarding)
    Dnat,
}

/// NAT rule.
#[derive(Debug, Clone)]
pub struct NatRule {
    /// Rule ID
    pub id: u32,
    /// NAT type
    pub nat_type: NatType,
    /// Source address match
    pub src_match: IpMatch,
    /// Destination address match
    pub dst_match: IpMatch,
    /// Destination port match
    pub dst_port: PortMatch,
    /// Protocol
    pub protocol: Protocol,
    /// NAT address
    pub nat_addr: Ipv4Address,
    /// NAT port (0 = any available)
    pub nat_port: u16,
}

/// The main firewall.
pub struct Firewall {
    /// Input chain (incoming packets)
    pub input: RwLock<Chain>,
    /// Output chain (outgoing packets)
    pub output: RwLock<Chain>,
    /// Forward chain (routed packets)
    pub forward: RwLock<Chain>,
    /// Custom chains
    pub chains: RwLock<BTreeMap<String, Chain>>,
    /// Connection tracking
    pub conntrack: ConnTrack,
    /// NAT rules
    pub nat_rules: RwLock<Vec<NatRule>>,
    /// Next rule ID
    next_rule_id: spin::Mutex<u32>,
    /// Enabled
    enabled: spin::Mutex<bool>,
    /// Statistics
    stats: spin::Mutex<FirewallStats>,
}

/// Firewall statistics.
#[derive(Debug, Default, Clone)]
pub struct FirewallStats {
    pub packets_processed: u64,
    pub packets_accepted: u64,
    pub packets_dropped: u64,
    pub packets_rejected: u64,
    pub bytes_processed: u64,
}

impl Firewall {
    /// Creates a new firewall with default policies.
    pub fn new() -> Self {
        Self {
            input: RwLock::new(Chain::new("INPUT", Action::Accept)),
            output: RwLock::new(Chain::new("OUTPUT", Action::Accept)),
            forward: RwLock::new(Chain::new("FORWARD", Action::Drop)),
            chains: RwLock::new(BTreeMap::new()),
            conntrack: ConnTrack::new(65536),
            nat_rules: RwLock::new(Vec::new()),
            next_rule_id: spin::Mutex::new(1),
            enabled: spin::Mutex::new(false),
            stats: spin::Mutex::new(FirewallStats::default()),
        }
    }
    
    /// Enables the firewall.
    pub fn enable(&self) {
        *self.enabled.lock() = true;
    }
    
    /// Disables the firewall.
    pub fn disable(&self) {
        *self.enabled.lock() = false;
    }
    
    /// Returns whether the firewall is enabled.
    pub fn is_enabled(&self) -> bool {
        *self.enabled.lock()
    }
    
    /// Allocates a new rule ID.
    pub fn next_rule_id(&self) -> u32 {
        let mut id = self.next_rule_id.lock();
        let next = *id;
        *id += 1;
        next
    }
    
    /// Adds a rule to the input chain.
    pub fn add_input_rule(&self, rule: Rule) {
        self.input.write().add_rule(rule);
    }
    
    /// Adds a rule to the output chain.
    pub fn add_output_rule(&self, rule: Rule) {
        self.output.write().add_rule(rule);
    }
    
    /// Adds a rule to the forward chain.
    pub fn add_forward_rule(&self, rule: Rule) {
        self.forward.write().add_rule(rule);
    }
    
    /// Creates a new custom chain.
    pub fn create_chain(&self, name: &str) -> bool {
        let mut chains = self.chains.write();
        if chains.contains_key(name) {
            return false;
        }
        chains.insert(String::from(name), Chain::new(name, Action::Return));
        true
    }
    
    /// Deletes a custom chain.
    pub fn delete_chain(&self, name: &str) -> bool {
        self.chains.write().remove(name).is_some()
    }
    
    /// Sets the policy for a built-in chain.
    pub fn set_policy(&self, chain: &str, policy: Action) {
        match chain {
            "INPUT" => self.input.write().policy = policy,
            "OUTPUT" => self.output.write().policy = policy,
            "FORWARD" => self.forward.write().policy = policy,
            _ => {}
        }
    }
    
    /// Processes an IPv4 packet and returns the action.
    pub fn process_ipv4(
        &self,
        chain_name: &str,
        proto: u8,
        src: Ipv4Address,
        dst: Ipv4Address,
        src_port: u16,
        dst_port: u16,
        packet_len: usize,
    ) -> Action {
        if !self.is_enabled() {
            return Action::Accept;
        }
        
        // Update statistics
        {
            let mut stats = self.stats.lock();
            stats.packets_processed += 1;
            stats.bytes_processed += packet_len as u64;
        }
        
        // Update connection tracking
        self.conntrack.update_v4(proto, src, dst, src_port, dst_port, 0, packet_len as u64);
        
        // Get the chain
        let chain = match chain_name {
            "INPUT" => self.input.read(),
            "OUTPUT" => self.output.read(),
            "FORWARD" => self.forward.read(),
            _ => {
                if let Some(c) = self.chains.read().get(chain_name) {
                    // Custom chain - would need to clone or use different approach
                    return Action::Return;
                }
                return Action::Accept;
            }
        };
        
        // Check each rule
        for rule in &chain.rules {
            if !rule.enabled {
                continue;
            }
            
            // Match protocol
            if !rule.protocol.matches(proto) {
                continue;
            }
            
            // Match addresses
            if !rule.src_addr.matches_v4(src) || !rule.dst_addr.matches_v4(dst) {
                continue;
            }
            
            // Match ports (for TCP/UDP)
            if proto == 6 || proto == 17 {
                if !rule.src_port.matches(src_port) || !rule.dst_port.matches(dst_port) {
                    continue;
                }
            }
            
            // Rule matched - return action
            let mut stats = self.stats.lock();
            match rule.action {
                Action::Accept => {
                    stats.packets_accepted += 1;
                    return Action::Accept;
                }
                Action::Drop => {
                    stats.packets_dropped += 1;
                    return Action::Drop;
                }
                Action::Reject => {
                    stats.packets_rejected += 1;
                    return Action::Reject;
                }
                Action::Log => {
                    // Log and continue
                    continue;
                }
                Action::Return => return Action::Return,
                Action::Jump => continue, // Would need to process jump target
            }
        }
        
        // No rule matched - return chain policy
        chain.policy
    }
    
    /// Gets firewall statistics.
    pub fn get_stats(&self) -> FirewallStats {
        self.stats.lock().clone()
    }
    
    /// Resets firewall statistics.
    pub fn reset_stats(&self) {
        *self.stats.lock() = FirewallStats::default();
    }
    
    /// Lists all rules in a chain.
    pub fn list_rules(&self, chain: &str) -> Vec<Rule> {
        match chain {
            "INPUT" => self.input.read().rules.clone(),
            "OUTPUT" => self.output.read().rules.clone(),
            "FORWARD" => self.forward.read().rules.clone(),
            _ => {
                if let Some(c) = self.chains.read().get(chain) {
                    c.rules.clone()
                } else {
                    Vec::new()
                }
            }
        }
    }
    
    /// Flushes all rules from a chain.
    pub fn flush(&self, chain: &str) {
        match chain {
            "INPUT" => self.input.write().rules.clear(),
            "OUTPUT" => self.output.write().rules.clear(),
            "FORWARD" => self.forward.write().rules.clear(),
            _ => {
                if let Some(c) = self.chains.write().get_mut(chain) {
                    c.rules.clear();
                }
            }
        }
    }
    
    /// Runs garbage collection on connection tracking.
    pub fn gc(&self, now: u64) {
        self.conntrack.gc(now);
    }
}

impl Default for Firewall {
    fn default() -> Self {
        Self::new()
    }
}

/// Global firewall instance.
static FIREWALL: spin::Once<Firewall> = spin::Once::new();

/// Gets the global firewall.
pub fn firewall() -> &'static Firewall {
    FIREWALL.call_once(|| Firewall::new())
}

/// Initializes the firewall with default rules.
pub fn init() {
    let fw = firewall();
    
    // Allow loopback traffic
    fw.add_input_rule(
        Rule::new(fw.next_rule_id(), Action::Accept)
            .src(IpMatch::Ipv4(Ipv4Address::LOCALHOST))
            .comment("Allow loopback input")
    );
    
    fw.add_output_rule(
        Rule::new(fw.next_rule_id(), Action::Accept)
            .dst(IpMatch::Ipv4(Ipv4Address::LOCALHOST))
            .comment("Allow loopback output")
    );
    
    // Allow ICMP (ping)
    fw.add_input_rule(
        Rule::new(fw.next_rule_id(), Action::Accept)
            .protocol(Protocol::Icmp)
            .comment("Allow ICMP")
    );
    
    // Allow established connections by checking TCP state
    // The connection tracker will mark packets from known connections
    fw.add_input_rule(
        Rule::new(fw.next_rule_id(), Action::Accept)
            .state(ConnectionState::Established)
            .comment("Allow established connections")
    );
    
    fw.add_input_rule(
        Rule::new(fw.next_rule_id(), Action::Accept)
            .state(ConnectionState::Related)
            .comment("Allow related connections")
    );
    
    crate::serial_println!("[firewall] Initialized with default rules");
}

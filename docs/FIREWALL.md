# Firewall Subsystem Documentation

## Overview

The Splax OS firewall provides stateful packet filtering, connection tracking, rate limiting, and security rules for both IPv4 and IPv6 network traffic.

## Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Incoming Packet                               │
└────────────────────────────────┬────────────────────────────────────┘
                                 ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      Connection Tracking                             │
│                    (Check existing sessions)                         │
└────────────────────────────────┬────────────────────────────────────┘
                                 ▼
              ┌─────────────────────────────────────┐
              │  Existing Connection?                │
              └─────────┬───────────────┬───────────┘
                   Yes  │               │  No
                        ▼               ▼
              ┌─────────────┐   ┌───────────────────┐
              │ ESTABLISHED │   │   Rule Matching   │
              │   Accept    │   │   (Sequential)    │
              └─────────────┘   └─────────┬─────────┘
                                          ▼
                                ┌─────────────────────┐
                                │  Rate Limiting      │
                                │  (per-rule limits)  │
                                └─────────┬───────────┘
                                          ▼
                                ┌─────────────────────┐
                                │  Action Execution   │
                                │  ACCEPT/DROP/REJECT │
                                └─────────┬───────────┘
                                          ▼
                                ┌─────────────────────┐
                                │  Logging (if set)   │
                                └─────────────────────┘
```

## Rule Structure

### FirewallRule

```rust
#[derive(Clone, Debug)]
pub struct FirewallRule {
    /// Rule name for identification
    pub name: String,
    
    /// Rule priority (lower = higher priority)
    pub priority: u32,
    
    /// Match criteria
    pub matches: RuleMatches,
    
    /// Action to take
    pub action: Action,
    
    /// Enable logging for this rule
    pub log: bool,
    
    /// Rate limiting configuration
    pub rate_limit: Option<RateLimit>,
    
    /// Rule statistics
    pub stats: RuleStats,
}

#[derive(Clone, Debug, Default)]
pub struct RuleMatches {
    /// Source address/prefix
    pub src_addr: Option<IpMatch>,
    
    /// Destination address/prefix
    pub dst_addr: Option<IpMatch>,
    
    /// Source port or range
    pub src_port: Option<PortMatch>,
    
    /// Destination port or range
    pub dst_port: Option<PortMatch>,
    
    /// Protocol (TCP, UDP, ICMP, etc.)
    pub protocol: Option<Protocol>,
    
    /// Network interface
    pub interface: Option<String>,
    
    /// Direction (in, out, forward)
    pub direction: Direction,
    
    /// TCP flags
    pub tcp_flags: Option<TcpFlags>,
    
    /// Connection state
    pub state: Option<ConnState>,
}
```

### IP Address Matching

```rust
#[derive(Clone, Debug)]
pub enum IpMatch {
    /// Single address
    Single(IpAddress),
    
    /// CIDR prefix
    Prefix(IpAddress, u8),
    
    /// Address range
    Range(IpAddress, IpAddress),
    
    /// Any address
    Any,
}

impl IpMatch {
    pub fn matches(&self, addr: &IpAddress) -> bool {
        match self {
            IpMatch::Single(a) => addr == a,
            IpMatch::Prefix(prefix, len) => {
                match (prefix, addr) {
                    (IpAddress::V4(p), IpAddress::V4(a)) => {
                        let mask = !0u32 << (32 - len);
                        (p.to_bits() & mask) == (a.to_bits() & mask)
                    }
                    (IpAddress::V6(p), IpAddress::V6(a)) => {
                        prefix_matches_v6(p, *len, a)
                    }
                    _ => false,
                }
            }
            IpMatch::Range(start, end) => addr >= start && addr <= end,
            IpMatch::Any => true,
        }
    }
}
```

### Port Matching

```rust
#[derive(Clone, Debug)]
pub enum PortMatch {
    /// Single port
    Single(u16),
    
    /// Port range (inclusive)
    Range(u16, u16),
    
    /// Multiple ports
    List(Vec<u16>),
    
    /// Any port
    Any,
}

impl PortMatch {
    pub fn matches(&self, port: u16) -> bool {
        match self {
            PortMatch::Single(p) => port == *p,
            PortMatch::Range(start, end) => port >= *start && port <= *end,
            PortMatch::List(ports) => ports.contains(&port),
            PortMatch::Any => true,
        }
    }
}
```

## Actions

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Accept the packet
    Accept,
    
    /// Silently drop the packet
    Drop,
    
    /// Reject with ICMP error
    Reject(RejectType),
    
    /// Jump to another chain
    Jump(ChainId),
    
    /// Return from current chain
    Return,
    
    /// Log and continue
    Log,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RejectType {
    /// ICMP port unreachable
    PortUnreachable,
    
    /// ICMP host unreachable
    HostUnreachable,
    
    /// ICMP network unreachable
    NetworkUnreachable,
    
    /// TCP RST
    TcpReset,
    
    /// ICMPv6 admin prohibited
    AdminProhibited,
}
```

## Connection Tracking

### Connection Entry

```rust
#[derive(Clone, Debug)]
pub struct Connection {
    /// Connection tuple
    pub tuple: ConnTuple,
    
    /// Connection state
    pub state: ConnState,
    
    /// Creation timestamp
    pub created: u64,
    
    /// Last packet timestamp
    pub last_seen: u64,
    
    /// Timeout value
    pub timeout: u64,
    
    /// Packet counts
    pub packets_in: u64,
    pub packets_out: u64,
    
    /// Byte counts
    pub bytes_in: u64,
    pub bytes_out: u64,
    
    /// Associated NAT mapping
    pub nat: Option<NatMapping>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ConnTuple {
    pub src_addr: IpAddress,
    pub dst_addr: IpAddress,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: Protocol,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConnState {
    /// New connection (SYN sent)
    New,
    
    /// Connection established
    Established,
    
    /// Related connection (e.g., FTP data)
    Related,
    
    /// Invalid packet
    Invalid,
    
    /// Connection closing
    TimeWait,
}
```

### Connection Table

```rust
static CONN_TABLE: RwLock<BTreeMap<ConnTuple, Connection>> = 
    RwLock::new(BTreeMap::new());

pub fn lookup_connection(tuple: &ConnTuple) -> Option<Connection> {
    // Check forward direction
    if let Some(conn) = CONN_TABLE.read().get(tuple) {
        return Some(conn.clone());
    }
    
    // Check reverse direction
    let reverse = ConnTuple {
        src_addr: tuple.dst_addr,
        dst_addr: tuple.src_addr,
        src_port: tuple.dst_port,
        dst_port: tuple.src_port,
        protocol: tuple.protocol,
    };
    
    CONN_TABLE.read().get(&reverse).cloned()
}

pub fn create_connection(tuple: ConnTuple) -> Connection {
    let conn = Connection {
        tuple: tuple.clone(),
        state: ConnState::New,
        created: get_timestamp(),
        last_seen: get_timestamp(),
        timeout: get_timeout_for_protocol(tuple.protocol),
        packets_in: 1,
        packets_out: 0,
        bytes_in: 0,
        bytes_out: 0,
        nat: None,
    };
    
    CONN_TABLE.write().insert(tuple, conn.clone());
    conn
}
```

### TCP State Tracking

```rust
pub fn update_tcp_state(conn: &mut Connection, flags: TcpFlags, direction: Direction) {
    conn.state = match (conn.state, flags, direction) {
        // New connection
        (ConnState::New, f, Direction::In) if f.syn && !f.ack => ConnState::New,
        
        // SYN-ACK received
        (ConnState::New, f, Direction::Out) if f.syn && f.ack => ConnState::New,
        
        // ACK completes handshake
        (ConnState::New, f, Direction::In) if f.ack => ConnState::Established,
        
        // FIN received
        (ConnState::Established, f, _) if f.fin => ConnState::TimeWait,
        
        // RST terminates
        (_, f, _) if f.rst => ConnState::Invalid,
        
        // Keep current state
        (state, _, _) => state,
    };
    
    conn.last_seen = get_timestamp();
}
```

## Rate Limiting

### Token Bucket Algorithm

```rust
#[derive(Clone, Debug)]
pub struct RateLimit {
    /// Maximum packets per interval
    pub limit: u32,
    
    /// Time interval in seconds
    pub interval: u32,
    
    /// Burst allowance
    pub burst: u32,
}

#[derive(Debug)]
struct TokenBucket {
    tokens: u32,
    last_update: u64,
    limit: u32,
    interval: u32,
    burst: u32,
}

impl TokenBucket {
    fn consume(&mut self) -> bool {
        let now = get_timestamp();
        let elapsed = now - self.last_update;
        
        // Refill tokens
        let refill = (elapsed * self.limit as u64) / (self.interval as u64 * 1000);
        self.tokens = core::cmp::min(
            self.tokens + refill as u32,
            self.burst
        );
        self.last_update = now;
        
        // Try to consume
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }
}

static RATE_LIMITERS: Mutex<BTreeMap<String, TokenBucket>> = 
    Mutex::new(BTreeMap::new());

pub fn check_rate_limit(rule_name: &str, limit: &RateLimit) -> bool {
    let mut limiters = RATE_LIMITERS.lock();
    
    let bucket = limiters.entry(rule_name.into()).or_insert_with(|| {
        TokenBucket {
            tokens: limit.burst,
            last_update: get_timestamp(),
            limit: limit.limit,
            interval: limit.interval,
            burst: limit.burst,
        }
    });
    
    bucket.consume()
}
```

## Rule Chains

### Built-in Chains

```rust
pub enum Chain {
    /// Incoming packets destined for local
    Input,
    
    /// Outgoing packets from local
    Output,
    
    /// Forwarded packets
    Forward,
    
    /// Pre-routing (before routing decision)
    PreRouting,
    
    /// Post-routing (after routing decision)
    PostRouting,
}

static INPUT_RULES: RwLock<Vec<FirewallRule>> = RwLock::new(Vec::new());
static OUTPUT_RULES: RwLock<Vec<FirewallRule>> = RwLock::new(Vec::new());
static FORWARD_RULES: RwLock<Vec<FirewallRule>> = RwLock::new(Vec::new());
```

### Rule Evaluation

```rust
pub fn evaluate_chain(chain: Chain, packet: &Packet) -> Action {
    let rules = match chain {
        Chain::Input => INPUT_RULES.read(),
        Chain::Output => OUTPUT_RULES.read(),
        Chain::Forward => FORWARD_RULES.read(),
        _ => return Action::Accept,
    };
    
    // Check connection tracking first
    if let Some(conn) = lookup_connection(&packet.tuple()) {
        if conn.state == ConnState::Established {
            return Action::Accept;
        }
    }
    
    // Evaluate rules in priority order
    for rule in rules.iter() {
        if matches_rule(rule, packet) {
            // Update statistics
            rule.stats.matches.fetch_add(1, Ordering::Relaxed);
            rule.stats.bytes.fetch_add(packet.len() as u64, Ordering::Relaxed);
            
            // Check rate limit
            if let Some(ref limit) = rule.rate_limit {
                if !check_rate_limit(&rule.name, limit) {
                    continue; // Rate limited, try next rule
                }
            }
            
            // Log if enabled
            if rule.log {
                log_packet(packet, &rule.name, rule.action);
            }
            
            return rule.action;
        }
    }
    
    // Default policy
    get_default_policy(chain)
}
```

## API

### Adding Rules

```rust
pub fn add_rule(chain: Chain, rule: FirewallRule) -> Result<(), FirewallError> {
    let rules = match chain {
        Chain::Input => &INPUT_RULES,
        Chain::Output => &OUTPUT_RULES,
        Chain::Forward => &FORWARD_RULES,
        _ => return Err(FirewallError::InvalidChain),
    };
    
    let mut rules = rules.write();
    
    // Check for duplicate names
    if rules.iter().any(|r| r.name == rule.name) {
        return Err(FirewallError::DuplicateName);
    }
    
    // Insert in priority order
    let pos = rules.iter()
        .position(|r| r.priority > rule.priority)
        .unwrap_or(rules.len());
    
    rules.insert(pos, rule);
    Ok(())
}
```

### Removing Rules

```rust
pub fn remove_rule(chain: Chain, name: &str) -> Result<(), FirewallError> {
    let rules = match chain {
        Chain::Input => &INPUT_RULES,
        Chain::Output => &OUTPUT_RULES,
        Chain::Forward => &FORWARD_RULES,
        _ => return Err(FirewallError::InvalidChain),
    };
    
    let mut rules = rules.write();
    
    if let Some(pos) = rules.iter().position(|r| r.name == name) {
        rules.remove(pos);
        Ok(())
    } else {
        Err(FirewallError::RuleNotFound)
    }
}
```

### Flushing Rules

```rust
pub fn flush_chain(chain: Chain) {
    match chain {
        Chain::Input => INPUT_RULES.write().clear(),
        Chain::Output => OUTPUT_RULES.write().clear(),
        Chain::Forward => FORWARD_RULES.write().clear(),
        _ => {}
    }
}
```

## Default Rules

```rust
pub fn init_default_rules() {
    // Allow loopback
    add_rule(Chain::Input, FirewallRule {
        name: "allow-loopback".into(),
        priority: 0,
        matches: RuleMatches {
            interface: Some("lo".into()),
            ..Default::default()
        },
        action: Action::Accept,
        ..Default::default()
    }).unwrap();
    
    // Allow established connections
    add_rule(Chain::Input, FirewallRule {
        name: "allow-established".into(),
        priority: 10,
        matches: RuleMatches {
            state: Some(ConnState::Established),
            ..Default::default()
        },
        action: Action::Accept,
        ..Default::default()
    }).unwrap();
    
    // Allow ICMPv6 neighbor discovery
    add_rule(Chain::Input, FirewallRule {
        name: "allow-ndp".into(),
        priority: 20,
        matches: RuleMatches {
            protocol: Some(Protocol::Icmpv6),
            ..Default::default()
        },
        action: Action::Accept,
        ..Default::default()
    }).unwrap();
    
    // Drop invalid packets
    add_rule(Chain::Input, FirewallRule {
        name: "drop-invalid".into(),
        priority: 100,
        matches: RuleMatches {
            state: Some(ConnState::Invalid),
            ..Default::default()
        },
        action: Action::Drop,
        log: true,
        ..Default::default()
    }).unwrap();
}
```

## Shell Commands

### firewall status

```
splax> firewall status
Firewall: ENABLED
Default policies:
  INPUT: DROP
  OUTPUT: ACCEPT
  FORWARD: DROP

Active connections: 42
Rate limiters: 3
```

### firewall rules

```
splax> firewall rules
Chain INPUT (policy DROP):
  1. allow-loopback      interface=lo                  ACCEPT
  2. allow-established   state=ESTABLISHED             ACCEPT
  3. allow-ssh           tcp dport=22                  ACCEPT (rate: 10/min)
  4. allow-http          tcp dport=80,443              ACCEPT
  5. drop-invalid        state=INVALID                 DROP [log]

Chain OUTPUT (policy ACCEPT):
  (no rules)

Chain FORWARD (policy DROP):
  1. forward-lan         src=192.168.1.0/24            ACCEPT
```

### firewall stats

```
splax> firewall stats
Rule Statistics:
  allow-loopback:     1,234 packets, 123.4 KB
  allow-established:  45,678 packets, 12.3 MB
  allow-ssh:          89 packets, 8.9 KB
  allow-http:         12,345 packets, 45.6 MB
  drop-invalid:       23 packets, 1.2 KB

Connection Tracking:
  Total entries: 42
  TCP: 35 (ESTABLISHED: 30, TIME_WAIT: 5)
  UDP: 7
```

### firewall add

```
splax> firewall add input allow-dns -p udp --dport 53 -j ACCEPT
Rule 'allow-dns' added to INPUT chain

splax> firewall add input rate-limit-icmp -p icmp -j ACCEPT --limit 10/s --burst 20
Rule 'rate-limit-icmp' added to INPUT chain
```

### firewall del

```
splax> firewall del input allow-dns
Rule 'allow-dns' removed from INPUT chain
```

## Statistics

```rust
#[derive(Default)]
pub struct FirewallStats {
    pub packets_received: AtomicU64,
    pub packets_accepted: AtomicU64,
    pub packets_dropped: AtomicU64,
    pub packets_rejected: AtomicU64,
    pub bytes_received: AtomicU64,
    pub bytes_accepted: AtomicU64,
    pub bytes_dropped: AtomicU64,
    pub connections_created: AtomicU64,
    pub connections_expired: AtomicU64,
}

static STATS: FirewallStats = FirewallStats::default();

pub fn get_stats() -> FirewallStats {
    // Return copy of current stats
}
```

## Logging

```rust
pub fn log_packet(packet: &Packet, rule_name: &str, action: Action) {
    serial_println!(
        "[FIREWALL] {} {} {} {}:{} -> {}:{} len={}",
        match action {
            Action::Accept => "ACCEPT",
            Action::Drop => "DROP",
            Action::Reject(_) => "REJECT",
            _ => "???",
        },
        rule_name,
        packet.protocol(),
        packet.src_addr(),
        packet.src_port(),
        packet.dst_addr(),
        packet.dst_port(),
        packet.len()
    );
}
```

## Error Handling

```rust
pub enum FirewallError {
    InvalidChain,
    RuleNotFound,
    DuplicateName,
    InvalidRule,
    InvalidAddress,
    InvalidPort,
    InvalidProtocol,
    RateLimitExceeded,
}
```

## Security Considerations

1. **SYN Flood Protection** - Rate limit new connections
2. **Port Scanning Detection** - Log and block scanners
3. **Spoofing Prevention** - BCP38 ingress filtering
4. **Fragment Attacks** - Careful fragment handling
5. **Connection Limits** - Per-IP connection limits

## Future Enhancements

1. **NAT Support** - SNAT/DNAT/Masquerade
2. **Zone-based Firewall** - Interface zones
3. **Application Layer Filtering** - Deep packet inspection
4. **Geolocation Filtering** - Block by country
5. **Fail2ban Integration** - Dynamic banning
6. **nftables-style Syntax** - Modern rule format

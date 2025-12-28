# IPv6 Network Stack Documentation

## Overview

The IPv6 subsystem implements Internet Protocol version 6, providing next-generation internet connectivity with expanded address space, simplified headers, and improved security.

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                         Application Layer                             │
│                  (Sockets, DNS, HTTP, SSH, etc.)                     │
├──────────────────────────────────────────────────────────────────────┤
│                        Transport Layer                                │
│              ┌──────────────┐    ┌──────────────┐                    │
│              │     TCP      │    │     UDP      │                    │
│              └──────────────┘    └──────────────┘                    │
├──────────────────────────────────────────────────────────────────────┤
│                         Network Layer                                 │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐ │
│  │    IPv6     │  │   ICMPv6    │  │     NDP     │  │    MLD      │ │
│  │             │  │             │  │             │  │             │ │
│  │ - Routing   │  │ - Echo      │  │ - Neighbor  │  │ - Multicast │ │
│  │ - Fragment  │  │ - Error     │  │ - Router    │  │ - Listener  │ │
│  │ - Extension │  │ - Redirect  │  │ - Discovery │  │ - Discovery │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘ │
├──────────────────────────────────────────────────────────────────────┤
│                         Link Layer                                    │
│              ┌──────────────┐    ┌──────────────┐                    │
│              │   Ethernet   │    │    WiFi      │                    │
│              └──────────────┘    └──────────────┘                    │
└──────────────────────────────────────────────────────────────────────┘
```

## IPv6 Address Format

### Structure

128-bit address, written as eight 16-bit hexadecimal groups:

```
2001:0db8:85a3:0000:0000:8a2e:0370:7334
       │    │    │    │    │    │    └─ Interface ID
       │    │    │    │    │    └────── Interface ID
       │    │    │    │    └─────────── Interface ID
       │    │    │    └──────────────── Interface ID
       │    │    └───────────────────── Subnet ID
       │    └────────────────────────── Site prefix
       └─────────────────────────────── Global routing prefix
```

### Address Types

| Type | Prefix | Description |
|------|--------|-------------|
| Unspecified | ::/128 | All zeros |
| Loopback | ::1/128 | Localhost |
| Link-Local | fe80::/10 | Local network only |
| Site-Local | fec0::/10 | Deprecated |
| Global Unicast | 2000::/3 | Internet routable |
| Multicast | ff00::/8 | One-to-many |
| Unique Local | fc00::/7 | Private networks |

### Implementation

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ipv6Address([u8; 16]);

impl Ipv6Address {
    /// Unspecified address (::)
    pub const UNSPECIFIED: Self = Self([0; 16]);
    
    /// Loopback address (::1)
    pub const LOOPBACK: Self = Self([
        0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 1
    ]);
    
    /// All-nodes multicast (ff02::1)
    pub const ALL_NODES: Self = Self([
        0xff, 0x02, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0x01
    ]);
    
    /// All-routers multicast (ff02::2)
    pub const ALL_ROUTERS: Self = Self([
        0xff, 0x02, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0x02
    ]);
    
    pub fn is_loopback(&self) -> bool {
        self.0[..15] == [0; 15] && self.0[15] == 1
    }
    
    pub fn is_link_local(&self) -> bool {
        self.0[0] == 0xfe && (self.0[1] & 0xc0) == 0x80
    }
    
    pub fn is_multicast(&self) -> bool {
        self.0[0] == 0xff
    }
    
    pub fn is_global(&self) -> bool {
        (self.0[0] & 0xe0) == 0x20 // 2000::/3
    }
    
    /// Generate link-local address from MAC
    pub fn from_mac_link_local(mac: [u8; 6]) -> Self {
        let mut addr = [0u8; 16];
        addr[0] = 0xfe;
        addr[1] = 0x80;
        // Modified EUI-64
        addr[8] = mac[0] ^ 0x02;
        addr[9] = mac[1];
        addr[10] = mac[2];
        addr[11] = 0xff;
        addr[12] = 0xfe;
        addr[13] = mac[3];
        addr[14] = mac[4];
        addr[15] = mac[5];
        Self(addr)
    }
    
    /// Solicited-node multicast address
    pub fn solicited_node(&self) -> Self {
        let mut addr = [0u8; 16];
        addr[0] = 0xff;
        addr[1] = 0x02;
        addr[11] = 0x01;
        addr[12] = 0xff;
        addr[13] = self.0[13];
        addr[14] = self.0[14];
        addr[15] = self.0[15];
        Self(addr)
    }
}

impl fmt::Display for Ipv6Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Compress zeros for display
        write!(f, "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
            u16::from_be_bytes([self.0[0], self.0[1]]),
            u16::from_be_bytes([self.0[2], self.0[3]]),
            u16::from_be_bytes([self.0[4], self.0[5]]),
            u16::from_be_bytes([self.0[6], self.0[7]]),
            u16::from_be_bytes([self.0[8], self.0[9]]),
            u16::from_be_bytes([self.0[10], self.0[11]]),
            u16::from_be_bytes([self.0[12], self.0[13]]),
            u16::from_be_bytes([self.0[14], self.0[15]]))
    }
}
```

## IPv6 Header

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|Version| Traffic Class |           Flow Label                  |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|         Payload Length        |  Next Header  |   Hop Limit   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                                                               +
|                                                               |
+                         Source Address                        +
|                                                               |
+                                                               +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                                                               |
+                                                               +
|                                                               |
+                      Destination Address                      +
|                                                               |
+                                                               +
|                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### Implementation

```rust
#[repr(C, packed)]
pub struct Ipv6Header {
    pub version_tc_fl: u32,       // Version (4), Traffic Class (8), Flow Label (20)
    pub payload_length: u16,      // Length of payload (big-endian)
    pub next_header: u8,          // Next header type
    pub hop_limit: u8,            // TTL equivalent
    pub source: [u8; 16],         // Source address
    pub destination: [u8; 16],    // Destination address
}

impl Ipv6Header {
    pub const SIZE: usize = 40;
    
    pub fn version(&self) -> u8 {
        ((u32::from_be(self.version_tc_fl) >> 28) & 0xF) as u8
    }
    
    pub fn traffic_class(&self) -> u8 {
        ((u32::from_be(self.version_tc_fl) >> 20) & 0xFF) as u8
    }
    
    pub fn flow_label(&self) -> u32 {
        u32::from_be(self.version_tc_fl) & 0xFFFFF
    }
    
    pub fn set_version_tc_fl(version: u8, tc: u8, flow: u32) -> u32 {
        ((version as u32) << 28) | ((tc as u32) << 20) | (flow & 0xFFFFF)
    }
}

// Next Header values
pub const NH_HOP_BY_HOP: u8 = 0;
pub const NH_ICMPV6: u8 = 58;
pub const NH_TCP: u8 = 6;
pub const NH_UDP: u8 = 17;
pub const NH_ROUTING: u8 = 43;
pub const NH_FRAGMENT: u8 = 44;
pub const NH_ESP: u8 = 50;
pub const NH_AH: u8 = 51;
pub const NH_NONE: u8 = 59;
pub const NH_DEST_OPTIONS: u8 = 60;
```

## Extension Headers

IPv6 uses optional extension headers for additional features:

```rust
pub struct ExtensionHeader {
    pub next_header: u8,
    pub length: u8,      // Length in 8-byte units (excluding first 8)
    pub data: Vec<u8>,
}

pub fn parse_extension_headers(packet: &[u8], start_nh: u8) -> (u8, usize) {
    let mut nh = start_nh;
    let mut offset = Ipv6Header::SIZE;
    
    loop {
        match nh {
            NH_HOP_BY_HOP | NH_ROUTING | NH_DEST_OPTIONS => {
                let next = packet[offset];
                let len = (packet[offset + 1] as usize + 1) * 8;
                offset += len;
                nh = next;
            }
            NH_FRAGMENT => {
                let next = packet[offset];
                offset += 8;
                nh = next;
            }
            _ => break,
        }
    }
    
    (nh, offset)
}
```

## ICMPv6

### Message Types

| Type | Name | Description |
|------|------|-------------|
| 1 | Destination Unreachable | Cannot deliver packet |
| 2 | Packet Too Big | MTU exceeded |
| 3 | Time Exceeded | Hop limit exceeded |
| 4 | Parameter Problem | Invalid header |
| 128 | Echo Request | Ping request |
| 129 | Echo Reply | Ping response |
| 133 | Router Solicitation | Ask for routers |
| 134 | Router Advertisement | Router announcement |
| 135 | Neighbor Solicitation | ARP equivalent |
| 136 | Neighbor Advertisement | ARP reply equivalent |
| 137 | Redirect | Better route available |

### Implementation

```rust
#[repr(C, packed)]
pub struct Icmpv6Header {
    pub msg_type: u8,
    pub code: u8,
    pub checksum: u16,
}

#[repr(C, packed)]
pub struct Icmpv6EchoHeader {
    pub header: Icmpv6Header,
    pub identifier: u16,
    pub sequence: u16,
}

pub fn send_echo_request(dest: Ipv6Address, seq: u16, data: &[u8]) {
    let mut packet = Vec::with_capacity(8 + data.len());
    
    // ICMPv6 Echo Request
    packet.push(128);  // Type
    packet.push(0);    // Code
    packet.extend(&[0, 0]); // Checksum (computed later)
    packet.extend(&(0x1234u16).to_be_bytes()); // Identifier
    packet.extend(&seq.to_be_bytes());
    packet.extend(data);
    
    // Compute checksum with pseudo-header
    let checksum = icmpv6_checksum(&source, &dest, &packet);
    packet[2..4].copy_from_slice(&checksum.to_be_bytes());
    
    // Send via IPv6
    ipv6_send(dest, NH_ICMPV6, &packet);
}

pub fn icmpv6_checksum(src: &Ipv6Address, dst: &Ipv6Address, data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    
    // Pseudo-header
    for chunk in src.0.chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    for chunk in dst.0.chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    sum += data.len() as u32;
    sum += NH_ICMPV6 as u32;
    
    // Data
    let mut i = 0;
    while i < data.len() {
        if i + 1 < data.len() {
            sum += u16::from_be_bytes([data[i], data[i+1]]) as u32;
        } else {
            sum += (data[i] as u32) << 8;
        }
        i += 2;
    }
    
    // Fold and complement
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    
    !(sum as u16)
}
```

## Neighbor Discovery Protocol (NDP)

Replaces ARP in IPv6:

### Neighbor Cache

```rust
pub struct NeighborEntry {
    pub ip: Ipv6Address,
    pub mac: [u8; 6],
    pub state: NeighborState,
    pub last_used: u64,
    pub probes: u8,
}

#[derive(Clone, Copy, PartialEq)]
pub enum NeighborState {
    Incomplete,    // Solicitation sent, awaiting reply
    Reachable,     // Recently confirmed reachable
    Stale,         // May be unreachable
    Delay,         // Waiting before probe
    Probe,         // Actively probing
}

static NEIGHBOR_CACHE: Mutex<BTreeMap<Ipv6Address, NeighborEntry>> = 
    Mutex::new(BTreeMap::new());
```

### Neighbor Solicitation

```rust
#[repr(C, packed)]
pub struct NeighborSolicitation {
    pub header: Icmpv6Header,
    pub reserved: u32,
    pub target: [u8; 16],
    // Options follow
}

pub fn send_neighbor_solicitation(target: Ipv6Address) {
    let dest = target.solicited_node();
    let mut packet = vec![0u8; 24];
    
    packet[0] = 135; // Type: Neighbor Solicitation
    packet[1] = 0;   // Code
    // Checksum at [2..4]
    // Reserved at [4..8]
    packet[8..24].copy_from_slice(&target.0);
    
    // Add Source Link-Layer Address option
    packet.push(1);   // Type: Source Link-Layer Address
    packet.push(1);   // Length: 1 (8 bytes)
    packet.extend(&get_mac_address());
    
    let checksum = icmpv6_checksum(&get_source_address(), &dest, &packet);
    packet[2..4].copy_from_slice(&checksum.to_be_bytes());
    
    ipv6_send(dest, NH_ICMPV6, &packet);
}
```

### Neighbor Advertisement

```rust
pub fn send_neighbor_advertisement(target: Ipv6Address, dest: Ipv6Address, solicited: bool) {
    let mut packet = vec![0u8; 24];
    
    packet[0] = 136; // Type: Neighbor Advertisement
    packet[1] = 0;   // Code
    
    // Flags: R=0, S=solicited, O=1
    let flags = if solicited { 0x60000000u32 } else { 0x20000000u32 };
    packet[4..8].copy_from_slice(&flags.to_be_bytes());
    packet[8..24].copy_from_slice(&target.0);
    
    // Add Target Link-Layer Address option
    packet.push(2);   // Type: Target Link-Layer Address
    packet.push(1);   // Length: 1 (8 bytes)
    packet.extend(&get_mac_address());
    
    let checksum = icmpv6_checksum(&target, &dest, &packet);
    packet[2..4].copy_from_slice(&checksum.to_be_bytes());
    
    ipv6_send(dest, NH_ICMPV6, &packet);
}
```

## Router Discovery

### Router Solicitation

```rust
pub fn send_router_solicitation() {
    let dest = Ipv6Address::ALL_ROUTERS;
    let mut packet = vec![0u8; 8];
    
    packet[0] = 133; // Type: Router Solicitation
    packet[1] = 0;   // Code
    // Reserved at [4..8]
    
    // Add Source Link-Layer Address option
    packet.push(1);
    packet.push(1);
    packet.extend(&get_mac_address());
    
    let checksum = icmpv6_checksum(&get_link_local(), &dest, &packet);
    packet[2..4].copy_from_slice(&checksum.to_be_bytes());
    
    ipv6_send(dest, NH_ICMPV6, &packet);
}
```

### Router Advertisement Parsing

```rust
pub struct RouterAdvertisement {
    pub hop_limit: u8,
    pub managed: bool,        // M flag: Use DHCPv6 for addresses
    pub other: bool,          // O flag: Use DHCPv6 for other config
    pub lifetime: u16,        // Router lifetime
    pub reachable_time: u32,
    pub retrans_timer: u32,
    pub prefixes: Vec<PrefixInfo>,
    pub mtu: Option<u32>,
}

pub struct PrefixInfo {
    pub prefix: Ipv6Address,
    pub prefix_len: u8,
    pub on_link: bool,
    pub autonomous: bool,     // Can form address from this prefix
    pub valid_lifetime: u32,
    pub preferred_lifetime: u32,
}
```

## Routing

### Routing Table

```rust
pub struct Route6 {
    pub prefix: Ipv6Address,
    pub prefix_len: u8,
    pub gateway: Option<Ipv6Address>,
    pub interface: String,
    pub metric: u32,
}

static ROUTING_TABLE: RwLock<Vec<Route6>> = RwLock::new(Vec::new());

pub fn route_lookup(dest: &Ipv6Address) -> Option<Route6> {
    let routes = ROUTING_TABLE.read();
    let mut best_match: Option<&Route6> = None;
    let mut best_len = 0;
    
    for route in routes.iter() {
        if prefix_matches(&route.prefix, route.prefix_len, dest) {
            if route.prefix_len > best_len {
                best_match = Some(route);
                best_len = route.prefix_len;
            }
        }
    }
    
    best_match.cloned()
}

fn prefix_matches(prefix: &Ipv6Address, len: u8, addr: &Ipv6Address) -> bool {
    let full_bytes = (len / 8) as usize;
    let remaining_bits = len % 8;
    
    if prefix.0[..full_bytes] != addr.0[..full_bytes] {
        return false;
    }
    
    if remaining_bits > 0 {
        let mask = 0xFF << (8 - remaining_bits);
        if (prefix.0[full_bytes] & mask) != (addr.0[full_bytes] & mask) {
            return false;
        }
    }
    
    true
}
```

## Stateless Address Autoconfiguration (SLAAC)

```rust
pub fn configure_slaac(prefix: &PrefixInfo, interface: &str) {
    if !prefix.autonomous {
        return;
    }
    
    let mac = get_interface_mac(interface);
    
    // Generate address using Modified EUI-64
    let mut addr = prefix.prefix.0;
    addr[8] = mac[0] ^ 0x02;
    addr[9] = mac[1];
    addr[10] = mac[2];
    addr[11] = 0xff;
    addr[12] = 0xfe;
    addr[13] = mac[3];
    addr[14] = mac[4];
    addr[15] = mac[5];
    
    let address = Ipv6Address(addr);
    
    // Duplicate Address Detection
    if !perform_dad(&address) {
        // Address is unique, add it
        add_address(interface, address, prefix.prefix_len);
    }
}
```

## Shell Commands

### ip6 addr

Display IPv6 addresses:

```
splax> ip6 addr
eth0:
    inet6 fe80::5054:ff:fe12:3456/64 scope link
    inet6 2001:db8::1/64 scope global
lo:
    inet6 ::1/128 scope host
```

### ip6 route

Display routing table:

```
splax> ip6 route
2001:db8::/64 dev eth0 proto kernel metric 256
fe80::/64 dev eth0 proto kernel metric 256
default via fe80::1 dev eth0 metric 1024
```

### ip6 neigh

Display neighbor cache:

```
splax> ip6 neigh
fe80::1 dev eth0 lladdr 52:54:00:12:34:56 REACHABLE
2001:db8::100 dev eth0 lladdr 52:54:00:ab:cd:ef STALE
```

### ping6

```
splax> ping6 ::1
PING6 ::1: 56 data bytes
64 bytes from ::1: icmp_seq=1 ttl=64 time=0.1 ms
64 bytes from ::1: icmp_seq=2 ttl=64 time=0.1 ms
```

## Multicast

### Multicast Listener Discovery (MLD)

```rust
pub fn join_multicast_group(group: Ipv6Address) {
    // Add to solicited-node multicast
    let solicited = group.solicited_node();
    MULTICAST_GROUPS.lock().insert(solicited);
    
    // Send MLD Report
    send_mld_report(group);
    
    // Configure Ethernet multicast filter
    let mac = multicast_mac(&group);
    add_multicast_filter(mac);
}

fn multicast_mac(group: &Ipv6Address) -> [u8; 6] {
    [0x33, 0x33, group.0[12], group.0[13], group.0[14], group.0[15]]
}
```

## Fragmentation

```rust
#[repr(C, packed)]
pub struct FragmentHeader {
    pub next_header: u8,
    pub reserved: u8,
    pub offset_flags: u16,  // Offset (13 bits) + Reserved (2) + M flag (1)
    pub identification: u32,
}

pub fn fragment_packet(data: &[u8], mtu: usize) -> Vec<Vec<u8>> {
    let max_payload = ((mtu - Ipv6Header::SIZE - 8) / 8) * 8;
    let mut fragments = Vec::new();
    let mut offset = 0;
    let id = next_fragment_id();
    
    while offset < data.len() {
        let end = core::cmp::min(offset + max_payload, data.len());
        let more = end < data.len();
        
        let mut frag = Vec::new();
        // Add fragment header
        frag.push(NH_UDP); // Original next header
        frag.push(0);
        let offset_flags = ((offset / 8) as u16) << 3 | if more { 1 } else { 0 };
        frag.extend(&offset_flags.to_be_bytes());
        frag.extend(&id.to_be_bytes());
        frag.extend(&data[offset..end]);
        
        fragments.push(frag);
        offset = end;
    }
    
    fragments
}
```

## Security Considerations

1. **IPsec** - Mandatory support in IPv6 specification
2. **Privacy Extensions** - Temporary addresses (RFC 4941)
3. **SeND** - Secure Neighbor Discovery
4. **RA Guard** - Protect against rogue Router Advertisements

## Future Enhancements

1. **DHCPv6** - Stateful address configuration
2. **IPsec Implementation** - ESP/AH headers
3. **Mobile IPv6** - Mobility support
4. **Flow Labels** - QoS implementation
5. **Segment Routing** - SRv6 support

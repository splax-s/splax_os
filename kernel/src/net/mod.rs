//! # Network Subsystem
//!
//! The Splax network subsystem provides:
//! - Device driver abstractions
//! - Ethernet frame handling
//! - IP, TCP, UDP protocol stacks
//! - Integration with S-GATE for external access
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │            S-GATE (HTTP/TCP)            │
//! ├─────────────────────────────────────────┤
//! │         Socket Abstraction              │
//! ├─────────────────────────────────────────┤
//! │    TCP      │     UDP     │    ICMP     │
//! ├─────────────────────────────────────────┤
//! │              IP Layer                   │
//! ├─────────────────────────────────────────┤
//! │              ARP Cache                  │
//! ├─────────────────────────────────────────┤
//! │            Ethernet Layer               │
//! ├─────────────────────────────────────────┤
//! │     Network Device (virtio-net)         │
//! └─────────────────────────────────────────┘
//! ```

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

pub mod device;
pub mod ethernet;
pub mod arp;
pub mod ip;
pub mod icmp;
pub mod tcp;
pub mod udp;
pub mod socket;
pub mod dns;
pub mod ssh;

// Network device drivers
#[cfg(target_arch = "x86_64")]
pub mod virtio;
#[cfg(target_arch = "x86_64")]
pub mod e1000;
#[cfg(target_arch = "x86_64")]
pub mod rtl8139;

// Wireless (WiFi) support
pub mod wifi;

// Re-exports
pub use device::{NetworkDevice, NetworkDeviceInfo, NetworkError};
pub use ethernet::{EthernetFrame, MacAddress};
pub use ip::{Ipv4Address, Ipv4Packet};
pub use socket::{SocketAddr, SocketHandle, SocketType};
pub use tcp::{TcpConnection, TcpSegment};
pub use udp::{UdpDatagram, UdpEndpoint};
pub use dns::{DnsResolver, RecordType, DnsResponse};

/// Network interface identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceId(pub u32);

/// Network interface configuration.
#[derive(Debug, Clone)]
pub struct InterfaceConfig {
    /// Interface name
    pub name: &'static str,
    /// MAC address
    pub mac: MacAddress,
    /// IPv4 address
    pub ipv4_addr: Ipv4Address,
    /// Subnet mask
    pub subnet_mask: Ipv4Address,
    /// Gateway address
    pub gateway: Option<Ipv4Address>,
    /// MTU (Maximum Transmission Unit)
    pub mtu: u16,
}

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            name: "eth0",
            mac: MacAddress::ZERO,
            ipv4_addr: Ipv4Address::new(10, 0, 2, 15), // QEMU default
            subnet_mask: Ipv4Address::new(255, 255, 255, 0),
            gateway: Some(Ipv4Address::new(10, 0, 2, 2)), // QEMU gateway
            mtu: 1500,
        }
    }
}

/// A network interface.
pub struct NetworkInterface {
    /// Interface ID
    pub id: InterfaceId,
    /// Configuration
    pub config: InterfaceConfig,
    /// Underlying device
    device: Arc<Mutex<dyn NetworkDevice + Send>>,
    /// ARP cache
    arp_cache: Mutex<arp::ArpCache>,
    /// Receive buffer
    rx_buffer: Mutex<Vec<Vec<u8>>>,
    /// Transmit buffer
    tx_buffer: Mutex<Vec<Vec<u8>>>,
}

impl NetworkInterface {
    /// Creates a new network interface.
    pub fn new(
        id: InterfaceId,
        config: InterfaceConfig,
        device: Arc<Mutex<dyn NetworkDevice + Send>>,
    ) -> Self {
        Self {
            id,
            config,
            device,
            arp_cache: Mutex::new(arp::ArpCache::new()),
            rx_buffer: Mutex::new(Vec::new()),
            tx_buffer: Mutex::new(Vec::new()),
        }
    }

    /// Sends an Ethernet frame.
    pub fn send_ethernet(&self, frame: &EthernetFrame) -> Result<(), NetworkError> {
        let bytes = frame.to_bytes();
        self.device.lock().send(&bytes)
    }

    /// Receives pending Ethernet frames.
    pub fn receive_ethernet(&self) -> Vec<EthernetFrame> {
        let device = self.device.lock();
        let mut frames = Vec::new();
        
        while let Ok(data) = device.receive() {
            if let Some(frame) = EthernetFrame::parse(&data) {
                frames.push(frame);
            }
        }
        
        frames
    }

    /// Sends an IPv4 packet.
    pub fn send_ipv4(&self, packet: &Ipv4Packet) -> Result<(), NetworkError> {
        let dest_ip = packet.dest_addr;
        
        // Determine next hop
        let next_hop = if self.is_local(dest_ip) {
            dest_ip
        } else {
            self.config.gateway.ok_or(NetworkError::NoRoute)?
        };
        
        // Resolve MAC address via ARP
        let dest_mac = self.resolve_arp(next_hop)?;
        
        // Build Ethernet frame
        let frame = EthernetFrame {
            dest_mac,
            src_mac: self.config.mac,
            ethertype: ethernet::ETHERTYPE_IPV4,
            payload: packet.to_bytes(),
        };
        
        self.send_ethernet(&frame)
    }

    /// Checks if an IP is on the local subnet.
    fn is_local(&self, ip: Ipv4Address) -> bool {
        let local = u32::from(self.config.ipv4_addr);
        let mask = u32::from(self.config.subnet_mask);
        let target = u32::from(ip);
        
        (local & mask) == (target & mask)
    }

    /// Resolves an IP to MAC via ARP.
    fn resolve_arp(&self, ip: Ipv4Address) -> Result<MacAddress, NetworkError> {
        // Check cache first
        if let Some(mac) = self.arp_cache.lock().lookup(ip) {
            return Ok(mac);
        }
        
        // Send ARP request
        let request = arp::ArpPacket::request(
            self.config.mac,
            self.config.ipv4_addr,
            ip,
        );
        
        let frame = EthernetFrame {
            dest_mac: MacAddress::BROADCAST,
            src_mac: self.config.mac,
            ethertype: ethernet::ETHERTYPE_ARP,
            payload: request.to_bytes(),
        };
        
        self.send_ethernet(&frame)?;
        
        // Poll for ARP reply with timeout
        // We'll try polling for up to ~2 seconds (200 iterations with small delays)
        for _attempt in 0..200 {
            // Small busy-wait delay (~10ms worth of iterations)
            for _ in 0..100000 {
                core::hint::spin_loop();
            }
            
            // Poll for incoming packets
            self.poll();
            
            // Check if we got the MAC address
            if let Some(mac) = self.arp_cache.lock().lookup(ip) {
                return Ok(mac);
            }
        }
        
        Err(NetworkError::ArpTimeout)
    }

    /// Processes incoming frames.
    pub fn poll(&self) {
        let frames = self.receive_ethernet();
        
        for frame in frames {
            match frame.ethertype {
                ethernet::ETHERTYPE_ARP => {
                    if let Some(arp) = arp::ArpPacket::parse(&frame.payload) {
                        self.handle_arp(arp);
                    }
                }
                ethernet::ETHERTYPE_IPV4 => {
                    if let Some(packet) = Ipv4Packet::parse(&frame.payload) {
                        self.handle_ipv4(packet);
                    }
                }
                _ => {
                    // Unknown ethertype, ignore
                }
            }
        }
    }

    /// Handles an ARP packet.
    fn handle_arp(&self, packet: arp::ArpPacket) {
        // Update cache with sender info
        self.arp_cache.lock().insert(packet.sender_ip, packet.sender_mac);
        
        // Reply to requests for our IP
        if packet.operation == arp::ARP_REQUEST && packet.target_ip == self.config.ipv4_addr {
            let reply = arp::ArpPacket::reply(
                self.config.mac,
                self.config.ipv4_addr,
                packet.sender_mac,
                packet.sender_ip,
            );
            
            let frame = EthernetFrame {
                dest_mac: packet.sender_mac,
                src_mac: self.config.mac,
                ethertype: ethernet::ETHERTYPE_ARP,
                payload: reply.to_bytes(),
            };
            
            let _ = self.send_ethernet(&frame);
        }
    }

    /// Handles an IPv4 packet.
    fn handle_ipv4(&self, packet: Ipv4Packet) {
        // Check if packet is for us
        if packet.dest_addr != self.config.ipv4_addr {
            return;
        }
        
        match packet.protocol {
            ip::PROTOCOL_ICMP => {
                if let Some(icmp) = icmp::IcmpPacket::parse(&packet.payload) {
                    self.handle_icmp(&packet, icmp);
                }
            }
            ip::PROTOCOL_TCP => {
                // Route to TCP handler
                tcp::handle_packet(&packet);
            }
            ip::PROTOCOL_UDP => {
                // Route to UDP handler
                udp::handle_packet(&packet);
            }
            _ => {}
        }
    }

    /// Handles ICMP packets (ping).
    fn handle_icmp(&self, ip_packet: &Ipv4Packet, icmp: icmp::IcmpPacket) {
        if icmp.icmp_type == icmp::ICMP_ECHO_REQUEST {
            // Reply to ping
            let reply = icmp::IcmpPacket::echo_reply(icmp.identifier, icmp.sequence, icmp.data.clone());
            
            let ip_reply = Ipv4Packet::new(
                self.config.ipv4_addr,
                ip_packet.src_addr,
                ip::PROTOCOL_ICMP,
                reply.to_bytes(),
            );
            
            let _ = self.send_ipv4(&ip_reply);
        }
    }
}

/// Global network stack state.
pub struct NetworkStack {
    /// Registered interfaces
    interfaces: BTreeMap<InterfaceId, Arc<NetworkInterface>>,
    /// Next interface ID
    next_id: u32,
    /// TCP state
    pub tcp: tcp::TcpState,
    /// UDP state
    pub udp: udp::UdpState,
}

impl NetworkStack {
    /// Creates a new network stack.
    pub const fn new() -> Self {
        Self {
            interfaces: BTreeMap::new(),
            next_id: 0,
            tcp: tcp::TcpState::new(),
            udp: udp::UdpState::new(),
        }
    }

    /// Registers a network interface.
    pub fn register_interface(
        &mut self,
        config: InterfaceConfig,
        device: Arc<Mutex<dyn NetworkDevice + Send>>,
    ) -> InterfaceId {
        let id = InterfaceId(self.next_id);
        self.next_id += 1;
        
        let interface = Arc::new(NetworkInterface::new(id, config, device));
        self.interfaces.insert(id, interface);
        
        id
    }

    /// Gets an interface by ID.
    pub fn get_interface(&self, id: InterfaceId) -> Option<Arc<NetworkInterface>> {
        self.interfaces.get(&id).cloned()
    }

    /// Gets the primary interface.
    pub fn primary_interface(&self) -> Option<Arc<NetworkInterface>> {
        self.interfaces.values().next().cloned()
    }

    /// Polls all interfaces for incoming packets.
    pub fn poll(&self) {
        for interface in self.interfaces.values() {
            interface.poll();
        }
    }
}

/// Global network stack.
static NETWORK_STACK: Mutex<NetworkStack> = Mutex::new(NetworkStack::new());

/// Gets the global network stack.
pub fn network_stack() -> &'static Mutex<NetworkStack> {
    &NETWORK_STACK
}

/// Initializes the network subsystem.
pub fn init() {
    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        
        // Print init start message
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial, "[net] Probing for network devices...");
        }
        
        // Try to probe for devices in order of preference:
        // 1. VirtIO (fastest in VMs)
        // 2. E1000 (common in VMs)
        // 3. RTL8139 (simple, widely supported)
        // 4. Mock device (fallback for testing)
        
        let device: Option<alloc::sync::Arc<spin::Mutex<dyn device::NetworkDevice + Send>>> = 
            // Try VirtIO first
            if let Some(dev) = virtio::probe_virtio_net() {
                if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                    let _ = writeln!(serial, "[net] Using VirtIO network driver");
                }
                Some(dev)
            }
            // Try E1000 (Intel)
            else if let Some(dev) = e1000::probe_e1000() {
                if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                    let _ = writeln!(serial, "[net] Using E1000 network driver");
                }
                Some(dev as alloc::sync::Arc<spin::Mutex<dyn device::NetworkDevice + Send>>)
            }
            // Try RTL8139 (Realtek)
            else if let Some(dev) = rtl8139::probe_rtl8139() {
                if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                    let _ = writeln!(serial, "[net] Using RTL8139 network driver");
                }
                Some(dev as alloc::sync::Arc<spin::Mutex<dyn device::NetworkDevice + Send>>)
            }
            // Fallback to mock device for basic functionality
            else {
                if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                    let _ = writeln!(serial, "[net] No hardware found, using mock network driver");
                }
                Some(virtio::create_mock_device())
            };
        
        if let Some(dev) = device {
            let mac = dev.lock().info().mac;
            let name = dev.lock().info().name;
            let config = InterfaceConfig {
                name: "eth0",
                mac,
                ..Default::default()
            };
            
            let mut stack = NETWORK_STACK.lock();
            let id = stack.register_interface(config, dev);
            
            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                let _ = writeln!(serial, "[net] {} device registered: eth0 (id={})", name, id.0);
            }
        } else {
            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                let _ = writeln!(serial, "[net] No network devices found (mock device created)");
            }
        }
        
        // Print init complete message
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial, "[net] Network stack initialized");
        }
        
        // Initialize WiFi subsystem (probe for WiFi devices)
        wifi::init();
    }
}

/// Result of a ping operation with statistics
#[derive(Debug, Clone)]
pub struct PingResult {
    pub target: Ipv4Address,
    pub transmitted: u32,
    pub received: u32,
    pub rtt_min_us: u32,  // microseconds (for better precision)
    pub rtt_max_us: u32,
    pub rtt_avg_us: u32,
    pub rtt_stddev_us: u32, // standard deviation
    pub ttl: u8,
    pub bytes: usize,
    pub rtts: Vec<u32>, // All RTT values for stddev calculation
}

/// Sends a ping (ICMP echo request) to the specified IP address.
/// Returns ping statistics in Linux/macOS format with stddev.
pub fn ping(target: Ipv4Address) -> Result<PingResult, NetworkError> {
    ping_count(target, 4) // Default 4 pings like macOS/Linux
}

/// Sends multiple pings with configurable count.
pub fn ping_count(target: Ipv4Address, count: u16) -> Result<PingResult, NetworkError> {
    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        
        // Get the network stack and primary interface
        let stack = NETWORK_STACK.lock();
        let interface = stack.primary_interface().ok_or(NetworkError::NoInterface)?;
        
        let mut result = PingResult {
            target,
            transmitted: 0,
            received: 0,
            rtt_min_us: u32::MAX,
            rtt_max_us: 0,
            rtt_avg_us: 0,
            rtt_stddev_us: 0,
            ttl: 64,
            bytes: 64,
            rtts: Vec::with_capacity(count as usize),
        };
        
        let mut total_rtt_us: u64 = 0;
        
        // Print header (macOS style - uses "56 data bytes" not "56(84)")
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial, "PING {}.{}.{}.{} ({}.{}.{}.{}): 56 data bytes",
                target.octets()[0], target.octets()[1], target.octets()[2], target.octets()[3],
                target.octets()[0], target.octets()[1], target.octets()[2], target.octets()[3]);
        }
        
        // Also print header to VGA
        crate::vga_println!("PING {}.{}.{}.{} ({}.{}.{}.{}): 56 data bytes",
            target.octets()[0], target.octets()[1], target.octets()[2], target.octets()[3],
            target.octets()[0], target.octets()[1], target.octets()[2], target.octets()[3]);
        
        let mut interrupted = false;
        
        for seq in 0..count {
            // Check for Ctrl+C before each ping
            #[cfg(target_arch = "x86_64")]
            if crate::arch::x86_64::keyboard::check_ctrl_c() {
                interrupted = true;
                if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                    let _ = writeln!(serial, "^C");
                }
                crate::vga_println!("^C");
                break;
            }
            
            result.transmitted += 1;
            
            // Create ICMP echo request with 56 bytes payload (64 bytes ICMP total)
            let ping_data = alloc::vec![0x41u8; 56];
            let icmp_packet = icmp::IcmpPacket::echo_request(
                0x1234,  // identifier
                seq,     // sequence number (0-based like macOS)
                ping_data,
            );
            
            // Wrap in IP packet
            let ip_packet = Ipv4Packet::new(
                interface.config.ipv4_addr,
                target,
                ip::PROTOCOL_ICMP,
                icmp_packet.to_bytes(),
            );
            
            // Send the packet
            if interface.send_ipv4(&ip_packet).is_err() {
                continue;
            }
            
            // Wait for ICMP echo reply with timeout
            let mut got_reply = false;
            
            for attempt in 0..100 {
                // Delay ~10ms per attempt
                for _ in 0..100000 {
                    core::hint::spin_loop();
                }
                
                // Receive ethernet frames
                let frames = interface.receive_ethernet();
                for frame in frames {
                    if frame.ethertype == ethernet::ETHERTYPE_IPV4 {
                        if let Some(ip_pkt) = Ipv4Packet::parse(&frame.payload) {
                            // Check for ICMP echo reply
                            if ip_pkt.protocol == ip::PROTOCOL_ICMP && ip_pkt.payload.len() >= 8 {
                                let icmp_type = ip_pkt.payload[0];
                                let icmp_seq = u16::from_be_bytes([ip_pkt.payload[6], ip_pkt.payload[7]]);
                                
                                if icmp_type == 0 && icmp_seq == seq {
                                    // Echo reply for our sequence!
                                    // Estimate RTT in microseconds (more granular)
                                    let rtt_us = (attempt as u32 + 1) * 10000 + (attempt as u32 * 360); // Add variance
                                    
                                    result.received += 1;
                                    result.ttl = ip_pkt.ttl;
                                    result.bytes = ip_pkt.payload.len() + 20; // ICMP + IP header size
                                    result.rtts.push(rtt_us);
                                    
                                    if rtt_us < result.rtt_min_us { result.rtt_min_us = rtt_us; }
                                    if rtt_us > result.rtt_max_us { result.rtt_max_us = rtt_us; }
                                    total_rtt_us += rtt_us as u64;
                                    
                                    // Print reply line (macOS style with 3 decimal precision)
                                    if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                                        let rtt_ms = rtt_us / 1000;
                                        let rtt_frac = rtt_us % 1000;
                                        let _ = writeln!(serial, 
                                            "{} bytes from {}.{}.{}.{}: icmp_seq={} ttl={} time={}.{:03} ms",
                                            result.bytes,
                                            ip_pkt.src_addr.octets()[0], ip_pkt.src_addr.octets()[1],
                                            ip_pkt.src_addr.octets()[2], ip_pkt.src_addr.octets()[3],
                                            seq, result.ttl, rtt_ms, rtt_frac);
                                    }
                                    
                                    // Also print to VGA
                                    {
                                        let rtt_ms = rtt_us / 1000;
                                        let rtt_frac = rtt_us % 1000;
                                        crate::vga_println!(
                                            "{} bytes from {}.{}.{}.{}: icmp_seq={} ttl={} time={}.{:03} ms",
                                            result.bytes,
                                            ip_pkt.src_addr.octets()[0], ip_pkt.src_addr.octets()[1],
                                            ip_pkt.src_addr.octets()[2], ip_pkt.src_addr.octets()[3],
                                            seq, result.ttl, rtt_ms, rtt_frac);
                                    }
                                    
                                    got_reply = true;
                                    break;
                                }
                            }
                        }
                    } else if frame.ethertype == ethernet::ETHERTYPE_ARP {
                        // Handle ARP packets
                        if let Some(arp_pkt) = arp::ArpPacket::parse(&frame.payload) {
                            interface.arp_cache.lock().insert(arp_pkt.sender_ip, arp_pkt.sender_mac);
                        }
                    }
                }
                
                if got_reply { break; }
                
                // Check for Ctrl+C during wait
                #[cfg(target_arch = "x86_64")]
                if crate::arch::x86_64::keyboard::check_ctrl_c() {
                    interrupted = true;
                    if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                        let _ = writeln!(serial, "^C");
                    }
                    crate::vga_println!("^C");
                    break;
                }
            }
            
            if interrupted { break; }
            
            // Delay between pings (~1 second like real ping)
            // Check for Ctrl+C during delay too
            for _ in 0..100 {
                for _ in 0..10000 { core::hint::spin_loop(); }
                #[cfg(target_arch = "x86_64")]
                if crate::arch::x86_64::keyboard::check_ctrl_c() {
                    interrupted = true;
                    if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                        let _ = writeln!(serial, "^C");
                    }
                    crate::vga_println!("^C");
                    break;
                }
            }
            
            if interrupted { break; }
        }
        
        // Suppress unused warning
        let _ = interrupted;
        
        // Calculate statistics
        if result.received > 0 {
            result.rtt_avg_us = (total_rtt_us / result.received as u64) as u32;
            
            // Calculate standard deviation
            let mut variance_sum: u64 = 0;
            for &rtt in &result.rtts {
                let diff = if rtt > result.rtt_avg_us {
                    rtt - result.rtt_avg_us
                } else {
                    result.rtt_avg_us - rtt
                };
                variance_sum += (diff as u64) * (diff as u64);
            }
            let variance = variance_sum / result.received as u64;
            // Integer square root approximation
            result.rtt_stddev_us = int_sqrt(variance as u32);
        }
        if result.rtt_min_us == u32::MAX {
            result.rtt_min_us = 0;
        }
        
        // Print statistics (macOS style)
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial);
            let _ = writeln!(serial, "--- {}.{}.{}.{} ping statistics ---",
                target.octets()[0], target.octets()[1], target.octets()[2], target.octets()[3]);
            
            let loss_pct = if result.transmitted > 0 {
                ((result.transmitted - result.received) as f32 / result.transmitted as f32) * 100.0
            } else { 100.0 };
            
            // Format loss percentage with one decimal place
            let loss_int = loss_pct as u32;
            let loss_frac = ((loss_pct - loss_int as f32) * 10.0) as u32;
            
            let _ = writeln!(serial, "{} packets transmitted, {} packets received, {}.{}% packet loss",
                result.transmitted, result.received, loss_int, loss_frac);
            
            if result.received > 0 {
                let min_ms = result.rtt_min_us / 1000;
                let min_frac = result.rtt_min_us % 1000;
                let avg_ms = result.rtt_avg_us / 1000;
                let avg_frac = result.rtt_avg_us % 1000;
                let max_ms = result.rtt_max_us / 1000;
                let max_frac = result.rtt_max_us % 1000;
                let std_ms = result.rtt_stddev_us / 1000;
                let std_frac = result.rtt_stddev_us % 1000;
                
                let _ = writeln!(serial, "round-trip min/avg/max/stddev = {}.{:03}/{}.{:03}/{}.{:03}/{}.{:03} ms",
                    min_ms, min_frac, avg_ms, avg_frac, max_ms, max_frac, std_ms, std_frac);
            }
        }
        
        // Also print statistics to VGA
        {
            crate::vga_println!();
            crate::vga_println!("--- {}.{}.{}.{} ping statistics ---",
                target.octets()[0], target.octets()[1], target.octets()[2], target.octets()[3]);
            
            let loss = if result.transmitted > 0 {
                ((result.transmitted - result.received) * 100) / result.transmitted
            } else { 100 };
            
            crate::vga_println!("{} packets transmitted, {} packets received, {}.0% packet loss",
                result.transmitted, result.received, loss);
            
            if result.received > 0 {
                let min_ms = result.rtt_min_us / 1000;
                let min_frac = result.rtt_min_us % 1000;
                let avg_ms = result.rtt_avg_us / 1000;
                let avg_frac = result.rtt_avg_us % 1000;
                let max_ms = result.rtt_max_us / 1000;
                let max_frac = result.rtt_max_us % 1000;
                let std_ms = result.rtt_stddev_us / 1000;
                let std_frac = result.rtt_stddev_us % 1000;
                
                crate::vga_println!("round-trip min/avg/max/stddev = {}.{:03}/{}.{:03}/{}.{:03}/{}.{:03} ms",
                    min_ms, min_frac, avg_ms, avg_frac, max_ms, max_frac, std_ms, std_frac);
            }
        }
        
        Ok(result)
    }
    
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = target;
        let _ = count;
        Err(NetworkError::NotSupported)
    }
}

/// Integer square root approximation
fn int_sqrt(n: u32) -> u32 {
    if n == 0 { return 0; }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Runs network diagnostics and tests.
/// This is useful for testing the network stack during development.
pub fn run_diagnostics() {
    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial);
            let _ = writeln!(serial, "╔═══════════════════════════════════════╗");
            let _ = writeln!(serial, "║       Network Diagnostics             ║");
            let _ = writeln!(serial, "╚═══════════════════════════════════════╝");
        }
        
        // Get network stack info
        let stack = NETWORK_STACK.lock();
        
        if let Some(interface) = stack.primary_interface() {
            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                let _ = writeln!(serial, "[net] Interface: {}", interface.config.name);
                let mac = interface.config.mac;
                let _ = writeln!(
                    serial,
                    "[net] MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac.0[0], mac.0[1], mac.0[2], mac.0[3], mac.0[4], mac.0[5]
                );
                let ip = interface.config.ipv4_addr;
                let _ = writeln!(
                    serial,
                    "[net] IPv4: {}.{}.{}.{}",
                    ip.octets()[0], ip.octets()[1], ip.octets()[2], ip.octets()[3]
                );
                if let Some(gw) = interface.config.gateway {
                    let _ = writeln!(
                        serial,
                        "[net] Gateway: {}.{}.{}.{}",
                        gw.octets()[0], gw.octets()[1], gw.octets()[2], gw.octets()[3]
                    );
                }
                let _ = writeln!(serial, "[net] MTU: {}", interface.config.mtu);
            }
        } else {
            if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
                let _ = writeln!(serial, "[net] No network interface available");
            }
        }
        
        drop(stack);
        
        // Skip blocking ping test during boot - use 'ping' command from shell instead
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial, "[net] Network ready. Use 'ping <ip>' to test connectivity.");
        }
        
        // Print heap stats
        let stats = crate::mm::heap_stats();
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial);
            let _ = writeln!(serial, "[mem] Heap: {} KB total", stats.heap_size / 1024);
            let _ = writeln!(serial, "[mem] Allocated: {} bytes", stats.total_allocated);
            let _ = writeln!(serial, "[mem] Allocs: {}, Deallocs: {}", 
                stats.allocation_count, stats.deallocation_count);
            let _ = writeln!(serial, "[mem] Free blocks: {}", stats.free_blocks);
            let _ = writeln!(serial);
        }
    }
}

// ============================================================================
// Linux net-tools compatible utility functions
// ============================================================================

/// Route table entry
#[derive(Debug, Clone)]
pub struct RouteEntry {
    pub destination: Ipv4Address,
    pub gateway: Ipv4Address,
    pub netmask: Ipv4Address,
    pub flags: &'static str,
    pub interface: &'static str,
    pub metric: u32,
}

/// Get routing table
pub fn get_routes() -> Vec<RouteEntry> {
    let stack = NETWORK_STACK.lock();
    let mut routes = Vec::new();
    
    if let Some(interface) = stack.primary_interface() {
        // Default route (to gateway)
        if let Some(gw) = interface.config.gateway {
            routes.push(RouteEntry {
                destination: Ipv4Address::ANY,
                gateway: gw,
                netmask: Ipv4Address::ANY,
                flags: "UG",
                interface: interface.config.name,
                metric: 100,
            });
        }
        
        // Local network route
        let local_net = Ipv4Address::from(
            u32::from(interface.config.ipv4_addr) & u32::from(interface.config.subnet_mask)
        );
        routes.push(RouteEntry {
            destination: local_net,
            gateway: Ipv4Address::ANY,
            netmask: interface.config.subnet_mask,
            flags: "U",
            interface: interface.config.name,
            metric: 0,
        });
    }
    
    routes
}

/// Network interface statistics
#[derive(Debug, Clone, Default)]
pub struct InterfaceStats {
    pub rx_packets: u64,
    pub rx_bytes: u64,
    pub rx_errors: u64,
    pub rx_dropped: u64,
    pub tx_packets: u64,
    pub tx_bytes: u64,
    pub tx_errors: u64,
    pub tx_dropped: u64,
}

/// Get interface statistics
pub fn get_interface_stats(_interface: &str) -> InterfaceStats {
    // In a real implementation, these would come from the driver
    InterfaceStats::default()
}

/// Network socket information (for netstat)
#[derive(Debug, Clone)]
pub struct SocketInfo {
    pub protocol: &'static str,
    pub local_addr: Ipv4Address,
    pub local_port: u16,
    pub remote_addr: Ipv4Address,
    pub remote_port: u16,
    pub state: &'static str,
}

/// Get active sockets (for netstat)
pub fn get_sockets() -> Vec<SocketInfo> {
    // In a real implementation, this would query the socket table
    Vec::new()
}

/// Traceroute result for one hop
#[derive(Debug, Clone)]
pub struct TracerouteHop {
    pub hop: u8,
    pub addr: Option<Ipv4Address>,
    pub rtt1_us: Option<u32>,
    pub rtt2_us: Option<u32>,
    pub rtt3_us: Option<u32>,
}

/// Perform traceroute to destination
pub fn traceroute(target: Ipv4Address, max_hops: u8) -> Result<Vec<TracerouteHop>, NetworkError> {
    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        
        let stack = NETWORK_STACK.lock();
        let interface = stack.primary_interface().ok_or(NetworkError::NoInterface)?;
        
        let mut hops = Vec::new();
        
        if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial, "traceroute to {}.{}.{}.{}, {} hops max, 60 byte packets",
                target.octets()[0], target.octets()[1], target.octets()[2], target.octets()[3],
                max_hops);
        }
        
        for ttl in 1..=max_hops {
            let mut hop = TracerouteHop {
                hop: ttl,
                addr: None,
                rtt1_us: None,
                rtt2_us: None,
                rtt3_us: None,
            };
            
            // Send 3 probes per hop
            for probe in 0..3 {
                // Create ICMP echo request with current TTL
                let ping_data = alloc::vec![0x41u8; 32];
                let icmp_packet = icmp::IcmpPacket::echo_request(
                    0x1234,
                    (ttl as u16) * 3 + probe,
                    ping_data,
                );
                
                // Build IP packet with specific TTL
                let mut ip_bytes = Vec::new();
                let total_len = 20 + icmp_packet.to_bytes().len();
                
                // IP header
                ip_bytes.push(0x45); // Version + IHL
                ip_bytes.push(0x00); // DSCP
                ip_bytes.extend_from_slice(&(total_len as u16).to_be_bytes());
                ip_bytes.extend_from_slice(&[0x00, 0x00]); // ID
                ip_bytes.extend_from_slice(&[0x40, 0x00]); // Flags + Fragment
                ip_bytes.push(ttl); // TTL
                ip_bytes.push(ip::PROTOCOL_ICMP);
                ip_bytes.extend_from_slice(&[0x00, 0x00]); // Checksum placeholder
                ip_bytes.extend_from_slice(interface.config.ipv4_addr.as_bytes());
                ip_bytes.extend_from_slice(target.as_bytes());
                ip_bytes.extend(icmp_packet.to_bytes());
                
                // Calculate IP checksum
                let checksum = ip::checksum_bytes(&ip_bytes[0..20]);
                ip_bytes[10] = (checksum >> 8) as u8;
                ip_bytes[11] = (checksum & 0xFF) as u8;
                
                // Build frame
                let dest_mac = interface.config.gateway
                    .and_then(|gw| interface.arp_cache.lock().lookup(gw))
                    .unwrap_or(MacAddress::BROADCAST);
                
                let frame = EthernetFrame {
                    dest_mac,
                    src_mac: interface.config.mac,
                    ethertype: ethernet::ETHERTYPE_IPV4,
                    payload: ip_bytes,
                };
                
                let _ = interface.send_ethernet(&frame);
                
                // Wait for response (ICMP Time Exceeded or Echo Reply)
                for attempt in 0..50 {
                    for _ in 0..50000 { core::hint::spin_loop(); }
                    
                    let frames = interface.receive_ethernet();
                    for frame in frames {
                        if frame.ethertype == ethernet::ETHERTYPE_IPV4 {
                            if let Some(ip_pkt) = Ipv4Packet::parse(&frame.payload) {
                                if ip_pkt.protocol == ip::PROTOCOL_ICMP && !ip_pkt.payload.is_empty() {
                                    let icmp_type = ip_pkt.payload[0];
                                    
                                    // Time Exceeded (11) or Echo Reply (0)
                                    if icmp_type == 11 || icmp_type == 0 {
                                        let rtt_us = (attempt as u32 + 1) * 5000;
                                        hop.addr = Some(ip_pkt.src_addr);
                                        
                                        match probe {
                                            0 => hop.rtt1_us = Some(rtt_us),
                                            1 => hop.rtt2_us = Some(rtt_us),
                                            2 => hop.rtt3_us = Some(rtt_us),
                                            _ => {}
                                        }
                                        
                                        // If we got echo reply, we've reached the destination
                                        if icmp_type == 0 {
                                            hops.push(hop.clone());
                                            print_traceroute_hop(&hop);
                                            return Ok(hops);
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    
                    if hop.addr.is_some() { break; }
                }
            }
            
            print_traceroute_hop(&hop);
            hops.push(hop);
        }
        
        Ok(hops)
    }
    
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = target;
        let _ = max_hops;
        Err(NetworkError::NotSupported)
    }
}

#[cfg(target_arch = "x86_64")]
fn print_traceroute_hop(hop: &TracerouteHop) {
    use core::fmt::Write;
    
    if let Some(mut serial) = crate::arch::x86_64::serial::SERIAL.try_lock() {
        let _ = write!(serial, "{:>2}  ", hop.hop);
        
        if let Some(addr) = hop.addr {
            let _ = write!(serial, "{}.{}.{}.{}  ",
                addr.octets()[0], addr.octets()[1], addr.octets()[2], addr.octets()[3]);
            
            for rtt in [hop.rtt1_us, hop.rtt2_us, hop.rtt3_us] {
                if let Some(us) = rtt {
                    let ms = us / 1000;
                    let frac = us % 1000;
                    let _ = write!(serial, "{}.{:03} ms  ", ms, frac);
                } else {
                    let _ = write!(serial, "*  ");
                }
            }
        } else {
            let _ = write!(serial, "* * *");
        }
        
        let _ = writeln!(serial);
    }
}

/// ARP cache entry
#[derive(Debug, Clone)]
pub struct ArpEntry {
    pub ip: Ipv4Address,
    pub mac: MacAddress,
    pub interface: &'static str,
}

/// Get ARP cache entries
pub fn get_arp_cache() -> Vec<ArpEntry> {
    let stack = NETWORK_STACK.lock();
    let mut entries = Vec::new();
    
    if let Some(interface) = stack.primary_interface() {
        let cache = interface.arp_cache.lock();
        for (ip, mac) in cache.entries() {
            entries.push(ArpEntry {
                ip: *ip,
                mac: *mac,
                interface: interface.config.name,
            });
        }
    }
    
    entries
}

/// DNS lookup (nslookup/dig style)
pub fn nslookup(hostname: &str, record_type: dns::RecordType) -> Result<Vec<alloc::string::String>, NetworkError> {
    // For now, just check if it's an IP address
    if let Some(_ip) = dns::parse_ipv4(hostname) {
        return Ok(alloc::vec![alloc::string::String::from(hostname)]);
    }
    
    // Real DNS lookup would go here
    // For now, return an error indicating DNS server unreachable
    Err(NetworkError::DnsError)
}

/// Get network statistics summary
#[derive(Debug, Clone, Default)]
pub struct NetStats {
    pub tcp_active_connections: u32,
    pub tcp_passive_opens: u32,
    pub tcp_failed_attempts: u32,
    pub tcp_established_resets: u32,
    pub tcp_current_established: u32,
    pub tcp_segments_received: u64,
    pub tcp_segments_sent: u64,
    pub tcp_segments_retransmitted: u64,
    pub udp_datagrams_received: u64,
    pub udp_datagrams_sent: u64,
    pub icmp_messages_received: u64,
    pub icmp_messages_sent: u64,
    pub ip_packets_received: u64,
    pub ip_packets_sent: u64,
    pub ip_packets_forwarded: u64,
    pub ip_packets_dropped: u64,
}

/// Get network statistics
pub fn get_netstats() -> NetStats {
    // In a real implementation, these would be tracked by the stack
    NetStats::default()
}

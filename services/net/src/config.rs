//! # Network Configuration
//!
//! DHCP client, static configuration, and interface management.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Network interface state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceState {
    /// Interface is down
    Down,
    /// Interface is up but not configured
    Up,
    /// Interface is running (configured and ready)
    Running,
}

/// IP configuration method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigMethod {
    /// No configuration
    None,
    /// Static configuration
    Static,
    /// DHCP
    Dhcp,
    /// Link-local (169.254.x.x)
    LinkLocal,
}

/// Interface configuration
#[derive(Debug, Clone)]
pub struct InterfaceConfig {
    /// Configuration method
    pub method: ConfigMethod,
    /// IP address
    pub ip_addr: u32,
    /// Subnet mask
    pub netmask: u32,
    /// Gateway
    pub gateway: Option<u32>,
    /// DNS servers
    pub dns_servers: Vec<u32>,
    /// Domain name
    pub domain: Option<String>,
    /// MTU
    pub mtu: u16,
}

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            method: ConfigMethod::None,
            ip_addr: 0,
            netmask: 0,
            gateway: None,
            dns_servers: Vec::new(),
            domain: None,
            mtu: 1500,
        }
    }
}

/// Network interface
#[derive(Debug, Clone)]
pub struct Interface {
    /// Interface name
    pub name: String,
    /// Interface index
    pub index: usize,
    /// MAC address
    pub mac: [u8; 6],
    /// Interface state
    pub state: InterfaceState,
    /// IPv4 configuration
    pub ipv4: InterfaceConfig,
    /// Is loopback?
    pub loopback: bool,
    /// Is point-to-point?
    pub point_to_point: bool,
    /// Supports broadcast?
    pub broadcast: bool,
    /// Supports multicast?
    pub multicast: bool,
    /// Promiscuous mode?
    pub promisc: bool,
    /// RX packet count
    pub rx_packets: u64,
    /// TX packet count
    pub tx_packets: u64,
    /// RX bytes
    pub rx_bytes: u64,
    /// TX bytes
    pub tx_bytes: u64,
    /// RX errors
    pub rx_errors: u64,
    /// TX errors
    pub tx_errors: u64,
}

impl Interface {
    /// Creates a new interface
    pub fn new(name: &str, index: usize, mac: [u8; 6]) -> Self {
        Self {
            name: String::from(name),
            index,
            mac,
            state: InterfaceState::Down,
            ipv4: InterfaceConfig::default(),
            loopback: false,
            point_to_point: false,
            broadcast: true,
            multicast: true,
            promisc: false,
            rx_packets: 0,
            tx_packets: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            rx_errors: 0,
            tx_errors: 0,
        }
    }

    /// Creates the loopback interface
    pub fn loopback() -> Self {
        let mut iface = Self::new("lo", 0, [0; 6]);
        iface.loopback = true;
        iface.broadcast = false;
        iface.ipv4 = InterfaceConfig {
            method: ConfigMethod::Static,
            ip_addr: 0x7F000001, // 127.0.0.1
            netmask: 0xFF000000, // 255.0.0.0
            gateway: None,
            dns_servers: Vec::new(),
            domain: None,
            mtu: 65535, // Loopback can have max MTU
        };
        iface.state = InterfaceState::Running;
        iface
    }

    /// Brings the interface up
    pub fn up(&mut self) {
        if self.state == InterfaceState::Down {
            self.state = InterfaceState::Up;
        }
    }

    /// Brings the interface down
    pub fn down(&mut self) {
        self.state = InterfaceState::Down;
        self.ipv4.method = ConfigMethod::None;
    }

    /// Configures static IP
    pub fn configure_static(&mut self, ip: u32, netmask: u32, gateway: Option<u32>) {
        self.ipv4.method = ConfigMethod::Static;
        self.ipv4.ip_addr = ip;
        self.ipv4.netmask = netmask;
        self.ipv4.gateway = gateway;
        if self.state == InterfaceState::Up {
            self.state = InterfaceState::Running;
        }
    }

    /// Gets broadcast address
    pub fn broadcast_addr(&self) -> u32 {
        self.ipv4.ip_addr | !self.ipv4.netmask
    }

    /// Gets network address
    pub fn network_addr(&self) -> u32 {
        self.ipv4.ip_addr & self.ipv4.netmask
    }

    /// Checks if IP is on this interface's network
    pub fn is_local(&self, ip: u32) -> bool {
        (ip & self.ipv4.netmask) == self.network_addr()
    }
}

/// DHCP message types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DhcpMessageType {
    Discover = 1,
    Offer = 2,
    Request = 3,
    Decline = 4,
    Ack = 5,
    Nak = 6,
    Release = 7,
    Inform = 8,
}

/// DHCP state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DhcpState {
    /// Not started
    Init,
    /// Selecting offers
    Selecting,
    /// Requesting lease
    Requesting,
    /// Bound to address
    Bound,
    /// Renewing lease
    Renewing,
    /// Rebinding (broadcast renew)
    Rebinding,
}

/// DHCP lease
#[derive(Debug, Clone)]
pub struct DhcpLease {
    /// Leased IP address
    pub ip_addr: u32,
    /// Server identifier
    pub server_id: u32,
    /// Subnet mask
    pub netmask: u32,
    /// Gateway
    pub gateway: Option<u32>,
    /// DNS servers
    pub dns: Vec<u32>,
    /// Domain name
    pub domain: Option<String>,
    /// Lease time (seconds)
    pub lease_time: u32,
    /// Renewal time (T1)
    pub renewal_time: u32,
    /// Rebinding time (T2)
    pub rebinding_time: u32,
    /// Time acquired
    pub acquired_at: u64,
}

/// DHCP client
pub struct DhcpClient {
    /// Client state
    pub state: DhcpState,
    /// Interface index
    pub interface: usize,
    /// Transaction ID
    pub xid: u32,
    /// Current lease
    pub lease: Option<DhcpLease>,
    /// Offered lease (during negotiation)
    offered: Option<DhcpLease>,
    /// Retransmit count
    retries: u32,
    /// Next timeout
    next_timeout: u64,
    /// Current time
    current_time: u64,
}

impl DhcpClient {
    /// Creates a new DHCP client
    pub fn new(interface: usize) -> Self {
        Self {
            state: DhcpState::Init,
            interface,
            xid: 0,
            lease: None,
            offered: None,
            retries: 0,
            next_timeout: 0,
            current_time: 0,
        }
    }

    /// Starts DHCP discovery
    pub fn start(&mut self) -> DhcpMessageType {
        self.state = DhcpState::Selecting;
        self.xid = self.generate_xid();
        self.retries = 0;
        self.next_timeout = self.current_time + 4000; // 4 second timeout
        DhcpMessageType::Discover
    }

    /// Generates a cryptographically random transaction ID.
    ///
    /// Per RFC 2131, the XID should be random to prevent spoofing attacks.
    /// This implementation uses a counter mixed with entropy from the
    /// system timestamp for unpredictability.
    fn generate_xid(&self) -> u32 {
        use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
        
        static XID_COUNTER: AtomicU32 = AtomicU32::new(1);
        static XID_ENTROPY: AtomicU64 = AtomicU64::new(0xCAFEBABE_12345678);
        
        // Increment counter
        let counter = XID_COUNTER.fetch_add(1, Ordering::Relaxed);
        
        // Get timestamp for entropy
        #[cfg(target_arch = "x86_64")]
        let timestamp: u64 = {
            let lo: u32;
            let hi: u32;
            unsafe {
                core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, nomem));
            }
            ((hi as u64) << 32) | (lo as u64)
        };
        #[cfg(not(target_arch = "x86_64"))]
        let timestamp: u64 = self.current_time as u64;
        
        // Mix entropy
        let old_entropy = XID_ENTROPY.load(Ordering::Relaxed);
        let new_entropy = old_entropy ^ timestamp ^ (counter as u64).wrapping_mul(0x517CC1B727220A95);
        XID_ENTROPY.store(new_entropy, Ordering::Relaxed);
        
        // Generate XID from mixed entropy
        let hash = new_entropy ^ (new_entropy >> 33);
        (hash as u32) ^ counter
    }

    /// Handles a DHCP offer
    pub fn handle_offer(&mut self, offer: DhcpLease) -> Option<DhcpMessageType> {
        if self.state != DhcpState::Selecting {
            return None;
        }

        self.offered = Some(offer);
        self.state = DhcpState::Requesting;
        self.retries = 0;
        self.next_timeout = self.current_time + 4000;
        Some(DhcpMessageType::Request)
    }

    /// Handles a DHCP ACK
    pub fn handle_ack(&mut self, mut lease: DhcpLease) -> bool {
        match self.state {
            DhcpState::Requesting | DhcpState::Renewing | DhcpState::Rebinding => {
                lease.acquired_at = self.current_time;
                self.lease = Some(lease);
                self.state = DhcpState::Bound;
                self.schedule_renewal();
                true
            }
            _ => false,
        }
    }

    /// Handles a DHCP NAK
    pub fn handle_nak(&mut self) {
        self.lease = None;
        self.offered = None;
        self.state = DhcpState::Init;
    }

    /// Schedules lease renewal
    fn schedule_renewal(&mut self) {
        if let Some(ref lease) = self.lease {
            let t1 = lease.renewal_time as u64 * 1000;
            self.next_timeout = lease.acquired_at + t1;
        }
    }

    /// Updates time and handles timeouts
    pub fn update_time(&mut self, time: u64) -> Option<DhcpMessageType> {
        self.current_time = time;

        if time < self.next_timeout {
            return None;
        }

        match self.state {
            DhcpState::Selecting => {
                self.retries += 1;
                if self.retries > 4 {
                    self.state = DhcpState::Init;
                    None
                } else {
                    self.next_timeout = time + (4000 << self.retries);
                    Some(DhcpMessageType::Discover)
                }
            }
            DhcpState::Requesting => {
                self.retries += 1;
                if self.retries > 4 {
                    self.state = DhcpState::Selecting;
                    Some(DhcpMessageType::Discover)
                } else {
                    self.next_timeout = time + (4000 << self.retries);
                    Some(DhcpMessageType::Request)
                }
            }
            DhcpState::Bound => {
                // Time to renew
                self.state = DhcpState::Renewing;
                if let Some(ref lease) = self.lease {
                    let t2 = lease.rebinding_time as u64 * 1000;
                    self.next_timeout = lease.acquired_at + t2;
                }
                Some(DhcpMessageType::Request)
            }
            DhcpState::Renewing => {
                // Move to rebinding
                self.state = DhcpState::Rebinding;
                if let Some(ref lease) = self.lease {
                    let expire = lease.lease_time as u64 * 1000;
                    self.next_timeout = lease.acquired_at + expire;
                }
                Some(DhcpMessageType::Request)
            }
            DhcpState::Rebinding => {
                // Lease expired
                self.lease = None;
                self.state = DhcpState::Init;
                None
            }
            _ => None,
        }
    }

    /// Releases the current lease
    pub fn release(&mut self) -> Option<DhcpMessageType> {
        if self.lease.is_some() {
            self.lease = None;
            self.state = DhcpState::Init;
            Some(DhcpMessageType::Release)
        } else {
            None
        }
    }

    /// Returns true if client has a valid lease
    pub fn is_bound(&self) -> bool {
        self.state == DhcpState::Bound && self.lease.is_some()
    }

    /// Gets current IP address
    pub fn ip_addr(&self) -> Option<u32> {
        self.lease.as_ref().map(|l| l.ip_addr)
    }
}

/// Network configuration manager
pub struct NetConfigManager {
    /// Interfaces
    pub interfaces: BTreeMap<usize, Interface>,
    /// DHCP clients per interface
    pub dhcp_clients: BTreeMap<usize, DhcpClient>,
    /// Next interface index
    next_index: usize,
}

impl NetConfigManager {
    /// Creates a new configuration manager
    pub fn new() -> Self {
        let mut manager = Self {
            interfaces: BTreeMap::new(),
            dhcp_clients: BTreeMap::new(),
            next_index: 1,
        };

        // Add loopback interface
        manager.interfaces.insert(0, Interface::loopback());

        manager
    }

    /// Adds an interface
    pub fn add_interface(&mut self, name: &str, mac: [u8; 6]) -> usize {
        let index = self.next_index;
        self.next_index += 1;
        self.interfaces.insert(index, Interface::new(name, index, mac));
        index
    }

    /// Gets an interface
    pub fn get_interface(&self, index: usize) -> Option<&Interface> {
        self.interfaces.get(&index)
    }

    /// Gets a mutable interface
    pub fn get_interface_mut(&mut self, index: usize) -> Option<&mut Interface> {
        self.interfaces.get_mut(&index)
    }

    /// Configures an interface with static IP
    pub fn configure_static(
        &mut self,
        index: usize,
        ip: u32,
        netmask: u32,
        gateway: Option<u32>,
    ) -> Result<(), &'static str> {
        let iface = self.interfaces.get_mut(&index).ok_or("Interface not found")?;
        iface.up();
        iface.configure_static(ip, netmask, gateway);
        Ok(())
    }

    /// Starts DHCP on an interface
    pub fn start_dhcp(&mut self, index: usize) -> Result<(), &'static str> {
        if !self.interfaces.contains_key(&index) {
            return Err("Interface not found");
        }

        let client = DhcpClient::new(index);
        self.dhcp_clients.insert(index, client);
        Ok(())
    }

    /// Finds interface for a destination IP
    pub fn find_interface(&self, dest_ip: u32) -> Option<usize> {
        for (index, iface) in &self.interfaces {
            if iface.state == InterfaceState::Running && iface.is_local(dest_ip) {
                return Some(*index);
            }
        }

        // Return first running non-loopback interface as default
        for (index, iface) in &self.interfaces {
            if iface.state == InterfaceState::Running && !iface.loopback {
                return Some(*index);
            }
        }

        None
    }

    /// Lists all interfaces
    pub fn list_interfaces(&self) -> Vec<&Interface> {
        self.interfaces.values().collect()
    }
}

impl Default for NetConfigManager {
    fn default() -> Self {
        Self::new()
    }
}

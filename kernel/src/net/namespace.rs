//! # Network Namespaces
//!
//! Network namespaces provide isolation of network resources between containers
//! and processes. Each namespace has its own:
//! - Network interfaces (virtual and physical)
//! - Routing tables
//! - Firewall rules
//! - Socket bindings
//! - Port allocations
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                     Network Namespace Registry                       │
//! │  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐         │
//! │  │   Namespace 0  │  │   Namespace 1  │  │   Namespace N  │   ...   │
//! │  │   (Default)    │  │   (Container)  │  │   (Container)  │         │
//! │  └───────┬────────┘  └───────┬────────┘  └───────┬────────┘         │
//! │          │                   │                   │                   │
//! │          ▼                   ▼                   ▼                   │
//! │  ┌───────────────┐   ┌───────────────┐   ┌───────────────┐          │
//! │  │  Interfaces   │   │  Interfaces   │   │  Interfaces   │          │
//! │  │  Routes       │   │  Routes       │   │  Routes       │          │
//! │  │  Sockets      │   │  Sockets      │   │  Sockets      │          │
//! │  │  Firewall     │   │  Firewall     │   │  Firewall     │          │
//! │  └───────────────┘   └───────────────┘   └───────────────┘          │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Capability-Based Access
//!
//! Network operations require capabilities:
//! - `CAP_NET_BIND`: Bind to ports < 1024
//! - `CAP_NET_RAW`: Access raw sockets
//! - `CAP_NET_ADMIN`: Configure interfaces and routes
//! - `CAP_NET_NAMESPACE`: Create/enter network namespaces

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, RwLock};
use core::sync::atomic::{AtomicU64, Ordering};

use super::ip::Ipv4Address;
use super::ethernet::MacAddress;
use super::socket::SocketHandle;

// =============================================================================
// Network Namespace Identifier
// =============================================================================

/// Network namespace identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NetNsId(pub u64);

impl NetNsId {
    /// Default/init network namespace (ID 0).
    pub const DEFAULT: NetNsId = NetNsId(0);
    
    /// Create a new namespace ID.
    pub const fn new(id: u64) -> Self {
        NetNsId(id)
    }
}

impl core::fmt::Display for NetNsId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "netns:{}", self.0)
    }
}

// =============================================================================
// Network Capabilities (for ACL)
// =============================================================================

/// Network capabilities for access control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum NetCapability {
    /// Bind to privileged ports (< 1024).
    BindPrivileged = 0x0001,
    /// Bind to any port.
    Bind = 0x0002,
    /// Connect to external addresses.
    Connect = 0x0004,
    /// Listen for incoming connections.
    Listen = 0x0008,
    /// Access raw sockets.
    RawSocket = 0x0010,
    /// Configure network interfaces.
    ConfigInterface = 0x0020,
    /// Modify routing tables.
    ConfigRoute = 0x0040,
    /// Modify firewall rules.
    ConfigFirewall = 0x0080,
    /// Create network namespaces.
    CreateNamespace = 0x0100,
    /// Enter other network namespaces.
    EnterNamespace = 0x0200,
    /// Full network administration.
    Admin = 0xFFFF,
}

/// Capability set for network access control.
#[derive(Debug, Clone, Copy)]
pub struct NetCapabilitySet(u32);

impl NetCapabilitySet {
    /// Empty capability set.
    pub const NONE: NetCapabilitySet = NetCapabilitySet(0);
    
    /// Default capabilities for unprivileged processes.
    pub const DEFAULT: NetCapabilitySet = NetCapabilitySet(
        NetCapability::Bind as u32 | 
        NetCapability::Connect as u32 | 
        NetCapability::Listen as u32
    );
    
    /// Full capabilities for privileged processes.
    pub const FULL: NetCapabilitySet = NetCapabilitySet(NetCapability::Admin as u32);
    
    /// Check if capability is present.
    pub fn has(&self, cap: NetCapability) -> bool {
        (self.0 & cap as u32) != 0
    }
    
    /// Add a capability.
    pub fn add(&mut self, cap: NetCapability) {
        self.0 |= cap as u32;
    }
    
    /// Remove a capability.
    pub fn remove(&mut self, cap: NetCapability) {
        self.0 &= !(cap as u32);
    }
    
    /// Create from raw bits.
    pub const fn from_bits(bits: u32) -> Self {
        NetCapabilitySet(bits)
    }
    
    /// Get raw bits.
    pub const fn bits(&self) -> u32 {
        self.0
    }
}

impl Default for NetCapabilitySet {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// =============================================================================
// Virtual Network Interface
// =============================================================================

/// Virtual network interface within a namespace.
#[derive(Debug, Clone)]
pub struct VirtualInterface {
    /// Interface name (e.g., "eth0", "veth0").
    pub name: String,
    /// MAC address.
    pub mac: MacAddress,
    /// IPv4 address.
    pub ipv4: Option<Ipv4Address>,
    /// Subnet mask.
    pub netmask: Option<Ipv4Address>,
    /// MTU.
    pub mtu: u16,
    /// Interface is up.
    pub up: bool,
    /// Peer interface ID (for veth pairs).
    pub peer: Option<(NetNsId, String)>,
}

impl VirtualInterface {
    /// Create a new virtual interface.
    pub fn new(name: String, mac: MacAddress) -> Self {
        Self {
            name,
            mac,
            ipv4: None,
            netmask: None,
            mtu: 1500,
            up: false,
            peer: None,
        }
    }
    
    /// Create a loopback interface.
    pub fn loopback() -> Self {
        Self {
            name: String::from("lo"),
            mac: MacAddress::ZERO,
            ipv4: Some(Ipv4Address::LOCALHOST),
            netmask: Some(Ipv4Address::new(255, 0, 0, 0)),
            mtu: 65536,
            up: true,
            peer: None,
        }
    }
}

// =============================================================================
// Routing Table
// =============================================================================

/// Route entry.
#[derive(Debug, Clone)]
pub struct RouteEntry {
    /// Destination network.
    pub destination: Ipv4Address,
    /// Subnet mask.
    pub netmask: Ipv4Address,
    /// Gateway address (None for directly connected).
    pub gateway: Option<Ipv4Address>,
    /// Output interface.
    pub interface: String,
    /// Route metric (lower is preferred).
    pub metric: u32,
}

impl RouteEntry {
    /// Create a default route (0.0.0.0/0).
    pub fn default_route(gateway: Ipv4Address, interface: String) -> Self {
        Self {
            destination: Ipv4Address::ANY,
            netmask: Ipv4Address::ANY,
            gateway: Some(gateway),
            interface,
            metric: 100,
        }
    }
    
    /// Create a directly connected route.
    pub fn connected(destination: Ipv4Address, netmask: Ipv4Address, interface: String) -> Self {
        Self {
            destination,
            netmask,
            gateway: None,
            interface,
            metric: 0,
        }
    }
    
    /// Check if address matches this route.
    pub fn matches(&self, addr: Ipv4Address) -> bool {
        let dest_bytes = self.destination.octets();
        let mask_bytes = self.netmask.octets();
        let addr_bytes = addr.octets();
        
        for i in 0..4 {
            if (addr_bytes[i] & mask_bytes[i]) != (dest_bytes[i] & mask_bytes[i]) {
                return false;
            }
        }
        true
    }
}

// =============================================================================
// Port Binding ACL
// =============================================================================

/// Port binding access control entry.
#[derive(Debug, Clone)]
pub struct PortAcl {
    /// Process ID allowed to bind.
    pub pid: u64,
    /// Port range start.
    pub port_start: u16,
    /// Port range end (inclusive).
    pub port_end: u16,
    /// Protocol (TCP=6, UDP=17, Any=0).
    pub protocol: u8,
}

impl PortAcl {
    /// Create ACL for a single port.
    pub fn single(pid: u64, port: u16, protocol: u8) -> Self {
        Self {
            pid,
            port_start: port,
            port_end: port,
            protocol,
        }
    }
    
    /// Create ACL for a port range.
    pub fn range(pid: u64, start: u16, end: u16, protocol: u8) -> Self {
        Self {
            pid,
            port_start: start,
            port_end: end,
            protocol,
        }
    }
    
    /// Check if this ACL allows the given binding.
    pub fn allows(&self, pid: u64, port: u16, protocol: u8) -> bool {
        self.pid == pid 
            && port >= self.port_start 
            && port <= self.port_end
            && (self.protocol == 0 || self.protocol == protocol)
    }
}

// =============================================================================
// Network Namespace
// =============================================================================

/// Network namespace containing isolated network resources.
pub struct NetworkNamespace {
    /// Namespace ID.
    id: NetNsId,
    /// Virtual interfaces.
    interfaces: BTreeMap<String, VirtualInterface>,
    /// Routing table.
    routes: Vec<RouteEntry>,
    /// Port binding ACLs.
    port_acls: Vec<PortAcl>,
    /// Bound sockets in this namespace.
    sockets: Vec<SocketHandle>,
    /// Default capabilities for processes in this namespace.
    default_caps: NetCapabilitySet,
    /// Reference count.
    refcount: AtomicU64,
}

impl NetworkNamespace {
    /// Create a new network namespace.
    pub fn new(id: NetNsId) -> Self {
        let mut ns = Self {
            id,
            interfaces: BTreeMap::new(),
            routes: Vec::new(),
            port_acls: Vec::new(),
            sockets: Vec::new(),
            default_caps: NetCapabilitySet::DEFAULT,
            refcount: AtomicU64::new(1),
        };
        
        // Add loopback interface by default
        let lo = VirtualInterface::loopback();
        ns.interfaces.insert(String::from("lo"), lo);
        
        // Add loopback route
        ns.routes.push(RouteEntry::connected(
            Ipv4Address::LOCALHOST,
            Ipv4Address::new(255, 0, 0, 0),
            String::from("lo"),
        ));
        
        ns
    }
    
    /// Create the default (init) namespace with host network access.
    pub fn default_namespace() -> Self {
        let mut ns = Self::new(NetNsId::DEFAULT);
        ns.default_caps = NetCapabilitySet::FULL;
        ns
    }
    
    /// Get namespace ID.
    pub fn id(&self) -> NetNsId {
        self.id
    }
    
    /// Add an interface to this namespace.
    pub fn add_interface(&mut self, iface: VirtualInterface) -> Result<(), NamespaceError> {
        if self.interfaces.contains_key(&iface.name) {
            return Err(NamespaceError::InterfaceExists);
        }
        self.interfaces.insert(iface.name.clone(), iface);
        Ok(())
    }
    
    /// Remove an interface from this namespace.
    pub fn remove_interface(&mut self, name: &str) -> Result<VirtualInterface, NamespaceError> {
        self.interfaces.remove(name).ok_or(NamespaceError::InterfaceNotFound)
    }
    
    /// Get an interface by name.
    pub fn get_interface(&self, name: &str) -> Option<&VirtualInterface> {
        self.interfaces.get(name)
    }
    
    /// Get mutable interface by name.
    pub fn get_interface_mut(&mut self, name: &str) -> Option<&mut VirtualInterface> {
        self.interfaces.get_mut(name)
    }
    
    /// List all interfaces.
    pub fn interfaces(&self) -> impl Iterator<Item = &VirtualInterface> {
        self.interfaces.values()
    }
    
    /// Add a route.
    pub fn add_route(&mut self, route: RouteEntry) {
        self.routes.push(route);
        // Sort by specificity (more specific routes first)
        self.routes.sort_by(|a, b| {
            let a_bits = a.netmask.octets().iter().map(|b| b.count_ones()).sum::<u32>();
            let b_bits = b.netmask.octets().iter().map(|b| b.count_ones()).sum::<u32>();
            b_bits.cmp(&a_bits).then(a.metric.cmp(&b.metric))
        });
    }
    
    /// Remove a route.
    pub fn remove_route(&mut self, destination: Ipv4Address, netmask: Ipv4Address) -> bool {
        let len_before = self.routes.len();
        self.routes.retain(|r| r.destination != destination || r.netmask != netmask);
        self.routes.len() < len_before
    }
    
    /// Look up route for destination.
    pub fn lookup_route(&self, destination: Ipv4Address) -> Option<&RouteEntry> {
        self.routes.iter().find(|r| r.matches(destination))
    }
    
    /// Add port binding ACL.
    pub fn add_port_acl(&mut self, acl: PortAcl) {
        self.port_acls.push(acl);
    }
    
    /// Check if port binding is allowed.
    pub fn check_port_binding(&self, pid: u64, port: u16, protocol: u8, caps: NetCapabilitySet) -> bool {
        // Privileged ports require special capability
        if port < 1024 && !caps.has(NetCapability::BindPrivileged) {
            return false;
        }
        
        // Check if process has bind capability
        if !caps.has(NetCapability::Bind) {
            return false;
        }
        
        // If no ACLs defined, allow all (with capabilities)
        if self.port_acls.is_empty() {
            return true;
        }
        
        // Check ACLs
        self.port_acls.iter().any(|acl| acl.allows(pid, port, protocol))
    }
    
    /// Register socket in this namespace.
    pub fn register_socket(&mut self, handle: SocketHandle) {
        if !self.sockets.contains(&handle) {
            self.sockets.push(handle);
        }
    }
    
    /// Unregister socket from this namespace.
    pub fn unregister_socket(&mut self, handle: SocketHandle) {
        self.sockets.retain(|h| *h != handle);
    }
    
    /// Get default capabilities.
    pub fn default_capabilities(&self) -> NetCapabilitySet {
        self.default_caps
    }
    
    /// Set default capabilities.
    pub fn set_default_capabilities(&mut self, caps: NetCapabilitySet) {
        self.default_caps = caps;
    }
    
    /// Increment reference count.
    pub fn acquire(&self) {
        self.refcount.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Decrement reference count, returns true if namespace should be destroyed.
    pub fn release(&self) -> bool {
        self.refcount.fetch_sub(1, Ordering::Relaxed) == 1
    }
    
    /// Get current reference count.
    pub fn refcount(&self) -> u64 {
        self.refcount.load(Ordering::Relaxed)
    }
}

// =============================================================================
// Namespace Errors
// =============================================================================

/// Namespace operation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceError {
    /// Namespace not found.
    NotFound,
    /// Namespace already exists.
    AlreadyExists,
    /// Interface not found.
    InterfaceNotFound,
    /// Interface already exists.
    InterfaceExists,
    /// Permission denied.
    PermissionDenied,
    /// Resource limit reached.
    ResourceLimit,
    /// Invalid operation.
    InvalidOperation,
}

// =============================================================================
// Namespace Registry (Global)
// =============================================================================

/// Global network namespace registry.
pub struct NamespaceRegistry {
    /// All namespaces by ID.
    namespaces: BTreeMap<NetNsId, Arc<Mutex<NetworkNamespace>>>,
    /// Next namespace ID.
    next_id: AtomicU64,
}

impl NamespaceRegistry {
    /// Create a new registry with default namespace.
    pub fn new() -> Self {
        let mut registry = Self {
            namespaces: BTreeMap::new(),
            next_id: AtomicU64::new(1), // Start from 1, 0 is default
        };
        
        // Create default namespace
        let default_ns = Arc::new(Mutex::new(NetworkNamespace::default_namespace()));
        registry.namespaces.insert(NetNsId::DEFAULT, default_ns);
        
        registry
    }
    
    /// Create a new network namespace.
    pub fn create(&mut self) -> Result<NetNsId, NamespaceError> {
        let id = NetNsId::new(self.next_id.fetch_add(1, Ordering::Relaxed));
        let ns = Arc::new(Mutex::new(NetworkNamespace::new(id)));
        self.namespaces.insert(id, ns);
        Ok(id)
    }
    
    /// Create a namespace with specific ID (for container namespaces).
    pub fn create_with_id(&mut self, id: u64) -> Result<NetNsId, NamespaceError> {
        let ns_id = NetNsId::new(id);
        
        if self.namespaces.contains_key(&ns_id) {
            return Err(NamespaceError::AlreadyExists);
        }
        
        let ns = Arc::new(Mutex::new(NetworkNamespace::new(ns_id)));
        self.namespaces.insert(ns_id, ns);
        
        // Update next_id if necessary
        let current = self.next_id.load(Ordering::Relaxed);
        if id >= current {
            self.next_id.store(id + 1, Ordering::Relaxed);
        }
        
        Ok(ns_id)
    }
    
    /// Get a namespace by ID.
    pub fn get(&self, id: NetNsId) -> Option<Arc<Mutex<NetworkNamespace>>> {
        self.namespaces.get(&id).cloned()
    }
    
    /// Get the default namespace.
    pub fn default_namespace(&self) -> Arc<Mutex<NetworkNamespace>> {
        self.namespaces.get(&NetNsId::DEFAULT).cloned()
            .expect("Default namespace must exist")
    }
    
    /// Remove a namespace.
    pub fn remove(&mut self, id: NetNsId) -> Result<(), NamespaceError> {
        if id == NetNsId::DEFAULT {
            return Err(NamespaceError::InvalidOperation);
        }
        
        if let Some(ns) = self.namespaces.get(&id) {
            let ns_lock = ns.lock();
            if ns_lock.refcount() > 1 {
                return Err(NamespaceError::InvalidOperation);
            }
            drop(ns_lock);
        }
        
        self.namespaces.remove(&id).ok_or(NamespaceError::NotFound)?;
        Ok(())
    }
    
    /// List all namespace IDs.
    pub fn list(&self) -> Vec<NetNsId> {
        self.namespaces.keys().copied().collect()
    }
    
    /// Get namespace count.
    pub fn count(&self) -> usize {
        self.namespaces.len()
    }
}

// =============================================================================
// Global Registry
// =============================================================================

static NAMESPACE_REGISTRY: RwLock<Option<NamespaceRegistry>> = RwLock::new(None);

/// Initialize the namespace subsystem.
pub fn init() {
    let mut registry = NAMESPACE_REGISTRY.write();
    if registry.is_none() {
        *registry = Some(NamespaceRegistry::new());
    }
}

/// Get the global namespace registry.
fn registry() -> impl core::ops::Deref<Target = NamespaceRegistry> + 'static {
    struct RegistryGuard;
    
    impl core::ops::Deref for RegistryGuard {
        type Target = NamespaceRegistry;
        
        fn deref(&self) -> &Self::Target {
            // Safety: We ensure the registry is initialized before any access
            NAMESPACE_REGISTRY.read().as_ref().expect("Namespace registry not initialized")
        }
    }
    
    RegistryGuard
}

/// Get the global namespace registry (mutable).
fn registry_mut() -> spin::RwLockWriteGuard<'static, Option<NamespaceRegistry>> {
    NAMESPACE_REGISTRY.write()
}

// =============================================================================
// Public API
// =============================================================================

/// Create a new network namespace.
pub fn create_namespace() -> Result<NetNsId, NamespaceError> {
    registry_mut()
        .as_mut()
        .ok_or(NamespaceError::InvalidOperation)?
        .create()
}

/// Create a namespace with specific ID (for containers).
pub fn create_namespace_with_id(id: u64) -> Result<NetNsId, NamespaceError> {
    registry_mut()
        .as_mut()
        .ok_or(NamespaceError::InvalidOperation)?
        .create_with_id(id)
}

/// Get a namespace by ID.
pub fn get_namespace(id: NetNsId) -> Option<Arc<Mutex<NetworkNamespace>>> {
    NAMESPACE_REGISTRY.read().as_ref()?.get(id)
}

/// Get the default namespace.
pub fn default_namespace() -> Option<Arc<Mutex<NetworkNamespace>>> {
    NAMESPACE_REGISTRY.read().as_ref().map(|r| r.default_namespace())
}

/// Remove a namespace.
pub fn remove_namespace(id: NetNsId) -> Result<(), NamespaceError> {
    registry_mut()
        .as_mut()
        .ok_or(NamespaceError::InvalidOperation)?
        .remove(id)
}

/// List all namespaces.
pub fn list_namespaces() -> Vec<NetNsId> {
    NAMESPACE_REGISTRY.read()
        .as_ref()
        .map(|r| r.list())
        .unwrap_or_default()
}

// =============================================================================
// Veth Pair Creation
// =============================================================================

/// Create a virtual ethernet pair connecting two namespaces.
pub fn create_veth_pair(
    ns1_id: NetNsId,
    name1: String,
    ns2_id: NetNsId,
    name2: String,
) -> Result<(), NamespaceError> {
    let registry = NAMESPACE_REGISTRY.read();
    let registry = registry.as_ref().ok_or(NamespaceError::InvalidOperation)?;
    
    let ns1 = registry.get(ns1_id).ok_or(NamespaceError::NotFound)?;
    let ns2 = registry.get(ns2_id).ok_or(NamespaceError::NotFound)?;
    
    // Generate MAC addresses based on interface names
    let mac1 = generate_mac(&name1);
    let mac2 = generate_mac(&name2);
    
    // Create interfaces
    let mut iface1 = VirtualInterface::new(name1.clone(), mac1);
    iface1.peer = Some((ns2_id, name2.clone()));
    
    let mut iface2 = VirtualInterface::new(name2.clone(), mac2);
    iface2.peer = Some((ns1_id, name1.clone()));
    
    // Add to namespaces
    ns1.lock().add_interface(iface1)?;
    ns2.lock().add_interface(iface2)?;
    
    Ok(())
}

/// Generate a MAC address from a name (deterministic).
fn generate_mac(name: &str) -> MacAddress {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV-1a offset
    for byte in name.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    
    MacAddress([
        0x02, // Locally administered, unicast
        ((hash >> 8) & 0xff) as u8,
        ((hash >> 16) & 0xff) as u8,
        ((hash >> 24) & 0xff) as u8,
        ((hash >> 32) & 0xff) as u8,
        ((hash >> 40) & 0xff) as u8,
    ])
}

// =============================================================================
// Process Namespace Binding
// =============================================================================

/// Per-process namespace assignment.
pub struct ProcessNetNs {
    /// Process ID.
    pub pid: u64,
    /// Network namespace ID.
    pub netns: NetNsId,
    /// Process capabilities.
    pub capabilities: NetCapabilitySet,
}

/// Process to namespace mapping.
static PROCESS_NS: RwLock<BTreeMap<u64, ProcessNetNs>> = RwLock::new(BTreeMap::new());

/// Bind a process to a network namespace.
pub fn bind_process(pid: u64, netns: NetNsId, caps: NetCapabilitySet) -> Result<(), NamespaceError> {
    // Verify namespace exists
    if get_namespace(netns).is_none() {
        return Err(NamespaceError::NotFound);
    }
    
    let binding = ProcessNetNs {
        pid,
        netns,
        capabilities: caps,
    };
    
    PROCESS_NS.write().insert(pid, binding);
    Ok(())
}

/// Unbind a process from its namespace.
pub fn unbind_process(pid: u64) {
    PROCESS_NS.write().remove(&pid);
}

/// Get the namespace for a process.
pub fn get_process_namespace(pid: u64) -> NetNsId {
    PROCESS_NS.read()
        .get(&pid)
        .map(|b| b.netns)
        .unwrap_or(NetNsId::DEFAULT)
}

/// Get the capabilities for a process.
pub fn get_process_capabilities(pid: u64) -> NetCapabilitySet {
    PROCESS_NS.read()
        .get(&pid)
        .map(|b| b.capabilities)
        .unwrap_or(NetCapabilitySet::DEFAULT)
}

/// Check if a process has a capability.
pub fn process_has_capability(pid: u64, cap: NetCapability) -> bool {
    get_process_capabilities(pid).has(cap)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_capability_set() {
        let mut caps = NetCapabilitySet::NONE;
        assert!(!caps.has(NetCapability::Bind));
        
        caps.add(NetCapability::Bind);
        assert!(caps.has(NetCapability::Bind));
        assert!(!caps.has(NetCapability::RawSocket));
        
        caps.remove(NetCapability::Bind);
        assert!(!caps.has(NetCapability::Bind));
    }
    
    #[test]
    fn test_route_matching() {
        let route = RouteEntry::connected(
            Ipv4Address::new(192, 168, 1, 0),
            Ipv4Address::new(255, 255, 255, 0),
            String::from("eth0"),
        );
        
        assert!(route.matches(Ipv4Address::new(192, 168, 1, 1)));
        assert!(route.matches(Ipv4Address::new(192, 168, 1, 254)));
        assert!(!route.matches(Ipv4Address::new(192, 168, 2, 1)));
    }
    
    #[test]
    fn test_port_acl() {
        let acl = PortAcl::range(1000, 8000, 8100, 6);
        
        assert!(acl.allows(1000, 8080, 6));
        assert!(!acl.allows(1000, 9000, 6));
        assert!(!acl.allows(2000, 8080, 6));
        assert!(!acl.allows(1000, 8080, 17)); // Wrong protocol
    }
}

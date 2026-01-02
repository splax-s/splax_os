//! # Network Access Control List (ACL)
//!
//! Capability-based network access control for the S-NET service.
//! This module implements fine-grained network permissions based on
//! S-CAP capabilities.
//!
//! ## Permission Model
//!
//! Network access is controlled by capabilities:
//! - `net:bind:<port>` - Permission to bind to a specific port
//! - `net:bind:privileged` - Permission to bind to ports < 1024
//! - `net:connect:<host>` - Permission to connect to a specific host
//! - `net:listen` - Permission to accept incoming connections
//! - `net:raw` - Permission for raw socket access
//!
//! ## Per-Namespace ACLs
//!
//! Each network namespace can have its own ACL rules, allowing
//! containers to have isolated and restricted network access.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::RwLock;

// =============================================================================
// Network Capability Types
// =============================================================================

/// Network capability type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetCapType {
    /// Bind to ports (non-privileged, >= 1024)
    Bind,
    /// Bind to privileged ports (< 1024)
    BindPrivileged,
    /// Connect to external addresses
    Connect,
    /// Listen for incoming connections
    Listen,
    /// Raw socket access
    RawSocket,
    /// Configure interfaces
    ConfigInterface,
    /// Modify routes
    ConfigRoute,
    /// Modify firewall rules
    ConfigFirewall,
}

/// Network capability with optional constraint.
#[derive(Debug, Clone)]
pub struct NetCapability {
    /// Capability type.
    pub cap_type: NetCapType,
    /// Optional port constraint (for bind).
    pub port: Option<u16>,
    /// Optional port range (for bind).
    pub port_range: Option<(u16, u16)>,
    /// Optional address constraint (for connect).
    pub address: Option<[u8; 4]>,
    /// Optional address mask (for connect).
    pub address_mask: Option<[u8; 4]>,
}

impl NetCapability {
    /// Create a bind capability for a specific port.
    pub fn bind_port(port: u16) -> Self {
        Self {
            cap_type: if port < 1024 { NetCapType::BindPrivileged } else { NetCapType::Bind },
            port: Some(port),
            port_range: None,
            address: None,
            address_mask: None,
        }
    }

    /// Create a bind capability for a port range.
    pub fn bind_range(start: u16, end: u16) -> Self {
        Self {
            cap_type: if start < 1024 { NetCapType::BindPrivileged } else { NetCapType::Bind },
            port: None,
            port_range: Some((start, end)),
            address: None,
            address_mask: None,
        }
    }

    /// Create a general bind capability.
    pub fn bind() -> Self {
        Self {
            cap_type: NetCapType::Bind,
            port: None,
            port_range: None,
            address: None,
            address_mask: None,
        }
    }

    /// Create a connect capability for a specific address.
    pub fn connect_to(addr: [u8; 4]) -> Self {
        Self {
            cap_type: NetCapType::Connect,
            port: None,
            port_range: None,
            address: Some(addr),
            address_mask: Some([255, 255, 255, 255]),
        }
    }

    /// Create a connect capability for a subnet.
    pub fn connect_subnet(addr: [u8; 4], mask: [u8; 4]) -> Self {
        Self {
            cap_type: NetCapType::Connect,
            port: None,
            port_range: None,
            address: Some(addr),
            address_mask: Some(mask),
        }
    }

    /// Create a general connect capability.
    pub fn connect() -> Self {
        Self {
            cap_type: NetCapType::Connect,
            port: None,
            port_range: None,
            address: None,
            address_mask: None,
        }
    }

    /// Create a listen capability.
    pub fn listen() -> Self {
        Self {
            cap_type: NetCapType::Listen,
            port: None,
            port_range: None,
            address: None,
            address_mask: None,
        }
    }

    /// Create a raw socket capability.
    pub fn raw() -> Self {
        Self {
            cap_type: NetCapType::RawSocket,
            port: None,
            port_range: None,
            address: None,
            address_mask: None,
        }
    }

    /// Check if this capability allows binding to a port.
    pub fn allows_bind(&self, port: u16) -> bool {
        match self.cap_type {
            NetCapType::Bind | NetCapType::BindPrivileged => {
                if let Some(p) = self.port {
                    return p == port;
                }
                if let Some((start, end)) = self.port_range {
                    return port >= start && port <= end;
                }
                // No constraint, allow all (for this capability type)
                if port < 1024 {
                    self.cap_type == NetCapType::BindPrivileged
                } else {
                    true
                }
            }
            _ => false,
        }
    }

    /// Check if this capability allows connecting to an address.
    pub fn allows_connect(&self, addr: [u8; 4]) -> bool {
        if self.cap_type != NetCapType::Connect {
            return false;
        }

        if let (Some(allowed_addr), Some(mask)) = (self.address, self.address_mask) {
            for i in 0..4 {
                if (addr[i] & mask[i]) != (allowed_addr[i] & mask[i]) {
                    return false;
                }
            }
            true
        } else {
            // No constraint, allow all
            true
        }
    }
}

// =============================================================================
// Process Capability Set
// =============================================================================

/// Capability set for a process.
#[derive(Debug, Clone)]
pub struct ProcessCapSet {
    /// Process ID.
    pub pid: u64,
    /// Namespace ID.
    pub namespace_id: u64,
    /// Granted capabilities.
    pub capabilities: Vec<NetCapability>,
}

impl ProcessCapSet {
    /// Create a new capability set.
    pub fn new(pid: u64, namespace_id: u64) -> Self {
        Self {
            pid,
            namespace_id,
            capabilities: Vec::new(),
        }
    }

    /// Create a default capability set (unprivileged).
    pub fn default_unprivileged(pid: u64, namespace_id: u64) -> Self {
        let mut caps = Self::new(pid, namespace_id);
        caps.capabilities.push(NetCapability::bind());
        caps.capabilities.push(NetCapability::connect());
        caps.capabilities.push(NetCapability::listen());
        caps
    }

    /// Create a full capability set (privileged).
    pub fn full(pid: u64, namespace_id: u64) -> Self {
        let mut caps = Self::new(pid, namespace_id);
        caps.capabilities.push(NetCapability {
            cap_type: NetCapType::BindPrivileged,
            port: None,
            port_range: None,
            address: None,
            address_mask: None,
        });
        caps.capabilities.push(NetCapability::bind());
        caps.capabilities.push(NetCapability::connect());
        caps.capabilities.push(NetCapability::listen());
        caps.capabilities.push(NetCapability::raw());
        caps.capabilities.push(NetCapability {
            cap_type: NetCapType::ConfigInterface,
            port: None,
            port_range: None,
            address: None,
            address_mask: None,
        });
        caps
    }

    /// Grant a capability.
    pub fn grant(&mut self, cap: NetCapability) {
        self.capabilities.push(cap);
    }

    /// Revoke a capability type.
    pub fn revoke(&mut self, cap_type: NetCapType) {
        self.capabilities.retain(|c| c.cap_type != cap_type);
    }

    /// Check if binding to a port is allowed.
    pub fn can_bind(&self, port: u16) -> bool {
        self.capabilities.iter().any(|c| c.allows_bind(port))
    }

    /// Check if connecting to an address is allowed.
    pub fn can_connect(&self, addr: [u8; 4]) -> bool {
        self.capabilities.iter().any(|c| c.allows_connect(addr))
    }

    /// Check if listening is allowed.
    pub fn can_listen(&self) -> bool {
        self.capabilities.iter().any(|c| c.cap_type == NetCapType::Listen)
    }

    /// Check if raw socket access is allowed.
    pub fn can_raw(&self) -> bool {
        self.capabilities.iter().any(|c| c.cap_type == NetCapType::RawSocket)
    }
}

// =============================================================================
// Namespace ACL
// =============================================================================

/// ACL rules for a network namespace.
#[derive(Debug, Clone)]
pub struct NamespaceAcl {
    /// Namespace ID.
    pub namespace_id: u64,
    /// Default policy (true = allow, false = deny).
    pub default_allow: bool,
    /// Allowed outbound ports.
    pub allowed_outbound_ports: Vec<u16>,
    /// Allowed outbound addresses (with masks).
    pub allowed_outbound_addrs: Vec<([u8; 4], [u8; 4])>,
    /// Allowed inbound ports.
    pub allowed_inbound_ports: Vec<u16>,
    /// Blocked addresses.
    pub blocked_addrs: Vec<([u8; 4], [u8; 4])>,
}

impl NamespaceAcl {
    /// Create a new ACL with default allow policy.
    pub fn new_allow(namespace_id: u64) -> Self {
        Self {
            namespace_id,
            default_allow: true,
            allowed_outbound_ports: Vec::new(),
            allowed_outbound_addrs: Vec::new(),
            allowed_inbound_ports: Vec::new(),
            blocked_addrs: Vec::new(),
        }
    }

    /// Create a new ACL with default deny policy.
    pub fn new_deny(namespace_id: u64) -> Self {
        Self {
            namespace_id,
            default_allow: false,
            allowed_outbound_ports: Vec::new(),
            allowed_outbound_addrs: Vec::new(),
            allowed_inbound_ports: Vec::new(),
            blocked_addrs: Vec::new(),
        }
    }

    /// Allow outbound connection to a port.
    pub fn allow_outbound_port(&mut self, port: u16) {
        if !self.allowed_outbound_ports.contains(&port) {
            self.allowed_outbound_ports.push(port);
        }
    }

    /// Allow outbound connection to an address.
    pub fn allow_outbound_addr(&mut self, addr: [u8; 4], mask: [u8; 4]) {
        self.allowed_outbound_addrs.push((addr, mask));
    }

    /// Allow inbound connection to a port.
    pub fn allow_inbound_port(&mut self, port: u16) {
        if !self.allowed_inbound_ports.contains(&port) {
            self.allowed_inbound_ports.push(port);
        }
    }

    /// Block an address.
    pub fn block_addr(&mut self, addr: [u8; 4], mask: [u8; 4]) {
        self.blocked_addrs.push((addr, mask));
    }

    /// Check if outbound connection to address:port is allowed.
    pub fn allows_outbound(&self, addr: [u8; 4], port: u16) -> bool {
        // First check if address is blocked
        for (blocked, mask) in &self.blocked_addrs {
            let mut matches = true;
            for i in 0..4 {
                if (addr[i] & mask[i]) != (blocked[i] & mask[i]) {
                    matches = false;
                    break;
                }
            }
            if matches {
                return false;
            }
        }

        if self.default_allow {
            true
        } else {
            // Check explicit allows
            let port_allowed = self.allowed_outbound_ports.is_empty() 
                || self.allowed_outbound_ports.contains(&port);
            
            let addr_allowed = self.allowed_outbound_addrs.is_empty() 
                || self.allowed_outbound_addrs.iter().any(|(allowed, mask)| {
                    let mut matches = true;
                    for i in 0..4 {
                        if (addr[i] & mask[i]) != (allowed[i] & mask[i]) {
                            matches = false;
                            break;
                        }
                    }
                    matches
                });
            
            port_allowed && addr_allowed
        }
    }

    /// Check if inbound connection to a port is allowed.
    pub fn allows_inbound(&self, port: u16) -> bool {
        if self.default_allow {
            true
        } else {
            self.allowed_inbound_ports.is_empty() 
                || self.allowed_inbound_ports.contains(&port)
        }
    }
}

// =============================================================================
// Global ACL Registry
// =============================================================================

/// Global registry for process capabilities and namespace ACLs.
pub struct AclRegistry {
    /// Per-process capability sets.
    process_caps: BTreeMap<u64, ProcessCapSet>,
    /// Per-namespace ACLs.
    namespace_acls: BTreeMap<u64, NamespaceAcl>,
}

impl AclRegistry {
    /// Create a new ACL registry.
    pub const fn new() -> Self {
        Self {
            process_caps: BTreeMap::new(),
            namespace_acls: BTreeMap::new(),
        }
    }

    /// Register process capabilities.
    pub fn register_process(&mut self, caps: ProcessCapSet) {
        self.process_caps.insert(caps.pid, caps);
    }

    /// Unregister a process.
    pub fn unregister_process(&mut self, pid: u64) {
        self.process_caps.remove(&pid);
    }

    /// Get process capabilities.
    pub fn get_process(&self, pid: u64) -> Option<&ProcessCapSet> {
        self.process_caps.get(&pid)
    }

    /// Get mutable process capabilities.
    pub fn get_process_mut(&mut self, pid: u64) -> Option<&mut ProcessCapSet> {
        self.process_caps.get_mut(&pid)
    }

    /// Register namespace ACL.
    pub fn register_namespace(&mut self, acl: NamespaceAcl) {
        self.namespace_acls.insert(acl.namespace_id, acl);
    }

    /// Unregister namespace ACL.
    pub fn unregister_namespace(&mut self, namespace_id: u64) {
        self.namespace_acls.remove(&namespace_id);
    }

    /// Get namespace ACL.
    pub fn get_namespace(&self, namespace_id: u64) -> Option<&NamespaceAcl> {
        self.namespace_acls.get(&namespace_id)
    }

    /// Check if a process can bind to a port.
    pub fn check_bind(&self, pid: u64, port: u16) -> bool {
        self.process_caps
            .get(&pid)
            .map(|c| c.can_bind(port))
            .unwrap_or(false)
    }

    /// Check if a process can connect to an address.
    pub fn check_connect(&self, pid: u64, namespace_id: u64, addr: [u8; 4], port: u16) -> bool {
        // Check process capabilities
        let can_connect = self.process_caps
            .get(&pid)
            .map(|c| c.can_connect(addr))
            .unwrap_or(false);
        
        if !can_connect {
            return false;
        }

        // Check namespace ACL
        self.namespace_acls
            .get(&namespace_id)
            .map(|acl| acl.allows_outbound(addr, port))
            .unwrap_or(true) // Allow by default if no ACL
    }

    /// Check if a process can listen.
    pub fn check_listen(&self, pid: u64, namespace_id: u64, port: u16) -> bool {
        // Check process capabilities
        let can_listen = self.process_caps
            .get(&pid)
            .map(|c| c.can_listen())
            .unwrap_or(false);
        
        if !can_listen {
            return false;
        }

        // Check namespace ACL for inbound
        self.namespace_acls
            .get(&namespace_id)
            .map(|acl| acl.allows_inbound(port))
            .unwrap_or(true)
    }
}

static ACL_REGISTRY: RwLock<AclRegistry> = RwLock::new(AclRegistry::new());

/// Get the global ACL registry for reading.
pub fn acl_registry() -> spin::RwLockReadGuard<'static, AclRegistry> {
    ACL_REGISTRY.read()
}

/// Get the global ACL registry for writing.
pub fn acl_registry_mut() -> spin::RwLockWriteGuard<'static, AclRegistry> {
    ACL_REGISTRY.write()
}

// =============================================================================
// Public API
// =============================================================================

/// Register a process with default unprivileged capabilities.
pub fn register_unprivileged_process(pid: u64, namespace_id: u64) {
    let caps = ProcessCapSet::default_unprivileged(pid, namespace_id);
    acl_registry_mut().register_process(caps);
}

/// Register a process with full privileged capabilities.
pub fn register_privileged_process(pid: u64, namespace_id: u64) {
    let caps = ProcessCapSet::full(pid, namespace_id);
    acl_registry_mut().register_process(caps);
}

/// Grant a capability to a process.
pub fn grant_capability(pid: u64, cap: NetCapability) {
    if let Some(caps) = acl_registry_mut().get_process_mut(pid) {
        caps.grant(cap);
    }
}

/// Check if bind operation is allowed.
pub fn can_bind(pid: u64, port: u16) -> bool {
    acl_registry().check_bind(pid, port)
}

/// Check if connect operation is allowed.
pub fn can_connect(pid: u64, namespace_id: u64, addr: [u8; 4], port: u16) -> bool {
    acl_registry().check_connect(pid, namespace_id, addr, port)
}

/// Check if listen operation is allowed.
pub fn can_listen(pid: u64, namespace_id: u64, port: u16) -> bool {
    acl_registry().check_listen(pid, namespace_id, port)
}

/// Create a restricted ACL for a container namespace.
pub fn create_container_acl(namespace_id: u64) {
    let mut acl = NamespaceAcl::new_deny(namespace_id);
    // By default, allow common outbound ports (HTTP, HTTPS, DNS)
    acl.allow_outbound_port(80);
    acl.allow_outbound_port(443);
    acl.allow_outbound_port(53);
    acl_registry_mut().register_namespace(acl);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_capability() {
        let cap = NetCapability::bind_port(8080);
        assert!(cap.allows_bind(8080));
        assert!(!cap.allows_bind(8081));
    }

    #[test]
    fn test_bind_range() {
        let cap = NetCapability::bind_range(8000, 9000);
        assert!(cap.allows_bind(8080));
        assert!(cap.allows_bind(8000));
        assert!(cap.allows_bind(9000));
        assert!(!cap.allows_bind(7999));
        assert!(!cap.allows_bind(9001));
    }

    #[test]
    fn test_connect_capability() {
        let cap = NetCapability::connect_to([192, 168, 1, 1]);
        assert!(cap.allows_connect([192, 168, 1, 1]));
        assert!(!cap.allows_connect([192, 168, 1, 2]));
    }

    #[test]
    fn test_connect_subnet() {
        let cap = NetCapability::connect_subnet([192, 168, 1, 0], [255, 255, 255, 0]);
        assert!(cap.allows_connect([192, 168, 1, 1]));
        assert!(cap.allows_connect([192, 168, 1, 254]));
        assert!(!cap.allows_connect([192, 168, 2, 1]));
    }

    #[test]
    fn test_namespace_acl() {
        let mut acl = NamespaceAcl::new_deny(1);
        acl.allow_outbound_port(443);
        acl.allow_outbound_addr([10, 0, 0, 0], [255, 0, 0, 0]);

        assert!(acl.allows_outbound([10, 0, 2, 5], 443));
        assert!(!acl.allows_outbound([192, 168, 1, 1], 443));
    }
}

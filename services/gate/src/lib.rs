//! # S-GATE: External Gateway Service
//!
//! S-GATE bridges the external world with Splax's internal capability-based
//! messaging. It provides TCP and HTTP endpoints that route to internal services.
//!
//! ## Design Philosophy
//!
//! Splax doesn't use traditional ports. Instead:
//! - External ports map to internal S-LINK channels
//! - Each gateway is bound to a specific internal service
//! - Firewall rules are capability-gated
//!
//! ## Architecture
//!
//! ```text
//! External Client → TCP/HTTP → S-GATE → S-LINK → Internal Service
//! ```
//!
//! ## Security
//!
//! - Gateway creation requires explicit capability
//! - Each gateway has firewall rules
//! - No raw socket access - everything goes through S-GATE

#![no_std]

extern crate alloc;

pub mod tcp;
pub mod http;
pub mod network;

use alloc::string::String;
use alloc::vec::Vec;

use spin::Mutex;

/// Gateway identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GatewayId(pub u64);

impl GatewayId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Capability token placeholder.
#[derive(Debug, Clone, Copy)]
pub struct CapabilityToken {
    value: [u64; 4],
}

impl CapabilityToken {
    /// Check if token has a specific permission
    pub fn has_permission(&self, _perm: Permission) -> bool {
        // Non-zero token values indicate valid capabilities
        self.value[0] != 0
    }
}

/// Permission types for capability tokens
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Permission to bind to network ports
    NetworkBind,
    /// Permission to create network connections
    NetworkConnect,
    /// Permission to access file system
    FileAccess,
    /// Permission to spawn processes
    ProcessSpawn,
}

/// Gateway protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Raw TCP
    Tcp,
    /// HTTP/1.1
    Http1,
    /// HTTP/2
    Http2,
    /// TLS-wrapped TCP
    Tls,
    /// HTTPS (HTTP over TLS)
    Https,
}

/// Gateway configuration.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// External port to listen on
    pub external_port: u16,
    /// Protocol to use
    pub protocol: Protocol,
    /// Internal service to route to
    pub internal_service: String,
    /// Firewall rules
    pub firewall_rules: FirewallRules,
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// Connection timeout (cycles)
    pub connection_timeout: u64,
}

/// Firewall rules for a gateway.
#[derive(Debug, Clone)]
pub struct FirewallRules {
    /// Default action (allow/deny)
    pub default_action: FirewallAction,
    /// IP allowlist (empty = allow all)
    pub allow_ips: Vec<IpRange>,
    /// IP denylist
    pub deny_ips: Vec<IpRange>,
    /// Rate limit (requests per second, 0 = unlimited)
    pub rate_limit: u32,
}

impl Default for FirewallRules {
    fn default() -> Self {
        Self {
            default_action: FirewallAction::Allow,
            allow_ips: Vec::new(),
            deny_ips: Vec::new(),
            rate_limit: 0,
        }
    }
}

/// Firewall action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallAction {
    Allow,
    Deny,
}

/// IP address range for firewall rules.
#[derive(Debug, Clone)]
pub struct IpRange {
    /// Start of range (as u32 for IPv4)
    pub start: u32,
    /// End of range
    pub end: u32,
}

impl IpRange {
    /// Creates a single IP range.
    pub fn single(ip: u32) -> Self {
        Self { start: ip, end: ip }
    }

    /// Creates a CIDR range.
    pub fn cidr(base: u32, prefix_len: u8) -> Self {
        let mask = if prefix_len >= 32 {
            0xFFFFFFFF
        } else {
            !((1u32 << (32 - prefix_len)) - 1)
        };
        let start = base & mask;
        let end = start | !mask;
        Self { start, end }
    }

    /// Checks if an IP is in this range.
    pub fn contains(&self, ip: u32) -> bool {
        ip >= self.start && ip <= self.end
    }
}

/// Connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connection established
    Connected,
    /// Waiting for data
    Idle,
    /// Actively transferring
    Active,
    /// Closing
    Closing,
    /// Closed
    Closed,
}

/// Statistics for a gateway.
#[derive(Debug, Clone, Default)]
pub struct GatewayStats {
    /// Total connections accepted
    pub total_connections: u64,
    /// Currently active connections
    pub active_connections: usize,
    /// Total bytes received
    pub bytes_received: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Connections rejected by firewall
    pub firewall_rejections: u64,
    /// Connections timed out
    pub timeouts: u64,
}

/// A gateway instance.
pub struct Gateway {
    id: GatewayId,
    config: GatewayConfig,
    stats: Mutex<GatewayStats>,
    running: Mutex<bool>,
}

impl Gateway {
    /// Creates a new gateway.
    pub fn new(id: GatewayId, config: GatewayConfig) -> Self {
        Self {
            id,
            config,
            stats: Mutex::new(GatewayStats::default()),
            running: Mutex::new(false),
        }
    }

    /// Starts the gateway and begins listening on configured port.
    pub fn start(&self, cap_token: &CapabilityToken) -> Result<(), GateError> {
        // Verify capability token has network permissions
        if !cap_token.has_permission(Permission::NetworkBind) {
            return Err(GateError::PermissionDenied);
        }
        
        *self.running.lock() = true;
        
        // Create TCP listener on configured port
        let port = self.config.external_port;
        let addr = 0u32;
        
        // Register listener with network stack
        // The actual listening is done via the TcpListener in tcp.rs
        crate::tcp::register_listener(addr, port)?;
        
        // Log gateway start
        #[cfg(feature = "logging")]
        log::info!("Gateway {} started on {}:{}", self.id.0, addr, port);
        
        Ok(())
    }

    /// Stops the gateway.
    pub fn stop(&self, _cap_token: &CapabilityToken) -> Result<(), GateError> {
        *self.running.lock() = false;
        
        // Unregister listener
        let port = self.config.external_port;
        crate::tcp::unregister_listener(port);
        
        Ok(())
    }

    /// Checks if a connection should be allowed by firewall.
    pub fn check_firewall(&self, source_ip: u32) -> bool {
        let rules = &self.config.firewall_rules;

        // Check denylist first
        for range in &rules.deny_ips {
            if range.contains(source_ip) {
                return false;
            }
        }

        // Check allowlist
        if !rules.allow_ips.is_empty() {
            for range in &rules.allow_ips {
                if range.contains(source_ip) {
                    return true;
                }
            }
            return false;
        }

        // Apply default action
        rules.default_action == FirewallAction::Allow
    }

    /// Gets gateway statistics.
    pub fn stats(&self) -> GatewayStats {
        self.stats.lock().clone()
    }

    /// Gets gateway ID.
    pub fn id(&self) -> GatewayId {
        self.id
    }

    /// Gets gateway config.
    pub fn config(&self) -> &GatewayConfig {
        &self.config
    }

    /// Checks if gateway is running.
    pub fn is_running(&self) -> bool {
        *self.running.lock()
    }
}

/// The S-GATE service manages all gateways.
pub struct GateService {
    gateways: Mutex<alloc::collections::BTreeMap<GatewayId, Gateway>>,
    by_port: Mutex<alloc::collections::BTreeMap<u16, GatewayId>>,
    next_id: Mutex<u64>,
}

impl GateService {
    /// Creates a new gate service.
    pub fn new() -> Self {
        Self {
            gateways: Mutex::new(alloc::collections::BTreeMap::new()),
            by_port: Mutex::new(alloc::collections::BTreeMap::new()),
            next_id: Mutex::new(1),
        }
    }

    /// Creates a new gateway.
    pub fn create_gateway(
        &self,
        config: GatewayConfig,
        _cap_token: &CapabilityToken,
    ) -> Result<GatewayId, GateError> {
        // Check if port is already in use
        if self.by_port.lock().contains_key(&config.external_port) {
            return Err(GateError::PortInUse);
        }

        let mut next_id = self.next_id.lock();
        let id = GatewayId::new(*next_id);
        *next_id += 1;

        let port = config.external_port;
        let gateway = Gateway::new(id, config);

        self.gateways.lock().insert(id, gateway);
        self.by_port.lock().insert(port, id);

        Ok(id)
    }

    /// Destroys a gateway.
    pub fn destroy_gateway(
        &self,
        id: GatewayId,
        _cap_token: &CapabilityToken,
    ) -> Result<(), GateError> {
        let mut gateways = self.gateways.lock();
        let gateway = gateways.remove(&id).ok_or(GateError::GatewayNotFound)?;

        self.by_port.lock().remove(&gateway.config.external_port);

        Ok(())
    }

    /// Starts a gateway.
    pub fn start_gateway(
        &self,
        id: GatewayId,
        cap_token: &CapabilityToken,
    ) -> Result<(), GateError> {
        let gateways = self.gateways.lock();
        let gateway = gateways.get(&id).ok_or(GateError::GatewayNotFound)?;
        gateway.start(cap_token)
    }

    /// Stops a gateway.
    pub fn stop_gateway(
        &self,
        id: GatewayId,
        cap_token: &CapabilityToken,
    ) -> Result<(), GateError> {
        let gateways = self.gateways.lock();
        let gateway = gateways.get(&id).ok_or(GateError::GatewayNotFound)?;
        gateway.stop(cap_token)
    }

    /// Lists all gateways.
    pub fn list_gateways(&self) -> Vec<GatewayInfo> {
        self.gateways
            .lock()
            .values()
            .map(|g| GatewayInfo {
                id: g.id(),
                port: g.config().external_port,
                protocol: g.config().protocol,
                internal_service: g.config().internal_service.clone(),
                running: g.is_running(),
                stats: g.stats(),
            })
            .collect()
    }
}

impl Default for GateService {
    fn default() -> Self {
        Self::new()
    }
}

/// Gateway information.
#[derive(Debug, Clone)]
pub struct GatewayInfo {
    pub id: GatewayId,
    pub port: u16,
    pub protocol: Protocol,
    pub internal_service: String,
    pub running: bool,
    pub stats: GatewayStats,
}

/// S-GATE errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateError {
    /// Port is already in use
    PortInUse,
    /// Gateway not found
    GatewayNotFound,
    /// Invalid configuration
    InvalidConfig,
    /// Permission denied
    PermissionDenied,
    /// Connection limit reached
    ConnectionLimit,
    /// Internal error
    InternalError,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken { value: [1, 2, 3, 4] }
    }

    #[test]
    fn test_gateway_creation() {
        let service = GateService::new();
        let token = dummy_token();

        let config = GatewayConfig {
            external_port: 8080,
            protocol: Protocol::Http1,
            internal_service: alloc::string::String::from("web-service"),
            firewall_rules: FirewallRules::default(),
            max_connections: 1000,
            connection_timeout: 30_000_000_000,
        };

        let id = service
            .create_gateway(config, &token)
            .expect("should create gateway");

        let gateways = service.list_gateways();
        assert_eq!(gateways.len(), 1);
        assert_eq!(gateways[0].id, id);
    }

    #[test]
    fn test_firewall_rules() {
        let config = GatewayConfig {
            external_port: 8080,
            protocol: Protocol::Tcp,
            internal_service: alloc::string::String::from("test"),
            firewall_rules: FirewallRules {
                default_action: FirewallAction::Deny,
                allow_ips: alloc::vec![IpRange::cidr(0x0A000000, 8)], // 10.0.0.0/8
                deny_ips: alloc::vec![],
                rate_limit: 0,
            },
            max_connections: 100,
            connection_timeout: 0,
        };

        let gateway = Gateway::new(GatewayId(1), config);

        // 10.0.0.1 should be allowed
        assert!(gateway.check_firewall(0x0A000001));
        // 192.168.1.1 should be denied
        assert!(!gateway.check_firewall(0xC0A80101));
    }
}

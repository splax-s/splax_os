//! # Service Mesh Integration
//!
//! Implements service mesh functionality for microservice communication,
//! including sidecar proxies, traffic management, and observability.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        Service Mesh                              │
//! ├─────────────────────────────────────────────────────────────────┤
//! │   Control Plane         │        Data Plane                     │
//! │   ┌─────────────┐       │   ┌──────────────────────┐           │
//! │   │ Mesh Config │       │   │   Sidecar Proxies    │           │
//! │   │  Registry   │       │   │  ┌────┐ ┌────┐      │           │
//! │   │  Policies   │◄──────┼───┤  │ S1 │ │ S2 │ ...  │           │
//! │   └─────────────┘       │   │  └────┘ └────┘      │           │
//! │                         │   └──────────────────────┘           │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

use crate::cap::CapabilityToken;
use crate::sched::ProcessId;

// =============================================================================
// Service Identity
// =============================================================================

/// Unique service identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceId(pub u64);

impl ServiceId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// Service endpoint address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceEndpoint {
    /// Host/IP address
    pub host: String,
    /// Port number
    pub port: u16,
    /// Protocol (tcp, udp, grpc, http)
    pub protocol: Protocol,
}

/// Communication protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Http,
    Http2,
    Grpc,
    Custom(u32),
}

/// Service instance information.
#[derive(Debug, Clone)]
pub struct ServiceInstance {
    /// Instance ID
    pub id: u64,
    /// Service ID
    pub service_id: ServiceId,
    /// Endpoint address
    pub endpoint: ServiceEndpoint,
    /// Health status
    pub healthy: bool,
    /// Weight for load balancing
    pub weight: u32,
    /// Metadata labels
    pub labels: BTreeMap<String, String>,
    /// Capability token for this instance
    pub capability: CapabilityToken,
}

// =============================================================================
// Traffic Management
// =============================================================================

/// Load balancing algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadBalancer {
    /// Round-robin distribution
    RoundRobin,
    /// Weighted round-robin
    WeightedRoundRobin,
    /// Least connections
    LeastConnections,
    /// Random selection
    Random,
    /// Consistent hashing
    ConsistentHash,
    /// Least latency
    LeastLatency,
}

impl Default for LoadBalancer {
    fn default() -> Self {
        LoadBalancer::RoundRobin
    }
}

/// Traffic routing rule.
#[derive(Debug, Clone)]
pub struct TrafficRule {
    /// Rule name
    pub name: String,
    /// Source service pattern
    pub source: Option<ServicePattern>,
    /// Destination service pattern
    pub destination: ServicePattern,
    /// Traffic split percentages
    pub split: Vec<WeightedDestination>,
    /// Timeout for requests
    pub timeout_ms: u64,
    /// Retry policy
    pub retries: RetryPolicy,
    /// Circuit breaker config
    pub circuit_breaker: Option<CircuitBreaker>,
}

/// Service matching pattern.
#[derive(Debug, Clone)]
pub struct ServicePattern {
    /// Service name pattern (supports wildcards)
    pub name: String,
    /// Required labels
    pub labels: BTreeMap<String, String>,
    /// Version constraint
    pub version: Option<String>,
}

/// Weighted traffic destination.
#[derive(Debug, Clone)]
pub struct WeightedDestination {
    /// Destination service
    pub service: ServicePattern,
    /// Traffic weight (0-100)
    pub weight: u8,
}

/// Retry policy configuration.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Retry on these status codes
    pub retry_on: Vec<u32>,
    /// Backoff between retries
    pub backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retry_on: vec![502, 503, 504],
            backoff_ms: 100,
        }
    }
}

/// Circuit breaker configuration.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    /// Failure threshold to open circuit
    pub failure_threshold: u32,
    /// Success threshold to close circuit
    pub success_threshold: u32,
    /// Half-open timeout
    pub half_open_timeout_ms: u64,
    /// Max concurrent requests in half-open
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            half_open_timeout_ms: 10000,
            half_open_max_requests: 5,
        }
    }
}

/// Circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

// =============================================================================
// Sidecar Proxy
// =============================================================================

/// Sidecar proxy for a service.
pub struct SidecarProxy {
    /// Service this proxy is for
    service_id: ServiceId,
    /// Proxy process ID
    process_id: ProcessId,
    /// Inbound port
    inbound_port: u16,
    /// Outbound port
    outbound_port: u16,
    /// Admin port
    admin_port: u16,
    /// Current connections
    connections: Mutex<Vec<ProxyConnection>>,
    /// Circuit breaker states
    circuit_states: Mutex<BTreeMap<ServiceId, CircuitState>>,
    /// Request counter
    request_count: AtomicU64,
    /// Error counter
    error_count: AtomicU64,
    /// Total latency (for averaging)
    total_latency_us: AtomicU64,
}

/// Active proxy connection.
#[derive(Debug, Clone)]
pub struct ProxyConnection {
    pub id: u64,
    pub source: ServiceId,
    pub destination: ServiceId,
    pub protocol: Protocol,
    pub started_at: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

impl SidecarProxy {
    /// Create a new sidecar proxy.
    pub fn new(
        service_id: ServiceId,
        process_id: ProcessId,
        inbound_port: u16,
        outbound_port: u16,
        admin_port: u16,
    ) -> Self {
        Self {
            service_id,
            process_id,
            inbound_port,
            outbound_port,
            admin_port,
            connections: Mutex::new(Vec::new()),
            circuit_states: Mutex::new(BTreeMap::new()),
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            total_latency_us: AtomicU64::new(0),
        }
    }

    /// Handle an outbound request.
    pub fn route_outbound(
        &self,
        destination: ServiceId,
        data: &[u8],
    ) -> Result<Vec<u8>, MeshError> {
        // Check circuit breaker
        let circuit_state = {
            let states = self.circuit_states.lock();
            states.get(&destination).copied().unwrap_or(CircuitState::Closed)
        };

        if circuit_state == CircuitState::Open {
            return Err(MeshError::CircuitOpen);
        }

        self.request_count.fetch_add(1, Ordering::Relaxed);

        // Route request (placeholder - would do actual networking)
        let start = crate::arch::read_cycle_counter();
        
        // Simulate routing...
        let result = Ok(Vec::new());
        
        let latency = crate::arch::read_cycle_counter() - start;
        self.total_latency_us.fetch_add(latency / 1000, Ordering::Relaxed);

        result
    }

    /// Handle an inbound request.
    pub fn handle_inbound(
        &self,
        source: ServiceId,
        data: &[u8],
    ) -> Result<Vec<u8>, MeshError> {
        // Validate source has permission to communicate
        // This would check capability tokens
        
        self.request_count.fetch_add(1, Ordering::Relaxed);
        
        // Forward to local service
        Ok(Vec::new())
    }

    /// Get proxy statistics.
    pub fn stats(&self) -> ProxyStats {
        let request_count = self.request_count.load(Ordering::Relaxed);
        let error_count = self.error_count.load(Ordering::Relaxed);
        let total_latency = self.total_latency_us.load(Ordering::Relaxed);
        
        ProxyStats {
            service_id: self.service_id,
            request_count,
            error_count,
            avg_latency_us: if request_count > 0 {
                total_latency / request_count
            } else {
                0
            },
            active_connections: self.connections.lock().len(),
        }
    }

    /// Update circuit breaker state.
    pub fn update_circuit(&self, destination: ServiceId, success: bool) {
        let mut states = self.circuit_states.lock();
        let state = states.entry(destination).or_insert(CircuitState::Closed);
        
        match *state {
            CircuitState::Closed => {
                if !success {
                    // Track failures, potentially open circuit
                    *state = CircuitState::Open;
                }
            }
            CircuitState::Open => {
                // After timeout, move to half-open
                *state = CircuitState::HalfOpen;
            }
            CircuitState::HalfOpen => {
                if success {
                    *state = CircuitState::Closed;
                } else {
                    *state = CircuitState::Open;
                }
            }
        }
    }
}

/// Proxy statistics.
#[derive(Debug, Clone)]
pub struct ProxyStats {
    pub service_id: ServiceId,
    pub request_count: u64,
    pub error_count: u64,
    pub avg_latency_us: u64,
    pub active_connections: usize,
}

// =============================================================================
// Service Mesh Control Plane
// =============================================================================

/// Mesh error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshError {
    /// Service not found
    ServiceNotFound,
    /// No healthy instances
    NoHealthyInstances,
    /// Circuit breaker open
    CircuitOpen,
    /// Timeout exceeded
    Timeout,
    /// Rate limit exceeded
    RateLimited,
    /// Authorization failed
    Unauthorized,
    /// Configuration error
    ConfigError,
}

/// Service mesh control plane.
pub struct ServiceMesh {
    /// Registered services
    services: Mutex<BTreeMap<ServiceId, ServiceInfo>>,
    /// Service instances
    instances: Mutex<BTreeMap<ServiceId, Vec<ServiceInstance>>>,
    /// Traffic rules
    rules: Mutex<Vec<TrafficRule>>,
    /// Sidecar proxies
    proxies: Mutex<BTreeMap<ServiceId, SidecarProxy>>,
    /// Next service ID
    next_service_id: AtomicU64,
    /// Next instance ID
    next_instance_id: AtomicU64,
    /// Mesh enabled
    enabled: AtomicBool,
    /// Global load balancer
    load_balancer: Mutex<LoadBalancer>,
}

/// Service metadata.
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub id: ServiceId,
    pub name: String,
    pub namespace: String,
    pub version: String,
    pub labels: BTreeMap<String, String>,
    pub required_capabilities: Vec<CapabilityToken>,
}

impl ServiceMesh {
    /// Create a new service mesh.
    pub const fn new() -> Self {
        Self {
            services: Mutex::new(BTreeMap::new()),
            instances: Mutex::new(BTreeMap::new()),
            rules: Mutex::new(Vec::new()),
            proxies: Mutex::new(BTreeMap::new()),
            next_service_id: AtomicU64::new(1),
            next_instance_id: AtomicU64::new(1),
            enabled: AtomicBool::new(false),
            load_balancer: Mutex::new(LoadBalancer::RoundRobin),
        }
    }

    /// Initialize the service mesh.
    pub fn init(&self) {
        self.enabled.store(true, Ordering::SeqCst);
        crate::serial_println!("[MESH] Service mesh initialized");
    }

    /// Register a service.
    pub fn register_service(
        &self,
        name: &str,
        namespace: &str,
        version: &str,
        labels: BTreeMap<String, String>,
    ) -> ServiceId {
        let id = ServiceId::new(self.next_service_id.fetch_add(1, Ordering::Relaxed));
        
        let info = ServiceInfo {
            id,
            name: String::from(name),
            namespace: String::from(namespace),
            version: String::from(version),
            labels,
            required_capabilities: Vec::new(),
        };

        self.services.lock().insert(id, info);
        self.instances.lock().insert(id, Vec::new());
        
        crate::serial_println!("[MESH] Registered service: {} (id: {:?})", name, id);
        
        id
    }

    /// Add a service instance.
    pub fn add_instance(
        &self,
        service_id: ServiceId,
        endpoint: ServiceEndpoint,
        weight: u32,
        capability: CapabilityToken,
    ) -> Result<u64, MeshError> {
        let instance_id = self.next_instance_id.fetch_add(1, Ordering::Relaxed);
        
        let instance = ServiceInstance {
            id: instance_id,
            service_id,
            endpoint,
            healthy: true,
            weight,
            labels: BTreeMap::new(),
            capability,
        };

        let mut instances = self.instances.lock();
        if let Some(list) = instances.get_mut(&service_id) {
            list.push(instance);
            Ok(instance_id)
        } else {
            Err(MeshError::ServiceNotFound)
        }
    }

    /// Remove a service instance.
    pub fn remove_instance(&self, service_id: ServiceId, instance_id: u64) -> Result<(), MeshError> {
        let mut instances = self.instances.lock();
        if let Some(list) = instances.get_mut(&service_id) {
            list.retain(|i| i.id != instance_id);
            Ok(())
        } else {
            Err(MeshError::ServiceNotFound)
        }
    }

    /// Get healthy instances for a service.
    pub fn get_healthy_instances(&self, service_id: ServiceId) -> Vec<ServiceInstance> {
        let instances = self.instances.lock();
        instances
            .get(&service_id)
            .map(|list| list.iter().filter(|i| i.healthy).cloned().collect())
            .unwrap_or_default()
    }

    /// Select an instance using load balancing.
    pub fn select_instance(&self, service_id: ServiceId) -> Result<ServiceInstance, MeshError> {
        let healthy = self.get_healthy_instances(service_id);
        
        if healthy.is_empty() {
            return Err(MeshError::NoHealthyInstances);
        }

        let lb = *self.load_balancer.lock();
        
        let instance = match lb {
            LoadBalancer::RoundRobin => {
                // Simple round-robin
                static RR_COUNTER: AtomicU64 = AtomicU64::new(0);
                let idx = RR_COUNTER.fetch_add(1, Ordering::Relaxed) as usize % healthy.len();
                healthy.get(idx).cloned()
            }
            LoadBalancer::Random => {
                // Random selection
                let idx = (crate::arch::read_cycle_counter() as usize) % healthy.len();
                healthy.get(idx).cloned()
            }
            LoadBalancer::WeightedRoundRobin => {
                // Weighted selection
                let total_weight: u32 = healthy.iter().map(|i| i.weight).sum();
                if total_weight == 0 {
                    healthy.first().cloned()
                } else {
                    let rand = (crate::arch::read_cycle_counter() % total_weight as u64) as u32;
                    let mut acc = 0;
                    let mut selected = healthy.first().cloned();
                    for instance in &healthy {
                        acc += instance.weight;
                        if acc > rand {
                            selected = Some(instance.clone());
                            break;
                        }
                    }
                    selected
                }
            }
            _ => healthy.first().cloned(),
        };

        instance.ok_or(MeshError::NoHealthyInstances)
    }

    /// Add a traffic rule.
    pub fn add_rule(&self, rule: TrafficRule) {
        self.rules.lock().push(rule);
    }

    /// Install a sidecar proxy for a service.
    pub fn install_sidecar(
        &self,
        service_id: ServiceId,
        process_id: ProcessId,
    ) -> Result<(), MeshError> {
        let proxy = SidecarProxy::new(
            service_id,
            process_id,
            15001, // Default inbound
            15002, // Default outbound
            15000, // Admin
        );

        self.proxies.lock().insert(service_id, proxy);
        
        crate::serial_println!("[MESH] Installed sidecar for service {:?}", service_id);
        
        Ok(())
    }

    /// Update instance health status.
    pub fn update_health(&self, service_id: ServiceId, instance_id: u64, healthy: bool) {
        let mut instances = self.instances.lock();
        if let Some(list) = instances.get_mut(&service_id) {
            for instance in list.iter_mut() {
                if instance.id == instance_id {
                    instance.healthy = healthy;
                    break;
                }
            }
        }
    }

    /// Get mesh statistics.
    pub fn stats(&self) -> MeshStats {
        let services = self.services.lock();
        let instances = self.instances.lock();
        let proxies = self.proxies.lock();
        
        let total_instances: usize = instances.values().map(|v| v.len()).sum();
        let healthy_instances: usize = instances
            .values()
            .flat_map(|v| v.iter())
            .filter(|i| i.healthy)
            .count();

        MeshStats {
            enabled: self.enabled.load(Ordering::Relaxed),
            service_count: services.len(),
            instance_count: total_instances,
            healthy_instance_count: healthy_instances,
            proxy_count: proxies.len(),
            rule_count: self.rules.lock().len(),
        }
    }
}

/// Mesh statistics.
#[derive(Debug, Clone)]
pub struct MeshStats {
    pub enabled: bool,
    pub service_count: usize,
    pub instance_count: usize,
    pub healthy_instance_count: usize,
    pub proxy_count: usize,
    pub rule_count: usize,
}

// =============================================================================
// mTLS Support
// =============================================================================

/// Mutual TLS configuration.
#[derive(Debug, Clone)]
pub struct MtlsConfig {
    /// Enable mTLS
    pub enabled: bool,
    /// Certificate rotation interval (seconds)
    pub cert_rotation_interval: u64,
    /// Allowed cipher suites
    pub cipher_suites: Vec<String>,
    /// Minimum TLS version
    pub min_tls_version: TlsVersion,
}

/// TLS version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsVersion {
    Tls12,
    Tls13,
}

impl Default for MtlsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cert_rotation_interval: 86400, // 24 hours
            cipher_suites: vec![
                String::from("TLS_AES_256_GCM_SHA384"),
                String::from("TLS_CHACHA20_POLY1305_SHA256"),
            ],
            min_tls_version: TlsVersion::Tls13,
        }
    }
}

// =============================================================================
// Observability
// =============================================================================

/// Distributed trace context.
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// Trace ID
    pub trace_id: u128,
    /// Span ID
    pub span_id: u64,
    /// Parent span ID
    pub parent_span_id: Option<u64>,
    /// Trace flags
    pub flags: u8,
}

impl TraceContext {
    /// Generate a new trace context.
    pub fn new() -> Self {
        Self {
            trace_id: crate::arch::read_cycle_counter() as u128 | 
                      ((crate::arch::read_cycle_counter() as u128) << 64),
            span_id: crate::arch::read_cycle_counter(),
            parent_span_id: None,
            flags: 0x01, // Sampled
        }
    }

    /// Create a child span.
    pub fn child_span(&self) -> Self {
        Self {
            trace_id: self.trace_id,
            span_id: crate::arch::read_cycle_counter(),
            parent_span_id: Some(self.span_id),
            flags: self.flags,
        }
    }

    /// Format as W3C Trace Context header.
    pub fn to_header(&self) -> String {
        alloc::format!(
            "00-{:032x}-{:016x}-{:02x}",
            self.trace_id,
            self.span_id,
            self.flags
        )
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Global State
// =============================================================================

static SERVICE_MESH: ServiceMesh = ServiceMesh::new();

/// Get the global service mesh.
pub fn mesh() -> &'static ServiceMesh {
    &SERVICE_MESH
}

/// Initialize the service mesh.
pub fn init() {
    SERVICE_MESH.init();
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_registration() {
        let mesh = ServiceMesh::new();
        
        let id = mesh.register_service("test-service", "default", "v1.0", BTreeMap::new());
        assert_eq!(id.as_u64(), 1);
        
        let stats = mesh.stats();
        assert_eq!(stats.service_count, 1);
    }

    #[test]
    fn test_load_balancing() {
        let mesh = ServiceMesh::new();
        
        let id = mesh.register_service("lb-test", "default", "v1.0", BTreeMap::new());
        
        // Add instances
        let ep1 = ServiceEndpoint {
            host: String::from("10.0.0.1"),
            port: 8080,
            protocol: Protocol::Http,
        };
        
        mesh.add_instance(id, ep1.clone(), 1, CapabilityToken::generate()).unwrap();
        
        // Select should work
        let selected = mesh.select_instance(id);
        assert!(selected.is_ok());
    }

    #[test]
    fn test_trace_context() {
        let ctx = TraceContext::new();
        let child = ctx.child_span();
        
        assert_eq!(child.trace_id, ctx.trace_id);
        assert_eq!(child.parent_span_id, Some(ctx.span_id));
    }
}

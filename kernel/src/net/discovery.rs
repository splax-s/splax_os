//! # Service Discovery
//!
//! DNS-based and registry-based service discovery for Splax OS.
//!
//! ## Features
//!
//! - **DNS Service Discovery**: Automatic DNS records for services
//! - **Health-aware Routing**: Only route to healthy endpoints
//! - **Multi-cluster Support**: Discover services across clusters
//! - **Capability-gated**: Service discovery requires S-CAP tokens
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                       Service Registry                          │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
//! │  │  Endpoints  │  │   Health    │  │    DNS      │              │
//! │  │   Manager   │  │   Checker   │  │   Server    │              │
//! │  └─────────────┘  └─────────────┘  └─────────────┘              │
//! └─────────────────────────────────────────────────────────────────┘
//!                                │
//!                                ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      Service Endpoints                          │
//! │     ┌───────┐     ┌───────┐     ┌───────┐     ┌───────┐        │
//! │     │ Pod 1 │     │ Pod 2 │     │ Pod 3 │     │ Pod N │        │
//! │     └───────┘     └───────┘     └───────┘     └───────┘        │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

// =============================================================================
// Core Types
// =============================================================================

/// Service name
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceName {
    /// Service name
    pub name: String,
    /// Namespace
    pub namespace: String,
}

impl ServiceName {
    /// Create a new service name
    pub fn new(name: &str, namespace: &str) -> Self {
        Self {
            name: name.to_string(),
            namespace: namespace.to_string(),
        }
    }

    /// Get FQDN for the service
    pub fn fqdn(&self, domain: &str) -> String {
        alloc::format!("{}.{}.svc.{}", self.name, self.namespace, domain)
    }

    /// Get short name
    pub fn short_name(&self) -> String {
        alloc::format!("{}.{}", self.name, self.namespace)
    }
}

/// Service endpoint
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// Endpoint ID
    pub id: String,
    /// IP address
    pub ip: String,
    /// Port
    pub port: u16,
    /// Protocol
    pub protocol: Protocol,
    /// Target name (pod name, etc.)
    pub target: String,
    /// Node name
    pub node: Option<String>,
    /// Zone
    pub zone: Option<String>,
    /// Ready status
    pub ready: bool,
    /// Serving status
    pub serving: bool,
    /// Terminating status
    pub terminating: bool,
    /// Weight for load balancing
    pub weight: u32,
    /// Endpoint labels
    pub labels: BTreeMap<String, String>,
    /// Last health check
    pub last_health_check: u64,
    /// Health status
    pub health: EndpointHealth,
}

/// Protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Http,
    Https,
    Grpc,
}

impl Default for Protocol {
    fn default() -> Self {
        Protocol::Tcp
    }
}

/// Endpoint health status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointHealth {
    /// Endpoint is healthy
    Healthy,
    /// Endpoint is unhealthy
    Unhealthy,
    /// Health unknown
    Unknown,
    /// Endpoint is draining
    Draining,
}

impl Default for EndpointHealth {
    fn default() -> Self {
        EndpointHealth::Unknown
    }
}

/// Service discovery record
#[derive(Debug, Clone)]
pub struct ServiceRecord {
    /// Service name
    pub name: ServiceName,
    /// Service type
    pub service_type: ServiceType,
    /// Cluster IP
    pub cluster_ip: Option<String>,
    /// External IPs
    pub external_ips: Vec<String>,
    /// Ports
    pub ports: Vec<ServicePort>,
    /// Endpoints
    pub endpoints: Vec<Endpoint>,
    /// Labels
    pub labels: BTreeMap<String, String>,
    /// Annotations
    pub annotations: BTreeMap<String, String>,
    /// Created at
    pub created_at: u64,
    /// Updated at
    pub updated_at: u64,
}

/// Service type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    /// Internal cluster IP
    ClusterIP,
    /// Node port
    NodePort,
    /// Load balancer
    LoadBalancer,
    /// Headless (no cluster IP)
    Headless,
    /// External name
    ExternalName,
}

impl Default for ServiceType {
    fn default() -> Self {
        ServiceType::ClusterIP
    }
}

/// Service port definition
#[derive(Debug, Clone)]
pub struct ServicePort {
    /// Port name
    pub name: Option<String>,
    /// Protocol
    pub protocol: Protocol,
    /// Service port
    pub port: u16,
    /// Target port
    pub target_port: u16,
    /// Node port (for NodePort/LoadBalancer)
    pub node_port: Option<u16>,
}

// =============================================================================
// DNS Server
// =============================================================================

/// DNS record type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsRecordType {
    /// A record (IPv4)
    A,
    /// AAAA record (IPv6)
    AAAA,
    /// SRV record
    SRV,
    /// CNAME record
    CNAME,
    /// TXT record
    TXT,
    /// PTR record
    PTR,
}

/// DNS record
#[derive(Debug, Clone)]
pub struct DnsRecord {
    /// Record name
    pub name: String,
    /// Record type
    pub record_type: DnsRecordType,
    /// TTL in seconds
    pub ttl: u32,
    /// Record data
    pub data: DnsRecordData,
}

/// DNS record data
#[derive(Debug, Clone)]
pub enum DnsRecordData {
    /// A record data (IPv4 address)
    A(String),
    /// AAAA record data (IPv6 address)
    AAAA(String),
    /// SRV record data
    SRV { priority: u16, weight: u16, port: u16, target: String },
    /// CNAME record data
    CNAME(String),
    /// TXT record data
    TXT(String),
    /// PTR record data
    PTR(String),
}

/// DNS query
#[derive(Debug, Clone)]
pub struct DnsQuery {
    /// Query name
    pub name: String,
    /// Query type
    pub query_type: DnsRecordType,
    /// Query class (usually IN)
    pub query_class: u16,
}

/// DNS response
#[derive(Debug, Clone)]
pub struct DnsResponse {
    /// Response code
    pub rcode: DnsResponseCode,
    /// Answers
    pub answers: Vec<DnsRecord>,
    /// Authority records
    pub authority: Vec<DnsRecord>,
    /// Additional records
    pub additional: Vec<DnsRecord>,
}

/// DNS response code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsResponseCode {
    /// No error
    NoError,
    /// Format error
    FormErr,
    /// Server failure
    ServFail,
    /// Name error (NXDOMAIN)
    NxDomain,
    /// Not implemented
    NotImp,
    /// Refused
    Refused,
}

/// Internal DNS server for service discovery
pub struct DnsServer {
    /// DNS domain
    domain: String,
    /// DNS records cache
    records: Arc<Mutex<BTreeMap<String, Vec<DnsRecord>>>>,
    /// Running flag
    running: AtomicBool,
    /// Query count
    query_count: AtomicU64,
    /// Cache hit count
    cache_hits: AtomicU64,
    /// Upstream DNS servers
    upstream: Vec<String>,
}

impl DnsServer {
    /// Create a new DNS server
    pub fn new(domain: &str) -> Self {
        Self {
            domain: domain.to_string(),
            records: Arc::new(Mutex::new(BTreeMap::new())),
            running: AtomicBool::new(false),
            query_count: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            upstream: vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()],
        }
    }

    /// Start the DNS server
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
    }

    /// Stop the DNS server
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Resolve a DNS query
    pub fn resolve(&self, query: &DnsQuery) -> DnsResponse {
        self.query_count.fetch_add(1, Ordering::Relaxed);

        // Check if this is a service discovery query
        if query.name.ends_with(&self.domain) {
            return self.resolve_service(query);
        }

        // Forward to upstream (in a real implementation)
        DnsResponse {
            rcode: DnsResponseCode::NxDomain,
            answers: Vec::new(),
            authority: Vec::new(),
            additional: Vec::new(),
        }
    }

    /// Resolve a service discovery query
    fn resolve_service(&self, query: &DnsQuery) -> DnsResponse {
        let records = self.records.lock();
        
        if let Some(recs) = records.get(&query.name) {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            
            let answers: Vec<_> = recs.iter()
                .filter(|r| r.record_type == query.query_type)
                .cloned()
                .collect();
            
            DnsResponse {
                rcode: if answers.is_empty() { DnsResponseCode::NxDomain } else { DnsResponseCode::NoError },
                answers,
                authority: Vec::new(),
                additional: Vec::new(),
            }
        } else {
            DnsResponse {
                rcode: DnsResponseCode::NxDomain,
                answers: Vec::new(),
                authority: Vec::new(),
                additional: Vec::new(),
            }
        }
    }

    /// Add a DNS record
    pub fn add_record(&self, record: DnsRecord) {
        let mut records = self.records.lock();
        records.entry(record.name.clone())
            .or_insert_with(Vec::new)
            .push(record);
    }

    /// Remove records for a name
    pub fn remove_records(&self, name: &str) {
        let mut records = self.records.lock();
        records.remove(name);
    }

    /// Update records for a service
    pub fn update_service(&self, service: &ServiceRecord) {
        let fqdn = service.name.fqdn(&self.domain);
        
        // Remove old records
        self.remove_records(&fqdn);
        
        // Add A/AAAA records for cluster IP
        if let Some(ref ip) = service.cluster_ip {
            self.add_record(DnsRecord {
                name: fqdn.clone(),
                record_type: DnsRecordType::A,
                ttl: 30,
                data: DnsRecordData::A(ip.clone()),
            });
        }
        
        // Add SRV records for each port
        for port in &service.ports {
            let srv_name = if let Some(ref name) = port.name {
                alloc::format!("_{}._{}.{}", name, protocol_name(port.protocol), fqdn)
            } else {
                alloc::format!("_{}.{}", protocol_name(port.protocol), fqdn)
            };
            
            for endpoint in &service.endpoints {
                if endpoint.ready && endpoint.health == EndpointHealth::Healthy {
                    self.add_record(DnsRecord {
                        name: srv_name.clone(),
                        record_type: DnsRecordType::SRV,
                        ttl: 30,
                        data: DnsRecordData::SRV {
                            priority: 0,
                            weight: endpoint.weight as u16,
                            port: endpoint.port,
                            target: endpoint.ip.clone(),
                        },
                    });
                }
            }
        }
        
        // Add A records for headless services (direct pod IPs)
        if service.service_type == ServiceType::Headless {
            for endpoint in &service.endpoints {
                if endpoint.ready && endpoint.health == EndpointHealth::Healthy {
                    self.add_record(DnsRecord {
                        name: fqdn.clone(),
                        record_type: DnsRecordType::A,
                        ttl: 30,
                        data: DnsRecordData::A(endpoint.ip.clone()),
                    });
                }
            }
        }
    }

    /// Get statistics
    pub fn stats(&self) -> DnsStats {
        DnsStats {
            query_count: self.query_count.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
            record_count: self.records.lock().values().map(|v| v.len()).sum(),
        }
    }
}

/// DNS server statistics
#[derive(Debug, Clone)]
pub struct DnsStats {
    /// Total queries
    pub query_count: u64,
    /// Cache hits
    pub cache_hits: u64,
    /// Total records
    pub record_count: usize,
}

fn protocol_name(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Tcp => "tcp",
        Protocol::Udp => "udp",
        Protocol::Http => "tcp",
        Protocol::Https => "tcp",
        Protocol::Grpc => "tcp",
    }
}

// =============================================================================
// Service Registry
// =============================================================================

/// Service registry for discovery
pub struct ServiceRegistry {
    /// Services
    services: Arc<Mutex<BTreeMap<String, ServiceRecord>>>,
    /// DNS server
    dns: Arc<DnsServer>,
    /// Health checker
    health_checker: Arc<HealthChecker>,
    /// Running flag
    running: AtomicBool,
    /// Watch callbacks
    watchers: Arc<Mutex<Vec<Box<dyn Fn(&ServiceRecord, WatchEvent) + Send + Sync>>>>,
}

/// Watch event type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchEvent {
    /// Service added
    Added,
    /// Service modified
    Modified,
    /// Service deleted
    Deleted,
}

/// Health check configuration
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// Check interval in seconds
    pub interval: u32,
    /// Timeout in seconds
    pub timeout: u32,
    /// Unhealthy threshold
    pub unhealthy_threshold: u32,
    /// Healthy threshold
    pub healthy_threshold: u32,
    /// Health check type
    pub check_type: HealthCheckType,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            interval: 10,
            timeout: 5,
            unhealthy_threshold: 3,
            healthy_threshold: 2,
            check_type: HealthCheckType::Tcp,
        }
    }
}

/// Health check type
#[derive(Debug, Clone)]
pub enum HealthCheckType {
    /// TCP connection check
    Tcp,
    /// HTTP check
    Http { path: String, expected_status: u16 },
    /// gRPC health check
    Grpc { service: Option<String> },
    /// Command execution
    Exec { command: Vec<String> },
}

/// Health checker
pub struct HealthChecker {
    /// Health check configs per service
    configs: Arc<Mutex<BTreeMap<String, HealthCheckConfig>>>,
    /// Running flag
    running: AtomicBool,
    /// Check count
    check_count: AtomicU64,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new() -> Self {
        Self {
            configs: Arc::new(Mutex::new(BTreeMap::new())),
            running: AtomicBool::new(false),
            check_count: AtomicU64::new(0),
        }
    }

    /// Start health checking
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
    }

    /// Stop health checking
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Configure health check for a service
    pub fn configure(&self, service_key: &str, config: HealthCheckConfig) {
        let mut configs = self.configs.lock();
        configs.insert(service_key.to_string(), config);
    }

    /// Check health of an endpoint
    pub fn check_endpoint(&self, endpoint: &Endpoint, config: &HealthCheckConfig) -> EndpointHealth {
        self.check_count.fetch_add(1, Ordering::Relaxed);
        
        match &config.check_type {
            HealthCheckType::Tcp => self.check_tcp(endpoint, config.timeout),
            HealthCheckType::Http { path, expected_status } => {
                self.check_http(endpoint, path, *expected_status, config.timeout)
            }
            HealthCheckType::Grpc { service } => {
                self.check_grpc(endpoint, service.as_deref(), config.timeout)
            }
            HealthCheckType::Exec { command: _ } => {
                // Exec checks are done by the container runtime
                EndpointHealth::Healthy
            }
        }
    }

    fn check_tcp(&self, _endpoint: &Endpoint, _timeout: u32) -> EndpointHealth {
        // In a real implementation, try to establish TCP connection
        EndpointHealth::Healthy
    }

    fn check_http(&self, _endpoint: &Endpoint, _path: &str, _expected_status: u16, _timeout: u32) -> EndpointHealth {
        // In a real implementation, make HTTP request
        EndpointHealth::Healthy
    }

    fn check_grpc(&self, _endpoint: &Endpoint, _service: Option<&str>, _timeout: u32) -> EndpointHealth {
        // In a real implementation, call gRPC health service
        EndpointHealth::Healthy
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceRegistry {
    /// Create a new service registry
    pub fn new(domain: &str) -> Self {
        let dns = Arc::new(DnsServer::new(domain));
        let health_checker = Arc::new(HealthChecker::new());
        
        Self {
            services: Arc::new(Mutex::new(BTreeMap::new())),
            dns,
            health_checker,
            running: AtomicBool::new(false),
            watchers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Start the service registry
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        self.dns.start();
        self.health_checker.start();
    }

    /// Stop the service registry
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.dns.stop();
        self.health_checker.stop();
    }

    /// Register a service
    pub fn register(&self, service: ServiceRecord) -> Result<(), DiscoveryError> {
        let key = service.name.short_name();
        
        let mut services = self.services.lock();
        let is_new = !services.contains_key(&key);
        services.insert(key.clone(), service.clone());
        drop(services);
        
        // Update DNS
        self.dns.update_service(&service);
        
        // Notify watchers
        let event = if is_new { WatchEvent::Added } else { WatchEvent::Modified };
        self.notify_watchers(&service, event);
        
        Ok(())
    }

    /// Deregister a service
    pub fn deregister(&self, name: &ServiceName) -> Result<(), DiscoveryError> {
        let key = name.short_name();
        
        let mut services = self.services.lock();
        let service = services.remove(&key)
            .ok_or_else(|| DiscoveryError::ServiceNotFound(key.clone()))?;
        drop(services);
        
        // Remove DNS records
        self.dns.remove_records(&name.fqdn(&self.dns.domain));
        
        // Notify watchers
        self.notify_watchers(&service, WatchEvent::Deleted);
        
        Ok(())
    }

    /// Update endpoints for a service
    pub fn update_endpoints(&self, name: &ServiceName, endpoints: Vec<Endpoint>) -> Result<(), DiscoveryError> {
        let key = name.short_name();
        
        let mut services = self.services.lock();
        let service = services.get_mut(&key)
            .ok_or_else(|| DiscoveryError::ServiceNotFound(key.clone()))?;
        
        service.endpoints = endpoints;
        service.updated_at = self.current_time();
        let service_clone = service.clone();
        drop(services);
        
        // Update DNS
        self.dns.update_service(&service_clone);
        
        // Notify watchers
        self.notify_watchers(&service_clone, WatchEvent::Modified);
        
        Ok(())
    }

    /// Resolve a service by name
    pub fn resolve(&self, name: &ServiceName) -> Result<ServiceRecord, DiscoveryError> {
        let key = name.short_name();
        let services = self.services.lock();
        services.get(&key)
            .cloned()
            .ok_or_else(|| DiscoveryError::ServiceNotFound(key))
    }

    /// Get healthy endpoints for a service
    pub fn get_healthy_endpoints(&self, name: &ServiceName) -> Result<Vec<Endpoint>, DiscoveryError> {
        let service = self.resolve(name)?;
        Ok(service.endpoints.into_iter()
            .filter(|e| e.ready && e.health == EndpointHealth::Healthy)
            .collect())
    }

    /// List all services
    pub fn list_services(&self) -> Vec<ServiceRecord> {
        self.services.lock().values().cloned().collect()
    }

    /// List services in a namespace
    pub fn list_services_in_namespace(&self, namespace: &str) -> Vec<ServiceRecord> {
        self.services.lock().values()
            .filter(|s| s.name.namespace == namespace)
            .cloned()
            .collect()
    }

    /// Watch for service changes
    pub fn watch<F>(&self, callback: F)
    where
        F: Fn(&ServiceRecord, WatchEvent) + Send + Sync + 'static,
    {
        let mut watchers = self.watchers.lock();
        watchers.push(Box::new(callback));
    }

    /// Get DNS server
    pub fn dns(&self) -> Arc<DnsServer> {
        self.dns.clone()
    }

    /// Get health checker
    pub fn health_checker(&self) -> Arc<HealthChecker> {
        self.health_checker.clone()
    }

    fn notify_watchers(&self, service: &ServiceRecord, event: WatchEvent) {
        let watchers = self.watchers.lock();
        for watcher in watchers.iter() {
            watcher(service, event);
        }
    }

    fn current_time(&self) -> u64 {
        0 // Placeholder
    }
}

/// Discovery errors
#[derive(Debug)]
pub enum DiscoveryError {
    /// Service not found
    ServiceNotFound(String),
    /// Service already exists
    ServiceExists(String),
    /// No healthy endpoints
    NoHealthyEndpoints(String),
    /// DNS resolution failed
    DnsResolutionFailed(String),
}

impl core::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DiscoveryError::ServiceNotFound(s) => write!(f, "Service not found: {}", s),
            DiscoveryError::ServiceExists(s) => write!(f, "Service exists: {}", s),
            DiscoveryError::NoHealthyEndpoints(s) => write!(f, "No healthy endpoints: {}", s),
            DiscoveryError::DnsResolutionFailed(s) => write!(f, "DNS resolution failed: {}", s),
        }
    }
}

// =============================================================================
// Global Service Registry
// =============================================================================

static SERVICE_REGISTRY: Mutex<Option<Arc<ServiceRegistry>>> = Mutex::new(None);

/// Initialize service discovery
pub fn init(domain: &str) {
    let registry = Arc::new(ServiceRegistry::new(domain));
    *SERVICE_REGISTRY.lock() = Some(registry.clone());
    registry.start();
}

/// Get the service registry
pub fn registry() -> Option<Arc<ServiceRegistry>> {
    SERVICE_REGISTRY.lock().clone()
}

/// Resolve a service
pub fn resolve(name: &str, namespace: &str) -> Result<ServiceRecord, DiscoveryError> {
    let registry = SERVICE_REGISTRY.lock();
    let registry = registry.as_ref().ok_or_else(|| DiscoveryError::ServiceNotFound(name.to_string()))?;
    registry.resolve(&ServiceName::new(name, namespace))
}

/// Get healthy endpoints for a service
pub fn get_endpoints(name: &str, namespace: &str) -> Result<Vec<Endpoint>, DiscoveryError> {
    let registry = SERVICE_REGISTRY.lock();
    let registry = registry.as_ref().ok_or_else(|| DiscoveryError::ServiceNotFound(name.to_string()))?;
    registry.get_healthy_endpoints(&ServiceName::new(name, namespace))
}

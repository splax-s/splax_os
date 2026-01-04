//! # Load Balancing
//!
//! Advanced load balancing for Splax OS services.
//!
//! ## Features
//!
//! - **Multiple Algorithms**: Round-robin, weighted, least connections, random, hash
//! - **Health-aware**: Automatic failover to healthy backends
//! - **Session Affinity**: Sticky sessions by IP or cookie
//! - **Zone-aware**: Prefer local zone backends
//! - **Circuit Breaking**: Prevent cascade failures
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                       Load Balancer                             │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
//! │  │  Algorithm  │  │   Health    │  │   Circuit   │              │
//! │  │   Selector  │  │   Monitor   │  │   Breaker   │              │
//! │  └─────────────┘  └─────────────┘  └─────────────┘              │
//! └─────────────────────────────────────────────────────────────────┘
//!                                │
//!                     ┌──────────┴──────────┐
//!                     ▼                     ▼
//!              ┌───────────┐         ┌───────────┐
//!              │ Backend 1 │   ...   │ Backend N │
//!              └───────────┘         └───────────┘
//! ```

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// =============================================================================
// Backend Definition
// =============================================================================

/// Backend server
#[derive(Debug, Clone)]
pub struct Backend {
    /// Backend ID
    pub id: String,
    /// Address (IP:port)
    pub address: String,
    /// Weight (for weighted algorithms)
    pub weight: u32,
    /// Maximum connections (0 = unlimited)
    pub max_connections: u32,
    /// Current connection count
    pub current_connections: AtomicU32,
    /// Health status
    pub healthy: AtomicBool,
    /// Zone for zone-aware balancing
    pub zone: Option<String>,
    /// Metadata
    pub metadata: BTreeMap<String, String>,
}

impl Backend {
    /// Create a new backend
    pub fn new(id: &str, address: &str) -> Self {
        Self {
            id: id.to_string(),
            address: address.to_string(),
            weight: 100,
            max_connections: 0,
            current_connections: AtomicU32::new(0),
            healthy: AtomicBool::new(true),
            zone: None,
            metadata: BTreeMap::new(),
        }
    }

    /// Set weight
    pub fn with_weight(mut self, weight: u32) -> Self {
        self.weight = weight;
        self
    }

    /// Set zone
    pub fn with_zone(mut self, zone: &str) -> Self {
        self.zone = Some(zone.to_string());
        self
    }

    /// Set max connections
    pub fn with_max_connections(mut self, max: u32) -> Self {
        self.max_connections = max;
        self
    }

    /// Check if backend is available
    pub fn is_available(&self) -> bool {
        if !self.healthy.load(Ordering::Relaxed) {
            return false;
        }
        if self.max_connections > 0 {
            return self.current_connections.load(Ordering::Relaxed) < self.max_connections;
        }
        true
    }

    /// Acquire a connection
    pub fn acquire(&self) -> bool {
        if self.max_connections > 0 {
            let current = self.current_connections.fetch_add(1, Ordering::Relaxed);
            if current >= self.max_connections {
                self.current_connections.fetch_sub(1, Ordering::Relaxed);
                return false;
            }
        } else {
            self.current_connections.fetch_add(1, Ordering::Relaxed);
        }
        true
    }

    /// Release a connection
    pub fn release(&self) {
        self.current_connections.fetch_sub(1, Ordering::Relaxed);
    }
}

// =============================================================================
// Load Balancing Algorithms
// =============================================================================

/// Load balancing algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// Round-robin
    RoundRobin,
    /// Weighted round-robin
    WeightedRoundRobin,
    /// Least connections
    LeastConnections,
    /// Weighted least connections
    WeightedLeastConnections,
    /// Random
    Random,
    /// Consistent hash
    ConsistentHash,
    /// IP hash
    IpHash,
    /// Least response time
    LeastResponseTime,
}

impl Default for Algorithm {
    fn default() -> Self {
        Algorithm::RoundRobin
    }
}

/// Round-robin state
pub struct RoundRobinState {
    /// Current index
    index: AtomicU64,
}

impl RoundRobinState {
    /// Create new round-robin state
    pub fn new() -> Self {
        Self {
            index: AtomicU64::new(0),
        }
    }

    /// Get next backend index
    pub fn next(&self, count: usize) -> usize {
        if count == 0 {
            return 0;
        }
        (self.index.fetch_add(1, Ordering::Relaxed) as usize) % count
    }
}

impl Default for RoundRobinState {
    fn default() -> Self {
        Self::new()
    }
}

/// Weighted round-robin state
pub struct WeightedRoundRobinState {
    /// Current weight
    current_weight: AtomicU32,
    /// Current index
    current_index: AtomicU64,
    /// GCD of weights
    gcd: AtomicU32,
    /// Max weight
    max_weight: AtomicU32,
}

impl WeightedRoundRobinState {
    /// Create new state
    pub fn new() -> Self {
        Self {
            current_weight: AtomicU32::new(0),
            current_index: AtomicU64::new(0),
            gcd: AtomicU32::new(1),
            max_weight: AtomicU32::new(100),
        }
    }

    /// Update weights
    pub fn update_weights(&self, backends: &[Backend]) {
        if backends.is_empty() {
            return;
        }
        
        let max = backends.iter().map(|b| b.weight).max().unwrap_or(100);
        self.max_weight.store(max, Ordering::Relaxed);
        
        let gcd = backends.iter().map(|b| b.weight).fold(0, gcd_fn);
        self.gcd.store(if gcd == 0 { 1 } else { gcd }, Ordering::Relaxed);
    }

    /// Get next backend
    pub fn next(&self, backends: &[Backend]) -> Option<usize> {
        let n = backends.len();
        if n == 0 {
            return None;
        }

        let gcd = self.gcd.load(Ordering::Relaxed);
        let max_weight = self.max_weight.load(Ordering::Relaxed);

        for _ in 0..n {
            let idx = (self.current_index.fetch_add(1, Ordering::Relaxed) as usize) % n;
            
            if idx == 0 {
                let cw = self.current_weight.load(Ordering::Relaxed);
                let new_cw = if cw <= gcd { max_weight } else { cw - gcd };
                self.current_weight.store(new_cw, Ordering::Relaxed);
            }

            let cw = self.current_weight.load(Ordering::Relaxed);
            if backends[idx].weight >= cw && backends[idx].is_available() {
                return Some(idx);
            }
        }

        // Fallback to first available
        backends.iter().position(|b| b.is_available())
    }
}

impl Default for WeightedRoundRobinState {
    fn default() -> Self {
        Self::new()
    }
}

fn gcd_fn(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd_fn(b, a % b) }
}

/// Consistent hash ring
pub struct ConsistentHashRing {
    /// Ring nodes (hash -> backend index)
    ring: Mutex<Vec<(u64, usize)>>,
    /// Virtual nodes per backend
    virtual_nodes: u32,
}

impl ConsistentHashRing {
    /// Create a new hash ring
    pub fn new(virtual_nodes: u32) -> Self {
        Self {
            ring: Mutex::new(Vec::new()),
            virtual_nodes,
        }
    }

    /// Build the ring from backends
    pub fn build(&self, backends: &[Backend]) {
        let mut ring = self.ring.lock();
        ring.clear();

        for (idx, backend) in backends.iter().enumerate() {
            for vn in 0..self.virtual_nodes {
                let key = alloc::format!("{}:{}", backend.id, vn);
                let hash = self.hash(&key);
                ring.push((hash, idx));
            }
        }

        ring.sort_by_key(|(hash, _)| *hash);
    }

    /// Get backend for a key
    pub fn get(&self, key: &str, backends: &[Backend]) -> Option<usize> {
        let ring = self.ring.lock();
        if ring.is_empty() {
            return None;
        }

        let hash = self.hash(key);
        
        // Binary search for the first node with hash >= key hash
        let idx = match ring.binary_search_by_key(&hash, |(h, _)| *h) {
            Ok(i) => i,
            Err(i) => if i >= ring.len() { 0 } else { i },
        };

        // Find first available backend starting from idx
        let n = ring.len();
        for i in 0..n {
            let backend_idx = ring[(idx + i) % n].1;
            if backends[backend_idx].is_available() {
                return Some(backend_idx);
            }
        }

        None
    }

    fn hash(&self, key: &str) -> u64 {
        // Simple FNV-1a hash
        let mut hash: u64 = 0xcbf29ce484222325;
        for byte in key.bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }
}

// =============================================================================
// Session Affinity
// =============================================================================

/// Session affinity configuration
#[derive(Debug, Clone)]
pub struct SessionAffinity {
    /// Affinity type
    pub affinity_type: AffinityType,
    /// Timeout in seconds
    pub timeout: u64,
}

impl Default for SessionAffinity {
    fn default() -> Self {
        Self {
            affinity_type: AffinityType::None,
            timeout: 10800, // 3 hours
        }
    }
}

/// Affinity type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AffinityType {
    /// No affinity
    None,
    /// Client IP affinity
    ClientIP,
    /// Cookie-based affinity
    Cookie,
    /// Header-based affinity
    Header,
}

/// Session store
pub struct SessionStore {
    /// Sessions (client key -> backend index)
    sessions: Mutex<BTreeMap<String, (usize, u64)>>,
    /// Timeout
    timeout: u64,
}

impl SessionStore {
    /// Create a new session store
    pub fn new(timeout: u64) -> Self {
        Self {
            sessions: Mutex::new(BTreeMap::new()),
            timeout,
        }
    }

    /// Get or create session
    pub fn get_or_set(&self, key: &str, backend_idx: usize, current_time: u64) -> usize {
        let mut sessions = self.sessions.lock();
        
        if let Some((idx, expires)) = sessions.get(key) {
            if *expires > current_time {
                return *idx;
            }
        }

        let expires = current_time + self.timeout;
        sessions.insert(key.to_string(), (backend_idx, expires));
        backend_idx
    }

    /// Get existing session
    pub fn get(&self, key: &str, current_time: u64) -> Option<usize> {
        let sessions = self.sessions.lock();
        if let Some((idx, expires)) = sessions.get(key) {
            if *expires > current_time {
                return Some(*idx);
            }
        }
        None
    }

    /// Remove session
    pub fn remove(&self, key: &str) {
        let mut sessions = self.sessions.lock();
        sessions.remove(key);
    }

    /// Clean expired sessions
    pub fn cleanup(&self, current_time: u64) {
        let mut sessions = self.sessions.lock();
        sessions.retain(|_, (_, expires)| *expires > current_time);
    }
}

// =============================================================================
// Circuit Breaker
// =============================================================================

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed (normal operation)
    Closed,
    /// Circuit is open (failing fast)
    Open,
    /// Circuit is half-open (testing recovery)
    HalfOpen,
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Failure threshold to open circuit
    pub failure_threshold: u32,
    /// Success threshold to close circuit
    pub success_threshold: u32,
    /// Timeout before trying half-open (seconds)
    pub timeout: u64,
    /// Window size for counting failures
    pub window_size: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout: 30,
            window_size: 60,
        }
    }
}

/// Circuit breaker
pub struct CircuitBreaker {
    /// Current state
    state: Mutex<CircuitState>,
    /// Failure count
    failures: AtomicU32,
    /// Success count (in half-open)
    successes: AtomicU32,
    /// Last failure time
    last_failure: AtomicU64,
    /// Configuration
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Mutex::new(CircuitState::Closed),
            failures: AtomicU32::new(0),
            successes: AtomicU32::new(0),
            last_failure: AtomicU64::new(0),
            config,
        }
    }

    /// Check if request is allowed
    pub fn allow(&self, current_time: u64) -> bool {
        let mut state = self.state.lock();
        
        match *state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                let last = self.last_failure.load(Ordering::Relaxed);
                if current_time > last + self.config.timeout {
                    *state = CircuitState::HalfOpen;
                    self.successes.store(0, Ordering::Relaxed);
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    /// Record success
    pub fn record_success(&self) {
        let mut state = self.state.lock();
        
        match *state {
            CircuitState::Closed => {
                self.failures.store(0, Ordering::Relaxed);
            }
            CircuitState::HalfOpen => {
                let successes = self.successes.fetch_add(1, Ordering::Relaxed) + 1;
                if successes >= self.config.success_threshold {
                    *state = CircuitState::Closed;
                    self.failures.store(0, Ordering::Relaxed);
                }
            }
            CircuitState::Open => {}
        }
    }

    /// Record failure
    pub fn record_failure(&self, current_time: u64) {
        let mut state = self.state.lock();
        
        match *state {
            CircuitState::Closed => {
                let failures = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
                if failures >= self.config.failure_threshold {
                    *state = CircuitState::Open;
                    self.last_failure.store(current_time, Ordering::Relaxed);
                }
            }
            CircuitState::HalfOpen => {
                *state = CircuitState::Open;
                self.last_failure.store(current_time, Ordering::Relaxed);
            }
            CircuitState::Open => {
                self.last_failure.store(current_time, Ordering::Relaxed);
            }
        }
    }

    /// Get current state
    pub fn state(&self) -> CircuitState {
        *self.state.lock()
    }

    /// Reset the circuit breaker
    pub fn reset(&self) {
        let mut state = self.state.lock();
        *state = CircuitState::Closed;
        self.failures.store(0, Ordering::Relaxed);
        self.successes.store(0, Ordering::Relaxed);
    }
}

// =============================================================================
// Load Balancer
// =============================================================================

/// Load balancer configuration
#[derive(Debug, Clone)]
pub struct LoadBalancerConfig {
    /// Load balancing algorithm
    pub algorithm: Algorithm,
    /// Session affinity
    pub session_affinity: SessionAffinity,
    /// Circuit breaker config
    pub circuit_breaker: Option<CircuitBreakerConfig>,
    /// Health check interval
    pub health_check_interval: u64,
    /// Local zone preference (0-100)
    pub zone_preference: u32,
    /// Maximum retries
    pub max_retries: u32,
    /// Retry timeout
    pub retry_timeout: u64,
}

impl Default for LoadBalancerConfig {
    fn default() -> Self {
        Self {
            algorithm: Algorithm::RoundRobin,
            session_affinity: SessionAffinity::default(),
            circuit_breaker: None,
            health_check_interval: 10,
            zone_preference: 0,
            max_retries: 3,
            retry_timeout: 5,
        }
    }
}

/// Load balancer
pub struct LoadBalancer {
    /// Name
    name: String,
    /// Configuration
    config: LoadBalancerConfig,
    /// Backends
    backends: Arc<Mutex<Vec<Backend>>>,
    /// Round-robin state
    rr_state: RoundRobinState,
    /// Weighted round-robin state
    wrr_state: WeightedRoundRobinState,
    /// Consistent hash ring
    hash_ring: ConsistentHashRing,
    /// Session store
    session_store: Option<SessionStore>,
    /// Circuit breakers per backend
    circuit_breakers: Arc<Mutex<BTreeMap<String, CircuitBreaker>>>,
    /// Local zone
    local_zone: Option<String>,
    /// Statistics
    stats: LoadBalancerStats,
}

/// Load balancer statistics
pub struct LoadBalancerStats {
    /// Total requests
    pub total_requests: AtomicU64,
    /// Successful requests
    pub successful_requests: AtomicU64,
    /// Failed requests
    pub failed_requests: AtomicU64,
    /// Active connections
    pub active_connections: AtomicU32,
    /// Circuit breaker trips
    pub circuit_breaker_trips: AtomicU64,
}

impl Default for LoadBalancerStats {
    fn default() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            active_connections: AtomicU32::new(0),
            circuit_breaker_trips: AtomicU64::new(0),
        }
    }
}

/// Request context for load balancing decisions
pub struct RequestContext {
    /// Client IP
    pub client_ip: Option<String>,
    /// Request path
    pub path: Option<String>,
    /// Headers
    pub headers: BTreeMap<String, String>,
    /// Current timestamp
    pub timestamp: u64,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self {
            client_ip: None,
            path: None,
            headers: BTreeMap::new(),
            timestamp: 0,
        }
    }
}

/// Selected backend result
#[derive(Debug, Clone)]
pub struct SelectedBackend {
    /// Backend index
    pub index: usize,
    /// Backend address
    pub address: String,
    /// Backend ID
    pub id: String,
}

impl LoadBalancer {
    /// Create a new load balancer
    pub fn new(name: &str, config: LoadBalancerConfig) -> Self {
        let session_store = if config.session_affinity.affinity_type != AffinityType::None {
            Some(SessionStore::new(config.session_affinity.timeout))
        } else {
            None
        };

        Self {
            name: name.to_string(),
            config,
            backends: Arc::new(Mutex::new(Vec::new())),
            rr_state: RoundRobinState::new(),
            wrr_state: WeightedRoundRobinState::new(),
            hash_ring: ConsistentHashRing::new(150),
            session_store,
            circuit_breakers: Arc::new(Mutex::new(BTreeMap::new())),
            local_zone: None,
            stats: LoadBalancerStats::default(),
        }
    }

    /// Set local zone for zone-aware balancing
    pub fn set_local_zone(&mut self, zone: &str) {
        self.local_zone = Some(zone.to_string());
    }

    /// Add a backend
    pub fn add_backend(&self, backend: Backend) {
        let mut backends = self.backends.lock();
        
        // Add circuit breaker if configured
        if let Some(ref cb_config) = self.config.circuit_breaker {
            let mut cbs = self.circuit_breakers.lock();
            cbs.insert(backend.id.clone(), CircuitBreaker::new(cb_config.clone()));
        }
        
        backends.push(backend);
        
        // Update algorithm states
        self.wrr_state.update_weights(&backends);
        self.hash_ring.build(&backends);
    }

    /// Remove a backend
    pub fn remove_backend(&self, id: &str) {
        let mut backends = self.backends.lock();
        backends.retain(|b| b.id != id);
        
        let mut cbs = self.circuit_breakers.lock();
        cbs.remove(id);
        
        self.wrr_state.update_weights(&backends);
        self.hash_ring.build(&backends);
    }

    /// Update backend health
    pub fn set_backend_health(&self, id: &str, healthy: bool) {
        let backends = self.backends.lock();
        if let Some(backend) = backends.iter().find(|b| b.id == id) {
            backend.healthy.store(healthy, Ordering::Relaxed);
        }
    }

    /// Select a backend for a request
    pub fn select(&self, ctx: &RequestContext) -> Result<SelectedBackend, LoadBalancerError> {
        self.stats.total_requests.fetch_add(1, Ordering::Relaxed);

        let backends = self.backends.lock();
        if backends.is_empty() {
            return Err(LoadBalancerError::NoBackends);
        }

        // Check session affinity first
        if let Some(ref store) = self.session_store {
            if let Some(key) = self.get_affinity_key(ctx) {
                if let Some(idx) = store.get(&key, ctx.timestamp) {
                    if idx < backends.len() && backends[idx].is_available() {
                        return Ok(SelectedBackend {
                            index: idx,
                            address: backends[idx].address.clone(),
                            id: backends[idx].id.clone(),
                        });
                    }
                }
            }
        }

        // Filter backends by circuit breaker
        let available: Vec<_> = backends.iter().enumerate()
            .filter(|(_, b)| {
                if !b.is_available() {
                    return false;
                }
                if let Some(ref cb_config) = self.config.circuit_breaker {
                    let _ = cb_config; // silence unused warning
                    let cbs = self.circuit_breakers.lock();
                    if let Some(cb) = cbs.get(&b.id) {
                        return cb.allow(ctx.timestamp);
                    }
                }
                true
            })
            .collect();

        if available.is_empty() {
            return Err(LoadBalancerError::NoHealthyBackends);
        }

        // Zone-aware filtering
        let zone_filtered = if self.config.zone_preference > 0 {
            if let Some(ref local_zone) = self.local_zone {
                let local: Vec<_> = available.iter()
                    .filter(|(_, b)| b.zone.as_ref() == Some(local_zone))
                    .cloned()
                    .collect();
                if !local.is_empty() {
                    local
                } else {
                    available.clone()
                }
            } else {
                available.clone()
            }
        } else {
            available.clone()
        };

        // Select based on algorithm
        let idx = self.select_by_algorithm(&zone_filtered, ctx, &backends)?;
        let backend = &backends[idx];

        // Store session if affinity is enabled
        if let Some(ref store) = self.session_store {
            if let Some(key) = self.get_affinity_key(ctx) {
                store.get_or_set(&key, idx, ctx.timestamp);
            }
        }

        // Acquire connection
        if !backend.acquire() {
            return Err(LoadBalancerError::BackendOverloaded(backend.id.clone()));
        }

        self.stats.active_connections.fetch_add(1, Ordering::Relaxed);

        Ok(SelectedBackend {
            index: idx,
            address: backend.address.clone(),
            id: backend.id.clone(),
        })
    }

    /// Record request completion
    pub fn complete(&self, backend_id: &str, success: bool, current_time: u64) {
        let backends = self.backends.lock();
        if let Some(backend) = backends.iter().find(|b| b.id == backend_id) {
            backend.release();
        }
        drop(backends);

        self.stats.active_connections.fetch_sub(1, Ordering::Relaxed);

        if success {
            self.stats.successful_requests.fetch_add(1, Ordering::Relaxed);
            
            if self.config.circuit_breaker.is_some() {
                let cbs = self.circuit_breakers.lock();
                if let Some(cb) = cbs.get(backend_id) {
                    cb.record_success();
                }
            }
        } else {
            self.stats.failed_requests.fetch_add(1, Ordering::Relaxed);
            
            if self.config.circuit_breaker.is_some() {
                let cbs = self.circuit_breakers.lock();
                if let Some(cb) = cbs.get(backend_id) {
                    cb.record_failure(current_time);
                }
            }
        }
    }

    /// Get statistics
    pub fn stats(&self) -> LoadBalancerSnapshot {
        LoadBalancerSnapshot {
            total_requests: self.stats.total_requests.load(Ordering::Relaxed),
            successful_requests: self.stats.successful_requests.load(Ordering::Relaxed),
            failed_requests: self.stats.failed_requests.load(Ordering::Relaxed),
            active_connections: self.stats.active_connections.load(Ordering::Relaxed),
            circuit_breaker_trips: self.stats.circuit_breaker_trips.load(Ordering::Relaxed),
            backend_count: self.backends.lock().len(),
            healthy_backend_count: self.backends.lock().iter().filter(|b| b.is_available()).count(),
        }
    }

    fn get_affinity_key(&self, ctx: &RequestContext) -> Option<String> {
        match self.config.session_affinity.affinity_type {
            AffinityType::None => None,
            AffinityType::ClientIP => ctx.client_ip.clone(),
            AffinityType::Cookie => ctx.headers.get("cookie")
                .and_then(|c| self.extract_session_cookie(c)),
            AffinityType::Header => ctx.headers.get("x-session-id").cloned(),
        }
    }

    fn extract_session_cookie(&self, _cookie: &str) -> Option<String> {
        // Parse cookie header and extract session cookie
        None // Placeholder
    }

    fn select_by_algorithm(
        &self,
        available: &[(usize, &Backend)],
        ctx: &RequestContext,
        all_backends: &[Backend],
    ) -> Result<usize, LoadBalancerError> {
        if available.is_empty() {
            return Err(LoadBalancerError::NoHealthyBackends);
        }

        let idx = match self.config.algorithm {
            Algorithm::RoundRobin => {
                let pos = self.rr_state.next(available.len());
                available[pos].0
            }
            Algorithm::WeightedRoundRobin => {
                self.wrr_state.next(all_backends)
                    .ok_or(LoadBalancerError::NoHealthyBackends)?
            }
            Algorithm::LeastConnections => {
                available.iter()
                    .min_by_key(|(_, b)| b.current_connections.load(Ordering::Relaxed))
                    .map(|(i, _)| *i)
                    .ok_or(LoadBalancerError::NoHealthyBackends)?
            }
            Algorithm::WeightedLeastConnections => {
                available.iter()
                    .min_by_key(|(_, b)| {
                        let conns = b.current_connections.load(Ordering::Relaxed);
                        if b.weight == 0 { u32::MAX } else { conns * 100 / b.weight }
                    })
                    .map(|(i, _)| *i)
                    .ok_or(LoadBalancerError::NoHealthyBackends)?
            }
            Algorithm::Random => {
                // Simple pseudo-random based on timestamp
                let idx = (ctx.timestamp as usize) % available.len();
                available[idx].0
            }
            Algorithm::ConsistentHash | Algorithm::IpHash => {
                let key = ctx.client_ip.as_ref()
                    .or(ctx.path.as_ref())
                    .map(|s| s.as_str())
                    .unwrap_or("default");
                self.hash_ring.get(key, all_backends)
                    .ok_or(LoadBalancerError::NoHealthyBackends)?
            }
            Algorithm::LeastResponseTime => {
                // Would need response time tracking - fallback to least connections
                available.iter()
                    .min_by_key(|(_, b)| b.current_connections.load(Ordering::Relaxed))
                    .map(|(i, _)| *i)
                    .ok_or(LoadBalancerError::NoHealthyBackends)?
            }
        };

        Ok(idx)
    }
}

/// Load balancer statistics snapshot
#[derive(Debug, Clone)]
pub struct LoadBalancerSnapshot {
    /// Total requests
    pub total_requests: u64,
    /// Successful requests
    pub successful_requests: u64,
    /// Failed requests
    pub failed_requests: u64,
    /// Active connections
    pub active_connections: u32,
    /// Circuit breaker trips
    pub circuit_breaker_trips: u64,
    /// Total backend count
    pub backend_count: usize,
    /// Healthy backend count
    pub healthy_backend_count: usize,
}

/// Load balancer errors
#[derive(Debug)]
pub enum LoadBalancerError {
    /// No backends configured
    NoBackends,
    /// No healthy backends available
    NoHealthyBackends,
    /// Backend overloaded
    BackendOverloaded(String),
    /// Circuit breaker open
    CircuitBreakerOpen(String),
    /// Request timeout
    Timeout,
}

impl core::fmt::Display for LoadBalancerError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LoadBalancerError::NoBackends => write!(f, "No backends configured"),
            LoadBalancerError::NoHealthyBackends => write!(f, "No healthy backends available"),
            LoadBalancerError::BackendOverloaded(id) => write!(f, "Backend overloaded: {}", id),
            LoadBalancerError::CircuitBreakerOpen(id) => write!(f, "Circuit breaker open: {}", id),
            LoadBalancerError::Timeout => write!(f, "Request timeout"),
        }
    }
}

// =============================================================================
// Global Load Balancer Registry
// =============================================================================

static LOAD_BALANCERS: Mutex<BTreeMap<String, Arc<LoadBalancer>>> = Mutex::new(BTreeMap::new());

/// Create a load balancer
pub fn create(name: &str, config: LoadBalancerConfig) -> Arc<LoadBalancer> {
    let lb = Arc::new(LoadBalancer::new(name, config));
    LOAD_BALANCERS.lock().insert(name.to_string(), lb.clone());
    lb
}

/// Get a load balancer
pub fn get(name: &str) -> Option<Arc<LoadBalancer>> {
    LOAD_BALANCERS.lock().get(name).cloned()
}

/// Remove a load balancer
pub fn remove(name: &str) {
    LOAD_BALANCERS.lock().remove(name);
}

/// List all load balancers
pub fn list() -> Vec<String> {
    LOAD_BALANCERS.lock().keys().cloned().collect()
}

/// Initialize load balancing module
pub fn init() {
    // Module initialized
}

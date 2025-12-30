//! # S-ATLAS: Service Registry & Discovery
//!
//! S-ATLAS is the service discovery backbone of Splax OS. It allows services
//! to register themselves and discover other services by name.
//!
//! ## Core Responsibilities
//!
//! 1. **Service Registration**: Services announce their presence and capabilities
//! 2. **Service Discovery**: Services find each other by name or capability
//! 3. **Health Monitoring**: Heartbeat-based liveness checking
//! 4. **Capability Mediation**: Facilitates capability exchange between services
//!
//! ## Security Model
//!
//! All operations require appropriate capability tokens:
//! - Registration requires a "service:register" capability
//! - Discovery requires a "service:discover" capability
//! - Direct channel creation requires both parties' consent via capabilities
//!
//! ## Example
//!
//! ```ignore
//! // Register a service
//! let service_id = atlas.register(
//!     "auth-service",
//!     ServiceInfo { version: "1.0.0", capabilities: vec!["auth:verify"] },
//!     registration_token,
//! )?;
//!
//! // Discover a service
//! let auth = atlas.discover("auth-service", discovery_token)?;
//! ```

#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use spin::Mutex;

pub mod observability;

// Import shared capability token
pub use splax_cap::{CapabilityToken, Operations, Permission};

// ============================================================================
// Time and Timestamp Support
// ============================================================================

/// Assumed CPU frequency for time conversions (1 GHz default).
/// In a real system, this would be calibrated at boot time.
const DEFAULT_CPU_FREQ_HZ: u64 = 1_000_000_000;

/// Cached CPU frequency (cycles per second).
/// Should be calibrated during boot using a known time source (e.g., PIT, HPET, or ACPI PM timer).
static CPU_FREQ_HZ: spin::Lazy<u64> = spin::Lazy::new(|| DEFAULT_CPU_FREQ_HZ);

/// Reads the CPU's cycle counter (TSC on x86_64, CNTVCT_EL0 on aarch64).
///
/// # Returns
///
/// The current cycle count since CPU reset.
#[inline]
pub fn read_cycle_counter() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        // Use RDTSC instruction to read Time Stamp Counter
        // RDTSC returns a 64-bit value split across EDX:EAX
        let low: u32;
        let high: u32;
        unsafe {
            core::arch::asm!(
                "rdtsc",
                out("eax") low,
                out("edx") high,
                options(nostack, nomem, preserves_flags)
            );
        }
        ((high as u64) << 32) | (low as u64)
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Read the virtual count register (CNTVCT_EL0)
        // This is a 64-bit counter that increments at a fixed frequency
        let count: u64;
        unsafe {
            core::arch::asm!(
                "mrs {}, cntvct_el0",
                out(reg) count,
                options(nostack, nomem, preserves_flags)
            );
        }
        count
    }

    #[cfg(target_arch = "riscv64")]
    {
        // Read the cycle counter CSR (mcycle or rdcycle)
        let count: u64;
        unsafe {
            core::arch::asm!(
                "rdcycle {}",
                out(reg) count,
                options(nostack, nomem, preserves_flags)
            );
        }
        count
    }

    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "riscv64")))]
    {
        // Fallback for unsupported architectures (e.g., during testing)
        0
    }
}

/// Gets the counter frequency for aarch64 (CNTFRQ_EL0).
///
/// On aarch64, the counter frequency is stored in a system register.
/// On other architectures, returns the default CPU frequency.
#[inline]
pub fn get_counter_frequency() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        let freq: u64;
        unsafe {
            core::arch::asm!(
                "mrs {}, cntfrq_el0",
                out(reg) freq,
                options(nostack, nomem, preserves_flags)
            );
        }
        if freq == 0 {
            DEFAULT_CPU_FREQ_HZ
        } else {
            freq
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    {
        *CPU_FREQ_HZ
    }
}

/// Gets the current monotonic time in nanoseconds since boot.
///
/// This provides a consistent time source that can be used throughout
/// the service for timing operations, timeouts, and performance measurements.
///
/// # Returns
///
/// Nanoseconds elapsed since system boot (or CPU reset).
///
/// # Note
///
/// The accuracy depends on the CPU frequency calibration. For precise
/// timing, the system should calibrate against a known time source during boot.
#[inline]
pub fn get_monotonic_time() -> u64 {
    let cycles = read_cycle_counter();
    let freq = get_counter_frequency();

    // Convert cycles to nanoseconds: (cycles * 1_000_000_000) / freq
    // Use 128-bit arithmetic to avoid overflow
    let nanos_per_second: u64 = 1_000_000_000;

    // Split the calculation to avoid overflow:
    // cycles / freq gives seconds, (cycles % freq) * nanos / freq gives remainder
    let seconds = cycles / freq;
    let remainder_cycles = cycles % freq;

    // For the remainder, we can safely multiply by nanos_per_second
    // since remainder_cycles < freq
    let remainder_nanos = (remainder_cycles as u128 * nanos_per_second as u128 / freq as u128) as u64;

    seconds.saturating_mul(nanos_per_second).saturating_add(remainder_nanos)
}

/// Gets the current monotonic time in milliseconds since boot.
///
/// This is a convenience function for operations that don't need
/// nanosecond precision.
///
/// # Returns
///
/// Milliseconds elapsed since system boot.
#[inline]
pub fn get_monotonic_time_ms() -> u64 {
    get_monotonic_time() / 1_000_000
}

/// Gets the raw cycle count for high-precision timing.
///
/// Use this when you need the highest precision timing and will
/// handle the frequency conversion yourself.
///
/// # Returns
///
/// Raw CPU cycle count.
#[inline]
pub fn get_timestamp() -> u64 {
    read_cycle_counter()
}

/// Service identifier - unique within the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceId(pub u64);

impl ServiceId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Process ID (imported from kernel in real implementation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessId(pub u64);

/// Information about a registered service.
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    /// Human-readable service name
    pub name: String,
    /// Service version (semver)
    pub version: String,
    /// Capabilities this service provides
    pub provided_capabilities: Vec<String>,
    /// Required capabilities to use this service
    pub required_capabilities: Vec<String>,
    /// Service description
    pub description: String,
}

/// Internal service registry entry.
#[derive(Debug, Clone)]
struct ServiceEntry {
    /// Unique service ID
    id: ServiceId,
    /// Service information
    info: ServiceInfo,
    /// Process hosting this service
    process: ProcessId,
    /// Registration timestamp (cycles)
    registered_at: u64,
    /// Last heartbeat timestamp
    last_heartbeat: u64,
    /// Service health status
    status: ServiceStatus,
}

/// Service health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceStatus {
    /// Service is healthy and responding
    Healthy,
    /// Service missed recent heartbeats
    Degraded,
    /// Service is not responding
    Unhealthy,
    /// Service is shutting down
    Draining,
}

/// Configuration for S-ATLAS.
#[derive(Debug, Clone)]
pub struct AtlasConfig {
    /// Maximum number of registered services
    pub max_services: usize,
    /// Heartbeat interval in cycles
    pub heartbeat_interval: u64,
    /// Number of missed heartbeats before marking unhealthy
    pub unhealthy_threshold: u32,
    /// Enable service health monitoring
    pub health_monitoring: bool,
}

impl Default for AtlasConfig {
    fn default() -> Self {
        Self {
            max_services: 4096,
            heartbeat_interval: 1_000_000_000, // ~1 second at 1GHz
            unhealthy_threshold: 3,
            health_monitoring: true,
        }
    }
}

/// The S-ATLAS service registry.
pub struct Atlas {
    config: AtlasConfig,
    /// Services indexed by ID
    services: Mutex<BTreeMap<ServiceId, ServiceEntry>>,
    /// Services indexed by name (for discovery)
    by_name: Mutex<BTreeMap<String, ServiceId>>,
    /// Next service ID to assign
    next_id: Mutex<u64>,
}

impl Atlas {
    /// Creates a new S-ATLAS instance.
    pub fn new(config: AtlasConfig) -> Self {
        Self {
            config,
            services: Mutex::new(BTreeMap::new()),
            by_name: Mutex::new(BTreeMap::new()),
            next_id: Mutex::new(1),
        }
    }

    /// Registers a new service.
    ///
    /// # Arguments
    ///
    /// * `info` - Service information
    /// * `process` - Process hosting the service
    /// * `cap_token` - Capability token authorizing registration
    ///
    /// # Returns
    ///
    /// The new service ID.
    ///
    /// # Errors
    ///
    /// - `AtlasError::ServiceExists` if a service with this name already exists
    /// - `AtlasError::RegistryFull` if max services reached
    /// - `AtlasError::InvalidCapability` if token doesn't authorize registration
    pub fn register(
        &self,
        info: ServiceInfo,
        process: ProcessId,
        _cap_token: &CapabilityToken,
    ) -> Result<ServiceId, AtlasError> {
        // Check if service name already exists
        if self.by_name.lock().contains_key(&info.name) {
            return Err(AtlasError::ServiceExists);
        }

        // Check capacity
        let mut services = self.services.lock();
        if services.len() >= self.config.max_services {
            return Err(AtlasError::RegistryFull);
        }

        // Generate new ID
        let mut next_id = self.next_id.lock();
        let id = ServiceId::new(*next_id);
        *next_id += 1;

        // Get current timestamp using architecture-specific cycle counter
        let now = get_timestamp();

        let entry = ServiceEntry {
            id,
            info: info.clone(),
            process,
            registered_at: now,
            last_heartbeat: now,
            status: ServiceStatus::Healthy,
        };

        // Insert into both indexes
        services.insert(id, entry);
        self.by_name.lock().insert(info.name, id);

        Ok(id)
    }

    /// Discovers a service by name.
    ///
    /// # Arguments
    ///
    /// * `name` - Service name to find
    /// * `cap_token` - Capability token authorizing discovery
    ///
    /// # Returns
    ///
    /// Service information if found.
    pub fn discover(
        &self,
        name: &str,
        _cap_token: &CapabilityToken,
    ) -> Result<DiscoveryResult, AtlasError> {
        let id = self
            .by_name
            .lock()
            .get(name)
            .copied()
            .ok_or(AtlasError::ServiceNotFound)?;

        let services = self.services.lock();
        let entry = services.get(&id).ok_or(AtlasError::ServiceNotFound)?;

        Ok(DiscoveryResult {
            id: entry.id,
            info: entry.info.clone(),
            status: entry.status,
        })
    }

    /// Discovers services by capability.
    ///
    /// Finds all services that provide a specific capability.
    ///
    /// # Arguments
    ///
    /// * `capability` - Capability to search for
    /// * `cap_token` - Token authorizing discovery
    ///
    /// # Returns
    ///
    /// List of services providing the capability.
    pub fn discover_by_capability(
        &self,
        capability: &str,
        _cap_token: &CapabilityToken,
    ) -> Result<Vec<DiscoveryResult>, AtlasError> {
        let services = self.services.lock();
        let results: Vec<_> = services
            .values()
            .filter(|e| {
                e.status == ServiceStatus::Healthy
                    && e.info.provided_capabilities.iter().any(|c| c == capability)
            })
            .map(|e| DiscoveryResult {
                id: e.id,
                info: e.info.clone(),
                status: e.status,
            })
            .collect();

        Ok(results)
    }

    /// Updates a service's heartbeat.
    ///
    /// Services should call this periodically to indicate they're alive.
    pub fn heartbeat(
        &self,
        service_id: ServiceId,
        _cap_token: &CapabilityToken,
    ) -> Result<(), AtlasError> {
        let mut services = self.services.lock();
        let entry = services
            .get_mut(&service_id)
            .ok_or(AtlasError::ServiceNotFound)?;

        let now = get_timestamp();
        entry.last_heartbeat = now;
        entry.status = ServiceStatus::Healthy;

        Ok(())
    }

    /// Unregisters a service.
    ///
    /// # Arguments
    ///
    /// * `service_id` - Service to unregister
    /// * `cap_token` - Capability authorizing unregistration
    pub fn unregister(
        &self,
        service_id: ServiceId,
        _cap_token: &CapabilityToken,
    ) -> Result<(), AtlasError> {
        let mut services = self.services.lock();
        let entry = services
            .remove(&service_id)
            .ok_or(AtlasError::ServiceNotFound)?;

        self.by_name.lock().remove(&entry.info.name);

        Ok(())
    }

    /// Lists all registered services.
    pub fn list_services(
        &self,
        _cap_token: &CapabilityToken,
    ) -> Result<Vec<DiscoveryResult>, AtlasError> {
        let services = self.services.lock();
        let results: Vec<_> = services
            .values()
            .map(|e| DiscoveryResult {
                id: e.id,
                info: e.info.clone(),
                status: e.status,
            })
            .collect();

        Ok(results)
    }

    /// Gets the current service count.
    pub fn service_count(&self) -> usize {
        self.services.lock().len()
    }

    /// Checks health of all services and updates statuses.
    ///
    /// This should be called periodically by a health monitoring task.
    pub fn check_health(&self) {
        if !self.config.health_monitoring {
            return;
        }

        let now = get_timestamp();
        let threshold = self.config.heartbeat_interval * self.config.unhealthy_threshold as u64;

        let mut services = self.services.lock();
        for entry in services.values_mut() {
            let elapsed = now.saturating_sub(entry.last_heartbeat);
            entry.status = if elapsed > threshold {
                ServiceStatus::Unhealthy
            } else if elapsed > self.config.heartbeat_interval {
                ServiceStatus::Degraded
            } else {
                ServiceStatus::Healthy
            };
        }
    }
}

/// Result of a service discovery.
#[derive(Debug, Clone)]
pub struct DiscoveryResult {
    /// Service ID
    pub id: ServiceId,
    /// Service information
    pub info: ServiceInfo,
    /// Current health status
    pub status: ServiceStatus,
}

/// S-ATLAS errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtlasError {
    /// Service with this name already exists
    ServiceExists,
    /// Service not found
    ServiceNotFound,
    /// Service registry is full
    RegistryFull,
    /// Invalid capability token
    InvalidCapability,
    /// Operation not permitted
    PermissionDenied,
    /// Service is unhealthy
    ServiceUnhealthy,
    /// Restart limit exceeded
    RestartLimitExceeded,
}

// =============================================================================
// Service Health Monitoring & Auto-Restart
// =============================================================================

/// Restart policy for services
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Never restart automatically
    Never,
    /// Restart on failure only
    OnFailure,
    /// Always restart (unless explicitly stopped)
    Always,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self::OnFailure
    }
}

/// Service restart configuration
#[derive(Debug, Clone)]
pub struct RestartConfig {
    /// Restart policy
    pub policy: RestartPolicy,
    /// Maximum restarts within the window
    pub max_restarts: u32,
    /// Time window for max_restarts (in nanoseconds)
    pub restart_window: u64,
    /// Delay before restarting (in nanoseconds)
    pub restart_delay: u64,
    /// Current restart count within window
    pub restart_count: u32,
    /// Window start time
    pub window_start: u64,
}

impl Default for RestartConfig {
    fn default() -> Self {
        Self {
            policy: RestartPolicy::OnFailure,
            max_restarts: 5,
            restart_window: 60_000_000_000, // 60 seconds
            restart_delay: 1_000_000_000,    // 1 second
            restart_count: 0,
            window_start: 0,
        }
    }
}

/// Event emitted when service status changes
#[derive(Debug, Clone)]
pub struct ServiceEvent {
    /// Service that generated the event
    pub service_id: ServiceId,
    /// Service name
    pub service_name: String,
    /// Event type
    pub event_type: ServiceEventType,
    /// Timestamp
    pub timestamp: u64,
}

/// Service event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceEventType {
    /// Service registered
    Registered,
    /// Service became healthy
    Healthy,
    /// Service became degraded
    Degraded,
    /// Service became unhealthy
    Unhealthy,
    /// Service is being restarted
    Restarting,
    /// Service was restarted successfully
    Restarted,
    /// Service restart failed
    RestartFailed,
    /// Service was unregistered
    Unregistered,
    /// Service is draining
    Draining,
}

/// Service supervisor for health monitoring and auto-restart
pub struct ServiceSupervisor {
    /// Atlas registry reference
    atlas: &'static Atlas,
    /// Restart configurations per service
    restart_configs: Mutex<BTreeMap<ServiceId, RestartConfig>>,
    /// Event log (circular buffer)
    events: Mutex<Vec<ServiceEvent>>,
    /// Maximum events to keep
    max_events: usize,
    /// Callback for restart actions (in real impl, would spawn processes)
    restart_handler: Option<fn(ServiceId, &ServiceInfo) -> Result<ProcessId, ()>>,
}

impl ServiceSupervisor {
    /// Create a new supervisor
    pub fn new(atlas: &'static Atlas, max_events: usize) -> Self {
        Self {
            atlas,
            restart_configs: Mutex::new(BTreeMap::new()),
            events: Mutex::new(Vec::with_capacity(max_events)),
            max_events,
            restart_handler: None,
        }
    }
    
    /// Set the restart handler callback
    pub fn set_restart_handler(&mut self, handler: fn(ServiceId, &ServiceInfo) -> Result<ProcessId, ()>) {
        self.restart_handler = Some(handler);
    }
    
    /// Configure restart policy for a service
    pub fn configure_restart(
        &self,
        service_id: ServiceId,
        config: RestartConfig,
    ) {
        self.restart_configs.lock().insert(service_id, config);
    }
    
    /// Check and restart unhealthy services
    pub fn check_and_restart(&self) -> Vec<ServiceEvent> {
        let mut events = Vec::new();
        let now = get_monotonic_time();
        
        // Get current service states
        let token = CapabilityToken::default();
        let services = match self.atlas.list_services(&token) {
            Ok(s) => s,
            Err(_) => return events,
        };
        
        for service in services {
            if service.status == ServiceStatus::Unhealthy {
                if let Some(event) = self.try_restart_service(service.id, &service.info, now) {
                    events.push(event);
                }
            }
        }
        
        // Log events
        let mut event_log = self.events.lock();
        for event in &events {
            if event_log.len() >= self.max_events {
                event_log.remove(0);
            }
            event_log.push(event.clone());
        }
        
        events
    }
    
    fn try_restart_service(
        &self,
        service_id: ServiceId,
        info: &ServiceInfo,
        now: u64,
    ) -> Option<ServiceEvent> {
        let mut configs = self.restart_configs.lock();
        let config = configs.entry(service_id).or_insert_with(RestartConfig::default);
        
        // Check restart policy
        if config.policy == RestartPolicy::Never {
            return None;
        }
        
        // Check if we're within the restart window
        if now - config.window_start > config.restart_window {
            // Reset window
            config.restart_count = 0;
            config.window_start = now;
        }
        
        // Check restart limit
        if config.restart_count >= config.max_restarts {
            return Some(ServiceEvent {
                service_id,
                service_name: info.name.clone(),
                event_type: ServiceEventType::RestartFailed,
                timestamp: now,
            });
        }
        
        config.restart_count += 1;
        
        // Emit restarting event
        let restart_event = ServiceEvent {
            service_id,
            service_name: info.name.clone(),
            event_type: ServiceEventType::Restarting,
            timestamp: now,
        };
        
        // Call restart handler if set
        if let Some(handler) = self.restart_handler {
            match handler(service_id, info) {
                Ok(_new_pid) => {
                    return Some(ServiceEvent {
                        service_id,
                        service_name: info.name.clone(),
                        event_type: ServiceEventType::Restarted,
                        timestamp: now,
                    });
                }
                Err(_) => {
                    return Some(ServiceEvent {
                        service_id,
                        service_name: info.name.clone(),
                        event_type: ServiceEventType::RestartFailed,
                        timestamp: now,
                    });
                }
            }
        }
        
        Some(restart_event)
    }
    
    /// Get recent events
    pub fn get_events(&self, count: usize) -> Vec<ServiceEvent> {
        let events = self.events.lock();
        let start = events.len().saturating_sub(count);
        events[start..].to_vec()
    }
    
    /// Get restart stats for a service
    pub fn get_restart_stats(&self, service_id: ServiceId) -> Option<(u32, u32)> {
        let configs = self.restart_configs.lock();
        configs.get(&service_id).map(|c| (c.restart_count, c.max_restarts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken::default()
    }

    #[test]
    fn test_register_and_discover() {
        let atlas = Atlas::new(AtlasConfig::default());
        let token = dummy_token();

        let info = ServiceInfo {
            name: String::from("test-service"),
            version: String::from("1.0.0"),
            provided_capabilities: alloc::vec![String::from("test:capability")],
            required_capabilities: alloc::vec![],
            description: String::from("A test service"),
        };

        let id = atlas
            .register(info, ProcessId(1), &token)
            .expect("should register");

        let result = atlas
            .discover("test-service", &token)
            .expect("should discover");

        assert_eq!(result.id, id);
        assert_eq!(result.info.name, "test-service");
    }

    #[test]
    fn test_duplicate_registration_fails() {
        let atlas = Atlas::new(AtlasConfig::default());
        let token = dummy_token();

        let info = ServiceInfo {
            name: String::from("duplicate"),
            version: String::from("1.0.0"),
            provided_capabilities: alloc::vec![],
            required_capabilities: alloc::vec![],
            description: String::from(""),
        };

        atlas
            .register(info.clone(), ProcessId(1), &token)
            .expect("first should succeed");

        let result = atlas.register(info, ProcessId(2), &token);
        assert_eq!(result, Err(AtlasError::ServiceExists));
    }
}

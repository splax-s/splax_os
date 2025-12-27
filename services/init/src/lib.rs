//! # S-INIT: SplaxOS Service Manager
//!
//! S-INIT is the first userspace process (PID 1) that runs on SplaxOS.
//! It is responsible for:
//!
//! - **Service Management**: Starting, stopping, and monitoring system services
//! - **Dependency Resolution**: Ensuring services start in the correct order
//! - **Process Supervision**: Restarting crashed services automatically
//! - **Runlevel Management**: Managing system states (boot, running, shutdown)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                        S-INIT (PID 1)                       │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
//! │  │   Service   │  │  Dependency │  │      Process        │  │
//! │  │   Registry  │  │   Resolver  │  │      Supervisor     │  │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘  │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
//! │  │   Runlevel  │  │    IPC      │  │      Signal         │  │
//! │  │   Manager   │  │   Handler   │  │      Handler        │  │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//!               │                │                │
//!       ┌───────┴───────┐  ┌─────┴─────┐  ┌───────┴───────┐
//!       │   s-storage   │  │  s-gate   │  │   s-atlas     │
//!       │   (storage)   │  │ (network) │  │  (gui/wm)     │
//!       └───────────────┘  └───────────┘  └───────────────┘
//! ```
//!
//! ## Service Definition
//!
//! Services are defined in `/etc/init/` as `.service` files:
//!
//! ```toml
//! [service]
//! name = "s-storage"
//! description = "Storage and filesystem service"
//! type = "forking"
//!
//! [exec]
//! start = "/sbin/s-storage"
//! stop = "/sbin/s-storage --stop"
//!
//! [dependencies]
//! after = []
//! requires = []
//!
//! [restart]
//! policy = "always"
//! delay_ms = 1000
//! max_retries = 5
//! ```

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::RwLock;

// =============================================================================
// SERVICE TYPES
// =============================================================================

/// Unique service identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceId(pub u64);

impl ServiceId {
    /// Create a new service ID
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Service execution type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    /// Simple service (runs in foreground)
    Simple,
    /// Forking service (forks a daemon)
    Forking,
    /// Oneshot service (runs once at startup)
    Oneshot,
    /// Notify service (notifies when ready)
    Notify,
    /// Idle service (runs when system is idle)
    Idle,
}

/// Current state of a service
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// Service is stopped
    Stopped,
    /// Service is starting
    Starting,
    /// Service is running
    Running,
    /// Service is stopping
    Stopping,
    /// Service failed to start
    Failed,
    /// Service was disabled
    Disabled,
}

/// Restart policy for a service
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Never restart
    No,
    /// Always restart
    Always,
    /// Restart on success only
    OnSuccess,
    /// Restart on failure only
    OnFailure,
    /// Restart on abnormal exit
    OnAbnormal,
}

/// Service dependency type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyType {
    /// Must start after this service
    After,
    /// Must start before this service
    Before,
    /// Requires this service (hard dependency)
    Requires,
    /// Wants this service (soft dependency)
    Wants,
    /// Conflicts with this service
    Conflicts,
}

/// A service dependency
#[derive(Debug, Clone)]
pub struct Dependency {
    /// Target service name
    pub target: String,
    /// Type of dependency
    pub dep_type: DependencyType,
}

/// Service definition
#[derive(Debug, Clone)]
pub struct ServiceDef {
    /// Service identifier
    pub id: ServiceId,
    /// Service name
    pub name: String,
    /// Description
    pub description: String,
    /// Service type
    pub service_type: ServiceType,
    /// Start command
    pub start_cmd: String,
    /// Stop command (optional)
    pub stop_cmd: Option<String>,
    /// Reload command (optional)
    pub reload_cmd: Option<String>,
    /// Dependencies
    pub dependencies: Vec<Dependency>,
    /// Restart policy
    pub restart_policy: RestartPolicy,
    /// Restart delay in milliseconds
    pub restart_delay_ms: u64,
    /// Maximum restart attempts
    pub max_restarts: u32,
    /// Environment variables
    pub environment: BTreeMap<String, String>,
}

impl ServiceDef {
    /// Create a new service definition
    pub fn new(id: ServiceId, name: &str, start_cmd: &str) -> Self {
        Self {
            id,
            name: String::from(name),
            description: String::new(),
            service_type: ServiceType::Simple,
            start_cmd: String::from(start_cmd),
            stop_cmd: None,
            reload_cmd: None,
            dependencies: Vec::new(),
            restart_policy: RestartPolicy::OnFailure,
            restart_delay_ms: 1000,
            max_restarts: 5,
            environment: BTreeMap::new(),
        }
    }
    
    /// Set description
    pub fn description(mut self, desc: &str) -> Self {
        self.description = String::from(desc);
        self
    }
    
    /// Set service type
    pub fn service_type(mut self, stype: ServiceType) -> Self {
        self.service_type = stype;
        self
    }
    
    /// Add a dependency
    pub fn depends_on(mut self, target: &str, dep_type: DependencyType) -> Self {
        self.dependencies.push(Dependency {
            target: String::from(target),
            dep_type,
        });
        self
    }
    
    /// Set restart policy
    pub fn restart(mut self, policy: RestartPolicy) -> Self {
        self.restart_policy = policy;
        self
    }
}

// =============================================================================
// RUNTIME STATE
// =============================================================================

/// Runtime state of a service
#[derive(Debug)]
pub struct ServiceRuntime {
    /// Service definition
    pub def: ServiceDef,
    /// Current state
    pub state: ServiceState,
    /// Process ID (if running)
    pub pid: Option<u64>,
    /// Number of restart attempts
    pub restart_count: u32,
    /// Last exit code
    pub last_exit_code: Option<i32>,
    /// Time service started (ticks)
    pub started_at: u64,
    /// Time service stopped (ticks)
    pub stopped_at: u64,
}

impl ServiceRuntime {
    /// Create new runtime from definition
    pub fn new(def: ServiceDef) -> Self {
        Self {
            def,
            state: ServiceState::Stopped,
            pid: None,
            restart_count: 0,
            last_exit_code: None,
            started_at: 0,
            stopped_at: 0,
        }
    }
    
    /// Check if service should be restarted
    pub fn should_restart(&self) -> bool {
        if self.restart_count >= self.def.max_restarts {
            return false;
        }
        
        match self.def.restart_policy {
            RestartPolicy::No => false,
            RestartPolicy::Always => true,
            RestartPolicy::OnSuccess => self.last_exit_code == Some(0),
            RestartPolicy::OnFailure => self.last_exit_code.map_or(true, |c| c != 0),
            RestartPolicy::OnAbnormal => {
                // Abnormal = signal death or non-zero exit
                self.last_exit_code.map_or(true, |c| c != 0 && c != 1)
            }
        }
    }
}

// =============================================================================
// RUNLEVEL MANAGEMENT
// =============================================================================

/// System runlevel
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Runlevel {
    /// System is booting
    Boot = 0,
    /// Single user mode (recovery)
    Single = 1,
    /// Multi-user without network
    MultiUser = 2,
    /// Multi-user with network
    Network = 3,
    /// Reserved
    Reserved = 4,
    /// Graphical (full desktop)
    Graphical = 5,
    /// Reboot
    Reboot = 6,
}

impl Runlevel {
    /// Get services for this runlevel
    pub fn services(&self) -> &[&str] {
        match self {
            Runlevel::Boot => &[],
            Runlevel::Single => &["s-storage"],
            Runlevel::MultiUser => &["s-storage", "s-link"],
            Runlevel::Network => &["s-storage", "s-link", "s-gate"],
            Runlevel::Reserved => &["s-storage", "s-link", "s-gate"],
            Runlevel::Graphical => &["s-storage", "s-link", "s-gate", "s-atlas"],
            Runlevel::Reboot => &[],
        }
    }
}

// =============================================================================
// SERVICE MANAGER
// =============================================================================

/// The service manager (S-INIT core)
pub struct ServiceManager {
    /// Registered services
    services: RwLock<BTreeMap<ServiceId, Box<ServiceRuntime>>>,
    /// Service name to ID mapping
    name_to_id: RwLock<BTreeMap<String, ServiceId>>,
    /// Current runlevel
    runlevel: RwLock<Runlevel>,
    /// Next service ID
    next_id: RwLock<u64>,
    /// Boot complete flag
    boot_complete: RwLock<bool>,
}

impl ServiceManager {
    /// Create a new service manager
    pub const fn new() -> Self {
        Self {
            services: RwLock::new(BTreeMap::new()),
            name_to_id: RwLock::new(BTreeMap::new()),
            runlevel: RwLock::new(Runlevel::Boot),
            next_id: RwLock::new(1),
            boot_complete: RwLock::new(false),
        }
    }
    
    /// Register a new service
    pub fn register(&self, def: ServiceDef) -> ServiceId {
        let id = def.id;
        let name = def.name.clone();
        
        let runtime = Box::new(ServiceRuntime::new(def));
        
        self.services.write().insert(id, runtime);
        self.name_to_id.write().insert(name, id);
        
        id
    }
    
    /// Create and register a new service
    pub fn create_service(&self, name: &str, start_cmd: &str) -> ServiceId {
        let id = {
            let mut next = self.next_id.write();
            let id = ServiceId(*next);
            *next += 1;
            id
        };
        
        let def = ServiceDef::new(id, name, start_cmd);
        self.register(def)
    }
    
    /// Get service by name
    pub fn get_by_name(&self, name: &str) -> Option<ServiceId> {
        self.name_to_id.read().get(name).copied()
    }
    
    /// Get service state
    pub fn state(&self, id: ServiceId) -> Option<ServiceState> {
        self.services.read().get(&id).map(|s| s.state)
    }
    
    /// Start a service
    pub fn start(&self, id: ServiceId) -> Result<(), InitError> {
        // First, check dependencies (before taking mutable borrow)
        let deps_to_check: Vec<(String, ServiceId)>;
        {
            let services = self.services.read();
            let service = services.get(&id).ok_or(InitError::ServiceNotFound)?;
            
            if service.state == ServiceState::Running {
                return Ok(()); // Already running
            }
            
            deps_to_check = service.def.dependencies.iter()
                .filter(|d| d.dep_type == DependencyType::Requires)
                .filter_map(|d| {
                    self.name_to_id.read()
                        .get(&d.target)
                        .map(|id| (d.target.clone(), *id))
                })
                .collect();
        }
        
        // Check dependency states
        {
            let services = self.services.read();
            for (dep_name, dep_id) in &deps_to_check {
                if let Some(dep_service) = services.get(dep_id) {
                    if dep_service.state != ServiceState::Running {
                        return Err(InitError::DependencyNotMet(dep_name.clone()));
                    }
                }
            }
        }
        
        // Now take mutable borrow and start service
        let mut services = self.services.write();
        let service = services.get_mut(&id).ok_or(InitError::ServiceNotFound)?;
        
        service.state = ServiceState::Starting;
        
        // In a real implementation, we would:
        // 1. Fork a new process
        // 2. Execute the start command
        // 3. Wait for ready notification (for Notify type)
        // 4. Update PID and state
        
        // For now, simulate immediate start
        service.state = ServiceState::Running;
        service.pid = Some(100 + id.0); // Mock PID
        service.restart_count = 0;
        
        Ok(())
    }
    
    /// Stop a service
    pub fn stop(&self, id: ServiceId) -> Result<(), InitError> {
        let mut services = self.services.write();
        let service = services.get_mut(&id).ok_or(InitError::ServiceNotFound)?;
        
        if service.state == ServiceState::Stopped {
            return Ok(()); // Already stopped
        }
        
        service.state = ServiceState::Stopping;
        
        // In a real implementation, we would:
        // 1. Send stop command or SIGTERM
        // 2. Wait for graceful shutdown
        // 3. Send SIGKILL if timeout
        
        service.state = ServiceState::Stopped;
        service.pid = None;
        
        Ok(())
    }
    
    /// Restart a service
    pub fn restart(&self, id: ServiceId) -> Result<(), InitError> {
        self.stop(id)?;
        self.start(id)
    }
    
    /// Handle service exit
    pub fn on_service_exit(&self, pid: u64, exit_code: i32) {
        let mut services = self.services.write();
        
        // Find service by PID
        let service_id = services.iter()
            .find(|(_, s)| s.pid == Some(pid))
            .map(|(id, _)| *id);
        
        if let Some(id) = service_id {
            if let Some(service) = services.get_mut(&id) {
                service.state = ServiceState::Stopped;
                service.pid = None;
                service.last_exit_code = Some(exit_code);
                
                // Check if we should restart
                if service.should_restart() {
                    service.restart_count += 1;
                    // Queue restart (would use a timer in real implementation)
                    service.state = ServiceState::Starting;
                } else if exit_code != 0 {
                    service.state = ServiceState::Failed;
                }
            }
        }
    }
    
    /// Set system runlevel
    pub fn set_runlevel(&self, level: Runlevel) -> Result<(), InitError> {
        let current = *self.runlevel.read();
        
        if level == current {
            return Ok(());
        }
        
        // Handle runlevel transition
        if level > current {
            // Starting services for higher runlevel
            for service_name in level.services() {
                if let Some(id) = self.get_by_name(service_name) {
                    let _ = self.start(id);
                }
            }
        } else {
            // Stopping services for lower runlevel
            let current_services: Vec<&str> = current.services().to_vec();
            let target_services: Vec<&str> = level.services().to_vec();
            
            for service_name in current_services {
                if !target_services.contains(&service_name) {
                    if let Some(id) = self.get_by_name(service_name) {
                        let _ = self.stop(id);
                    }
                }
            }
        }
        
        *self.runlevel.write() = level;
        Ok(())
    }
    
    /// Get current runlevel
    pub fn runlevel(&self) -> Runlevel {
        *self.runlevel.read()
    }
    
    /// Mark boot as complete
    pub fn boot_complete(&self) {
        *self.boot_complete.write() = true;
    }
    
    /// Check if boot is complete
    pub fn is_boot_complete(&self) -> bool {
        *self.boot_complete.read()
    }
    
    /// List all services
    pub fn list_services(&self) -> Vec<(ServiceId, String, ServiceState)> {
        self.services.read()
            .iter()
            .map(|(id, s)| (*id, s.def.name.clone(), s.state))
            .collect()
    }
}

impl Default for ServiceManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// ERRORS
// =============================================================================

/// S-INIT errors
#[derive(Debug, Clone)]
pub enum InitError {
    /// Service not found
    ServiceNotFound,
    /// Dependency not met
    DependencyNotMet(String),
    /// Service already running
    AlreadyRunning,
    /// Service failed to start
    StartFailed,
    /// Invalid service definition
    InvalidDefinition,
    /// Permission denied
    PermissionDenied,
}

impl core::fmt::Display for InitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            InitError::ServiceNotFound => write!(f, "Service not found"),
            InitError::DependencyNotMet(s) => write!(f, "Dependency not met: {}", s),
            InitError::AlreadyRunning => write!(f, "Service already running"),
            InitError::StartFailed => write!(f, "Failed to start service"),
            InitError::InvalidDefinition => write!(f, "Invalid service definition"),
            InitError::PermissionDenied => write!(f, "Permission denied"),
        }
    }
}

// =============================================================================
// GLOBAL INSTANCE
// =============================================================================

/// Global service manager instance
pub static INIT: ServiceManager = ServiceManager::new();

// =============================================================================
// BUILT-IN SERVICES
// =============================================================================

/// Register built-in SplaxOS services
pub fn register_builtin_services() {
    // S-STORAGE: Storage and filesystem service
    let storage = ServiceDef::new(
        ServiceId::new(1),
        "s-storage",
        "/sbin/s-storage",
    )
    .description("Storage and filesystem management service")
    .service_type(ServiceType::Notify)
    .restart(RestartPolicy::Always);
    
    INIT.register(storage);
    
    // S-LINK: IPC and messaging service
    let link = ServiceDef::new(
        ServiceId::new(2),
        "s-link",
        "/sbin/s-link",
    )
    .description("Inter-process communication service")
    .service_type(ServiceType::Notify)
    .depends_on("s-storage", DependencyType::Requires)
    .restart(RestartPolicy::Always);
    
    INIT.register(link);
    
    // S-GATE: Network service
    let gate = ServiceDef::new(
        ServiceId::new(3),
        "s-gate",
        "/sbin/s-gate",
    )
    .description("Network and connectivity service")
    .service_type(ServiceType::Notify)
    .depends_on("s-storage", DependencyType::Requires)
    .depends_on("s-link", DependencyType::After)
    .restart(RestartPolicy::Always);
    
    INIT.register(gate);
    
    // S-ATLAS: Display and window management
    let atlas = ServiceDef::new(
        ServiceId::new(4),
        "s-atlas",
        "/sbin/s-atlas",
    )
    .description("Display server and window manager")
    .service_type(ServiceType::Notify)
    .depends_on("s-storage", DependencyType::Requires)
    .depends_on("s-link", DependencyType::Requires)
    .restart(RestartPolicy::OnFailure);
    
    INIT.register(atlas);
}

/// Initialize S-INIT (called as PID 1)
pub fn init_main() -> ! {
    // Register built-in services
    register_builtin_services();
    
    // Start boot sequence
    let _ = INIT.set_runlevel(Runlevel::Graphical);
    
    // Mark boot complete
    INIT.boot_complete();
    
    // Main loop: handle signals, monitor services
    loop {
        // In a real implementation:
        // 1. Wait for signals (SIGCHLD, etc.)
        // 2. Handle child process exits
        // 3. Process service restart queue
        // 4. Handle control commands via IPC
        
        // For now, infinite loop (would use proper blocking)
        core::hint::spin_loop();
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_service_registration() {
        let manager = ServiceManager::new();
        
        let id = manager.create_service("test-service", "/bin/test");
        assert_eq!(manager.get_by_name("test-service"), Some(id));
        assert_eq!(manager.state(id), Some(ServiceState::Stopped));
    }
    
    #[test]
    fn test_service_lifecycle() {
        let manager = ServiceManager::new();
        
        let id = manager.create_service("test", "/bin/test");
        
        // Start
        assert!(manager.start(id).is_ok());
        assert_eq!(manager.state(id), Some(ServiceState::Running));
        
        // Stop
        assert!(manager.stop(id).is_ok());
        assert_eq!(manager.state(id), Some(ServiceState::Stopped));
    }
    
    #[test]
    fn test_runlevel() {
        let manager = ServiceManager::new();
        
        assert_eq!(manager.runlevel(), Runlevel::Boot);
        
        let _ = manager.set_runlevel(Runlevel::Network);
        assert_eq!(manager.runlevel(), Runlevel::Network);
    }
}

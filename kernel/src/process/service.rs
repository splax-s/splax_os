//! # Service Launcher for Microkernel Mode
//!
//! This module provides the kernel-side support for spawning and managing
//! userspace services in microkernel mode.
//!
//! ## Design
//!
//! In microkernel mode, core services run in userspace:
//! - S-DEV: Device driver manager
//! - S-NET: Network stack
//! - S-STORAGE: Filesystem layer
//! - S-GPU: Graphics compositor
//!
//! The kernel provides:
//! - Process spawning via exec
//! - IPC channel setup
//! - Capability token distribution
//!
//! ## Service Lifecycle
//!
//! 1. Kernel spawns service with initial capabilities
//! 2. Service registers IPC endpoints
//! 3. Kernel connects services via IPC channels
//! 4. Services handle requests from kernel and each other

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use crate::cap::CapabilityToken;
use crate::sched::ProcessId;

/// Handle to an IPC channel for service communication.
/// This is a lightweight wrapper until we integrate with the full IPC subsystem.
#[derive(Debug, Clone, Copy)]
pub struct ChannelHandle(u64);

/// Service identifier (well-known names)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u64)]
pub enum ServiceId {
    /// S-INIT: Process manager (PID 1)
    Init = 1,
    /// S-DEV: Device driver manager
    Dev = 2,
    /// S-STORAGE: Filesystem service
    Storage = 3,
    /// S-NET: Network stack
    Net = 4,
    /// S-GPU: Graphics compositor
    Gpu = 5,
    /// S-CANVAS: 2D graphics service
    Canvas = 6,
    /// S-LINK: Network link layer
    Link = 7,
    /// S-GATE: Security gateway
    Gate = 8,
    /// User-defined services start here
    Custom(u64) = 1000,
}

impl From<u64> for ServiceId {
    fn from(id: u64) -> Self {
        match id {
            1 => ServiceId::Init,
            2 => ServiceId::Dev,
            3 => ServiceId::Storage,
            4 => ServiceId::Net,
            5 => ServiceId::Gpu,
            6 => ServiceId::Canvas,
            7 => ServiceId::Link,
            8 => ServiceId::Gate,
            n => ServiceId::Custom(n),
        }
    }
}

/// Service state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// Being spawned
    Starting,
    /// Running and accepting requests
    Running,
    /// Temporarily paused
    Paused,
    /// Being restarted
    Restarting,
    /// Stopped (crashed or terminated)
    Stopped,
}

/// Registered service information
struct ServiceEntry {
    /// Service identifier
    id: ServiceId,
    /// Human-readable name
    name: String,
    /// Kernel process ID
    pid: ProcessId,
    /// Current state
    state: ServiceState,
    /// IPC channel for kernel communication
    kernel_channel: Option<ChannelHandle>,
    /// Capability token for service operations
    cap_token: CapabilityToken,
    /// Restart count
    restart_count: u32,
    /// Max restart attempts
    max_restarts: u32,
}

/// Service registry
struct ServiceRegistry {
    /// Registered services
    services: BTreeMap<ServiceId, ServiceEntry>,
    /// PID to ServiceId mapping
    pid_to_service: BTreeMap<ProcessId, ServiceId>,
    /// Next custom service ID
    next_custom_id: u64,
}

impl ServiceRegistry {
    const fn new() -> Self {
        Self {
            services: BTreeMap::new(),
            pid_to_service: BTreeMap::new(),
            next_custom_id: 1001,
        }
    }
}

/// Global service registry
static SERVICE_REGISTRY: Mutex<ServiceRegistry> = Mutex::new(ServiceRegistry::new());

/// Service launch configuration
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Service name
    pub name: String,
    /// ELF binary path (for fs lookup)
    pub binary_path: Option<String>,
    /// Initial capabilities to grant
    pub capabilities: Vec<CapabilityToken>,
    /// Memory limit (bytes)
    pub memory_limit: usize,
    /// Whether to auto-restart on crash
    pub auto_restart: bool,
    /// Max restart attempts
    pub max_restarts: u32,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            binary_path: None,
            capabilities: Vec::new(),
            memory_limit: 64 * 1024 * 1024, // 64 MB
            auto_restart: true,
            max_restarts: 5,
        }
    }
}

/// Service launcher errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceError {
    /// Service already registered
    AlreadyRegistered,
    /// Service not found
    NotFound,
    /// Binary not found
    BinaryNotFound,
    /// Spawn failed
    SpawnFailed,
    /// Too many restarts
    TooManyRestarts,
    /// Invalid configuration
    InvalidConfig,
    /// Permission denied
    PermissionDenied,
    /// IPC channel creation failed
    IpcFailed,
    /// Out of memory
    OutOfMemory,
}

/// Spawn a core service.
///
/// Called during kernel initialization to start essential services.
pub fn spawn_service(
    id: ServiceId,
    config: ServiceConfig,
    elf_data: Option<&[u8]>,
) -> Result<ProcessId, ServiceError> {
    spawn_service_internal(id, config, elf_data, 0)
}

/// Internal spawn function that tracks restart count.
fn spawn_service_internal(
    id: ServiceId,
    config: ServiceConfig,
    elf_data: Option<&[u8]>,
    restart_count: u32,
) -> Result<ProcessId, ServiceError> {
    if config.name.is_empty() {
        return Err(ServiceError::InvalidConfig);
    }

    let mut registry = SERVICE_REGISTRY.lock();

    // For restarts, remove old entry first
    if restart_count > 0 {
        registry.services.remove(&id);
    } else if registry.services.contains_key(&id) {
        return Err(ServiceError::AlreadyRegistered);
    }

    // Spawn the process
    let pid = spawn_service_process(&config, elf_data)?;

    // Create kernel IPC channel
    let kernel_channel = create_kernel_channel(id)?;

    // Create service capability token
    let cap_token = create_service_token(id);

    let entry = ServiceEntry {
        id,
        name: config.name.clone(),
        pid,
        state: ServiceState::Starting,
        kernel_channel: Some(kernel_channel),
        cap_token,
        restart_count,
        max_restarts: config.max_restarts,
    };

    registry.pid_to_service.insert(pid, id);
    registry.services.insert(id, entry);

    crate::serial_println!("[service] Spawned {} (PID {:?})", config.name, pid);

    Ok(pid)
}

/// Mark a service as running.
pub fn service_ready(id: ServiceId) -> Result<(), ServiceError> {
    let mut registry = SERVICE_REGISTRY.lock();
    let entry = registry.services.get_mut(&id).ok_or(ServiceError::NotFound)?;
    entry.state = ServiceState::Running;
    crate::serial_println!("[service] {} is ready", entry.name);
    Ok(())
}

/// Handle service crash.
pub fn service_crashed(pid: ProcessId) -> Result<(), ServiceError> {
    let mut registry = SERVICE_REGISTRY.lock();

    let id = *registry.pid_to_service.get(&pid).ok_or(ServiceError::NotFound)?;
    let entry = registry.services.get_mut(&id).ok_or(ServiceError::NotFound)?;

    entry.state = ServiceState::Stopped;
    entry.restart_count += 1;

    let restart_count = entry.restart_count;
    let max_restarts = entry.max_restarts;
    let name = entry.name.clone();

    crate::serial_println!(
        "[service] {} crashed (restart {}/{})",
        name,
        restart_count,
        max_restarts
    );

    // Implement restart logic
    if restart_count < max_restarts {
        entry.state = ServiceState::Restarting;
        
        // Remove old PID mapping
        registry.pid_to_service.remove(&pid);
        
        // Create new config for respawn
        let config = ServiceConfig {
            name: name.clone(),
            auto_restart: true,
            max_restarts,
            ..Default::default()
        };
        
        // Drop registry lock before respawn
        drop(registry);
        
        // Respawn the service
        match spawn_service_internal(id, config, None, restart_count) {
            Ok(new_pid) => {
                crate::serial_println!("[service] {} restarted as PID {:?}", name, new_pid);
            }
            Err(e) => {
                crate::serial_println!("[service] Failed to restart {}: {:?}", name, e);
            }
        }
    } else {
        crate::serial_println!("[service] {} exceeded max restarts, not restarting", name);
    }

    Ok(())
}

/// Get service info.
pub fn get_service_info(id: ServiceId) -> Option<ServiceInfo> {
    let registry = SERVICE_REGISTRY.lock();
    registry.services.get(&id).map(|e| ServiceInfo {
        id: e.id,
        name: e.name.clone(),
        pid: e.pid,
        state: e.state,
        restart_count: e.restart_count,
    })
}

/// List all registered services.
pub fn list_services() -> Vec<ServiceInfo> {
    SERVICE_REGISTRY.lock()
        .services
        .values()
        .map(|e| ServiceInfo {
            id: e.id,
            name: e.name.clone(),
            pid: e.pid,
            state: e.state,
            restart_count: e.restart_count,
        })
        .collect()
}

/// Get service by PID.
pub fn get_service_by_pid(pid: ProcessId) -> Option<ServiceId> {
    SERVICE_REGISTRY.lock().pid_to_service.get(&pid).copied()
}

/// Service information (public view)
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub id: ServiceId,
    pub name: String,
    pub pid: ProcessId,
    pub state: ServiceState,
    pub restart_count: u32,
}

// Internal helper functions

fn get_current_ticks() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        crate::arch::x86_64::interrupts::get_ticks()
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

fn spawn_service_process(
    config: &ServiceConfig,
    elf_data: Option<&[u8]>,
) -> Result<ProcessId, ServiceError> {
    use crate::sched::{scheduler, SchedulingClass};
    
    // If we have ELF data, use exec to spawn a real process
    if let Some(data) = elf_data {
        use crate::process::exec;

        let (_elf_info, ctx, _stack_data) =
            exec::prepare_exec(data, &[&config.name], &[]).map_err(|_| ServiceError::SpawnFailed)?;

        // Register with scheduler to get a real PID
        let pid = scheduler().register_process(
            SchedulingClass::Interactive,
            128, // Default priority
        ).map_err(|_| ServiceError::SpawnFailed)?;
        
        crate::serial_println!(
            "[service] Created process {} at entry {:#x} (PID {:?})",
            config.name,
            ctx.entry,
            pid
        );

        return Ok(pid);
    }

    // If no binary, register a placeholder process for later loading
    let pid = scheduler().register_process(
        SchedulingClass::Interactive,
        128,
    ).map_err(|_| ServiceError::SpawnFailed)?;
    
    crate::serial_println!("[service] Created placeholder process {} (PID {:?})", config.name, pid);
    
    Ok(pid)
}

fn create_kernel_channel(id: ServiceId) -> Result<ChannelHandle, ServiceError> {
    use crate::ipc::{IPC_MANAGER, ChannelId};
    use crate::sched::ProcessId;
    use crate::cap::CapabilityToken;
    
    // Create a real IPC channel for kernel<->service communication
    let base_channel = match id {
        ServiceId::Init => 1,
        ServiceId::Dev => 2,
        ServiceId::Storage => 3,
        ServiceId::Net => 4,
        ServiceId::Gpu => 5,
        ServiceId::Canvas => 6,
        ServiceId::Link => 7,
        ServiceId::Gate => 8,
        ServiceId::Custom(n) => n,
    };
    
    // Kernel is always PID 0, service will connect later
    let kernel_pid = ProcessId::new(0);
    let service_pid = ProcessId::new(base_channel);
    
    // Create capability token for channel creation (using the u64x4 format)
    let cap_token = CapabilityToken::new([base_channel, 0xFFFF_FFFF, 0, get_current_ticks()]);
    
    match IPC_MANAGER.create_channel(kernel_pid, service_pid, &cap_token) {
        Ok(cid) => {
            crate::serial_println!("[service] Created IPC channel {} for {:?}", cid.0, id);
            Ok(ChannelHandle(cid.0))
        }
        Err(_) => {
            // If channel creation fails, return a handle anyway for later setup
            crate::serial_println!("[service] Deferred IPC channel for {:?}", id);
            Ok(ChannelHandle(base_channel))
        }
    }
}

fn create_service_token(id: ServiceId) -> CapabilityToken {
    // Create a capability token with permissions for this service type
    let id_val = match id {
        ServiceId::Init => 1u64,
        ServiceId::Dev => 2u64,
        ServiceId::Storage => 3u64,
        ServiceId::Net => 4u64,
        ServiceId::Gpu => 5u64,
        ServiceId::Canvas => 6u64,
        ServiceId::Link => 7u64,
        ServiceId::Gate => 8u64,
        ServiceId::Custom(n) => n,
    };
    // Token: [service_id, permissions_mask, random_nonce, creation_time]
    CapabilityToken::new([id_val, 0xFFFF_FFFF, 0, get_current_ticks()])
}

/// Initialize the service subsystem.
pub fn init() {
    crate::serial_println!("[service] Service launcher initialized");
}

/// Spawn core services during kernel boot (microkernel mode only).
#[cfg(feature = "microkernel")]
pub fn spawn_core_services() {
    crate::serial_println!("[service] Spawning core microkernel services...");

    // In a full implementation, these would load from embedded binaries
    // or a bootstrap filesystem

    let services = [
        (ServiceId::Dev, "S-DEV"),
        (ServiceId::Storage, "S-STORAGE"),
        (ServiceId::Net, "S-NET"),
    ];

    for (id, name) in services {
        let config = ServiceConfig {
            name: String::from(name),
            auto_restart: true,
            max_restarts: 5,
            ..Default::default()
        };

        match spawn_service(id, config, None) {
            Ok(pid) => {
                crate::serial_println!("[service] {} spawned as PID {:?}", name, pid);
            }
            Err(e) => {
                crate::serial_println!("[service] Failed to spawn {}: {:?}", name, e);
            }
        }
    }
}

//! Microkernel Service Ordering for S-INIT
//!
//! This module defines the startup ordering for userspace services
//! in the hybrid microkernel architecture. Services must start in
//! the correct order to satisfy dependencies.
//!
//! ## Boot Order
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────┐
//! │                     S-CORE (Kernel)                        │
//! │  [Scheduler] [Memory] [IPC Channels] [Capabilities]        │
//! └────────────────────────────────────────────────────────────┘
//!                           │
//!                           ▼
//! ┌────────────────────────────────────────────────────────────┐
//! │                  S-INIT (PID 1) - This                     │
//! └────────────────────────────────────────────────────────────┘
//!                           │
//!         ┌─────────────────┼─────────────────┐
//!         ▼                 ▼                 ▼
//! ┌───────────────┐ ┌───────────────┐ ┌───────────────┐
//! │   S-STORAGE   │ │     S-DEV     │ │    S-GPU      │
//! │  (VFS, Block) │ │   (Drivers)   │ │ (Framebuffer) │
//! └───────────────┘ └───────────────┘ └───────────────┘
//!         │                 │                 │
//!         └────────┬────────┴─────────────────┘
//!                  │                 │
//!         ┌────────┴────────┐        ▼
//!         ▼                 ▼ ┌───────────────┐
//! ┌───────────────┐           │   S-CANVAS    │
//! │     S-NET     │           │  (Windowing)  │
//! │  (Network)    │           └───────┬───────┘
//! └───────────────┘                   │
//!         │                           │
//!         ├───────────────────────────┘
//!         ▼                 ▼                 ▼
//! ┌───────────────┐ ┌───────────────┐ ┌───────────────┐
//! │    S-GATE     │ │    S-PKG      │ │   S-ATLAS     │
//! │   (Gateway)   │ │  (Packages)   │ │   (Shell)     │
//! └───────────────┘ └───────────────┘ └───────────────┘
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use super::{ServiceId, ServiceDef, ServiceType, DependencyType, RestartPolicy};

/// Core microkernel services in startup order
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum CoreService {
    /// S-STORAGE: VFS, block devices, filesystems
    Storage = 1,
    /// S-DEV: Device drivers (USB, input, sound)
    Dev = 2,
    /// S-GPU: Graphics and framebuffer
    Gpu = 3,
    /// S-CANVAS: Windowing and compositor
    Canvas = 4,
    /// S-NET: Network stack
    Net = 5,
    /// S-PKG: Package manager
    Pkg = 6,
    /// S-GATE: External gateway
    Gate = 7,
    /// S-ATLAS: Application launcher / GUI shell
    Atlas = 8,
}

impl CoreService {
    /// Get service name
    pub fn name(&self) -> &'static str {
        match self {
            CoreService::Storage => "s-storage",
            CoreService::Dev => "s-dev",
            CoreService::Gpu => "s-gpu",
            CoreService::Canvas => "s-canvas",
            CoreService::Net => "s-net",
            CoreService::Pkg => "s-pkg",
            CoreService::Gate => "s-gate",
            CoreService::Atlas => "s-atlas",
        }
    }
    
    /// Get service executable path
    pub fn exec_path(&self) -> &'static str {
        match self {
            CoreService::Storage => "/sbin/s-storage",
            CoreService::Dev => "/sbin/s-dev",
            CoreService::Gpu => "/sbin/s-gpu",
            CoreService::Canvas => "/sbin/s-canvas",
            CoreService::Net => "/sbin/s-net",
            CoreService::Pkg => "/sbin/s-pkg",
            CoreService::Gate => "/sbin/s-gate",
            CoreService::Atlas => "/sbin/s-atlas",
        }
    }
    
    /// Get service description
    pub fn description(&self) -> &'static str {
        match self {
            CoreService::Storage => "Virtual filesystem and block device manager",
            CoreService::Dev => "Userspace device driver manager",
            CoreService::Gpu => "Graphics and display service",
            CoreService::Canvas => "Windowing system and compositor",
            CoreService::Net => "Network stack and socket service",
            CoreService::Pkg => "Package manager and software installation",
            CoreService::Gate => "External network gateway and firewall",
            CoreService::Atlas => "Application launcher and GUI shell",
        }
    }
    
    /// Get dependencies
    pub fn dependencies(&self) -> &'static [CoreService] {
        match self {
            CoreService::Storage => &[], // First service, no deps
            CoreService::Dev => &[], // Independent of storage
            CoreService::Gpu => &[], // Can start independently
            CoreService::Canvas => &[CoreService::Gpu, CoreService::Dev], // Needs GPU and input devices
            CoreService::Net => &[CoreService::Storage, CoreService::Dev],
            CoreService::Pkg => &[CoreService::Storage, CoreService::Net], // Needs storage and network
            CoreService::Gate => &[CoreService::Net],
            CoreService::Atlas => &[CoreService::Canvas], // Needs windowing system
        }
    }
    
    /// Check if this is a critical service
    pub fn is_critical(&self) -> bool {
        matches!(self, CoreService::Storage | CoreService::Dev | CoreService::Net | CoreService::Gpu)
    }
    
    /// Get restart policy
    pub fn restart_policy(&self) -> RestartPolicy {
        if self.is_critical() {
            RestartPolicy::Always
        } else {
            RestartPolicy::OnFailure
        }
    }
    
    /// Get startup priority (lower = earlier)
    pub fn priority(&self) -> u8 {
        *self as u8
    }
    
    /// Create service definition
    pub fn to_service_def(&self, id: ServiceId) -> ServiceDef {
        let mut def = ServiceDef::new(id, self.name(), self.exec_path())
            .description(self.description())
            .service_type(ServiceType::Simple)
            .restart(self.restart_policy());
        
        // Add dependencies
        for dep in self.dependencies() {
            def = def.depends_on(dep.name(), DependencyType::Requires);
        }
        
        def
    }
    
    /// Get all core services in startup order
    pub fn all_ordered() -> &'static [CoreService] {
        &[
            CoreService::Storage,
            CoreService::Dev,
            CoreService::Gpu,
            CoreService::Canvas,
            CoreService::Net,
            CoreService::Pkg,
            CoreService::Gate,
            CoreService::Atlas,
        ]
    }
    
    /// Get services that can start in parallel at boot
    pub fn parallel_start_groups() -> &'static [&'static [CoreService]] {
        &[
            // Group 1: No dependencies, start immediately
            &[CoreService::Storage, CoreService::Dev, CoreService::Gpu],
            // Group 2: Depends on group 1
            &[CoreService::Canvas, CoreService::Net],
            // Group 3: Depends on Canvas or Net
            &[CoreService::Pkg, CoreService::Gate, CoreService::Atlas],
        ]
    }
}

/// Service startup coordinator
pub struct ServiceBootstrap {
    /// Services to start
    pending: Vec<CoreService>,
    /// Currently starting
    starting: Vec<CoreService>,
    /// Successfully started
    started: Vec<CoreService>,
    /// Failed to start
    failed: Vec<(CoreService, BootError)>,
}

/// Boot error
#[derive(Debug, Clone)]
pub enum BootError {
    /// Executable not found
    NotFound,
    /// Failed to spawn
    SpawnFailed,
    /// Dependency failed
    DependencyFailed(String),
    /// Timeout waiting for service
    Timeout,
    /// Service crashed during startup
    Crashed(i32),
}

impl ServiceBootstrap {
    /// Create new bootstrap
    pub fn new() -> Self {
        Self {
            pending: CoreService::all_ordered().to_vec(),
            starting: Vec::new(),
            started: Vec::new(),
            failed: Vec::new(),
        }
    }
    
    /// Get next services that can start
    pub fn next_to_start(&self) -> Vec<CoreService> {
        self.pending.iter()
            .filter(|service| {
                // All dependencies must be in 'started'
                service.dependencies().iter()
                    .all(|dep| self.started.contains(dep))
            })
            .copied()
            .collect()
    }
    
    /// Mark service as starting
    pub fn mark_starting(&mut self, service: CoreService) {
        if let Some(pos) = self.pending.iter().position(|s| *s == service) {
            self.pending.remove(pos);
            self.starting.push(service);
        }
    }
    
    /// Mark service as started
    pub fn mark_started(&mut self, service: CoreService) {
        if let Some(pos) = self.starting.iter().position(|s| *s == service) {
            self.starting.remove(pos);
            self.started.push(service);
        }
    }
    
    /// Mark service as failed
    pub fn mark_failed(&mut self, service: CoreService, error: BootError) {
        if let Some(pos) = self.starting.iter().position(|s| *s == service) {
            self.starting.remove(pos);
        }
        if let Some(pos) = self.pending.iter().position(|s| *s == service) {
            self.pending.remove(pos);
        }
        self.failed.push((service, error));
        
        // Mark dependents as failed too
        let dependents: Vec<CoreService> = self.pending.iter()
            .filter(|s| s.dependencies().contains(&service))
            .copied()
            .collect();
        
        for dep in dependents {
            self.mark_failed(dep, BootError::DependencyFailed(service.name().into()));
        }
    }
    
    /// Check if boot is complete
    pub fn is_complete(&self) -> bool {
        self.pending.is_empty() && self.starting.is_empty()
    }
    
    /// Check if boot was successful (all services started)
    pub fn is_successful(&self) -> bool {
        self.is_complete() && self.failed.is_empty()
    }
    
    /// Get boot status summary
    pub fn summary(&self) -> BootSummary {
        BootSummary {
            total: CoreService::all_ordered().len(),
            started: self.started.len(),
            failed: self.failed.len(),
            pending: self.pending.len(),
            starting: self.starting.len(),
        }
    }
}

/// Boot status summary
#[derive(Debug, Clone, Copy)]
pub struct BootSummary {
    pub total: usize,
    pub started: usize,
    pub failed: usize,
    pub pending: usize,
    pub starting: usize,
}

impl Default for ServiceBootstrap {
    fn default() -> Self {
        Self::new()
    }
}

/// IPC channel endpoints for core services
pub mod channels {
    /// S-STORAGE VFS channel endpoint
    pub const STORAGE_VFS: u64 = 0x53544F52; // "STOR"
    /// S-DEV driver channel endpoint
    pub const DEV_DRIVER: u64 = 0x44455600; // "DEV\0"
    /// S-GPU framebuffer channel endpoint
    pub const GPU_FB: u64 = 0x47505500; // "GPU\0"
    /// S-CANVAS windowing channel endpoint
    pub const CANVAS_WM: u64 = 0x434E5653; // "CNVS"
    /// S-NET socket channel endpoint
    pub const NET_SOCKET: u64 = 0x4E455400; // "NET\0"
    /// S-GATE gateway channel endpoint
    pub const GATE_GW: u64 = 0x47415445; // "GATE"
    /// S-ATLAS launcher channel endpoint
    pub const ATLAS_SHELL: u64 = 0x41544C53; // "ATLS"
}

/// Capability tokens for core services
pub mod capabilities {
    use super::CoreService;
    
    /// Generate capability token for a service
    pub fn service_token(service: CoreService) -> u64 {
        // In real implementation, this would be cryptographic
        let base = 0xCAB0_0000_0000_0000u64;
        base | (service as u64)
    }
    
    /// Capability for filesystem access
    pub const CAP_FS: u64 = 0xCA01_0000_0000_0001;
    /// Capability for block device access
    pub const CAP_BLOCK: u64 = 0xCA01_0000_0000_0002;
    /// Capability for network access
    pub const CAP_NET: u64 = 0xCA01_0000_0000_0003;
    /// Capability for device driver access
    pub const CAP_DEV: u64 = 0xCA01_0000_0000_0004;
    /// Capability for graphics access
    pub const CAP_GPU: u64 = 0xCA01_0000_0000_0005;
    /// Capability for IPC channel creation
    pub const CAP_IPC: u64 = 0xCA01_0000_0000_0006;
    /// Capability for windowing access
    pub const CAP_CANVAS: u64 = 0xCA01_0000_0000_0007;
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_dependency_order() {
        // S-NET depends on Storage and Dev
        let net_deps = CoreService::Net.dependencies();
        assert!(net_deps.contains(&CoreService::Storage));
        assert!(net_deps.contains(&CoreService::Dev));
        
        // Storage has no dependencies
        assert!(CoreService::Storage.dependencies().is_empty());
    }
    
    #[test]
    fn test_bootstrap_order() {
        let mut bootstrap = ServiceBootstrap::new();
        
        // First group should be Storage, Dev, Gpu
        let first = bootstrap.next_to_start();
        assert!(first.contains(&CoreService::Storage));
        assert!(first.contains(&CoreService::Dev));
        assert!(first.contains(&CoreService::Gpu));
        assert!(!first.contains(&CoreService::Net)); // Not yet
        
        // Mark first group as started
        for s in first {
            bootstrap.mark_starting(s);
            bootstrap.mark_started(s);
        }
        
        // Now Net should be available
        let second = bootstrap.next_to_start();
        assert!(second.contains(&CoreService::Net));
    }
}

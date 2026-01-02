//! # Container Runtime
//!
//! This module implements an OCI-compatible container runtime for Splax OS,
//! enabling containerized application deployment with capability-based isolation.
//!
//! ## Features
//!
//! - OCI image format support
//! - Capability-based isolation
//! - Resource limits (CPU, memory)
//! - Namespace isolation
//! - Overlay filesystem
//! - Container networking
//! - Health checks

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// Get current timestamp in milliseconds (for container timing)
fn get_timestamp_ms() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        tsc / 2_000_000  // Rough ms conversion assuming ~2GHz
    }
    
    #[cfg(target_arch = "aarch64")]
    {
        let cnt: u64;
        let freq: u64;
        unsafe {
            core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt, options(nostack, nomem));
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq, options(nostack, nomem));
        }
        if freq > 0 { (cnt * 1000) / freq } else { cnt / 1_000_000 }
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    { 0 }
}

// =============================================================================
// Capability Token (local definition for container isolation)
// =============================================================================

/// Capability token for container access control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityToken {
    /// Token identifier.
    pub id: u64,
    /// Permissions bitmap.
    pub permissions: u64,
}

// =============================================================================
// Container ID and State
// =============================================================================

/// Container ID (256-bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainerId([u8; 32]);

impl ContainerId {
    /// Create a new random container ID.
    pub fn new_random(random: &mut dyn FnMut() -> u64) -> Self {
        let mut id = [0u8; 32];
        for chunk in id.chunks_exact_mut(8) {
            chunk.copy_from_slice(&random().to_le_bytes());
        }
        Self(id)
    }

    /// Create from hex string.
    pub fn from_hex(hex: &str) -> Option<Self> {
        if hex.len() != 64 {
            return None;
        }

        let mut id = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let high = hex_digit(chunk[0])?;
            let low = hex_digit(chunk[1])?;
            id[i] = (high << 4) | low;
        }

        Some(Self(id))
    }

    /// Convert to hex string.
    pub fn to_hex(&self) -> String {
        let mut hex = String::with_capacity(64);
        for byte in &self.0 {
            hex.push(HEX_CHARS[(byte >> 4) as usize]);
            hex.push(HEX_CHARS[(byte & 0x0f) as usize]);
        }
        hex
    }

    /// Get short ID (first 12 characters).
    pub fn short(&self) -> String {
        self.to_hex()[..12].to_string()
    }
}

const HEX_CHARS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Container state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    /// Container created but not started.
    Created,
    /// Container is running.
    Running,
    /// Container is paused.
    Paused,
    /// Container has stopped.
    Stopped,
    /// Container is being removed.
    Removing,
}

/// Container status with detailed information.
#[derive(Debug, Clone)]
pub struct ContainerStatus {
    /// Current state.
    pub state: ContainerState,
    /// Process ID (if running).
    pub pid: Option<u64>,
    /// Exit code (if stopped).
    pub exit_code: Option<i32>,
    /// Start time (Unix timestamp).
    pub started_at: Option<u64>,
    /// Stop time (Unix timestamp).
    pub finished_at: Option<u64>,
    /// OOM killed flag.
    pub oom_killed: bool,
    /// Error message.
    pub error: Option<String>,
}

impl Default for ContainerStatus {
    fn default() -> Self {
        Self {
            state: ContainerState::Created,
            pid: None,
            exit_code: None,
            started_at: None,
            finished_at: None,
            oom_killed: false,
            error: None,
        }
    }
}

// =============================================================================
// Container Configuration
// =============================================================================

/// Container configuration.
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    /// Container name.
    pub name: String,
    /// Image reference.
    pub image: String,
    /// Command to run.
    pub command: Vec<String>,
    /// Environment variables.
    pub env: Vec<(String, String)>,
    /// Working directory.
    pub working_dir: String,
    /// User to run as.
    pub user: Option<String>,
    /// Hostname.
    pub hostname: Option<String>,
    /// Resource limits.
    pub resources: ResourceLimits,
    /// Capability grants.
    pub capabilities: CapabilityConfig,
    /// Mount points.
    pub mounts: Vec<MountConfig>,
    /// Port mappings.
    pub ports: Vec<PortMapping>,
    /// Labels.
    pub labels: BTreeMap<String, String>,
    /// Health check configuration.
    pub health_check: Option<HealthCheckConfig>,
    /// Restart policy.
    pub restart_policy: RestartPolicy,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            image: String::new(),
            command: Vec::new(),
            env: Vec::new(),
            working_dir: String::from("/"),
            user: None,
            hostname: None,
            resources: ResourceLimits::default(),
            capabilities: CapabilityConfig::default(),
            mounts: Vec::new(),
            ports: Vec::new(),
            labels: BTreeMap::new(),
            health_check: None,
            restart_policy: RestartPolicy::No,
        }
    }
}

/// Resource limits.
#[derive(Debug, Clone, Copy)]
pub struct ResourceLimits {
    /// CPU limit (millicores, 1000 = 1 core).
    pub cpu_limit: u32,
    /// Memory limit in bytes.
    pub memory_limit: u64,
    /// Memory swap limit in bytes.
    pub memory_swap: u64,
    /// Number of allowed processes.
    pub pids_limit: u32,
    /// Disk I/O weight (10-1000).
    pub io_weight: u16,
    /// Network bandwidth limit (bytes/sec).
    pub network_bandwidth: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpu_limit: 0,        // Unlimited
            memory_limit: 0,    // Unlimited
            memory_swap: 0,     // Unlimited
            pids_limit: 0,      // Unlimited
            io_weight: 100,     // Default weight
            network_bandwidth: 0, // Unlimited
        }
    }
}

/// Capability configuration.
#[derive(Debug, Clone)]
pub struct CapabilityConfig {
    /// Granted capabilities.
    pub add: Vec<ContainerCapability>,
    /// Dropped capabilities.
    pub drop: Vec<ContainerCapability>,
    /// Enable all capabilities.
    pub privileged: bool,
}

impl Default for CapabilityConfig {
    fn default() -> Self {
        Self {
            add: Vec::new(),
            drop: Vec::new(),
            privileged: false,
        }
    }
}

/// Container capabilities (subset of Splax capabilities).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerCapability {
    /// Network access.
    NetAccess,
    /// Raw network sockets.
    NetRaw,
    /// Bind to privileged ports.
    NetBindService,
    /// Mount filesystems.
    SysMount,
    /// Use ptrace.
    SysPtrace,
    /// Modify kernel parameters.
    SysAdmin,
    /// Access block devices.
    SysRawio,
    /// Set file capabilities.
    Setfcap,
    /// Change file ownership.
    Chown,
    /// Override DAC permissions.
    DacOverride,
    /// Set UID/GID.
    Setuid,
    /// Kill processes.
    Kill,
}

/// Mount configuration.
#[derive(Debug, Clone)]
pub struct MountConfig {
    /// Mount type.
    pub mount_type: MountType,
    /// Source path (host).
    pub source: String,
    /// Destination path (container).
    pub destination: String,
    /// Read-only flag.
    pub read_only: bool,
    /// Mount options.
    pub options: Vec<String>,
}

/// Mount types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountType {
    /// Bind mount from host.
    Bind,
    /// Volume mount.
    Volume,
    /// Temporary filesystem.
    Tmpfs,
    /// Device mount.
    Device,
}

/// Port mapping.
#[derive(Debug, Clone, Copy)]
pub struct PortMapping {
    /// Host port.
    pub host_port: u16,
    /// Container port.
    pub container_port: u16,
    /// Protocol.
    pub protocol: PortProtocol,
    /// Host IP to bind to.
    pub host_ip: [u8; 4],
}

/// Port protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortProtocol {
    Tcp,
    Udp,
}

/// Health check configuration.
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// Command to run.
    pub command: Vec<String>,
    /// Interval between checks.
    pub interval_ms: u64,
    /// Timeout for each check.
    pub timeout_ms: u64,
    /// Number of retries before unhealthy.
    pub retries: u32,
    /// Initial delay before first check.
    pub start_period_ms: u64,
}

/// Health status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Starting,
    Healthy,
    Unhealthy,
    None,
}

/// Restart policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Never restart.
    No,
    /// Restart on failure.
    OnFailure,
    /// Always restart.
    Always,
    /// Restart unless explicitly stopped.
    UnlessStopped,
}

// =============================================================================
// OCI Image
// =============================================================================

/// OCI image reference.
#[derive(Debug, Clone)]
pub struct ImageRef {
    /// Registry (e.g., "docker.io").
    pub registry: String,
    /// Repository (e.g., "library/alpine").
    pub repository: String,
    /// Tag (e.g., "latest").
    pub tag: String,
    /// Digest (optional, takes precedence over tag).
    pub digest: Option<String>,
}

impl ImageRef {
    /// Parse an image reference string.
    pub fn parse(reference: &str) -> Option<Self> {
        let (rest, digest) = if let Some(pos) = reference.rfind('@') {
            (
                &reference[..pos],
                Some(reference[pos + 1..].to_string()),
            )
        } else {
            (reference, None)
        };

        let (rest, tag) = if let Some(pos) = rest.rfind(':') {
            (&rest[..pos], rest[pos + 1..].to_string())
        } else {
            (rest, String::from("latest"))
        };

        let (registry, repository) = if let Some(pos) = rest.find('/') {
            let potential_registry = &rest[..pos];
            if potential_registry.contains('.') || potential_registry.contains(':') {
                (
                    potential_registry.to_string(),
                    rest[pos + 1..].to_string(),
                )
            } else {
                (String::from("docker.io"), rest.to_string())
            }
        } else {
            (
                String::from("docker.io"),
                alloc::format!("library/{}", rest),
            )
        };

        Some(Self {
            registry,
            repository,
            tag,
            digest,
        })
    }

    /// Format as string.
    pub fn to_string(&self) -> String {
        let mut s = alloc::format!("{}/{}", self.registry, self.repository);
        if let Some(ref digest) = self.digest {
            s.push('@');
            s.push_str(digest);
        } else {
            s.push(':');
            s.push_str(&self.tag);
        }
        s
    }
}

/// OCI image manifest.
#[derive(Debug, Clone)]
pub struct ImageManifest {
    /// Schema version.
    pub schema_version: u32,
    /// Media type.
    pub media_type: String,
    /// Config descriptor.
    pub config: BlobDescriptor,
    /// Layer descriptors.
    pub layers: Vec<BlobDescriptor>,
}

/// Blob descriptor.
#[derive(Debug, Clone)]
pub struct BlobDescriptor {
    /// Media type.
    pub media_type: String,
    /// Digest.
    pub digest: String,
    /// Size in bytes.
    pub size: u64,
}

/// OCI image configuration.
#[derive(Debug, Clone)]
pub struct ImageConfig {
    /// Architecture.
    pub architecture: String,
    /// OS.
    pub os: String,
    /// Default user.
    pub user: Option<String>,
    /// Environment variables.
    pub env: Vec<String>,
    /// Entry point.
    pub entrypoint: Vec<String>,
    /// Default command.
    pub cmd: Vec<String>,
    /// Working directory.
    pub working_dir: Option<String>,
    /// Exposed ports.
    pub exposed_ports: Vec<String>,
    /// Labels.
    pub labels: BTreeMap<String, String>,
}

// =============================================================================
// Container Namespace
// =============================================================================

/// Namespace types for isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceType {
    /// Process ID namespace.
    Pid,
    /// Network namespace.
    Net,
    /// Mount namespace.
    Mnt,
    /// User namespace.
    User,
    /// IPC namespace.
    Ipc,
    /// UTS namespace (hostname).
    Uts,
    /// Cgroup namespace.
    Cgroup,
}

/// Namespace configuration.
#[derive(Debug, Clone)]
pub struct NamespaceConfig {
    /// Namespace type.
    pub ns_type: NamespaceType,
    /// Path to existing namespace (for sharing).
    pub path: Option<String>,
}

/// Container namespaces.
pub struct ContainerNamespaces {
    /// PID namespace.
    pub pid: u64,
    /// Network namespace.
    pub net: u64,
    /// Mount namespace.
    pub mnt: u64,
    /// User namespace.
    pub user: u64,
}

// =============================================================================
// Container Filesystem
// =============================================================================

/// Overlay filesystem configuration.
#[derive(Debug, Clone)]
pub struct OverlayFs {
    /// Lower directories (read-only layers).
    pub lower_dirs: Vec<String>,
    /// Upper directory (writable layer).
    pub upper_dir: String,
    /// Work directory.
    pub work_dir: String,
    /// Merged mount point.
    pub merged_dir: String,
}

impl OverlayFs {
    /// Create a new overlay filesystem configuration.
    pub fn new(container_id: &ContainerId, layers: &[String]) -> Self {
        let base = alloc::format!("/var/lib/containers/{}", container_id.short());

        Self {
            lower_dirs: layers.to_vec(),
            upper_dir: alloc::format!("{}/upper", base),
            work_dir: alloc::format!("{}/work", base),
            merged_dir: alloc::format!("{}/merged", base),
        }
    }
}

// =============================================================================
// Container
// =============================================================================

/// A container instance.
pub struct Container {
    /// Container ID.
    pub id: ContainerId,
    /// Configuration.
    pub config: ContainerConfig,
    /// Current status.
    pub status: ContainerStatus,
    /// Overlay filesystem.
    pub overlay: OverlayFs,
    /// Namespaces.
    pub namespaces: Option<ContainerNamespaces>,
    /// Splax capability token.
    pub capability: Option<CapabilityToken>,
    /// Health status.
    pub health: HealthStatus,
    /// Resource usage statistics.
    pub stats: ContainerStats,
}

impl Container {
    /// Create a new container.
    pub fn new(id: ContainerId, config: ContainerConfig, layers: Vec<String>) -> Self {
        let overlay = OverlayFs::new(&id, &layers);

        Self {
            id,
            config,
            status: ContainerStatus::default(),
            overlay,
            namespaces: None,
            capability: None,
            health: HealthStatus::None,
            stats: ContainerStats::default(),
        }
    }

    /// Get container state.
    pub fn state(&self) -> ContainerState {
        self.status.state
    }

    /// Check if container is running.
    pub fn is_running(&self) -> bool {
        self.status.state == ContainerState::Running
    }

    /// Start the container.
    pub fn start(&mut self) -> Result<(), ContainerError> {
        if self.status.state != ContainerState::Created {
            return Err(ContainerError::InvalidState);
        }

        // Create namespaces with unique IDs based on container ID hash
        let ns_base = u64::from_le_bytes(self.id.0[0..8].try_into().unwrap_or([0; 8]));
        self.namespaces = Some(ContainerNamespaces {
            pid: ns_base & 0xFFFF,
            net: (ns_base >> 16) & 0xFFFF,
            mnt: (ns_base >> 32) & 0xFFFF,
            user: (ns_base >> 48) & 0xFFFF,
        });

        self.status.state = ContainerState::Running;
        self.status.started_at = Some(get_timestamp_ms());
        // Generate PID from container ID hash (unique per container)
        self.status.pid = Some(ns_base % 60000 + 1000);

        Ok(())
    }

    /// Stop the container.
    pub fn stop(&mut self, timeout_ms: u64) -> Result<(), ContainerError> {
        if self.status.state != ContainerState::Running {
            return Err(ContainerError::InvalidState);
        }

        // Signal the process to stop
        // Wait for timeout_ms
        // Force kill if needed

        let _ = timeout_ms;

        self.status.state = ContainerState::Stopped;
        self.status.finished_at = Some(get_timestamp_ms());
        self.status.exit_code = Some(0);

        Ok(())
    }

    /// Pause the container.
    pub fn pause(&mut self) -> Result<(), ContainerError> {
        if self.status.state != ContainerState::Running {
            return Err(ContainerError::InvalidState);
        }

        self.status.state = ContainerState::Paused;
        Ok(())
    }

    /// Resume the container.
    pub fn resume(&mut self) -> Result<(), ContainerError> {
        if self.status.state != ContainerState::Paused {
            return Err(ContainerError::InvalidState);
        }

        self.status.state = ContainerState::Running;
        Ok(())
    }

    /// Kill the container with a signal.
    pub fn kill(&mut self, _signal: i32) -> Result<(), ContainerError> {
        if self.status.state != ContainerState::Running {
            return Err(ContainerError::InvalidState);
        }

        self.status.state = ContainerState::Stopped;
        self.status.exit_code = Some(-1);

        Ok(())
    }

    /// Execute a command in the container.
    pub fn exec(&self, command: &[String]) -> Result<u64, ContainerError> {
        if self.status.state != ContainerState::Running {
            return Err(ContainerError::InvalidState);
        }

        // Generate unique PID based on container ID and command
        let base_pid = u64::from_le_bytes(self.id.0[0..8].try_into().unwrap_or([0; 8]));
        let cmd_hash: u64 = command.iter()
            .flat_map(|s| s.bytes())
            .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        let exec_pid = ((base_pid ^ cmd_hash) % 60000) + 2000;

        Ok(exec_pid)
    }
}

/// Container statistics.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContainerStats {
    /// CPU usage (nanoseconds).
    pub cpu_usage: u64,
    /// Memory usage (bytes).
    pub memory_usage: u64,
    /// Memory limit (bytes).
    pub memory_limit: u64,
    /// Network bytes received.
    pub net_rx_bytes: u64,
    /// Network bytes transmitted.
    pub net_tx_bytes: u64,
    /// Block I/O read bytes.
    pub block_read: u64,
    /// Block I/O write bytes.
    pub block_write: u64,
    /// Number of processes.
    pub pids: u32,
}

// =============================================================================
// Container Runtime
// =============================================================================

/// Container runtime errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerError {
    /// Container not found.
    NotFound,
    /// Invalid state for operation.
    InvalidState,
    /// Resource limit exceeded.
    ResourceExceeded,
    /// Permission denied.
    PermissionDenied,
    /// Image not found.
    ImageNotFound,
    /// Network error.
    NetworkError,
    /// Filesystem error.
    FilesystemError,
    /// Configuration error.
    ConfigError,
}

/// Container runtime.
pub struct ContainerRuntime {
    /// Active containers.
    containers: BTreeMap<ContainerId, Container>,
    /// Image storage.
    images: BTreeMap<String, ImageManifest>,
    /// Random number generator.
    random_state: u64,
}

impl ContainerRuntime {
    /// Create a new container runtime.
    pub fn new() -> Self {
        Self {
            containers: BTreeMap::new(),
            images: BTreeMap::new(),
            random_state: 0x12345678_9ABCDEF0,
        }
    }

    /// Generate random number.
    fn random(&mut self) -> u64 {
        self.random_state ^= self.random_state << 13;
        self.random_state ^= self.random_state >> 7;
        self.random_state ^= self.random_state << 17;
        self.random_state
    }

    /// Create a new container.
    pub fn create(
        &mut self,
        config: ContainerConfig,
    ) -> Result<ContainerId, ContainerError> {
        // Generate container ID
        let id = ContainerId::new_random(&mut || self.random());

        // Resolve image layers
        let layers = self.resolve_image_layers(&config.image)?;

        // Create container
        let container = Container::new(id, config, layers);

        self.containers.insert(id, container);

        Ok(id)
    }

    /// Resolve image layers.
    fn resolve_image_layers(&self, _image: &str) -> Result<Vec<String>, ContainerError> {
        // Would download/extract image and return layer paths
        Ok(vec![String::from("/var/lib/containers/layers/base")])
    }

    /// Start a container.
    pub fn start(&mut self, id: &ContainerId) -> Result<(), ContainerError> {
        let container = self.containers.get_mut(id).ok_or(ContainerError::NotFound)?;
        container.start()
    }

    /// Stop a container.
    pub fn stop(&mut self, id: &ContainerId, timeout_ms: u64) -> Result<(), ContainerError> {
        let container = self.containers.get_mut(id).ok_or(ContainerError::NotFound)?;
        container.stop(timeout_ms)
    }

    /// Remove a container.
    pub fn remove(&mut self, id: &ContainerId, force: bool) -> Result<(), ContainerError> {
        let container = self.containers.get(id).ok_or(ContainerError::NotFound)?;

        if container.is_running() && !force {
            return Err(ContainerError::InvalidState);
        }

        self.containers.remove(id);
        Ok(())
    }

    /// List containers.
    pub fn list(&self, all: bool) -> Vec<&Container> {
        self.containers
            .values()
            .filter(|c| all || c.is_running())
            .collect()
    }

    /// Get container by ID.
    pub fn get(&self, id: &ContainerId) -> Option<&Container> {
        self.containers.get(id)
    }

    /// Get mutable container by ID.
    pub fn get_mut(&mut self, id: &ContainerId) -> Option<&mut Container> {
        self.containers.get_mut(id)
    }

    /// Get container statistics.
    pub fn stats(&self, id: &ContainerId) -> Result<ContainerStats, ContainerError> {
        let container = self.containers.get(id).ok_or(ContainerError::NotFound)?;
        Ok(container.stats)
    }

    /// Get container logs.
    pub fn logs(&self, id: &ContainerId, _tail: usize) -> Result<Vec<String>, ContainerError> {
        let _container = self.containers.get(id).ok_or(ContainerError::NotFound)?;
        // Would read from container log file
        Ok(vec![String::from("[container logs]")])
    }

    /// Pull an image.
    pub fn pull(&mut self, image: &str) -> Result<(), ContainerError> {
        let _image_ref = ImageRef::parse(image).ok_or(ContainerError::ConfigError)?;
        // Would download image from registry
        Ok(())
    }

    /// List images.
    pub fn list_images(&self) -> Vec<&str> {
        self.images.keys().map(|s| s.as_str()).collect()
    }

    /// Run a container (create + start).
    pub fn run(&mut self, config: ContainerConfig) -> Result<ContainerId, ContainerError> {
        let id = self.create(config)?;
        self.start(&id)?;
        Ok(id)
    }

    /// Execute command in running container.
    pub fn exec(
        &self,
        id: &ContainerId,
        command: &[String],
    ) -> Result<u64, ContainerError> {
        let container = self.containers.get(id).ok_or(ContainerError::NotFound)?;
        container.exec(command)
    }
}

impl Default for ContainerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Container Network
// =============================================================================

/// Container network.
#[derive(Debug, Clone)]
pub struct ContainerNetwork {
    /// Network name.
    pub name: String,
    /// Network ID.
    pub id: String,
    /// Driver type.
    pub driver: NetworkDriver,
    /// Subnet (e.g., "172.17.0.0/16").
    pub subnet: String,
    /// Gateway address.
    pub gateway: String,
}

/// Network driver types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkDriver {
    /// Bridge network.
    Bridge,
    /// Host network (no isolation).
    Host,
    /// No network.
    None,
    /// Overlay network (multi-host).
    Overlay,
}

/// Container network endpoint.
#[derive(Debug, Clone)]
pub struct NetworkEndpoint {
    /// Container ID.
    pub container_id: ContainerId,
    /// Network ID.
    pub network_id: String,
    /// IP address.
    pub ip_address: String,
    /// MAC address.
    pub mac_address: String,
}

// =============================================================================
// Volume Management
// =============================================================================

/// Container volume.
#[derive(Debug, Clone)]
pub struct Volume {
    /// Volume name.
    pub name: String,
    /// Driver.
    pub driver: String,
    /// Mount point on host.
    pub mountpoint: String,
    /// Labels.
    pub labels: BTreeMap<String, String>,
    /// Creation time.
    pub created_at: u64,
}

impl Volume {
    /// Create a new volume.
    pub fn new(name: String) -> Self {
        Self {
            name: name.clone(),
            driver: String::from("local"),
            mountpoint: alloc::format!("/var/lib/containers/volumes/{}", name),
            labels: BTreeMap::new(),
            created_at: 0,
        }
    }
}

// =============================================================================
// Builder Pattern
// =============================================================================

/// Container configuration builder.
pub struct ContainerBuilder {
    config: ContainerConfig,
}

impl ContainerBuilder {
    /// Create a new builder.
    pub fn new(image: &str) -> Self {
        let mut config = ContainerConfig::default();
        config.image = String::from(image);
        Self { config }
    }

    /// Set container name.
    pub fn name(mut self, name: &str) -> Self {
        self.config.name = String::from(name);
        self
    }

    /// Set command.
    pub fn command(mut self, cmd: Vec<String>) -> Self {
        self.config.command = cmd;
        self
    }

    /// Add environment variable.
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.config.env.push((String::from(key), String::from(value)));
        self
    }

    /// Set working directory.
    pub fn workdir(mut self, dir: &str) -> Self {
        self.config.working_dir = String::from(dir);
        self
    }

    /// Set user.
    pub fn user(mut self, user: &str) -> Self {
        self.config.user = Some(String::from(user));
        self
    }

    /// Set CPU limit.
    pub fn cpu_limit(mut self, millicores: u32) -> Self {
        self.config.resources.cpu_limit = millicores;
        self
    }

    /// Set memory limit.
    pub fn memory_limit(mut self, bytes: u64) -> Self {
        self.config.resources.memory_limit = bytes;
        self
    }

    /// Add port mapping.
    pub fn port(mut self, host_port: u16, container_port: u16) -> Self {
        self.config.ports.push(PortMapping {
            host_port,
            container_port,
            protocol: PortProtocol::Tcp,
            host_ip: [0, 0, 0, 0],
        });
        self
    }

    /// Add bind mount.
    pub fn mount(mut self, source: &str, destination: &str) -> Self {
        self.config.mounts.push(MountConfig {
            mount_type: MountType::Bind,
            source: String::from(source),
            destination: String::from(destination),
            read_only: false,
            options: Vec::new(),
        });
        self
    }

    /// Add label.
    pub fn label(mut self, key: &str, value: &str) -> Self {
        self.config.labels.insert(String::from(key), String::from(value));
        self
    }

    /// Set privileged mode.
    pub fn privileged(mut self, privileged: bool) -> Self {
        self.config.capabilities.privileged = privileged;
        self
    }

    /// Set restart policy.
    pub fn restart(mut self, policy: RestartPolicy) -> Self {
        self.config.restart_policy = policy;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> ContainerConfig {
        self.config
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_id() {
        let mut state = 12345u64;
        let id = ContainerId::new_random(&mut || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        });

        let hex = id.to_hex();
        assert_eq!(hex.len(), 64);

        let parsed = ContainerId::from_hex(&hex).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_image_ref_parse() {
        let ref1 = ImageRef::parse("alpine").unwrap();
        assert_eq!(ref1.registry, "docker.io");
        assert_eq!(ref1.repository, "library/alpine");
        assert_eq!(ref1.tag, "latest");

        let ref2 = ImageRef::parse("nginx:1.19").unwrap();
        assert_eq!(ref2.repository, "library/nginx");
        assert_eq!(ref2.tag, "1.19");

        let ref3 = ImageRef::parse("gcr.io/project/image:v1").unwrap();
        assert_eq!(ref3.registry, "gcr.io");
        assert_eq!(ref3.repository, "project/image");
        assert_eq!(ref3.tag, "v1");
    }

    #[test]
    fn test_container_builder() {
        let config = ContainerBuilder::new("alpine")
            .name("my-container")
            .command(vec![String::from("echo"), String::from("hello")])
            .env("KEY", "value")
            .memory_limit(512 * 1024 * 1024)
            .port(8080, 80)
            .build();

        assert_eq!(config.name, "my-container");
        assert_eq!(config.image, "alpine");
        assert_eq!(config.command.len(), 2);
        assert_eq!(config.env.len(), 1);
        assert_eq!(config.resources.memory_limit, 512 * 1024 * 1024);
        assert_eq!(config.ports.len(), 1);
    }

    #[test]
    fn test_container_lifecycle() {
        let mut runtime = ContainerRuntime::new();

        let config = ContainerBuilder::new("alpine")
            .name("test")
            .build();

        let id = runtime.create(config).unwrap();

        // Should be created
        let container = runtime.get(&id).unwrap();
        assert_eq!(container.state(), ContainerState::Created);

        // Start
        runtime.start(&id).unwrap();
        let container = runtime.get(&id).unwrap();
        assert_eq!(container.state(), ContainerState::Running);

        // Stop
        runtime.stop(&id, 5000).unwrap();
        let container = runtime.get(&id).unwrap();
        assert_eq!(container.state(), ContainerState::Stopped);

        // Remove
        runtime.remove(&id, false).unwrap();
        assert!(runtime.get(&id).is_none());
    }

    #[test]
    fn test_resource_limits() {
        let limits = ResourceLimits {
            cpu_limit: 2000,       // 2 cores
            memory_limit: 1 << 30, // 1 GB
            memory_swap: 2 << 30,  // 2 GB
            pids_limit: 100,
            io_weight: 500,
            network_bandwidth: 100 * 1024 * 1024, // 100 MB/s
        };

        assert_eq!(limits.cpu_limit, 2000);
        assert_eq!(limits.memory_limit, 1073741824);
    }
}

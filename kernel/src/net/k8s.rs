//! # Kubernetes Node Support
//!
//! This module implements Kubernetes node integration for Splax OS,
//! allowing Splax OS to act as a Kubernetes node (kubelet equivalent).
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Kubernetes API Server                        │
//! └─────────────────────────────────────────────────────────────────┘
//!                                │
//!                         gRPC/HTTPS
//!                                │
//!                                ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      S-KUBELET (This Module)                    │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
//! │  │ Pod Manager │  │   CRI Shim  │  │  CNI Plugin │              │
//! │  └─────────────┘  └─────────────┘  └─────────────┘              │
//! └─────────────────────────────────────────────────────────────────┘
//!                                │
//!                                ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Splax Container Runtime                      │
//! │              (OCI-compatible, capability-based)                 │
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
// Kubernetes Types
// =============================================================================

/// Unique identifier for a Kubernetes pod
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PodId(pub String);

/// Unique identifier for a container within a pod
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainerId(pub String);

/// Kubernetes namespace
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Namespace(pub String);

impl Default for Namespace {
    fn default() -> Self {
        Namespace("default".to_string())
    }
}

/// Pod phase (lifecycle state)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodPhase {
    /// Pod has been accepted but containers not yet created
    Pending,
    /// All containers have been created, at least one is running
    Running,
    /// All containers terminated successfully
    Succeeded,
    /// All containers terminated, at least one failed
    Failed,
    /// Pod state unknown
    Unknown,
}

/// Container state
#[derive(Debug, Clone)]
pub enum ContainerState {
    /// Container is waiting to start
    Waiting { reason: String },
    /// Container is running
    Running { started_at: u64 },
    /// Container has terminated
    Terminated { exit_code: i32, reason: String, finished_at: u64 },
}

/// Container restart policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Always restart containers
    Always,
    /// Restart only on failure
    OnFailure,
    /// Never restart
    Never,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy::Always
    }
}

/// Resource requirements for a container
#[derive(Debug, Clone)]
pub struct ResourceRequirements {
    /// CPU request in millicores (1000 = 1 CPU)
    pub cpu_request: u64,
    /// CPU limit in millicores
    pub cpu_limit: u64,
    /// Memory request in bytes
    pub memory_request: u64,
    /// Memory limit in bytes
    pub memory_limit: u64,
    /// Ephemeral storage request in bytes
    pub storage_request: u64,
    /// Ephemeral storage limit in bytes
    pub storage_limit: u64,
}

impl Default for ResourceRequirements {
    fn default() -> Self {
        Self {
            cpu_request: 100,      // 0.1 CPU
            cpu_limit: 1000,       // 1 CPU
            memory_request: 64 * 1024 * 1024,  // 64 MB
            memory_limit: 256 * 1024 * 1024,   // 256 MB
            storage_request: 0,
            storage_limit: 1024 * 1024 * 1024, // 1 GB
        }
    }
}

/// Container port definition
#[derive(Debug, Clone)]
pub struct ContainerPort {
    /// Name of the port
    pub name: Option<String>,
    /// Container port number
    pub container_port: u16,
    /// Host port (if mapped)
    pub host_port: Option<u16>,
    /// Protocol (TCP/UDP)
    pub protocol: PortProtocol,
}

/// Port protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortProtocol {
    Tcp,
    Udp,
    Sctp,
}

impl Default for PortProtocol {
    fn default() -> Self {
        PortProtocol::Tcp
    }
}

/// Environment variable definition
#[derive(Debug, Clone)]
pub struct EnvVar {
    /// Variable name
    pub name: String,
    /// Variable value
    pub value: String,
}

/// Volume mount definition
#[derive(Debug, Clone)]
pub struct VolumeMount {
    /// Name of the volume
    pub name: String,
    /// Path in the container
    pub mount_path: String,
    /// Read-only mount
    pub read_only: bool,
    /// Subpath within the volume
    pub sub_path: Option<String>,
}

/// Container specification
#[derive(Debug, Clone)]
pub struct ContainerSpec {
    /// Container name
    pub name: String,
    /// Container image (OCI reference)
    pub image: String,
    /// Image pull policy
    pub image_pull_policy: ImagePullPolicy,
    /// Command to run
    pub command: Vec<String>,
    /// Arguments to the command
    pub args: Vec<String>,
    /// Working directory
    pub working_dir: Option<String>,
    /// Environment variables
    pub env: Vec<EnvVar>,
    /// Ports to expose
    pub ports: Vec<ContainerPort>,
    /// Volume mounts
    pub volume_mounts: Vec<VolumeMount>,
    /// Resource requirements
    pub resources: ResourceRequirements,
    /// Liveness probe
    pub liveness_probe: Option<Probe>,
    /// Readiness probe
    pub readiness_probe: Option<Probe>,
    /// Startup probe
    pub startup_probe: Option<Probe>,
}

/// Image pull policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagePullPolicy {
    /// Always pull the image
    Always,
    /// Pull only if not present
    IfNotPresent,
    /// Never pull
    Never,
}

impl Default for ImagePullPolicy {
    fn default() -> Self {
        ImagePullPolicy::IfNotPresent
    }
}

/// Health check probe
#[derive(Debug, Clone)]
pub struct Probe {
    /// Probe handler
    pub handler: ProbeHandler,
    /// Delay before starting probes
    pub initial_delay_seconds: u32,
    /// Period between probes
    pub period_seconds: u32,
    /// Timeout for probe
    pub timeout_seconds: u32,
    /// Success threshold
    pub success_threshold: u32,
    /// Failure threshold
    pub failure_threshold: u32,
}

/// Probe handler type
#[derive(Debug, Clone)]
pub enum ProbeHandler {
    /// HTTP GET probe
    HttpGet { path: String, port: u16, scheme: String },
    /// TCP socket probe
    TcpSocket { port: u16 },
    /// Exec probe
    Exec { command: Vec<String> },
    /// gRPC probe
    Grpc { port: u16, service: Option<String> },
}

/// Pod specification
#[derive(Debug, Clone)]
pub struct PodSpec {
    /// Pod namespace
    pub namespace: Namespace,
    /// Pod name
    pub name: String,
    /// Pod labels
    pub labels: BTreeMap<String, String>,
    /// Pod annotations
    pub annotations: BTreeMap<String, String>,
    /// Containers
    pub containers: Vec<ContainerSpec>,
    /// Init containers
    pub init_containers: Vec<ContainerSpec>,
    /// Restart policy
    pub restart_policy: RestartPolicy,
    /// Termination grace period in seconds
    pub termination_grace_period: u64,
    /// Node selector
    pub node_selector: BTreeMap<String, String>,
    /// Service account name
    pub service_account: String,
    /// Host network mode
    pub host_network: bool,
    /// DNS policy
    pub dns_policy: DnsPolicy,
}

/// DNS policy for pods
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsPolicy {
    /// Use cluster DNS
    ClusterFirst,
    /// Use cluster DNS with host fallback
    ClusterFirstWithHostNet,
    /// Use host DNS
    Default,
    /// No DNS
    None,
}

impl Default for DnsPolicy {
    fn default() -> Self {
        DnsPolicy::ClusterFirst
    }
}

/// Container status
#[derive(Debug, Clone)]
pub struct ContainerStatus {
    /// Container name
    pub name: String,
    /// Container ID
    pub container_id: Option<ContainerId>,
    /// Container state
    pub state: ContainerState,
    /// Last terminated state
    pub last_state: Option<ContainerState>,
    /// Ready flag
    pub ready: bool,
    /// Restart count
    pub restart_count: u32,
    /// Image being used
    pub image: String,
    /// Image ID
    pub image_id: String,
    /// Container started
    pub started: bool,
}

/// Pod status
#[derive(Debug, Clone)]
pub struct PodStatus {
    /// Pod phase
    pub phase: PodPhase,
    /// Pod IP address
    pub pod_ip: Option<String>,
    /// Host IP address
    pub host_ip: Option<String>,
    /// Pod start time
    pub start_time: Option<u64>,
    /// Container statuses
    pub container_statuses: Vec<ContainerStatus>,
    /// Init container statuses
    pub init_container_statuses: Vec<ContainerStatus>,
    /// Conditions
    pub conditions: Vec<PodCondition>,
}

/// Pod condition
#[derive(Debug, Clone)]
pub struct PodCondition {
    /// Condition type
    pub condition_type: PodConditionType,
    /// Condition status
    pub status: bool,
    /// Last probe time
    pub last_probe_time: Option<u64>,
    /// Last transition time
    pub last_transition_time: u64,
    /// Reason
    pub reason: Option<String>,
    /// Message
    pub message: Option<String>,
}

/// Pod condition types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodConditionType {
    /// Pod scheduled to a node
    PodScheduled,
    /// All containers ready
    ContainersReady,
    /// Init containers completed
    Initialized,
    /// Pod ready to serve
    Ready,
}

// =============================================================================
// Node Information
// =============================================================================

/// Node information reported to Kubernetes
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// Node name
    pub name: String,
    /// Node labels
    pub labels: BTreeMap<String, String>,
    /// Node annotations
    pub annotations: BTreeMap<String, String>,
    /// Node capacity
    pub capacity: NodeResources,
    /// Node allocatable resources
    pub allocatable: NodeResources,
    /// Node conditions
    pub conditions: Vec<NodeCondition>,
    /// Node addresses
    pub addresses: Vec<NodeAddress>,
    /// Kubelet version (Splax version)
    pub kubelet_version: String,
    /// Container runtime version
    pub container_runtime_version: String,
    /// OS image
    pub os_image: String,
    /// Architecture
    pub architecture: String,
}

/// Node resources
#[derive(Debug, Clone)]
pub struct NodeResources {
    /// CPU in millicores
    pub cpu: u64,
    /// Memory in bytes
    pub memory: u64,
    /// Ephemeral storage in bytes
    pub ephemeral_storage: u64,
    /// Number of pods
    pub pods: u32,
}

/// Node condition
#[derive(Debug, Clone)]
pub struct NodeCondition {
    /// Condition type
    pub condition_type: NodeConditionType,
    /// Condition status
    pub status: bool,
    /// Last heartbeat time
    pub last_heartbeat_time: u64,
    /// Last transition time
    pub last_transition_time: u64,
    /// Reason
    pub reason: String,
    /// Message
    pub message: String,
}

/// Node condition types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeConditionType {
    /// Node is ready
    Ready,
    /// Node has sufficient memory
    MemoryPressure,
    /// Node has sufficient disk
    DiskPressure,
    /// Node has sufficient PIDs
    PIDPressure,
    /// Node network is available
    NetworkUnavailable,
}

/// Node address
#[derive(Debug, Clone)]
pub struct NodeAddress {
    /// Address type
    pub address_type: NodeAddressType,
    /// Address value
    pub address: String,
}

/// Node address types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeAddressType {
    /// Internal IP
    InternalIP,
    /// External IP
    ExternalIP,
    /// Hostname
    Hostname,
    /// Internal DNS
    InternalDNS,
    /// External DNS
    ExternalDNS,
}

// =============================================================================
// Kubelet Implementation
// =============================================================================

/// S-KUBELET: Splax Kubernetes node agent
pub struct Kubelet {
    /// Node name
    node_name: String,
    /// Node info
    node_info: Arc<Mutex<NodeInfo>>,
    /// Running pods
    pods: Arc<Mutex<BTreeMap<PodId, Pod>>>,
    /// Pod count
    pod_count: AtomicU64,
    /// API server endpoint
    api_server: String,
    /// Node token for authentication
    node_token: String,
    /// Running flag
    running: AtomicBool,
    /// Heartbeat interval in seconds
    heartbeat_interval: u64,
    /// Pod sync interval in seconds
    sync_interval: u64,
}

/// Running pod instance
pub struct Pod {
    /// Pod specification
    pub spec: PodSpec,
    /// Pod status
    pub status: PodStatus,
    /// Pod ID
    pub id: PodId,
    /// Container instances
    pub containers: BTreeMap<String, Container>,
    /// Creation timestamp
    pub created_at: u64,
}

/// Running container instance
pub struct Container {
    /// Container spec
    pub spec: ContainerSpec,
    /// Container status
    pub status: ContainerStatus,
    /// Container ID
    pub id: ContainerId,
    /// Process ID (if running natively)
    pub pid: Option<u64>,
    /// Capability token for container
    pub capability_token: Option<u64>,
}

impl Kubelet {
    /// Create a new Kubelet instance
    pub fn new(node_name: &str, api_server: &str, node_token: &str) -> Self {
        let node_info = NodeInfo {
            name: node_name.to_string(),
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            capacity: NodeResources {
                cpu: 4000,              // 4 CPUs
                memory: 8 * 1024 * 1024 * 1024, // 8 GB
                ephemeral_storage: 100 * 1024 * 1024 * 1024, // 100 GB
                pods: 110,
            },
            allocatable: NodeResources {
                cpu: 3800,              // Reserve 200m for system
                memory: 7 * 1024 * 1024 * 1024, // Reserve 1 GB for system
                ephemeral_storage: 90 * 1024 * 1024 * 1024,
                pods: 100,
            },
            conditions: vec![
                NodeCondition {
                    condition_type: NodeConditionType::Ready,
                    status: true,
                    last_heartbeat_time: 0,
                    last_transition_time: 0,
                    reason: "KubeletReady".to_string(),
                    message: "S-KUBELET is ready".to_string(),
                },
            ],
            addresses: Vec::new(),
            kubelet_version: "v1.0.0-splax".to_string(),
            container_runtime_version: "splax-oci://1.0.0".to_string(),
            os_image: "Splax OS".to_string(),
            architecture: "amd64".to_string(),
        };

        Self {
            node_name: node_name.to_string(),
            node_info: Arc::new(Mutex::new(node_info)),
            pods: Arc::new(Mutex::new(BTreeMap::new())),
            pod_count: AtomicU64::new(0),
            api_server: api_server.to_string(),
            node_token: node_token.to_string(),
            running: AtomicBool::new(false),
            heartbeat_interval: 10,
            sync_interval: 10,
        }
    }

    /// Start the kubelet
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        // In a real implementation, this would:
        // 1. Register with the API server
        // 2. Start the heartbeat loop
        // 3. Start the pod sync loop
        // 4. Start the CNI plugin
        // 5. Start the CRI shim
    }

    /// Stop the kubelet
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Create a pod
    pub fn create_pod(&self, spec: PodSpec) -> Result<PodId, KubeletError> {
        let pod_id = PodId(format!("{}/{}", spec.namespace.0, spec.name));
        
        let mut pods = self.pods.lock();
        if pods.contains_key(&pod_id) {
            return Err(KubeletError::PodExists(pod_id.0.clone()));
        }

        let pod = Pod {
            spec: spec.clone(),
            status: PodStatus {
                phase: PodPhase::Pending,
                pod_ip: None,
                host_ip: None,
                start_time: None,
                container_statuses: Vec::new(),
                init_container_statuses: Vec::new(),
                conditions: Vec::new(),
            },
            id: pod_id.clone(),
            containers: BTreeMap::new(),
            created_at: self.current_time(),
        };

        pods.insert(pod_id.clone(), pod);
        self.pod_count.fetch_add(1, Ordering::SeqCst);

        // Schedule pod creation asynchronously
        self.schedule_pod_creation(&pod_id);

        Ok(pod_id)
    }

    /// Delete a pod
    pub fn delete_pod(&self, pod_id: &PodId, grace_period: u64) -> Result<(), KubeletError> {
        let mut pods = self.pods.lock();
        let pod = pods.get_mut(pod_id).ok_or_else(|| KubeletError::PodNotFound(pod_id.0.clone()))?;

        // Send SIGTERM to all containers
        for container in pod.containers.values() {
            self.stop_container(&container.id, grace_period);
        }

        // Mark pod as terminating
        pod.status.phase = PodPhase::Failed;

        Ok(())
    }

    /// Get pod status
    pub fn get_pod_status(&self, pod_id: &PodId) -> Result<PodStatus, KubeletError> {
        let pods = self.pods.lock();
        let pod = pods.get(pod_id).ok_or_else(|| KubeletError::PodNotFound(pod_id.0.clone()))?;
        Ok(pod.status.clone())
    }

    /// List all pods
    pub fn list_pods(&self) -> Vec<(PodId, PodStatus)> {
        let pods = self.pods.lock();
        pods.iter()
            .map(|(id, pod)| (id.clone(), pod.status.clone()))
            .collect()
    }

    /// Get node info
    pub fn get_node_info(&self) -> NodeInfo {
        self.node_info.lock().clone()
    }

    /// Update node resources
    pub fn update_resources(&self, capacity: NodeResources, allocatable: NodeResources) {
        let mut info = self.node_info.lock();
        info.capacity = capacity;
        info.allocatable = allocatable;
    }

    /// Add node label
    pub fn add_label(&self, key: &str, value: &str) {
        let mut info = self.node_info.lock();
        info.labels.insert(key.to_string(), value.to_string());
    }

    /// Add node address
    pub fn add_address(&self, address_type: NodeAddressType, address: &str) {
        let mut info = self.node_info.lock();
        info.addresses.push(NodeAddress {
            address_type,
            address: address.to_string(),
        });
    }

    // Internal methods

    fn schedule_pod_creation(&self, _pod_id: &PodId) {
        // In a real implementation, this would:
        // 1. Pull container images
        // 2. Set up networking (CNI)
        // 3. Create and start containers
        // 4. Run init containers first
        // 5. Start health checks
    }

    fn stop_container(&self, _container_id: &ContainerId, _grace_period: u64) {
        // In a real implementation, this would:
        // 1. Send SIGTERM to the container process
        // 2. Wait for grace period
        // 3. Send SIGKILL if still running
        // 4. Clean up resources
    }

    fn current_time(&self) -> u64 {
        // Return current timestamp
        0 // Placeholder
    }
}

/// Kubelet errors
#[derive(Debug)]
pub enum KubeletError {
    /// Pod already exists
    PodExists(String),
    /// Pod not found
    PodNotFound(String),
    /// Container not found
    ContainerNotFound(String),
    /// Image pull failed
    ImagePullFailed(String),
    /// Resource exhausted
    ResourceExhausted(String),
    /// Network error
    NetworkError(String),
    /// API server error
    ApiServerError(String),
}

impl core::fmt::Display for KubeletError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            KubeletError::PodExists(name) => write!(f, "Pod already exists: {}", name),
            KubeletError::PodNotFound(name) => write!(f, "Pod not found: {}", name),
            KubeletError::ContainerNotFound(name) => write!(f, "Container not found: {}", name),
            KubeletError::ImagePullFailed(msg) => write!(f, "Image pull failed: {}", msg),
            KubeletError::ResourceExhausted(msg) => write!(f, "Resource exhausted: {}", msg),
            KubeletError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            KubeletError::ApiServerError(msg) => write!(f, "API server error: {}", msg),
        }
    }
}

// =============================================================================
// CRI (Container Runtime Interface) Shim
// =============================================================================

/// CRI shim for Splax container runtime
pub struct CriShim {
    /// Runtime endpoint
    runtime_endpoint: String,
    /// Image service endpoint
    image_endpoint: String,
}

impl CriShim {
    /// Create a new CRI shim
    pub fn new(runtime_endpoint: &str, image_endpoint: &str) -> Self {
        Self {
            runtime_endpoint: runtime_endpoint.to_string(),
            image_endpoint: image_endpoint.to_string(),
        }
    }

    /// Create a container sandbox (pod)
    pub fn run_pod_sandbox(&self, _config: &PodSpec) -> Result<String, KubeletError> {
        // Returns sandbox ID
        Ok("sandbox-0".to_string())
    }

    /// Stop a pod sandbox
    pub fn stop_pod_sandbox(&self, _sandbox_id: &str) -> Result<(), KubeletError> {
        Ok(())
    }

    /// Remove a pod sandbox
    pub fn remove_pod_sandbox(&self, _sandbox_id: &str) -> Result<(), KubeletError> {
        Ok(())
    }

    /// List pod sandboxes
    pub fn list_pod_sandboxes(&self) -> Vec<String> {
        Vec::new()
    }

    /// Create a container
    pub fn create_container(
        &self,
        _sandbox_id: &str,
        _config: &ContainerSpec,
    ) -> Result<String, KubeletError> {
        Ok("container-0".to_string())
    }

    /// Start a container
    pub fn start_container(&self, _container_id: &str) -> Result<(), KubeletError> {
        Ok(())
    }

    /// Stop a container
    pub fn stop_container(&self, _container_id: &str, _timeout: u64) -> Result<(), KubeletError> {
        Ok(())
    }

    /// Remove a container
    pub fn remove_container(&self, _container_id: &str) -> Result<(), KubeletError> {
        Ok(())
    }

    /// Pull an image
    pub fn pull_image(&self, image: &str) -> Result<String, KubeletError> {
        // Returns image ID
        Ok(format!("sha256:{}", image))
    }

    /// List images
    pub fn list_images(&self) -> Vec<String> {
        Vec::new()
    }

    /// Remove an image
    pub fn remove_image(&self, _image: &str) -> Result<(), KubeletError> {
        Ok(())
    }
}

// =============================================================================
// CNI (Container Network Interface) Plugin
// =============================================================================

/// CNI plugin for pod networking
pub struct CniPlugin {
    /// CNI configuration directory
    config_dir: String,
    /// CNI binary directory
    bin_dir: String,
    /// Default network name
    default_network: String,
}

/// CNI network configuration
#[derive(Debug, Clone)]
pub struct CniConfig {
    /// Network name
    pub name: String,
    /// CNI version
    pub cni_version: String,
    /// Plugin type (bridge, ptp, macvlan, etc.)
    pub plugin_type: String,
    /// Bridge name (for bridge plugin)
    pub bridge: Option<String>,
    /// Subnet for IP allocation
    pub subnet: Option<String>,
    /// Gateway IP
    pub gateway: Option<String>,
    /// IPAM configuration
    pub ipam: Option<IpamConfig>,
}

/// IPAM configuration
#[derive(Debug, Clone)]
pub struct IpamConfig {
    /// IPAM type (host-local, dhcp, static)
    pub ipam_type: String,
    /// Subnet
    pub subnet: String,
    /// Range start
    pub range_start: Option<String>,
    /// Range end
    pub range_end: Option<String>,
    /// Gateway
    pub gateway: Option<String>,
    /// Routes
    pub routes: Vec<CniRoute>,
}

/// CNI route
#[derive(Debug, Clone)]
pub struct CniRoute {
    /// Destination network
    pub dst: String,
    /// Gateway (optional, uses default if not specified)
    pub gw: Option<String>,
}

/// CNI result
#[derive(Debug, Clone)]
pub struct CniResult {
    /// Assigned IP addresses
    pub ips: Vec<CniIp>,
    /// Routes
    pub routes: Vec<CniRoute>,
    /// DNS configuration
    pub dns: CniDns,
}

/// CNI IP assignment
#[derive(Debug, Clone)]
pub struct CniIp {
    /// IP address with prefix
    pub address: String,
    /// Gateway
    pub gateway: Option<String>,
    /// Interface index
    pub interface: u32,
}

/// CNI DNS configuration
#[derive(Debug, Clone)]
pub struct CniDns {
    /// Nameservers
    pub nameservers: Vec<String>,
    /// Search domains
    pub search: Vec<String>,
    /// DNS options
    pub options: Vec<String>,
}

impl CniPlugin {
    /// Create a new CNI plugin
    pub fn new(config_dir: &str, bin_dir: &str, default_network: &str) -> Self {
        Self {
            config_dir: config_dir.to_string(),
            bin_dir: bin_dir.to_string(),
            default_network: default_network.to_string(),
        }
    }

    /// Set up networking for a container
    pub fn setup(&self, _container_id: &str, _netns: &str) -> Result<CniResult, KubeletError> {
        // In a real implementation:
        // 1. Create veth pair
        // 2. Move one end to container namespace
        // 3. Configure IP address
        // 4. Set up routing
        // 5. Configure iptables rules

        Ok(CniResult {
            ips: vec![CniIp {
                address: "10.244.0.2/24".to_string(),
                gateway: Some("10.244.0.1".to_string()),
                interface: 0,
            }],
            routes: vec![CniRoute {
                dst: "0.0.0.0/0".to_string(),
                gw: Some("10.244.0.1".to_string()),
            }],
            dns: CniDns {
                nameservers: vec!["10.96.0.10".to_string()],
                search: vec!["default.svc.cluster.local".to_string()],
                options: vec!["ndots:5".to_string()],
            },
        })
    }

    /// Tear down networking for a container
    pub fn teardown(&self, _container_id: &str, _netns: &str) -> Result<(), KubeletError> {
        // Clean up networking resources
        Ok(())
    }

    /// Check CNI plugin status
    pub fn check(&self) -> bool {
        // Verify CNI plugins are available
        true
    }
}

// =============================================================================
// Global Kubelet Instance
// =============================================================================

static KUBELET: Mutex<Option<Arc<Kubelet>>> = Mutex::new(None);

/// Initialize the Kubelet
pub fn init(node_name: &str, api_server: &str, node_token: &str) {
    let kubelet = Arc::new(Kubelet::new(node_name, api_server, node_token));
    *KUBELET.lock() = Some(kubelet.clone());
    kubelet.start();
}

/// Get the global Kubelet instance
pub fn kubelet() -> Option<Arc<Kubelet>> {
    KUBELET.lock().clone()
}

/// Check if Kubelet is running
pub fn is_running() -> bool {
    KUBELET.lock().as_ref().map(|k| k.running.load(Ordering::SeqCst)).unwrap_or(false)
}

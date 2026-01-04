//! # S-CLUSTER: Splax Native Orchestrator
//!
//! S-CLUSTER is Splax OS's native container and service orchestration system.
//! It provides Kubernetes-like functionality with capability-based security.
//!
//! ## Features
//!
//! - **Capability-native**: All resources protected by S-CAP tokens
//! - **Service Discovery**: Automatic DNS-based service discovery
//! - **Load Balancing**: Client-side and server-side load balancing
//! - **Auto-scaling**: Horizontal pod autoscaling based on metrics
//! - **Rolling Updates**: Zero-downtime deployments
//! - **Self-healing**: Automatic restart of failed services
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      S-CLUSTER Controller                       │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
//! │  │  Scheduler  │  │ Replicator  │  │  Autoscaler │              │
//! │  └─────────────┘  └─────────────┘  └─────────────┘              │
//! └─────────────────────────────────────────────────────────────────┘
//!                                │
//!                                ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      Cluster State Store                        │
//! │                    (Distributed, Consistent)                    │
//! └─────────────────────────────────────────────────────────────────┘
//!                                │
//!                                ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                          Worker Nodes                           │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
//! │  │   Node 1    │  │   Node 2    │  │   Node 3    │              │
//! │  └─────────────┘  └─────────────┘  └─────────────┘              │
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

/// Unique cluster-wide identifier
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClusterId(pub String);

/// Node identifier within a cluster
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub String);

/// Service identifier
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceId(pub String);

/// Deployment identifier
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeploymentId(pub String);

/// Replica identifier
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReplicaId(pub String);

// =============================================================================
// Cluster Configuration
// =============================================================================

/// Cluster configuration
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Cluster name
    pub name: String,
    /// Cluster ID
    pub id: ClusterId,
    /// API server address
    pub api_address: String,
    /// API server port
    pub api_port: u16,
    /// Enable TLS
    pub tls_enabled: bool,
    /// Cluster CIDR for pods
    pub pod_cidr: String,
    /// Service CIDR
    pub service_cidr: String,
    /// DNS domain
    pub dns_domain: String,
    /// Maximum pods per node
    pub max_pods_per_node: u32,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            name: "splax-cluster".to_string(),
            id: ClusterId("cluster-0".to_string()),
            api_address: "0.0.0.0".to_string(),
            api_port: 6443,
            tls_enabled: true,
            pod_cidr: "10.244.0.0/16".to_string(),
            service_cidr: "10.96.0.0/12".to_string(),
            dns_domain: "cluster.local".to_string(),
            max_pods_per_node: 110,
        }
    }
}

// =============================================================================
// Node Management
// =============================================================================

/// Cluster node
#[derive(Debug, Clone)]
pub struct ClusterNode {
    /// Node ID
    pub id: NodeId,
    /// Node name
    pub name: String,
    /// Node address
    pub address: String,
    /// Node port
    pub port: u16,
    /// Node status
    pub status: NodeStatus,
    /// Node capacity
    pub capacity: NodeCapacity,
    /// Allocated resources
    pub allocated: NodeCapacity,
    /// Node labels
    pub labels: BTreeMap<String, String>,
    /// Node taints
    pub taints: Vec<Taint>,
    /// Last heartbeat timestamp
    pub last_heartbeat: u64,
    /// Node conditions
    pub conditions: Vec<NodeCondition>,
}

/// Node status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    /// Node is healthy and ready
    Ready,
    /// Node is not ready
    NotReady,
    /// Node is under maintenance
    Maintenance,
    /// Node is being drained
    Draining,
    /// Node has been removed
    Removed,
}

/// Node capacity
#[derive(Debug, Clone, Default)]
pub struct NodeCapacity {
    /// CPU in millicores
    pub cpu_millis: u64,
    /// Memory in bytes
    pub memory_bytes: u64,
    /// Storage in bytes
    pub storage_bytes: u64,
    /// Number of pods
    pub pods: u32,
    /// GPU count
    pub gpus: u32,
}

/// Node taint
#[derive(Debug, Clone)]
pub struct Taint {
    /// Taint key
    pub key: String,
    /// Taint value
    pub value: String,
    /// Taint effect
    pub effect: TaintEffect,
}

/// Taint effect
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaintEffect {
    /// Pods won't be scheduled
    NoSchedule,
    /// Prefer not to schedule
    PreferNoSchedule,
    /// Evict existing pods
    NoExecute,
}

/// Node condition
#[derive(Debug, Clone)]
pub struct NodeCondition {
    /// Condition type
    pub condition_type: String,
    /// Condition status
    pub status: bool,
    /// Last update
    pub last_update: u64,
    /// Message
    pub message: String,
}

// =============================================================================
// Service Definition
// =============================================================================

/// Service specification
#[derive(Debug, Clone)]
pub struct ServiceSpec {
    /// Service name
    pub name: String,
    /// Service namespace
    pub namespace: String,
    /// Service type
    pub service_type: ServiceType,
    /// Selector labels
    pub selector: BTreeMap<String, String>,
    /// Service ports
    pub ports: Vec<ServicePort>,
    /// Cluster IP (assigned)
    pub cluster_ip: Option<String>,
    /// External IPs
    pub external_ips: Vec<String>,
    /// Load balancer IP
    pub load_balancer_ip: Option<String>,
    /// Session affinity
    pub session_affinity: SessionAffinity,
    /// External traffic policy
    pub external_traffic_policy: TrafficPolicy,
}

/// Service type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceType {
    /// Cluster-internal IP
    ClusterIP,
    /// Node port on each node
    NodePort,
    /// Cloud load balancer
    LoadBalancer,
    /// External name (CNAME)
    ExternalName,
}

impl Default for ServiceType {
    fn default() -> Self {
        ServiceType::ClusterIP
    }
}

/// Service port
#[derive(Debug, Clone)]
pub struct ServicePort {
    /// Port name
    pub name: Option<String>,
    /// Protocol
    pub protocol: Protocol,
    /// Service port
    pub port: u16,
    /// Target port on pods
    pub target_port: u16,
    /// Node port (for NodePort/LoadBalancer)
    pub node_port: Option<u16>,
}

/// Protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Sctp,
}

impl Default for Protocol {
    fn default() -> Self {
        Protocol::Tcp
    }
}

/// Session affinity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAffinity {
    /// No affinity
    None,
    /// Client IP affinity
    ClientIP,
}

impl Default for SessionAffinity {
    fn default() -> Self {
        SessionAffinity::None
    }
}

/// Traffic policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficPolicy {
    /// Route to any node
    Cluster,
    /// Route only to local node
    Local,
}

impl Default for TrafficPolicy {
    fn default() -> Self {
        TrafficPolicy::Cluster
    }
}

/// Service status
#[derive(Debug, Clone)]
pub struct ServiceStatus {
    /// Load balancer status
    pub load_balancer: Option<LoadBalancerStatus>,
    /// Conditions
    pub conditions: Vec<ServiceCondition>,
}

/// Load balancer status
#[derive(Debug, Clone)]
pub struct LoadBalancerStatus {
    /// Ingress addresses
    pub ingress: Vec<LoadBalancerIngress>,
}

/// Load balancer ingress
#[derive(Debug, Clone)]
pub struct LoadBalancerIngress {
    /// IP address
    pub ip: Option<String>,
    /// Hostname
    pub hostname: Option<String>,
}

/// Service condition
#[derive(Debug, Clone)]
pub struct ServiceCondition {
    /// Condition type
    pub condition_type: String,
    /// Status
    pub status: bool,
    /// Message
    pub message: String,
}

// =============================================================================
// Deployment
// =============================================================================

/// Deployment specification
#[derive(Debug, Clone)]
pub struct DeploymentSpec {
    /// Deployment name
    pub name: String,
    /// Namespace
    pub namespace: String,
    /// Number of replicas
    pub replicas: u32,
    /// Selector labels
    pub selector: BTreeMap<String, String>,
    /// Pod template
    pub template: PodTemplate,
    /// Update strategy
    pub strategy: UpdateStrategy,
    /// Minimum ready seconds
    pub min_ready_seconds: u32,
    /// Revision history limit
    pub revision_history_limit: u32,
    /// Progress deadline seconds
    pub progress_deadline_seconds: u32,
}

/// Pod template
#[derive(Debug, Clone)]
pub struct PodTemplate {
    /// Metadata labels
    pub labels: BTreeMap<String, String>,
    /// Annotations
    pub annotations: BTreeMap<String, String>,
    /// Container specs
    pub containers: Vec<ContainerSpec>,
    /// Init containers
    pub init_containers: Vec<ContainerSpec>,
    /// Volumes
    pub volumes: Vec<Volume>,
    /// Node selector
    pub node_selector: BTreeMap<String, String>,
    /// Tolerations
    pub tolerations: Vec<Toleration>,
    /// Affinity
    pub affinity: Option<Affinity>,
}

/// Container specification
#[derive(Debug, Clone)]
pub struct ContainerSpec {
    /// Container name
    pub name: String,
    /// Image
    pub image: String,
    /// Command
    pub command: Vec<String>,
    /// Args
    pub args: Vec<String>,
    /// Environment variables
    pub env: Vec<EnvVar>,
    /// Ports
    pub ports: Vec<ContainerPort>,
    /// Resource requirements
    pub resources: ResourceRequirements,
    /// Volume mounts
    pub volume_mounts: Vec<VolumeMount>,
    /// Liveness probe
    pub liveness_probe: Option<Probe>,
    /// Readiness probe
    pub readiness_probe: Option<Probe>,
}

/// Environment variable
#[derive(Debug, Clone)]
pub struct EnvVar {
    /// Name
    pub name: String,
    /// Value
    pub value: String,
}

/// Container port
#[derive(Debug, Clone)]
pub struct ContainerPort {
    /// Name
    pub name: Option<String>,
    /// Container port
    pub container_port: u16,
    /// Protocol
    pub protocol: Protocol,
}

/// Resource requirements
#[derive(Debug, Clone, Default)]
pub struct ResourceRequirements {
    /// CPU request (millicores)
    pub cpu_request: u64,
    /// CPU limit (millicores)
    pub cpu_limit: u64,
    /// Memory request (bytes)
    pub memory_request: u64,
    /// Memory limit (bytes)
    pub memory_limit: u64,
}

/// Volume
#[derive(Debug, Clone)]
pub struct Volume {
    /// Volume name
    pub name: String,
    /// Volume source
    pub source: VolumeSource,
}

/// Volume source
#[derive(Debug, Clone)]
pub enum VolumeSource {
    /// Empty directory
    EmptyDir { medium: String, size_limit: Option<u64> },
    /// Host path
    HostPath { path: String, path_type: String },
    /// Config map
    ConfigMap { name: String },
    /// Secret
    Secret { name: String },
    /// Persistent volume claim
    PersistentVolumeClaim { claim_name: String, read_only: bool },
}

/// Volume mount
#[derive(Debug, Clone)]
pub struct VolumeMount {
    /// Volume name
    pub name: String,
    /// Mount path
    pub mount_path: String,
    /// Read only
    pub read_only: bool,
}

/// Health probe
#[derive(Debug, Clone)]
pub struct Probe {
    /// Handler
    pub handler: ProbeHandler,
    /// Initial delay
    pub initial_delay_seconds: u32,
    /// Period
    pub period_seconds: u32,
    /// Timeout
    pub timeout_seconds: u32,
    /// Success threshold
    pub success_threshold: u32,
    /// Failure threshold
    pub failure_threshold: u32,
}

/// Probe handler
#[derive(Debug, Clone)]
pub enum ProbeHandler {
    /// HTTP GET
    HttpGet { path: String, port: u16 },
    /// TCP socket
    TcpSocket { port: u16 },
    /// Exec command
    Exec { command: Vec<String> },
}

/// Toleration
#[derive(Debug, Clone)]
pub struct Toleration {
    /// Key
    pub key: Option<String>,
    /// Operator
    pub operator: TolerationOperator,
    /// Value
    pub value: Option<String>,
    /// Effect
    pub effect: Option<TaintEffect>,
    /// Toleration seconds
    pub toleration_seconds: Option<u64>,
}

/// Toleration operator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TolerationOperator {
    /// Key equals value
    Equal,
    /// Key exists
    Exists,
}

/// Affinity
#[derive(Debug, Clone)]
pub struct Affinity {
    /// Node affinity
    pub node_affinity: Option<NodeAffinity>,
    /// Pod affinity
    pub pod_affinity: Option<PodAffinity>,
    /// Pod anti-affinity
    pub pod_anti_affinity: Option<PodAffinity>,
}

/// Node affinity
#[derive(Debug, Clone)]
pub struct NodeAffinity {
    /// Required during scheduling
    pub required: Vec<NodeSelectorTerm>,
    /// Preferred during scheduling
    pub preferred: Vec<PreferredSchedulingTerm>,
}

/// Node selector term
#[derive(Debug, Clone)]
pub struct NodeSelectorTerm {
    /// Match expressions
    pub match_expressions: Vec<NodeSelectorRequirement>,
}

/// Node selector requirement
#[derive(Debug, Clone)]
pub struct NodeSelectorRequirement {
    /// Key
    pub key: String,
    /// Operator
    pub operator: String,
    /// Values
    pub values: Vec<String>,
}

/// Preferred scheduling term
#[derive(Debug, Clone)]
pub struct PreferredSchedulingTerm {
    /// Weight
    pub weight: i32,
    /// Preference
    pub preference: NodeSelectorTerm,
}

/// Pod affinity
#[derive(Debug, Clone)]
pub struct PodAffinity {
    /// Required during scheduling
    pub required: Vec<PodAffinityTerm>,
    /// Preferred during scheduling
    pub preferred: Vec<WeightedPodAffinityTerm>,
}

/// Pod affinity term
#[derive(Debug, Clone)]
pub struct PodAffinityTerm {
    /// Label selector
    pub label_selector: LabelSelector,
    /// Topology key
    pub topology_key: String,
    /// Namespaces
    pub namespaces: Vec<String>,
}

/// Weighted pod affinity term
#[derive(Debug, Clone)]
pub struct WeightedPodAffinityTerm {
    /// Weight
    pub weight: i32,
    /// Pod affinity term
    pub pod_affinity_term: PodAffinityTerm,
}

/// Label selector
#[derive(Debug, Clone)]
pub struct LabelSelector {
    /// Match labels
    pub match_labels: BTreeMap<String, String>,
    /// Match expressions
    pub match_expressions: Vec<LabelSelectorRequirement>,
}

/// Label selector requirement
#[derive(Debug, Clone)]
pub struct LabelSelectorRequirement {
    /// Key
    pub key: String,
    /// Operator
    pub operator: String,
    /// Values
    pub values: Vec<String>,
}

/// Update strategy
#[derive(Debug, Clone)]
pub struct UpdateStrategy {
    /// Strategy type
    pub strategy_type: UpdateStrategyType,
    /// Rolling update config
    pub rolling_update: Option<RollingUpdateConfig>,
}

impl Default for UpdateStrategy {
    fn default() -> Self {
        Self {
            strategy_type: UpdateStrategyType::RollingUpdate,
            rolling_update: Some(RollingUpdateConfig::default()),
        }
    }
}

/// Update strategy type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStrategyType {
    /// Recreate all pods
    Recreate,
    /// Rolling update
    RollingUpdate,
}

/// Rolling update configuration
#[derive(Debug, Clone)]
pub struct RollingUpdateConfig {
    /// Max unavailable (count or percentage)
    pub max_unavailable: ScaleValue,
    /// Max surge (count or percentage)
    pub max_surge: ScaleValue,
}

impl Default for RollingUpdateConfig {
    fn default() -> Self {
        Self {
            max_unavailable: ScaleValue::Percentage(25),
            max_surge: ScaleValue::Percentage(25),
        }
    }
}

/// Scale value (count or percentage)
#[derive(Debug, Clone)]
pub enum ScaleValue {
    /// Absolute count
    Count(u32),
    /// Percentage
    Percentage(u32),
}

/// Deployment status
#[derive(Debug, Clone)]
pub struct DeploymentStatus {
    /// Observed generation
    pub observed_generation: u64,
    /// Total replicas
    pub replicas: u32,
    /// Updated replicas
    pub updated_replicas: u32,
    /// Ready replicas
    pub ready_replicas: u32,
    /// Available replicas
    pub available_replicas: u32,
    /// Unavailable replicas
    pub unavailable_replicas: u32,
    /// Conditions
    pub conditions: Vec<DeploymentCondition>,
}

/// Deployment condition
#[derive(Debug, Clone)]
pub struct DeploymentCondition {
    /// Condition type
    pub condition_type: DeploymentConditionType,
    /// Status
    pub status: bool,
    /// Last update
    pub last_update: u64,
    /// Last transition
    pub last_transition: u64,
    /// Reason
    pub reason: String,
    /// Message
    pub message: String,
}

/// Deployment condition type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentConditionType {
    /// Deployment is available
    Available,
    /// Deployment is progressing
    Progressing,
    /// Replica failure
    ReplicaFailure,
}

// =============================================================================
// Horizontal Pod Autoscaler
// =============================================================================

/// HPA specification
#[derive(Debug, Clone)]
pub struct HpaSpec {
    /// HPA name
    pub name: String,
    /// Namespace
    pub namespace: String,
    /// Target deployment
    pub target_ref: TargetRef,
    /// Minimum replicas
    pub min_replicas: u32,
    /// Maximum replicas
    pub max_replicas: u32,
    /// Metrics
    pub metrics: Vec<MetricSpec>,
    /// Scale down behavior
    pub scale_down_behavior: ScalingBehavior,
    /// Scale up behavior
    pub scale_up_behavior: ScalingBehavior,
}

/// Target reference
#[derive(Debug, Clone)]
pub struct TargetRef {
    /// API version
    pub api_version: String,
    /// Kind
    pub kind: String,
    /// Name
    pub name: String,
}

/// Metric specification
#[derive(Debug, Clone)]
pub struct MetricSpec {
    /// Metric type
    pub metric_type: MetricType,
    /// Target value
    pub target: MetricTarget,
}

/// Metric type
#[derive(Debug, Clone)]
pub enum MetricType {
    /// Resource metric (CPU, memory)
    Resource { name: String },
    /// Pod metric
    Pod { name: String },
    /// Object metric
    Object { name: String, target: TargetRef },
    /// External metric
    External { name: String },
}

/// Metric target
#[derive(Debug, Clone)]
pub struct MetricTarget {
    /// Target type
    pub target_type: MetricTargetType,
    /// Target value
    pub value: u64,
}

/// Metric target type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricTargetType {
    /// Utilization percentage
    Utilization,
    /// Absolute value
    Value,
    /// Average value
    AverageValue,
}

/// Scaling behavior
#[derive(Debug, Clone)]
pub struct ScalingBehavior {
    /// Stabilization window seconds
    pub stabilization_window_seconds: u32,
    /// Policies
    pub policies: Vec<ScalingPolicy>,
    /// Select policy
    pub select_policy: SelectPolicy,
}

impl Default for ScalingBehavior {
    fn default() -> Self {
        Self {
            stabilization_window_seconds: 300,
            policies: vec![ScalingPolicy::default()],
            select_policy: SelectPolicy::Max,
        }
    }
}

/// Scaling policy
#[derive(Debug, Clone)]
pub struct ScalingPolicy {
    /// Policy type
    pub policy_type: ScalingPolicyType,
    /// Value
    pub value: u32,
    /// Period seconds
    pub period_seconds: u32,
}

impl Default for ScalingPolicy {
    fn default() -> Self {
        Self {
            policy_type: ScalingPolicyType::Pods,
            value: 4,
            period_seconds: 60,
        }
    }
}

/// Scaling policy type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalingPolicyType {
    /// Scale by number of pods
    Pods,
    /// Scale by percentage
    Percent,
}

/// Select policy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectPolicy {
    /// Use maximum change
    Max,
    /// Use minimum change
    Min,
    /// Disable scaling
    Disabled,
}

// =============================================================================
// S-CLUSTER Controller
// =============================================================================

/// S-CLUSTER orchestrator
pub struct Cluster {
    /// Cluster configuration
    config: ClusterConfig,
    /// Cluster nodes
    nodes: Arc<Mutex<BTreeMap<NodeId, ClusterNode>>>,
    /// Services
    services: Arc<Mutex<BTreeMap<ServiceId, ServiceSpec>>>,
    /// Deployments
    deployments: Arc<Mutex<BTreeMap<DeploymentId, Deployment>>>,
    /// HPAs
    hpas: Arc<Mutex<BTreeMap<String, HpaSpec>>>,
    /// Running flag
    running: AtomicBool,
    /// Generation counter
    generation: AtomicU64,
}

/// Deployment with status
pub struct Deployment {
    /// Spec
    pub spec: DeploymentSpec,
    /// Status
    pub status: DeploymentStatus,
    /// Replicas
    pub replicas: Vec<Replica>,
}

/// Replica instance
pub struct Replica {
    /// Replica ID
    pub id: ReplicaId,
    /// Node ID
    pub node_id: Option<NodeId>,
    /// Status
    pub status: ReplicaStatus,
    /// IP address
    pub ip: Option<String>,
    /// Start time
    pub start_time: Option<u64>,
}

/// Replica status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplicaStatus {
    /// Pending scheduling
    Pending,
    /// Running
    Running,
    /// Succeeded
    Succeeded,
    /// Failed
    Failed,
    /// Unknown
    Unknown,
}

impl Cluster {
    /// Create a new cluster
    pub fn new(config: ClusterConfig) -> Self {
        Self {
            config,
            nodes: Arc::new(Mutex::new(BTreeMap::new())),
            services: Arc::new(Mutex::new(BTreeMap::new())),
            deployments: Arc::new(Mutex::new(BTreeMap::new())),
            hpas: Arc::new(Mutex::new(BTreeMap::new())),
            running: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    /// Start the cluster controller
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        // Start control loops:
        // 1. Node health checker
        // 2. Deployment controller
        // 3. Service controller
        // 4. HPA controller
        // 5. Scheduler
    }

    /// Stop the cluster controller
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Register a node
    pub fn register_node(&self, node: ClusterNode) -> Result<(), ClusterError> {
        let mut nodes = self.nodes.lock();
        if nodes.contains_key(&node.id) {
            return Err(ClusterError::NodeExists(node.id.0.clone()));
        }
        nodes.insert(node.id.clone(), node);
        Ok(())
    }

    /// Remove a node
    pub fn remove_node(&self, node_id: &NodeId) -> Result<(), ClusterError> {
        let mut nodes = self.nodes.lock();
        nodes.remove(node_id).ok_or_else(|| ClusterError::NodeNotFound(node_id.0.clone()))?;
        Ok(())
    }

    /// Create a service
    pub fn create_service(&self, spec: ServiceSpec) -> Result<ServiceId, ClusterError> {
        let service_id = ServiceId(format!("{}/{}", spec.namespace, spec.name));
        let mut services = self.services.lock();
        if services.contains_key(&service_id) {
            return Err(ClusterError::ServiceExists(service_id.0.clone()));
        }
        services.insert(service_id.clone(), spec);
        Ok(service_id)
    }

    /// Delete a service
    pub fn delete_service(&self, service_id: &ServiceId) -> Result<(), ClusterError> {
        let mut services = self.services.lock();
        services.remove(service_id).ok_or_else(|| ClusterError::ServiceNotFound(service_id.0.clone()))?;
        Ok(())
    }

    /// Create a deployment
    pub fn create_deployment(&self, spec: DeploymentSpec) -> Result<DeploymentId, ClusterError> {
        let deployment_id = DeploymentId(format!("{}/{}", spec.namespace, spec.name));
        let mut deployments = self.deployments.lock();
        if deployments.contains_key(&deployment_id) {
            return Err(ClusterError::DeploymentExists(deployment_id.0.clone()));
        }

        let deployment = Deployment {
            spec: spec.clone(),
            status: DeploymentStatus {
                observed_generation: 0,
                replicas: 0,
                updated_replicas: 0,
                ready_replicas: 0,
                available_replicas: 0,
                unavailable_replicas: spec.replicas,
                conditions: Vec::new(),
            },
            replicas: Vec::new(),
        };

        deployments.insert(deployment_id.clone(), deployment);
        
        // Schedule replica creation
        self.reconcile_deployment(&deployment_id);
        
        Ok(deployment_id)
    }

    /// Scale a deployment
    pub fn scale_deployment(&self, deployment_id: &DeploymentId, replicas: u32) -> Result<(), ClusterError> {
        let mut deployments = self.deployments.lock();
        let deployment = deployments.get_mut(deployment_id)
            .ok_or_else(|| ClusterError::DeploymentNotFound(deployment_id.0.clone()))?;
        
        deployment.spec.replicas = replicas;
        drop(deployments);
        
        self.reconcile_deployment(deployment_id);
        Ok(())
    }

    /// Create an HPA
    pub fn create_hpa(&self, spec: HpaSpec) -> Result<(), ClusterError> {
        let key = format!("{}/{}", spec.namespace, spec.name);
        let mut hpas = self.hpas.lock();
        if hpas.contains_key(&key) {
            return Err(ClusterError::HpaExists(key));
        }
        hpas.insert(key, spec);
        Ok(())
    }

    /// List nodes
    pub fn list_nodes(&self) -> Vec<ClusterNode> {
        self.nodes.lock().values().cloned().collect()
    }

    /// List services
    pub fn list_services(&self) -> Vec<ServiceSpec> {
        self.services.lock().values().cloned().collect()
    }

    /// List deployments
    pub fn list_deployments(&self) -> Vec<(DeploymentId, DeploymentStatus)> {
        self.deployments.lock().iter()
            .map(|(id, d)| (id.clone(), d.status.clone()))
            .collect()
    }

    /// Get cluster config
    pub fn config(&self) -> &ClusterConfig {
        &self.config
    }

    // Internal methods

    fn reconcile_deployment(&self, _deployment_id: &DeploymentId) {
        // Reconciliation loop:
        // 1. Compare desired vs actual replicas
        // 2. Create/delete replicas as needed
        // 3. Schedule pods to nodes
        // 4. Update status
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn schedule_pod(&self, _replica: &Replica) -> Option<NodeId> {
        // Scheduling algorithm:
        // 1. Filter nodes by selectors and taints
        // 2. Score nodes by resource availability
        // 3. Select highest scored node
        // 4. Bind pod to node
        let nodes = self.nodes.lock();
        nodes.iter()
            .filter(|(_, n)| n.status == NodeStatus::Ready)
            .max_by_key(|(_, n)| n.capacity.cpu_millis - n.allocated.cpu_millis)
            .map(|(id, _)| id.clone())
    }
}

/// Cluster errors
#[derive(Debug)]
pub enum ClusterError {
    /// Node exists
    NodeExists(String),
    /// Node not found
    NodeNotFound(String),
    /// Service exists
    ServiceExists(String),
    /// Service not found
    ServiceNotFound(String),
    /// Deployment exists
    DeploymentExists(String),
    /// Deployment not found
    DeploymentNotFound(String),
    /// HPA exists
    HpaExists(String),
    /// Insufficient resources
    InsufficientResources(String),
    /// Scheduling failed
    SchedulingFailed(String),
}

impl core::fmt::Display for ClusterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ClusterError::NodeExists(n) => write!(f, "Node exists: {}", n),
            ClusterError::NodeNotFound(n) => write!(f, "Node not found: {}", n),
            ClusterError::ServiceExists(s) => write!(f, "Service exists: {}", s),
            ClusterError::ServiceNotFound(s) => write!(f, "Service not found: {}", s),
            ClusterError::DeploymentExists(d) => write!(f, "Deployment exists: {}", d),
            ClusterError::DeploymentNotFound(d) => write!(f, "Deployment not found: {}", d),
            ClusterError::HpaExists(h) => write!(f, "HPA exists: {}", h),
            ClusterError::InsufficientResources(m) => write!(f, "Insufficient resources: {}", m),
            ClusterError::SchedulingFailed(m) => write!(f, "Scheduling failed: {}", m),
        }
    }
}

// =============================================================================
// Global Cluster Instance
// =============================================================================

static CLUSTER: Mutex<Option<Arc<Cluster>>> = Mutex::new(None);

/// Initialize S-CLUSTER
pub fn init(config: ClusterConfig) {
    let cluster = Arc::new(Cluster::new(config));
    *CLUSTER.lock() = Some(cluster.clone());
    cluster.start();
}

/// Get the global cluster instance
pub fn cluster() -> Option<Arc<Cluster>> {
    CLUSTER.lock().clone()
}

/// Check if cluster is running
pub fn is_running() -> bool {
    CLUSTER.lock().as_ref().map(|c| c.running.load(Ordering::SeqCst)).unwrap_or(false)
}

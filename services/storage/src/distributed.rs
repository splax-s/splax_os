//! # Distributed Storage Protocol
//!
//! Protocol for distributed storage across Splax nodes.
//! Implements:
//! - Node discovery and membership
//! - Data replication with configurable factors
//! - Consistent hashing for data placement
//! - Failure detection and recovery
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     Distributed Storage Client                   │
//! └───────────────────────────┬─────────────────────────────────────┘
//!                             │
//! ┌───────────────────────────▼─────────────────────────────────────┐
//! │                   Coordinator Node                               │
//! │   ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
//! │   │ Consistent  │  │  Replica    │  │     Failure             │ │
//! │   │   Hash      │  │  Manager    │  │     Detector            │ │
//! │   └─────────────┘  └─────────────┘  └─────────────────────────┘ │
//! └───────────────────────────┬─────────────────────────────────────┘
//!                             │
//!        ┌────────────────────┼────────────────────┐
//!        ▼                    ▼                    ▼
//! ┌─────────────┐      ┌─────────────┐      ┌─────────────┐
//! │   Node A    │      │   Node B    │      │   Node C    │
//! │   (Primary) │◄────►│  (Replica)  │◄────►│  (Replica)  │
//! └─────────────┘      └─────────────┘      └─────────────┘
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use super::cas::ContentAddress;

/// Node identifier in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

impl NodeId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Computes hash position on the ring.
    pub fn ring_position(&self) -> u64 {
        // Simple hash for ring position
        let mut hash = self.0;
        hash ^= hash >> 33;
        hash = hash.wrapping_mul(0xff51afd7ed558ccd);
        hash ^= hash >> 33;
        hash = hash.wrapping_mul(0xc4ceb9fe1a85ec53);
        hash ^= hash >> 33;
        hash
    }
}

/// Node status in the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    /// Node is healthy and accepting requests.
    Healthy,
    /// Node is suspected of being down.
    Suspected,
    /// Node is confirmed down.
    Down,
    /// Node is recovering/syncing.
    Recovering,
    /// Node is leaving the cluster.
    Leaving,
}

/// Node information.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub id: NodeId,
    pub address: NodeAddress,
    pub status: NodeStatus,
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    pub last_heartbeat: u64,
    pub virtual_nodes: u16,
}

/// Network address of a node.
#[derive(Debug, Clone)]
pub struct NodeAddress {
    pub host: [u8; 4],
    pub port: u16,
}

impl NodeAddress {
    pub fn new(host: [u8; 4], port: u16) -> Self {
        Self { host, port }
    }
}

/// Replication configuration.
#[derive(Debug, Clone)]
pub struct ReplicationConfig {
    /// Number of replicas (including primary).
    pub replication_factor: u8,
    /// Minimum replicas for write success.
    pub write_quorum: u8,
    /// Minimum replicas for read success.
    pub read_quorum: u8,
    /// Enable read repair.
    pub read_repair: bool,
    /// Anti-entropy sync interval (ms).
    pub sync_interval_ms: u64,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            replication_factor: 3,
            write_quorum: 2,
            read_quorum: 1,
            read_repair: true,
            sync_interval_ms: 60_000,
        }
    }
}

/// Consistent hash ring for data placement.
pub struct HashRing {
    /// Sorted ring positions.
    ring: Vec<(u64, NodeId)>,
    /// Node information.
    nodes: BTreeMap<NodeId, NodeInfo>,
}

impl HashRing {
    /// Creates a new empty ring.
    pub fn new() -> Self {
        Self {
            ring: Vec::new(),
            nodes: BTreeMap::new(),
        }
    }

    /// Adds a node to the ring.
    pub fn add_node(&mut self, info: NodeInfo) {
        let id = info.id;
        let vnodes = info.virtual_nodes;

        self.nodes.insert(id, info);

        // Add virtual nodes
        for i in 0..vnodes {
            let vnode_id = id.0.wrapping_add(i as u64);
            let position = NodeId(vnode_id).ring_position();
            self.ring.push((position, id));
        }

        // Keep ring sorted
        self.ring.sort_by_key(|(pos, _)| *pos);
    }

    /// Removes a node from the ring.
    pub fn remove_node(&mut self, id: NodeId) {
        self.nodes.remove(&id);
        self.ring.retain(|(_, node_id)| *node_id != id);
    }

    /// Finds the primary node for a key.
    pub fn primary(&self, key: &ContentAddress) -> Option<NodeId> {
        if self.ring.is_empty() {
            return None;
        }

        let hash = key_hash(key);
        
        // Find first node with position >= hash
        for (pos, node_id) in &self.ring {
            if *pos >= hash {
                return Some(*node_id);
            }
        }

        // Wrap around to first node
        Some(self.ring[0].1)
    }

    /// Finds nodes for a key (primary + replicas).
    pub fn nodes_for_key(&self, key: &ContentAddress, count: usize) -> Vec<NodeId> {
        if self.ring.is_empty() {
            return Vec::new();
        }

        let hash = key_hash(key);
        let mut result = Vec::with_capacity(count);
        let mut seen = alloc::collections::BTreeSet::new();

        // Find starting position
        let start = self.ring.iter()
            .position(|(pos, _)| *pos >= hash)
            .unwrap_or(0);

        // Collect unique nodes
        for i in 0..self.ring.len() {
            let idx = (start + i) % self.ring.len();
            let node_id = self.ring[idx].1;

            if !seen.contains(&node_id) {
                seen.insert(node_id);
                result.push(node_id);

                if result.len() >= count {
                    break;
                }
            }
        }

        result
    }

    /// Gets node info.
    pub fn node_info(&self, id: NodeId) -> Option<&NodeInfo> {
        self.nodes.get(&id)
    }

    /// Lists all nodes.
    pub fn all_nodes(&self) -> Vec<&NodeInfo> {
        self.nodes.values().collect()
    }

    /// Lists healthy nodes.
    pub fn healthy_nodes(&self) -> Vec<&NodeInfo> {
        self.nodes
            .values()
            .filter(|n| n.status == NodeStatus::Healthy)
            .collect()
    }
}

/// Computes hash of a content address for ring placement.
fn key_hash(key: &ContentAddress) -> u64 {
    let bytes = key.as_bytes();
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

/// Distributed storage protocol messages.
#[derive(Debug, Clone)]
pub enum DistributedMessage {
    // Membership
    Join { node: NodeInfo },
    Leave { node_id: NodeId },
    Heartbeat { node_id: NodeId, timestamp: u64 },
    MembershipUpdate { nodes: Vec<NodeInfo> },

    // Data operations
    Put {
        key: ContentAddress,
        data: Vec<u8>,
        request_id: u64,
    },
    Get {
        key: ContentAddress,
        request_id: u64,
    },
    Delete {
        key: ContentAddress,
        request_id: u64,
    },

    // Responses
    PutAck {
        key: ContentAddress,
        request_id: u64,
        success: bool,
    },
    GetResponse {
        key: ContentAddress,
        request_id: u64,
        data: Option<Vec<u8>>,
        version: u64,
    },
    DeleteAck {
        key: ContentAddress,
        request_id: u64,
        success: bool,
    },

    // Replication
    Replicate {
        key: ContentAddress,
        data: Vec<u8>,
        version: u64,
    },
    ReplicateAck {
        key: ContentAddress,
        version: u64,
        success: bool,
    },

    // Anti-entropy
    MerkleTree {
        node_id: NodeId,
        root_hash: [u8; 32],
    },
    MerkleSync {
        node_id: NodeId,
        range_start: u64,
        range_end: u64,
    },
    MerkleSyncData {
        keys: Vec<(ContentAddress, u64)>, // key, version
    },
}

/// Distributed storage coordinator.
pub struct DistributedCoordinator {
    /// This node's ID.
    local_node: NodeId,
    /// Hash ring.
    ring: Mutex<HashRing>,
    /// Replication config.
    config: ReplicationConfig,
    /// Pending requests.
    pending: Mutex<BTreeMap<u64, PendingRequest>>,
    /// Next request ID.
    next_request_id: Mutex<u64>,
}

/// Pending distributed request.
#[derive(Debug)]
struct PendingRequest {
    key: ContentAddress,
    required_acks: u8,
    received_acks: u8,
    success: bool,
}

impl DistributedCoordinator {
    /// Creates a new coordinator.
    pub fn new(local_node: NodeId, config: ReplicationConfig) -> Self {
        Self {
            local_node,
            ring: Mutex::new(HashRing::new()),
            config,
            pending: Mutex::new(BTreeMap::new()),
            next_request_id: Mutex::new(1),
        }
    }

    /// Joins the cluster.
    pub fn join(&self, info: NodeInfo) -> DistributedMessage {
        self.ring.lock().add_node(info.clone());
        DistributedMessage::Join { node: info }
    }

    /// Prepares a put operation.
    pub fn prepare_put(&self, key: ContentAddress, data: Vec<u8>) -> (Vec<NodeId>, DistributedMessage) {
        let nodes = self.ring.lock().nodes_for_key(&key, self.config.replication_factor as usize);
        
        let mut next_id = self.next_request_id.lock();
        let request_id = *next_id;
        *next_id += 1;

        self.pending.lock().insert(request_id, PendingRequest {
            key,
            required_acks: self.config.write_quorum,
            received_acks: 0,
            success: false,
        });

        let msg = DistributedMessage::Put {
            key,
            data,
            request_id,
        };

        (nodes, msg)
    }

    /// Prepares a get operation.
    pub fn prepare_get(&self, key: ContentAddress) -> (Vec<NodeId>, DistributedMessage) {
        let nodes = self.ring.lock().nodes_for_key(&key, self.config.read_quorum as usize);
        
        let mut next_id = self.next_request_id.lock();
        let request_id = *next_id;
        *next_id += 1;

        let msg = DistributedMessage::Get { key, request_id };

        (nodes, msg)
    }

    /// Handles a put acknowledgement.
    pub fn handle_put_ack(&self, request_id: u64, success: bool) -> Option<bool> {
        let mut pending = self.pending.lock();
        
        if let Some(req) = pending.get_mut(&request_id) {
            if success {
                req.received_acks += 1;
            }

            if req.received_acks >= req.required_acks {
                req.success = true;
                let result = req.success;
                pending.remove(&request_id);
                return Some(result);
            }
        }

        None
    }

    /// Updates node status.
    pub fn update_node_status(&self, node_id: NodeId, status: NodeStatus) {
        let mut ring = self.ring.lock();
        if let Some(info) = ring.nodes.get_mut(&node_id) {
            info.status = status;
        }
    }

    /// Gets cluster statistics.
    pub fn stats(&self) -> ClusterStats {
        let ring = self.ring.lock();
        let nodes = ring.all_nodes();
        
        ClusterStats {
            total_nodes: nodes.len(),
            healthy_nodes: nodes.iter().filter(|n| n.status == NodeStatus::Healthy).count(),
            total_capacity: nodes.iter().map(|n| n.capacity_bytes).sum(),
            used_bytes: nodes.iter().map(|n| n.used_bytes).sum(),
        }
    }
}

/// Cluster statistics.
#[derive(Debug, Clone)]
pub struct ClusterStats {
    pub total_nodes: usize,
    pub healthy_nodes: usize,
    pub total_capacity: u64,
    pub used_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_ring() {
        let mut ring = HashRing::new();

        ring.add_node(NodeInfo {
            id: NodeId(1),
            address: NodeAddress::new([127, 0, 0, 1], 8001),
            status: NodeStatus::Healthy,
            capacity_bytes: 1024 * 1024 * 1024,
            used_bytes: 0,
            last_heartbeat: 0,
            virtual_nodes: 16,
        });

        ring.add_node(NodeInfo {
            id: NodeId(2),
            address: NodeAddress::new([127, 0, 0, 2], 8002),
            status: NodeStatus::Healthy,
            capacity_bytes: 1024 * 1024 * 1024,
            used_bytes: 0,
            last_heartbeat: 0,
            virtual_nodes: 16,
        });

        let key = ContentAddress::from_data(b"test key");
        let primary = ring.primary(&key);
        assert!(primary.is_some());

        let nodes = ring.nodes_for_key(&key, 2);
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn test_coordinator() {
        let config = ReplicationConfig {
            replication_factor: 3,
            write_quorum: 2,
            read_quorum: 1,
            ..Default::default()
        };

        let coord = DistributedCoordinator::new(NodeId(0), config);

        // Add nodes to ring
        {
            let mut ring = coord.ring.lock();
            for i in 1..=3 {
                ring.add_node(NodeInfo {
                    id: NodeId(i),
                    address: NodeAddress::new([127, 0, 0, i as u8], 8000 + i as u16),
                    status: NodeStatus::Healthy,
                    capacity_bytes: 1024 * 1024 * 1024,
                    used_bytes: 0,
                    last_heartbeat: 0,
                    virtual_nodes: 16,
                });
            }
        }

        let key = ContentAddress::from_data(b"test data");
        let (nodes, msg) = coord.prepare_put(key, b"test data".to_vec());

        assert!(!nodes.is_empty());
        match msg {
            DistributedMessage::Put { request_id, .. } => {
                // Simulate quorum acks
                assert!(coord.handle_put_ack(request_id, true).is_none()); // 1 ack
                assert!(coord.handle_put_ack(request_id, true).is_some()); // 2 acks = quorum
            }
            _ => panic!("Expected Put message"),
        }
    }
}

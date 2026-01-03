//! # Capability Revocation Engine
//!
//! Advanced capability revocation with delegation chains and time-limited tokens.
//!
//! ## Features
//!
//! - **Immediate revocation**: Instantly invalidate tokens
//! - **Cascade revocation**: Revoke all derived capabilities
//! - **Delegation chains**: Track and audit capability delegation
//! - **Time-limited tokens**: Automatic expiration
//! - **Revocation lists**: Efficient revocation checking
//!
//! ## Design
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   Revocation Engine                         │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
//! │  │   Active     │  │  Revocation  │  │  Delegation  │      │
//! │  │   Tokens     │  │    List      │  │    Chains    │      │
//! │  └──────────────┘  └──────────────┘  └──────────────┘      │
//! ├─────────────────────────────────────────────────────────────┤
//! │              Bloom Filter (Fast Revocation Check)           │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;

use spin::Mutex;

use super::{CapabilityToken, CapabilityEntry, CapError, Operations, ResourceId};
use crate::sched::ProcessId;

// =============================================================================
// Time-Limited Capabilities
// =============================================================================

/// Duration units for time-limited capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duration {
    /// Cycles (raw CPU cycles)
    Cycles(u64),
    /// Milliseconds (approximate, depends on CPU frequency)
    Milliseconds(u64),
    /// Seconds
    Seconds(u64),
    /// Minutes
    Minutes(u64),
    /// Hours
    Hours(u64),
}

impl Duration {
    /// Converts duration to cycles (assuming ~2GHz CPU).
    pub fn to_cycles(&self) -> u64 {
        const CYCLES_PER_MS: u64 = 2_000_000; // 2GHz
        match *self {
            Self::Cycles(c) => c,
            Self::Milliseconds(ms) => ms * CYCLES_PER_MS,
            Self::Seconds(s) => s * 1000 * CYCLES_PER_MS,
            Self::Minutes(m) => m * 60 * 1000 * CYCLES_PER_MS,
            Self::Hours(h) => h * 60 * 60 * 1000 * CYCLES_PER_MS,
        }
    }
}

/// Time-limited capability configuration.
#[derive(Debug, Clone)]
pub struct TimeLimitedConfig {
    /// Duration until expiration
    pub duration: Duration,
    /// Whether the token can be renewed
    pub renewable: bool,
    /// Maximum renewals (0 = unlimited if renewable)
    pub max_renewals: u32,
    /// Whether expiration triggers cascade revocation
    pub cascade_on_expire: bool,
}

impl Default for TimeLimitedConfig {
    fn default() -> Self {
        Self {
            duration: Duration::Hours(1),
            renewable: false,
            max_renewals: 0,
            cascade_on_expire: false,
        }
    }
}

// =============================================================================
// Delegation Chain
// =============================================================================

/// A node in the delegation chain.
#[derive(Debug, Clone)]
pub struct DelegationNode {
    /// The capability token
    pub token: CapabilityToken,
    /// Process that granted this capability
    pub granter: ProcessId,
    /// Process that received this capability
    pub grantee: ProcessId,
    /// Operations granted
    pub operations: Operations,
    /// Timestamp of grant
    pub granted_at: u64,
    /// Optional constraints (e.g., time limit)
    pub constraints: DelegationConstraints,
}

/// Constraints on a delegated capability.
#[derive(Debug, Clone, Default)]
pub struct DelegationConstraints {
    /// Maximum delegation depth (None = unlimited)
    pub max_depth: Option<u32>,
    /// Current depth in chain
    pub current_depth: u32,
    /// Time limit configuration
    pub time_limit: Option<TimeLimitedConfig>,
    /// Allowed sub-operations for further delegation
    pub delegable_ops: Operations,
    /// Restrict to specific processes
    pub allowed_grantees: Option<Vec<ProcessId>>,
}

/// Complete delegation chain from root to leaf.
#[derive(Debug, Clone)]
pub struct DelegationChain {
    /// Root token (original capability)
    pub root: CapabilityToken,
    /// Chain of delegations from root to target
    pub chain: Vec<DelegationNode>,
    /// Total depth
    pub depth: u32,
}

impl DelegationChain {
    /// Creates a new chain with just a root.
    pub fn new(root: CapabilityToken) -> Self {
        Self {
            root,
            chain: Vec::new(),
            depth: 0,
        }
    }

    /// Adds a node to the chain.
    pub fn push(&mut self, node: DelegationNode) {
        self.chain.push(node);
        self.depth += 1;
    }

    /// Gets the current token (end of chain).
    pub fn current(&self) -> CapabilityToken {
        self.chain.last().map(|n| n.token).unwrap_or(self.root)
    }

    /// Validates the chain integrity.
    pub fn validate(&self) -> Result<(), ChainError> {
        let mut expected_parent = self.root;
        
        for (i, node) in self.chain.iter().enumerate() {
            // Check depth constraints
            if let Some(max_depth) = node.constraints.max_depth {
                if i as u32 >= max_depth {
                    return Err(ChainError::DepthExceeded);
                }
            }
            
            // Check operation attenuation
            if i > 0 {
                let prev_ops = self.chain[i - 1].operations;
                if !prev_ops.contains(node.operations) {
                    return Err(ChainError::AttenuationViolation);
                }
            }
            
            expected_parent = node.token;
        }
        
        Ok(())
    }
}

/// Delegation chain errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainError {
    /// Chain depth exceeded maximum
    DepthExceeded,
    /// Operation attenuation violated
    AttenuationViolation,
    /// Grantee not in allowed list
    UnauthorizedGrantee,
    /// Chain broken (missing node)
    BrokenChain,
    /// Chain validation failed
    ValidationFailed,
}

// =============================================================================
// Revocation List
// =============================================================================

/// Revocation entry with metadata.
#[derive(Debug, Clone)]
pub struct RevocationEntry {
    /// Revoked token
    pub token: CapabilityToken,
    /// Who revoked it
    pub revoker: ProcessId,
    /// When it was revoked
    pub revoked_at: u64,
    /// Reason for revocation
    pub reason: RevocationReason,
    /// Whether children were also revoked
    pub cascaded: bool,
    /// Number of children revoked
    pub children_revoked: u32,
}

/// Reason for capability revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevocationReason {
    /// Explicit revocation by owner
    ExplicitRevoke,
    /// Parent capability was revoked
    ParentRevoked,
    /// Capability expired
    Expired,
    /// Security violation detected
    SecurityViolation,
    /// Process terminated
    ProcessTerminated,
    /// Resource deleted
    ResourceDeleted,
    /// Administrative action
    Administrative,
}

/// Bloom filter for fast revocation checking.
struct RevocationBloomFilter {
    bits: [u64; 256], // 16KB bloom filter
    hash_count: u8,
}

impl RevocationBloomFilter {
    fn new() -> Self {
        Self {
            bits: [0; 256],
            hash_count: 4,
        }
    }

    fn insert(&mut self, token: &CapabilityToken) {
        let bytes = token.as_bytes();
        for i in 0..self.hash_count {
            let hash = self.hash(&bytes, i);
            let idx = (hash / 64) as usize % 256;
            let bit = hash % 64;
            self.bits[idx] |= 1 << bit;
        }
    }

    fn may_contain(&self, token: &CapabilityToken) -> bool {
        let bytes = token.as_bytes();
        for i in 0..self.hash_count {
            let hash = self.hash(&bytes, i);
            let idx = (hash / 64) as usize % 256;
            let bit = hash % 64;
            if (self.bits[idx] & (1 << bit)) == 0 {
                return false;
            }
        }
        true
    }

    fn hash(&self, data: &[u8], seed: u8) -> u64 {
        // FNV-1a hash variant
        let mut h: u64 = 0xcbf29ce484222325;
        h = h.wrapping_mul(0x100000001b3).wrapping_add(seed as u64);
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    fn clear(&mut self) {
        self.bits = [0; 256];
    }
}

// =============================================================================
// Revocation Engine
// =============================================================================

/// Revocation engine configuration.
#[derive(Debug, Clone)]
pub struct RevocationConfig {
    /// Maximum revocation list size
    pub max_revocations: usize,
    /// Enable bloom filter for fast checking
    pub use_bloom_filter: bool,
    /// Enable cascade revocation by default
    pub default_cascade: bool,
    /// Keep revocation history for audit
    pub keep_history: bool,
    /// History retention limit
    pub history_limit: usize,
}

impl Default for RevocationConfig {
    fn default() -> Self {
        Self {
            max_revocations: 100_000,
            use_bloom_filter: true,
            default_cascade: true,
            keep_history: true,
            history_limit: 10_000,
        }
    }
}

/// The revocation engine.
pub struct RevocationEngine {
    /// Configuration
    config: RevocationConfig,
    /// Active revocations (tokens currently revoked)
    revoked: Mutex<BTreeSet<CapabilityToken>>,
    /// Delegation chains (token -> parent chain)
    chains: Mutex<BTreeMap<CapabilityToken, DelegationChain>>,
    /// Children map (parent -> children)
    children: Mutex<BTreeMap<CapabilityToken, Vec<CapabilityToken>>>,
    /// Bloom filter for fast revocation check
    bloom: Mutex<RevocationBloomFilter>,
    /// Revocation history
    history: Mutex<VecDeque<RevocationEntry>>,
    /// Time-limited tokens (sorted by expiration)
    timed_tokens: Mutex<BTreeMap<u64, Vec<CapabilityToken>>>,
}

impl RevocationEngine {
    /// Creates a new revocation engine.
    pub fn new(config: RevocationConfig) -> Self {
        Self {
            config,
            revoked: Mutex::new(BTreeSet::new()),
            chains: Mutex::new(BTreeMap::new()),
            children: Mutex::new(BTreeMap::new()),
            bloom: Mutex::new(RevocationBloomFilter::new()),
            history: Mutex::new(VecDeque::new()),
            timed_tokens: Mutex::new(BTreeMap::new()),
        }
    }

    /// Registers a new capability (for tracking).
    pub fn register(
        &self,
        token: CapabilityToken,
        parent: Option<CapabilityToken>,
        granter: ProcessId,
        grantee: ProcessId,
        operations: Operations,
        constraints: DelegationConstraints,
    ) {
        // Build delegation chain
        let chain = if let Some(parent_token) = parent {
            let chains = self.chains.lock();
            let mut chain = chains
                .get(&parent_token)
                .cloned()
                .unwrap_or_else(|| DelegationChain::new(parent_token));
            drop(chains);

            chain.push(DelegationNode {
                token,
                granter,
                grantee,
                operations,
                granted_at: crate::arch::read_cycle_counter(),
                constraints: constraints.clone(),
            });
            chain
        } else {
            DelegationChain::new(token)
        };

        // Store chain
        self.chains.lock().insert(token, chain);

        // Update parent's children list
        if let Some(parent_token) = parent {
            self.children
                .lock()
                .entry(parent_token)
                .or_default()
                .push(token);
        }

        // Register time-limited tokens
        if let Some(ref time_config) = constraints.time_limit {
            let expires_at = crate::arch::read_cycle_counter() + time_config.duration.to_cycles();
            self.timed_tokens
                .lock()
                .entry(expires_at)
                .or_default()
                .push(token);
        }
    }

    /// Revokes a capability.
    pub fn revoke(
        &self,
        token: CapabilityToken,
        revoker: ProcessId,
        reason: RevocationReason,
        cascade: bool,
    ) -> Result<u32, CapError> {
        let mut total_revoked = 0;

        // Add to revocation set
        {
            let mut revoked = self.revoked.lock();
            if revoked.len() >= self.config.max_revocations {
                // Remove oldest revocations if needed
                // In production, implement proper eviction
            }
            revoked.insert(token);
            total_revoked += 1;
        }

        // Update bloom filter
        if self.config.use_bloom_filter {
            self.bloom.lock().insert(&token);
        }

        // Cascade revocation to children
        let children_revoked = if cascade || self.config.default_cascade {
            self.revoke_children(token, revoker, reason)?
        } else {
            0
        };
        total_revoked += children_revoked;

        // Record in history
        if self.config.keep_history {
            let mut history = self.history.lock();
            if history.len() >= self.config.history_limit {
                history.pop_front();
            }
            history.push_back(RevocationEntry {
                token,
                revoker,
                revoked_at: crate::arch::read_cycle_counter(),
                reason,
                cascaded: cascade,
                children_revoked,
            });
        }

        Ok(total_revoked)
    }

    /// Revokes all children of a token.
    fn revoke_children(
        &self,
        parent: CapabilityToken,
        revoker: ProcessId,
        reason: RevocationReason,
    ) -> Result<u32, CapError> {
        let children: Vec<CapabilityToken> = {
            let children_map = self.children.lock();
            children_map.get(&parent).cloned().unwrap_or_default()
        };

        let mut total = 0;
        for child in children {
            total += self.revoke(child, revoker, RevocationReason::ParentRevoked, true)?;
        }

        Ok(total)
    }

    /// Checks if a token is revoked (fast path).
    pub fn is_revoked(&self, token: &CapabilityToken) -> bool {
        // Fast bloom filter check
        if self.config.use_bloom_filter {
            if !self.bloom.lock().may_contain(token) {
                return false;
            }
        }

        // Full check
        self.revoked.lock().contains(token)
    }

    /// Checks and expires time-limited tokens.
    pub fn check_expirations(&self) -> Vec<CapabilityToken> {
        let now = crate::arch::read_cycle_counter();
        let mut expired = Vec::new();

        let mut timed = self.timed_tokens.lock();
        let expired_times: Vec<u64> = timed
            .range(..=now)
            .map(|(&t, _)| t)
            .collect();

        for time in expired_times {
            if let Some(tokens) = timed.remove(&time) {
                for token in tokens {
                    if !self.is_revoked(&token) {
                        expired.push(token);
                    }
                }
            }
        }

        expired
    }

    /// Renews a time-limited token.
    pub fn renew(
        &self,
        token: CapabilityToken,
        owner: ProcessId,
        new_duration: Duration,
    ) -> Result<u64, CapError> {
        // Check if token is renewable
        let chains = self.chains.lock();
        let chain = chains.get(&token).ok_or(CapError::TokenNotFound)?;

        let constraints = chain.chain.last()
            .map(|n| &n.constraints)
            .ok_or(CapError::TokenNotFound)?;

        let time_config = constraints.time_limit.as_ref()
            .ok_or(CapError::OperationNotAllowed)?;

        if !time_config.renewable {
            return Err(CapError::OperationNotAllowed);
        }

        drop(chains);

        // Calculate new expiration
        let new_expires = crate::arch::read_cycle_counter() + new_duration.to_cycles();

        // Update timed tokens
        self.timed_tokens
            .lock()
            .entry(new_expires)
            .or_default()
            .push(token);

        Ok(new_expires)
    }

    /// Gets the delegation chain for a token.
    pub fn get_chain(&self, token: &CapabilityToken) -> Option<DelegationChain> {
        self.chains.lock().get(token).cloned()
    }

    /// Gets revocation history.
    pub fn get_history(&self, limit: usize) -> Vec<RevocationEntry> {
        let history = self.history.lock();
        history.iter().rev().take(limit).cloned().collect()
    }

    /// Clears expired revocations (older than retention period).
    pub fn cleanup(&self, retention_cycles: u64) {
        let now = crate::arch::read_cycle_counter();
        let cutoff = now.saturating_sub(retention_cycles);

        // Clean history
        let mut history = self.history.lock();
        while let Some(front) = history.front() {
            if front.revoked_at < cutoff {
                history.pop_front();
            } else {
                break;
            }
        }

        // Could also rebuild bloom filter here if needed
    }
}

// =============================================================================
// Global Revocation Engine
// =============================================================================

use spin::Once;

static REVOCATION_ENGINE: Once<RevocationEngine> = Once::new();

/// Initializes the global revocation engine.
pub fn init_revocation(config: RevocationConfig) {
    REVOCATION_ENGINE.call_once(|| RevocationEngine::new(config));
}

/// Gets the global revocation engine.
pub fn revocation() -> &'static RevocationEngine {
    REVOCATION_ENGINE.get().expect("Revocation engine not initialized")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_revocation() {
        let engine = RevocationEngine::new(RevocationConfig::default());
        let token = CapabilityToken::new([1, 2, 3, 4]);
        let owner = ProcessId::new(1);

        // Register token
        engine.register(
            token,
            None,
            owner,
            owner,
            Operations::ALL,
            DelegationConstraints::default(),
        );

        assert!(!engine.is_revoked(&token));

        // Revoke
        engine.revoke(token, owner, RevocationReason::ExplicitRevoke, false).unwrap();

        assert!(engine.is_revoked(&token));
    }

    #[test]
    fn test_cascade_revocation() {
        let engine = RevocationEngine::new(RevocationConfig::default());
        let owner = ProcessId::new(1);
        let child_owner = ProcessId::new(2);

        let parent = CapabilityToken::new([1, 0, 0, 0]);
        let child = CapabilityToken::new([2, 0, 0, 0]);

        // Register parent
        engine.register(parent, None, owner, owner, Operations::ALL, DelegationConstraints::default());

        // Register child
        engine.register(child, Some(parent), owner, child_owner, Operations::READ, DelegationConstraints::default());

        // Revoke parent with cascade
        let count = engine.revoke(parent, owner, RevocationReason::ExplicitRevoke, true).unwrap();

        assert_eq!(count, 2); // Parent + child
        assert!(engine.is_revoked(&parent));
        assert!(engine.is_revoked(&child));
    }

    #[test]
    fn test_delegation_chain() {
        let mut chain = DelegationChain::new(CapabilityToken::new([1, 0, 0, 0]));

        chain.push(DelegationNode {
            token: CapabilityToken::new([2, 0, 0, 0]),
            granter: ProcessId::new(1),
            grantee: ProcessId::new(2),
            operations: Operations::READ,
            granted_at: 0,
            constraints: DelegationConstraints::default(),
        });

        assert_eq!(chain.depth, 1);
        assert!(chain.validate().is_ok());
    }
}

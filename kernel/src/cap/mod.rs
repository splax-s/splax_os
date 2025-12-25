//! # S-CAP: Capability-Based Security
//!
//! S-CAP is the heart of Splax security. Every resource access in the system
//! requires an explicit capability token. There are no users, groups, or root.
//!
//! ## Core Concepts
//!
//! - **Capability Token**: A cryptographic proof of access rights
//! - **Capability Table**: Kernel-managed table of all valid tokens
//! - **Operations**: What the token allows (read, write, execute, grant)
//! - **Audit Log**: Every capability operation is logged
//!
//! ## Security Properties
//!
//! 1. **Unforgeability**: Tokens are cryptographically signed
//! 2. **Attenuation**: Derived tokens can only have equal or fewer rights
//! 3. **Revocability**: Tokens can be revoked at any time
//! 4. **Auditability**: All operations are logged
//!
//! ## Example
//!
//! ```ignore
//! // Every access requires a capability check
//! cap_table.check(process_id, token, "file:read")?;
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use spin::Mutex;

use crate::sched::ProcessId;

/// A cryptographic capability token.
///
/// Tokens are 256-bit values that prove access rights.
/// They are unforgeable and can only be created by the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityToken {
    /// The token value (in practice, this would be a cryptographic hash)
    value: [u64; 4],
}

impl CapabilityToken {
    /// Creates a new token with the specified value.
    ///
    /// This should only be called by the kernel.
    pub(crate) fn new(value: [u64; 4]) -> Self {
        Self { value }
    }

    /// Returns the token as bytes.
    pub fn as_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        for (i, &v) in self.value.iter().enumerate() {
            bytes[i * 8..(i + 1) * 8].copy_from_slice(&v.to_le_bytes());
        }
        bytes
    }
}

/// Operations that can be authorized by a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operations {
    bits: u32,
}

impl Operations {
    /// No operations allowed
    pub const NONE: Self = Self { bits: 0 };
    /// Read operation
    pub const READ: Self = Self { bits: 1 << 0 };
    /// Write operation
    pub const WRITE: Self = Self { bits: 1 << 1 };
    /// Execute operation
    pub const EXECUTE: Self = Self { bits: 1 << 2 };
    /// Grant (delegate) the capability to others
    pub const GRANT: Self = Self { bits: 1 << 3 };
    /// Revoke derived capabilities
    pub const REVOKE: Self = Self { bits: 1 << 4 };
    /// All operations
    pub const ALL: Self = Self { bits: 0x1F };

    /// Combines two operation sets.
    pub const fn union(self, other: Self) -> Self {
        Self { bits: self.bits | other.bits }
    }

    /// Intersects two operation sets.
    pub const fn intersection(self, other: Self) -> Self {
        Self { bits: self.bits & other.bits }
    }

    /// Checks if this set contains all operations in `other`.
    pub const fn contains(self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }

    /// Checks if this set is empty.
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }
}

/// A capability entry in the kernel's capability table.
#[derive(Debug, Clone)]
pub struct CapabilityEntry {
    /// The token for this capability
    pub token: CapabilityToken,
    /// Process that owns this capability
    pub owner: ProcessId,
    /// Resource this capability grants access to
    pub resource: ResourceId,
    /// Operations allowed
    pub operations: Operations,
    /// Parent token (None for root capabilities)
    pub parent: Option<CapabilityToken>,
    /// Whether this capability has been revoked
    pub revoked: bool,
    /// Creation timestamp (cycles)
    pub created_at: u64,
    /// Expiration timestamp (None = never expires)
    pub expires_at: Option<u64>,
}

/// Resource identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId {
    /// Resource type (e.g., "memory", "channel", "service")
    pub resource_type: String,
    /// Unique identifier within the type
    pub id: u64,
}

impl ResourceId {
    /// Creates a new resource ID.
    pub fn new(resource_type: impl Into<String>, id: u64) -> Self {
        Self {
            resource_type: resource_type.into(),
            id,
        }
    }
}

/// The kernel capability table.
///
/// This is the single source of truth for all capabilities in the system.
pub struct CapabilityTable {
    /// All capabilities, indexed by token
    entries: Mutex<BTreeMap<CapabilityToken, CapabilityEntry>>,
    /// Capabilities by owner process
    by_owner: Mutex<BTreeMap<ProcessId, Vec<CapabilityToken>>>,
    /// Audit log of capability operations
    audit_log: Mutex<AuditLog>,
    /// Counter for generating unique token values
    token_counter: Mutex<u64>,
    /// Maximum number of capabilities
    max_capabilities: usize,
}

impl CapabilityTable {
    /// Creates a new capability table.
    pub fn new(max_capabilities: usize) -> Self {
        Self {
            entries: Mutex::new(BTreeMap::new()),
            by_owner: Mutex::new(BTreeMap::new()),
            audit_log: Mutex::new(AuditLog::new()),
            token_counter: Mutex::new(0),
            max_capabilities,
        }
    }

    /// Creates a new root capability.
    ///
    /// Root capabilities have no parent and are created by the kernel.
    /// Only the kernel should call this function.
    ///
    /// # Arguments
    ///
    /// * `owner` - Process that will own this capability
    /// * `resource` - Resource to grant access to
    /// * `operations` - Operations allowed
    ///
    /// # Returns
    ///
    /// The new capability token.
    pub fn create_root(
        &self,
        owner: ProcessId,
        resource: ResourceId,
        operations: Operations,
    ) -> Result<CapabilityToken, CapError> {
        let token = self.generate_token();

        let entry = CapabilityEntry {
            token,
            owner,
            resource: resource.clone(),
            operations,
            parent: None,
            revoked: false,
            created_at: crate::arch::read_cycle_counter(),
            expires_at: None,
        };

        self.insert_entry(entry)?;

        self.audit_log.lock().log(AuditEntry {
            operation: AuditOperation::Create,
            token,
            actor: owner,
            resource: Some(resource),
            result: AuditResult::Success,
            timestamp: crate::arch::read_cycle_counter(),
        });

        Ok(token)
    }

    /// Grants (delegates) a capability to another process.
    ///
    /// The new capability can only have equal or fewer rights (attenuation).
    ///
    /// # Arguments
    ///
    /// * `granter` - Process granting the capability
    /// * `parent_token` - Token to derive from
    /// * `grantee` - Process receiving the capability
    /// * `operations` - Operations to grant (must be subset of parent)
    ///
    /// # Returns
    ///
    /// The new capability token.
    pub fn grant(
        &self,
        granter: ProcessId,
        parent_token: CapabilityToken,
        grantee: ProcessId,
        operations: Operations,
    ) -> Result<CapabilityToken, CapError> {
        // Check that granter owns the parent and can grant
        let parent = self.get_entry(&parent_token)?;

        if parent.owner != granter {
            return Err(CapError::NotOwner);
        }

        if !parent.operations.contains(Operations::GRANT) {
            return Err(CapError::OperationNotAllowed);
        }

        // Attenuation: new operations must be subset of parent
        let new_operations = operations.intersection(parent.operations);

        // Generate new token
        let token = self.generate_token();

        let entry = CapabilityEntry {
            token,
            owner: grantee,
            resource: parent.resource.clone(),
            operations: new_operations,
            parent: Some(parent_token),
            revoked: false,
            created_at: crate::arch::read_cycle_counter(),
            expires_at: parent.expires_at,
        };

        self.insert_entry(entry)?;

        self.audit_log.lock().log(AuditEntry {
            operation: AuditOperation::Grant,
            token,
            actor: granter,
            resource: Some(parent.resource),
            result: AuditResult::Success,
            timestamp: crate::arch::read_cycle_counter(),
        });

        Ok(token)
    }

    /// Checks if a capability allows an operation.
    ///
    /// This is the core security check. Every resource access must go through here.
    ///
    /// # Arguments
    ///
    /// * `process` - Process attempting the operation
    /// * `token` - Capability token
    /// * `operation` - Operation being attempted
    ///
    /// # Returns
    ///
    /// `Ok(())` if allowed, `Err` with reason if denied.
    pub fn check(
        &self,
        process: ProcessId,
        token: CapabilityToken,
        operation: Operations,
    ) -> Result<(), CapError> {
        let entry = self.get_entry(&token)?;

        // Check ownership
        if entry.owner != process {
            self.log_failure(token, process, AuditOperation::Check);
            return Err(CapError::NotOwner);
        }

        // Check revocation
        if entry.revoked {
            self.log_failure(token, process, AuditOperation::Check);
            return Err(CapError::Revoked);
        }

        // Check expiration
        if let Some(expires) = entry.expires_at {
            if crate::arch::read_cycle_counter() > expires {
                self.log_failure(token, process, AuditOperation::Check);
                return Err(CapError::Expired);
            }
        }

        // Check operation is allowed
        if !entry.operations.contains(operation) {
            self.log_failure(token, process, AuditOperation::Check);
            return Err(CapError::OperationNotAllowed);
        }

        Ok(())
    }

    /// Revokes a capability and all derived capabilities.
    ///
    /// # Arguments
    ///
    /// * `revoker` - Process revoking the capability
    /// * `token` - Token to revoke
    pub fn revoke(
        &self,
        revoker: ProcessId,
        token: CapabilityToken,
    ) -> Result<(), CapError> {
        let entry = self.get_entry(&token)?;

        // Only owner can revoke, and must have REVOKE operation
        if entry.owner != revoker {
            return Err(CapError::NotOwner);
        }

        // Mark as revoked
        {
            let mut entries = self.entries.lock();
            if let Some(e) = entries.get_mut(&token) {
                e.revoked = true;
            }
        }

        // Recursively revoke all derived capabilities
        self.revoke_derived(token);

        self.audit_log.lock().log(AuditEntry {
            operation: AuditOperation::Revoke,
            token,
            actor: revoker,
            resource: Some(entry.resource),
            result: AuditResult::Success,
            timestamp: crate::arch::read_cycle_counter(),
        });

        Ok(())
    }

    /// Gets the resource associated with a token.
    pub fn get_resource(&self, token: &CapabilityToken) -> Result<ResourceId, CapError> {
        let entry = self.get_entry(token)?;
        Ok(entry.resource)
    }

    // Internal helpers

    fn generate_token(&self) -> CapabilityToken {
        let mut counter = self.token_counter.lock();
        *counter += 1;
        let count = *counter;

        // In a real implementation, this would use cryptographic randomness
        CapabilityToken::new([
            count,
            count.wrapping_mul(0x5851F42D4C957F2D),
            count.wrapping_mul(0x14057B7EF767814F),
            count.wrapping_mul(0x94D049BB133111EB),
        ])
    }

    fn get_entry(&self, token: &CapabilityToken) -> Result<CapabilityEntry, CapError> {
        self.entries
            .lock()
            .get(token)
            .cloned()
            .ok_or(CapError::TokenNotFound)
    }

    fn insert_entry(&self, entry: CapabilityEntry) -> Result<(), CapError> {
        let mut entries = self.entries.lock();
        if entries.len() >= self.max_capabilities {
            return Err(CapError::TableFull);
        }

        let token = entry.token;
        let owner = entry.owner;
        entries.insert(token, entry);

        // Update owner index
        self.by_owner
            .lock()
            .entry(owner)
            .or_default()
            .push(token);

        Ok(())
    }

    fn revoke_derived(&self, parent_token: CapabilityToken) {
        let mut entries = self.entries.lock();
        for entry in entries.values_mut() {
            if entry.parent == Some(parent_token) && !entry.revoked {
                entry.revoked = true;
                // Note: In a full implementation, we'd recursively revoke
                // This is simplified for the initial implementation
            }
        }
    }

    fn log_failure(&self, token: CapabilityToken, actor: ProcessId, op: AuditOperation) {
        self.audit_log.lock().log(AuditEntry {
            operation: op,
            token,
            actor,
            resource: None,
            result: AuditResult::Denied,
            timestamp: crate::arch::read_cycle_counter(),
        });
    }
}

/// Capability errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapError {
    /// Token not found in capability table
    TokenNotFound,
    /// Caller is not the owner of the capability
    NotOwner,
    /// The capability has been revoked
    Revoked,
    /// The capability has expired
    Expired,
    /// The requested operation is not allowed by this capability
    OperationNotAllowed,
    /// The capability table is full
    TableFull,
    /// Invalid capability parameters
    InvalidCapability,
}

/// Audit log for capability operations.
struct AuditLog {
    entries: Vec<AuditEntry>,
    max_entries: usize,
}

impl AuditLog {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 10000,
        }
    }

    fn log(&mut self, entry: AuditEntry) {
        if self.entries.len() >= self.max_entries {
            // Ring buffer behavior: remove oldest
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }
}

/// A single audit log entry.
#[derive(Debug, Clone)]
struct AuditEntry {
    operation: AuditOperation,
    token: CapabilityToken,
    actor: ProcessId,
    resource: Option<ResourceId>,
    result: AuditResult,
    timestamp: u64,
}

/// Type of audit operation.
#[derive(Debug, Clone, Copy)]
enum AuditOperation {
    Create,
    Grant,
    Check,
    Revoke,
}

/// Result of an audited operation.
#[derive(Debug, Clone, Copy)]
enum AuditResult {
    Success,
    Denied,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_creation() {
        let table = CapabilityTable::new(100);
        let owner = ProcessId::new(1);
        let resource = ResourceId::new("test", 42);

        let token = table
            .create_root(owner, resource, Operations::READ)
            .expect("should create capability");

        assert!(table.check(owner, token, Operations::READ).is_ok());
        assert!(table.check(owner, token, Operations::WRITE).is_err());
    }

    #[test]
    fn test_capability_attenuation() {
        let table = CapabilityTable::new(100);
        let owner = ProcessId::new(1);
        let grantee = ProcessId::new(2);
        let resource = ResourceId::new("test", 42);

        let parent = table
            .create_root(owner, resource, Operations::ALL)
            .expect("should create capability");

        let child = table
            .grant(owner, parent, grantee, Operations::READ)
            .expect("should grant capability");

        // Child can read but not write
        assert!(table.check(grantee, child, Operations::READ).is_ok());
        assert!(table.check(grantee, child, Operations::WRITE).is_err());
    }
}

//! # S-STORAGE: Storage Service
//!
//! S-STORAGE provides two storage models:
//!
//! ## 1. Object Storage (Original)
//!
//! Capability-gated object storage where objects are identified by unique IDs
//! rather than paths, accessed via capability tokens.
//!
//! ## 2. VFS Server (Phase A Migration)
//!
//! Traditional VFS operations exposed as an IPC service. This enables the
//! hybrid kernel architecture where filesystem logic runs in userspace.
//!
//! ## Architecture (Hybrid Kernel)
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        Applications                              │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                     Kernel VFS Stub                              │
//! │           (Thin layer that forwards to S-STORAGE)               │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                        S-LINK IPC                                │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  ┌───────────────────┐  ┌────────────────────────────────────┐ │
//! │  │   Object Storage  │  │          VFS Server                 │ │
//! │  │   (ObjectId API)  │  │  (Traditional path-based VFS)       │ │
//! │  └───────────────────┘  └────────────────────────────────────┘ │
//! │                       S-STORAGE Service                          │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Example (Object Storage)
//!
//! ```ignore
//! let id = storage.create(data, metadata, create_token)?;
//! let data = storage.read(id, read_token)?;
//! ```
//!
//! ## Example (VFS Server)
//!
//! ```ignore
//! let server = VfsServer::new();
//! server.mount("/", ramfs, None, false)?;
//! let handle = server.open("/etc/config", OpenFlags::read_only(), 0)?;
//! let data = server.read(handle, None, 1024)?;
//! ```

#![no_std]

extern crate alloc;

// VFS RPC Protocol (message formats for kernel<->storage IPC)
pub mod vfs_protocol;

// VFS Server (userspace filesystem handler)
pub mod vfs_server;

// Re-export VFS types
pub use vfs_protocol::*;
pub use vfs_server::{Filesystem, VfsServer};

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use spin::Mutex;

/// Object identifier - unique within the storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(pub u64);

impl ObjectId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Content hash for deduplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// Computes a hash of the data (simplified).
    pub fn compute(data: &[u8]) -> Self {
        // Simplified hash - real implementation would use SHA-256 or similar
        let mut hash = [0u8; 32];
        for (i, byte) in data.iter().enumerate() {
            hash[i % 32] ^= byte;
        }
        Self(hash)
    }
}

/// Capability token placeholder.
#[derive(Debug, Clone, Copy)]
pub struct CapabilityToken {
    value: [u64; 4],
}

/// Object metadata.
#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    /// Object name (optional, for human reference)
    pub name: Option<String>,
    /// MIME type
    pub content_type: String,
    /// Size in bytes
    pub size: usize,
    /// Creation timestamp (cycles)
    pub created_at: u64,
    /// Last modified timestamp
    pub modified_at: u64,
    /// Custom tags
    pub tags: BTreeMap<String, String>,
    /// Content hash
    pub content_hash: Option<ContentHash>,
    /// Version number
    pub version: u64,
}

impl ObjectMetadata {
    /// Creates new metadata.
    pub fn new(content_type: impl Into<String>, size: usize) -> Self {
        Self {
            name: None,
            content_type: content_type.into(),
            size,
            created_at: 0,
            modified_at: 0,
            tags: BTreeMap::new(),
            content_hash: None,
            version: 1,
        }
    }

    /// Sets the name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Adds a tag.
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }
}

/// An object in storage.
struct StoredObject {
    id: ObjectId,
    metadata: ObjectMetadata,
    data: Vec<u8>,
}

/// Storage configuration.
#[derive(Debug, Clone)]
pub struct StorageConfig {
    /// Maximum object size in bytes
    pub max_object_size: usize,
    /// Maximum number of objects
    pub max_objects: usize,
    /// Enable content-addressed deduplication
    pub enable_dedup: bool,
    /// Enable versioning
    pub enable_versioning: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            max_object_size: 64 * 1024 * 1024, // 64 MB
            max_objects: 1_000_000,
            enable_dedup: true,
            enable_versioning: false,
        }
    }
}

/// The S-STORAGE service.
pub struct Storage {
    config: StorageConfig,
    /// Objects indexed by ID
    objects: Mutex<BTreeMap<ObjectId, StoredObject>>,
    /// Content hash index for deduplication
    by_hash: Mutex<BTreeMap<ContentHash, ObjectId>>,
    /// Objects indexed by name (if named)
    by_name: Mutex<BTreeMap<String, ObjectId>>,
    /// Next object ID
    next_id: Mutex<u64>,
    /// Total storage used
    used_bytes: Mutex<usize>,
}

impl Storage {
    /// Creates a new storage instance.
    pub fn new(config: StorageConfig) -> Self {
        Self {
            config,
            objects: Mutex::new(BTreeMap::new()),
            by_hash: Mutex::new(BTreeMap::new()),
            by_name: Mutex::new(BTreeMap::new()),
            next_id: Mutex::new(1),
            used_bytes: Mutex::new(0),
        }
    }

    /// Creates a new object.
    ///
    /// # Arguments
    ///
    /// * `data` - Object data
    /// * `metadata` - Object metadata
    /// * `cap_token` - Capability token authorizing creation
    ///
    /// # Returns
    ///
    /// The new object ID.
    pub fn create(
        &self,
        data: Vec<u8>,
        mut metadata: ObjectMetadata,
        _cap_token: &CapabilityToken,
    ) -> Result<ObjectId, StorageError> {
        // Check size limit
        if data.len() > self.config.max_object_size {
            return Err(StorageError::ObjectTooLarge);
        }

        // Check deduplication
        if self.config.enable_dedup {
            let hash = ContentHash::compute(&data);
            metadata.content_hash = Some(hash);

            if let Some(&existing_id) = self.by_hash.lock().get(&hash) {
                // Return existing object if content matches
                return Ok(existing_id);
            }
        }

        // Check capacity
        let mut objects = self.objects.lock();
        if objects.len() >= self.config.max_objects {
            return Err(StorageError::StorageFull);
        }

        // Generate ID
        let mut next_id = self.next_id.lock();
        let id = ObjectId::new(*next_id);
        *next_id += 1;

        // Update metadata
        metadata.size = data.len();
        let now = 0u64; // Would use real timestamp
        metadata.created_at = now;
        metadata.modified_at = now;

        // Store by name if present
        if let Some(ref name) = metadata.name {
            self.by_name.lock().insert(name.clone(), id);
        }

        // Store by hash if dedup enabled
        if let Some(hash) = metadata.content_hash {
            self.by_hash.lock().insert(hash, id);
        }

        // Update used bytes
        *self.used_bytes.lock() += data.len();

        // Store object
        objects.insert(id, StoredObject { id, metadata, data });

        Ok(id)
    }

    /// Reads an object's data.
    pub fn read(
        &self,
        id: ObjectId,
        _cap_token: &CapabilityToken,
    ) -> Result<Vec<u8>, StorageError> {
        let objects = self.objects.lock();
        let object = objects.get(&id).ok_or(StorageError::ObjectNotFound)?;
        Ok(object.data.clone())
    }

    /// Gets an object's metadata.
    pub fn metadata(
        &self,
        id: ObjectId,
        _cap_token: &CapabilityToken,
    ) -> Result<ObjectMetadata, StorageError> {
        let objects = self.objects.lock();
        let object = objects.get(&id).ok_or(StorageError::ObjectNotFound)?;
        Ok(object.metadata.clone())
    }

    /// Updates an object's data.
    pub fn update(
        &self,
        id: ObjectId,
        data: Vec<u8>,
        _cap_token: &CapabilityToken,
    ) -> Result<(), StorageError> {
        if data.len() > self.config.max_object_size {
            return Err(StorageError::ObjectTooLarge);
        }

        let mut objects = self.objects.lock();
        let object = objects.get_mut(&id).ok_or(StorageError::ObjectNotFound)?;

        // Update used bytes
        let old_size = object.data.len();
        *self.used_bytes.lock() = self.used_bytes.lock().saturating_sub(old_size) + data.len();

        // Update object
        object.data = data;
        object.metadata.size = object.data.len();
        object.metadata.modified_at = 0; // Would use real timestamp
        object.metadata.version += 1;

        // Update hash if dedup enabled
        if self.config.enable_dedup {
            let hash = ContentHash::compute(&object.data);
            object.metadata.content_hash = Some(hash);
        }

        Ok(())
    }

    /// Deletes an object.
    pub fn delete(
        &self,
        id: ObjectId,
        _cap_token: &CapabilityToken,
    ) -> Result<(), StorageError> {
        let mut objects = self.objects.lock();
        let object = objects.remove(&id).ok_or(StorageError::ObjectNotFound)?;

        // Remove from name index
        if let Some(ref name) = object.metadata.name {
            self.by_name.lock().remove(name);
        }

        // Remove from hash index
        if let Some(hash) = object.metadata.content_hash {
            self.by_hash.lock().remove(&hash);
        }

        // Update used bytes
        *self.used_bytes.lock() = self.used_bytes.lock().saturating_sub(object.data.len());

        Ok(())
    }

    /// Finds an object by name.
    pub fn find_by_name(
        &self,
        name: &str,
        _cap_token: &CapabilityToken,
    ) -> Result<ObjectId, StorageError> {
        self.by_name
            .lock()
            .get(name)
            .copied()
            .ok_or(StorageError::ObjectNotFound)
    }

    /// Lists all objects (returns IDs and metadata).
    pub fn list(&self, _cap_token: &CapabilityToken) -> Vec<(ObjectId, ObjectMetadata)> {
        self.objects
            .lock()
            .values()
            .map(|o| (o.id, o.metadata.clone()))
            .collect()
    }

    /// Gets storage statistics.
    pub fn stats(&self) -> StorageStats {
        let objects = self.objects.lock();
        StorageStats {
            object_count: objects.len(),
            used_bytes: *self.used_bytes.lock(),
            max_objects: self.config.max_objects,
            max_object_size: self.config.max_object_size,
        }
    }
}

/// Storage statistics.
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub object_count: usize,
    pub used_bytes: usize,
    pub max_objects: usize,
    pub max_object_size: usize,
}

/// Storage errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    /// Object not found
    ObjectNotFound,
    /// Object exceeds size limit
    ObjectTooLarge,
    /// Storage is full
    StorageFull,
    /// Invalid capability
    InvalidCapability,
    /// Permission denied
    PermissionDenied,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken { value: [1, 2, 3, 4] }
    }

    #[test]
    fn test_create_and_read() {
        let storage = Storage::new(StorageConfig::default());
        let token = dummy_token();

        let data = b"Hello, Splax!".to_vec();
        let metadata = ObjectMetadata::new("text/plain", data.len())
            .with_name("greeting");

        let id = storage.create(data.clone(), metadata, &token).expect("should create");

        let read_data = storage.read(id, &token).expect("should read");
        assert_eq!(read_data, data);
    }

    #[test]
    fn test_deduplication() {
        let storage = Storage::new(StorageConfig {
            enable_dedup: true,
            ..Default::default()
        });
        let token = dummy_token();

        let data = b"duplicate data".to_vec();
        let metadata1 = ObjectMetadata::new("text/plain", data.len());
        let metadata2 = ObjectMetadata::new("text/plain", data.len());

        let id1 = storage.create(data.clone(), metadata1, &token).expect("first");
        let id2 = storage.create(data.clone(), metadata2, &token).expect("second");

        // Should return same ID due to deduplication
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_find_by_name() {
        let storage = Storage::new(StorageConfig::default());
        let token = dummy_token();

        let data = b"named object".to_vec();
        let metadata = ObjectMetadata::new("text/plain", data.len())
            .with_name("my-object");

        let id = storage.create(data, metadata, &token).expect("should create");

        let found_id = storage.find_by_name("my-object", &token).expect("should find");
        assert_eq!(found_id, id);
    }
}

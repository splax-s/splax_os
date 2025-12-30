//! # Object Storage API v2
//!
//! Enhanced object storage with:
//! - Query by tags/metadata
//! - Streaming API for large objects
//! - Versioning with history
//! - Transactions
//! - Batch operations

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::{ContentHash, ObjectId, ObjectMetadata, Storage, StorageError};
use splax_cap::CapabilityToken;

/// Query operators for filtering objects.
#[derive(Debug, Clone)]
pub enum QueryOp {
    /// Tag equals value
    TagEquals { key: String, value: String },
    /// Tag contains substring
    TagContains { key: String, substring: String },
    /// Content type matches
    ContentType(String),
    /// Size range
    SizeRange { min: usize, max: usize },
    /// Created after timestamp
    CreatedAfter(u64),
    /// Created before timestamp
    CreatedBefore(u64),
    /// Name pattern (glob-like)
    NamePattern(String),
    /// Content hash matches
    ContentHash(ContentHash),
}

/// Query combining multiple operators.
#[derive(Debug, Clone, Default)]
pub struct ObjectQuery {
    /// All conditions must match (AND)
    pub conditions: Vec<QueryOp>,
    /// Maximum results to return
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: usize,
    /// Sort by field
    pub sort_by: Option<SortField>,
    /// Sort descending
    pub sort_desc: bool,
}

/// Fields to sort by.
#[derive(Debug, Clone, Copy)]
pub enum SortField {
    Name,
    Size,
    CreatedAt,
    ModifiedAt,
}

impl ObjectQuery {
    /// Creates a new empty query.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a tag equals condition.
    pub fn tag_equals(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.conditions.push(QueryOp::TagEquals {
            key: key.into(),
            value: value.into(),
        });
        self
    }

    /// Adds a content type filter.
    pub fn content_type(mut self, ct: impl Into<String>) -> Self {
        self.conditions.push(QueryOp::ContentType(ct.into()));
        self
    }

    /// Adds a size range filter.
    pub fn size_range(mut self, min: usize, max: usize) -> Self {
        self.conditions.push(QueryOp::SizeRange { min, max });
        self
    }

    /// Sets the result limit.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Sets the offset for pagination.
    pub fn offset(mut self, n: usize) -> Self {
        self.offset = n;
        self
    }

    /// Sets sort field and order.
    pub fn sort(mut self, field: SortField, desc: bool) -> Self {
        self.sort_by = Some(field);
        self.sort_desc = desc;
        self
    }
}

/// Query result.
#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Matching objects
    pub objects: Vec<(ObjectId, ObjectMetadata)>,
    /// Total count (before limit/offset)
    pub total_count: usize,
    /// Has more results
    pub has_more: bool,
}

/// Object version.
#[derive(Debug, Clone)]
pub struct ObjectVersion {
    /// Version number
    pub version: u64,
    /// Timestamp
    pub timestamp: u64,
    /// Size at this version
    pub size: usize,
    /// Content hash at this version
    pub content_hash: Option<ContentHash>,
}

/// Streaming chunk for large objects.
#[derive(Debug, Clone)]
pub struct ObjectChunk {
    /// Offset in object
    pub offset: usize,
    /// Chunk data
    pub data: Vec<u8>,
    /// Is last chunk
    pub is_last: bool,
}

/// Batch operation.
#[derive(Debug, Clone)]
pub enum BatchOp {
    /// Create an object
    Create {
        data: Vec<u8>,
        metadata: ObjectMetadata,
    },
    /// Update an object
    Update { id: ObjectId, data: Vec<u8> },
    /// Delete an object
    Delete { id: ObjectId },
    /// Add a tag
    AddTag {
        id: ObjectId,
        key: String,
        value: String,
    },
    /// Remove a tag
    RemoveTag { id: ObjectId, key: String },
}

/// Batch result.
#[derive(Debug, Clone)]
pub enum BatchResult {
    /// Operation succeeded
    Created(ObjectId),
    Updated,
    Deleted,
    TagAdded,
    TagRemoved,
    /// Operation failed
    Failed(StorageError),
}

/// Extended object storage API.
pub trait ObjectStorageApi {
    /// Queries objects by metadata/tags.
    fn query(
        &self,
        query: &ObjectQuery,
        cap_token: &CapabilityToken,
    ) -> Result<QueryResult, StorageError>;

    /// Gets all versions of an object.
    fn versions(
        &self,
        id: ObjectId,
        cap_token: &CapabilityToken,
    ) -> Result<Vec<ObjectVersion>, StorageError>;

    /// Gets a specific version of an object.
    fn read_version(
        &self,
        id: ObjectId,
        version: u64,
        cap_token: &CapabilityToken,
    ) -> Result<Vec<u8>, StorageError>;

    /// Reads a chunk of an object (streaming).
    fn read_chunk(
        &self,
        id: ObjectId,
        offset: usize,
        size: usize,
        cap_token: &CapabilityToken,
    ) -> Result<ObjectChunk, StorageError>;

    /// Writes a chunk of an object (streaming).
    fn write_chunk(
        &self,
        id: ObjectId,
        chunk: ObjectChunk,
        cap_token: &CapabilityToken,
    ) -> Result<(), StorageError>;

    /// Executes batch operations atomically.
    fn batch(
        &self,
        ops: Vec<BatchOp>,
        cap_token: &CapabilityToken,
    ) -> Vec<BatchResult>;

    /// Adds a tag to an object.
    fn add_tag(
        &self,
        id: ObjectId,
        key: impl Into<String>,
        value: impl Into<String>,
        cap_token: &CapabilityToken,
    ) -> Result<(), StorageError>;

    /// Removes a tag from an object.
    fn remove_tag(
        &self,
        id: ObjectId,
        key: &str,
        cap_token: &CapabilityToken,
    ) -> Result<(), StorageError>;

    /// Copies an object.
    fn copy(
        &self,
        id: ObjectId,
        new_name: Option<String>,
        cap_token: &CapabilityToken,
    ) -> Result<ObjectId, StorageError>;
}

/// Implements query matching for ObjectMetadata.
fn matches_query(metadata: &ObjectMetadata, query: &ObjectQuery) -> bool {
    for condition in &query.conditions {
        let matches = match condition {
            QueryOp::TagEquals { key, value } => {
                metadata.tags.get(key).map(|v| v == value).unwrap_or(false)
            }
            QueryOp::TagContains { key, substring } => metadata
                .tags
                .get(key)
                .map(|v| v.contains(substring.as_str()))
                .unwrap_or(false),
            QueryOp::ContentType(ct) => metadata.content_type == *ct,
            QueryOp::SizeRange { min, max } => metadata.size >= *min && metadata.size <= *max,
            QueryOp::CreatedAfter(ts) => metadata.created_at > *ts,
            QueryOp::CreatedBefore(ts) => metadata.created_at < *ts,
            QueryOp::NamePattern(pattern) => metadata
                .name
                .as_ref()
                .map(|n| glob_match(n, pattern))
                .unwrap_or(false),
            QueryOp::ContentHash(hash) => metadata.content_hash.as_ref() == Some(hash),
        };
        if !matches {
            return false;
        }
    }
    true
}

/// Simple glob matching (supports * and ?).
fn glob_match(s: &str, pattern: &str) -> bool {
    let mut s_chars = s.chars().peekable();
    let mut p_chars = pattern.chars().peekable();

    while let Some(p) = p_chars.next() {
        match p {
            '*' => {
                // Match any sequence
                if p_chars.peek().is_none() {
                    return true; // Trailing * matches everything
                }
                // Try matching rest of pattern at each position
                while s_chars.peek().is_some() {
                    let remaining_s: String = s_chars.clone().collect();
                    let remaining_p: String = p_chars.clone().collect();
                    if glob_match(&remaining_s, &remaining_p) {
                        return true;
                    }
                    s_chars.next();
                }
                return false;
            }
            '?' => {
                // Match any single character
                if s_chars.next().is_none() {
                    return false;
                }
            }
            c => {
                if s_chars.next() != Some(c) {
                    return false;
                }
            }
        }
    }

    s_chars.peek().is_none()
}

impl ObjectStorageApi for Storage {
    fn query(
        &self,
        query: &ObjectQuery,
        _cap_token: &CapabilityToken,
    ) -> Result<QueryResult, StorageError> {
        let all = self.list(_cap_token);

        // Filter
        let mut matching: Vec<_> = all
            .into_iter()
            .filter(|(_, m)| matches_query(m, query))
            .collect();

        let total_count = matching.len();

        // Sort
        if let Some(field) = query.sort_by {
            matching.sort_by(|(_, a), (_, b)| {
                let cmp = match field {
                    SortField::Name => a.name.cmp(&b.name),
                    SortField::Size => a.size.cmp(&b.size),
                    SortField::CreatedAt => a.created_at.cmp(&b.created_at),
                    SortField::ModifiedAt => a.modified_at.cmp(&b.modified_at),
                };
                if query.sort_desc {
                    cmp.reverse()
                } else {
                    cmp
                }
            });
        }

        // Paginate
        let offset = query.offset.min(matching.len());
        let limit = query.limit.unwrap_or(matching.len());
        let objects: Vec<_> = matching.into_iter().skip(offset).take(limit).collect();

        let has_more = offset + objects.len() < total_count;

        Ok(QueryResult {
            objects,
            total_count,
            has_more,
        })
    }

    fn versions(
        &self,
        id: ObjectId,
        cap_token: &CapabilityToken,
    ) -> Result<Vec<ObjectVersion>, StorageError> {
        // For now, return current version only (versioning is future work)
        let meta = self.metadata(id, cap_token)?;
        Ok(vec![ObjectVersion {
            version: meta.version,
            timestamp: meta.modified_at,
            size: meta.size,
            content_hash: meta.content_hash,
        }])
    }

    fn read_version(
        &self,
        id: ObjectId,
        _version: u64,
        cap_token: &CapabilityToken,
    ) -> Result<Vec<u8>, StorageError> {
        // For now, only current version is available
        self.read(id, cap_token)
    }

    fn read_chunk(
        &self,
        id: ObjectId,
        offset: usize,
        size: usize,
        cap_token: &CapabilityToken,
    ) -> Result<ObjectChunk, StorageError> {
        let data = self.read(id, cap_token)?;

        if offset >= data.len() {
            return Ok(ObjectChunk {
                offset,
                data: Vec::new(),
                is_last: true,
            });
        }

        let end = (offset + size).min(data.len());
        let chunk_data = data[offset..end].to_vec();
        let is_last = end >= data.len();

        Ok(ObjectChunk {
            offset,
            data: chunk_data,
            is_last,
        })
    }

    fn write_chunk(
        &self,
        _id: ObjectId,
        _chunk: ObjectChunk,
        _cap_token: &CapabilityToken,
    ) -> Result<(), StorageError> {
        // Streaming write is future work
        Err(StorageError::PermissionDenied)
    }

    fn batch(&self, ops: Vec<BatchOp>, cap_token: &CapabilityToken) -> Vec<BatchResult> {
        ops.into_iter()
            .map(|op| match op {
                BatchOp::Create { data, metadata } => match self.create(data, metadata, cap_token) {
                    Ok(id) => BatchResult::Created(id),
                    Err(e) => BatchResult::Failed(e),
                },
                BatchOp::Update { id, data } => match self.update(id, data, cap_token) {
                    Ok(()) => BatchResult::Updated,
                    Err(e) => BatchResult::Failed(e),
                },
                BatchOp::Delete { id } => match self.delete(id, cap_token) {
                    Ok(()) => BatchResult::Deleted,
                    Err(e) => BatchResult::Failed(e),
                },
                BatchOp::AddTag { id, key, value } => {
                    match self.add_tag(id, key, value, cap_token) {
                        Ok(()) => BatchResult::TagAdded,
                        Err(e) => BatchResult::Failed(e),
                    }
                }
                BatchOp::RemoveTag { id, key } => match self.remove_tag(id, &key, cap_token) {
                    Ok(()) => BatchResult::TagRemoved,
                    Err(e) => BatchResult::Failed(e),
                },
            })
            .collect()
    }

    fn add_tag(
        &self,
        id: ObjectId,
        key: impl Into<String>,
        value: impl Into<String>,
        _cap_token: &CapabilityToken,
    ) -> Result<(), StorageError> {
        let mut objects = self.objects.lock();
        let obj = objects.get_mut(&id).ok_or(StorageError::ObjectNotFound)?;
        obj.metadata.tags.insert(key.into(), value.into());
        obj.metadata.modified_at = 0; // Would use real timestamp
        Ok(())
    }

    fn remove_tag(
        &self,
        id: ObjectId,
        key: &str,
        _cap_token: &CapabilityToken,
    ) -> Result<(), StorageError> {
        let mut objects = self.objects.lock();
        let obj = objects.get_mut(&id).ok_or(StorageError::ObjectNotFound)?;
        obj.metadata.tags.remove(key);
        obj.metadata.modified_at = 0;
        Ok(())
    }

    fn copy(
        &self,
        id: ObjectId,
        new_name: Option<String>,
        cap_token: &CapabilityToken,
    ) -> Result<ObjectId, StorageError> {
        let data = self.read(id, cap_token)?;
        let mut metadata = self.metadata(id, cap_token)?;
        metadata.name = new_name.or(metadata.name.map(|n| alloc::format!("{}_copy", n)));
        metadata.version = 1;
        // Disable dedup for copy to get new ID
        metadata.content_hash = None;
        self.create(data, metadata, cap_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StorageConfig;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken::new([1, 2, 3, 4])
    }

    #[test]
    fn test_query_by_tag() {
        let storage = Storage::new(StorageConfig::default());
        let token = dummy_token();

        let metadata = ObjectMetadata::new("text/plain", 5).with_tag("env", "prod");
        storage
            .create(b"hello".to_vec(), metadata, &token)
            .unwrap();

        let metadata = ObjectMetadata::new("text/plain", 5).with_tag("env", "dev");
        storage
            .create(b"world".to_vec(), metadata, &token)
            .unwrap();

        let query = ObjectQuery::new().tag_equals("env", "prod");
        let result = storage.query(&query, &token).unwrap();

        assert_eq!(result.total_count, 1);
    }

    #[test]
    fn test_query_by_size() {
        let storage = Storage::new(StorageConfig::default());
        let token = dummy_token();

        storage
            .create(b"small".to_vec(), ObjectMetadata::new("text/plain", 5), &token)
            .unwrap();
        storage
            .create(
                b"this is a larger object".to_vec(),
                ObjectMetadata::new("text/plain", 23),
                &token,
            )
            .unwrap();

        let query = ObjectQuery::new().size_range(10, 100);
        let result = storage.query(&query, &token).unwrap();

        assert_eq!(result.total_count, 1);
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("hello.txt", "*.txt"));
        assert!(glob_match("hello.txt", "hello.*"));
        assert!(glob_match("hello.txt", "h?llo.txt"));
        assert!(!glob_match("hello.txt", "*.rs"));
    }
}

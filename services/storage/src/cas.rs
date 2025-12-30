//! # Content-Addressed Storage (CAS)
//!
//! Content-addressed storage where objects are identified by their content hash.
//! This provides:
//! - Automatic deduplication
//! - Data integrity verification
//! - Immutable content references
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                  Application                        │
//! │                                                     │
//! │   store(data) -> ContentAddress                     │
//! │   retrieve(ContentAddress) -> data                  │
//! └────────────────────┬────────────────────────────────┘
//!                      │
//! ┌────────────────────▼────────────────────────────────┐
//! │              Content-Addressed Store                │
//! │                                                     │
//! │   ┌─────────────┐    ┌─────────────────────────┐   │
//! │   │ Hash Index  │    │    Blob Storage         │   │
//! │   │ SHA-256 ->  │────│  [chunk1][chunk2]...    │   │
//! │   │  location   │    └─────────────────────────┘   │
//! │   └─────────────┘                                   │
//! └─────────────────────────────────────────────────────┘
//! ```

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use super::ContentHash;
use splax_cap::CapabilityToken;

/// Content address - the identity of content in CAS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContentAddress(pub [u8; 32]);

impl ContentAddress {
    /// Computes the content address from data.
    pub fn from_data(data: &[u8]) -> Self {
        Self(splax_cap::compute_sha256(data))
    }

    /// Creates from a raw hash.
    pub fn from_hash(hash: [u8; 32]) -> Self {
        Self(hash)
    }

    /// Returns the hash bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Converts to a hex string.
    pub fn to_hex(&self) -> [u8; 64] {
        let mut hex = [0u8; 64];
        const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
        for (i, &byte) in self.0.iter().enumerate() {
            hex[i * 2] = HEX_CHARS[(byte >> 4) as usize];
            hex[i * 2 + 1] = HEX_CHARS[(byte & 0x0f) as usize];
        }
        hex
    }
}

impl From<ContentHash> for ContentAddress {
    fn from(hash: ContentHash) -> Self {
        // ContentHash is also [u8; 32], just a different wrapper
        Self(hash.as_bytes())
    }
}

impl ContentHash {
    /// Returns the raw bytes of the hash.
    pub fn as_bytes(&self) -> [u8; 32] {
        self.0
    }
}

/// CAS configuration.
#[derive(Debug, Clone)]
pub struct CasConfig {
    /// Maximum blob size (large blobs are chunked).
    pub max_blob_size: usize,
    /// Chunk size for large blobs.
    pub chunk_size: usize,
    /// Enable reference counting for garbage collection.
    pub enable_refcount: bool,
}

impl Default for CasConfig {
    fn default() -> Self {
        Self {
            max_blob_size: 64 * 1024 * 1024, // 64 MB
            chunk_size: 1024 * 1024,          // 1 MB chunks
            enable_refcount: true,
        }
    }
}

/// Stored blob with metadata.
struct StoredBlob {
    data: Vec<u8>,
    refcount: u32,
    stored_at: u64,
}

/// Content-Addressed Store.
pub struct ContentAddressedStore {
    config: CasConfig,
    /// Blobs indexed by content address.
    blobs: Mutex<BTreeMap<ContentAddress, StoredBlob>>,
    /// Total bytes stored.
    total_bytes: Mutex<usize>,
}

impl ContentAddressedStore {
    /// Creates a new CAS.
    pub fn new(config: CasConfig) -> Self {
        Self {
            config,
            blobs: Mutex::new(BTreeMap::new()),
            total_bytes: Mutex::new(0),
        }
    }

    /// Stores data and returns its content address.
    ///
    /// If the content already exists, increments refcount and returns existing address.
    pub fn store(&self, data: Vec<u8>, _cap_token: &CapabilityToken) -> Result<ContentAddress, CasError> {
        if data.len() > self.config.max_blob_size {
            return Err(CasError::BlobTooLarge);
        }

        let addr = ContentAddress::from_data(&data);

        let mut blobs = self.blobs.lock();

        if let Some(existing) = blobs.get_mut(&addr) {
            // Content already exists - increment refcount
            if self.config.enable_refcount {
                existing.refcount = existing.refcount.saturating_add(1);
            }
            return Ok(addr);
        }

        // Store new blob
        let size = data.len();
        blobs.insert(
            addr,
            StoredBlob {
                data,
                refcount: 1,
                stored_at: 0, // Would use real timestamp
            },
        );

        *self.total_bytes.lock() += size;

        Ok(addr)
    }

    /// Retrieves data by content address.
    pub fn retrieve(&self, addr: ContentAddress, _cap_token: &CapabilityToken) -> Result<Vec<u8>, CasError> {
        let blobs = self.blobs.lock();
        let blob = blobs.get(&addr).ok_or(CasError::NotFound)?;
        Ok(blob.data.clone())
    }

    /// Verifies that stored content matches its address.
    pub fn verify(&self, addr: ContentAddress, _cap_token: &CapabilityToken) -> Result<bool, CasError> {
        let blobs = self.blobs.lock();
        let blob = blobs.get(&addr).ok_or(CasError::NotFound)?;
        let computed = ContentAddress::from_data(&blob.data);
        Ok(computed == addr)
    }

    /// Decrements refcount. If zero, optionally removes the blob.
    pub fn release(&self, addr: ContentAddress, _cap_token: &CapabilityToken) -> Result<(), CasError> {
        if !self.config.enable_refcount {
            return Ok(());
        }

        let mut blobs = self.blobs.lock();
        let blob = blobs.get_mut(&addr).ok_or(CasError::NotFound)?;

        blob.refcount = blob.refcount.saturating_sub(1);

        if blob.refcount == 0 {
            let size = blob.data.len();
            blobs.remove(&addr);
            *self.total_bytes.lock() = self.total_bytes.lock().saturating_sub(size);
        }

        Ok(())
    }

    /// Checks if content exists.
    pub fn exists(&self, addr: ContentAddress) -> bool {
        self.blobs.lock().contains_key(&addr)
    }

    /// Gets the reference count of a blob.
    pub fn refcount(&self, addr: ContentAddress) -> Option<u32> {
        self.blobs.lock().get(&addr).map(|b| b.refcount)
    }

    /// Returns storage statistics.
    pub fn stats(&self) -> CasStats {
        let blobs = self.blobs.lock();
        CasStats {
            blob_count: blobs.len(),
            total_bytes: *self.total_bytes.lock(),
        }
    }

    /// Stores chunked data (for large blobs).
    pub fn store_chunked(
        &self,
        data: &[u8],
        cap_token: &CapabilityToken,
    ) -> Result<ChunkedContent, CasError> {
        let mut chunks = Vec::new();

        for chunk in data.chunks(self.config.chunk_size) {
            let addr = self.store(chunk.to_vec(), cap_token)?;
            chunks.push(ChunkRef {
                addr,
                size: chunk.len(),
            });
        }

        let manifest = ChunkedContent {
            total_size: data.len(),
            chunks,
        };

        Ok(manifest)
    }

    /// Retrieves chunked data.
    pub fn retrieve_chunked(
        &self,
        manifest: &ChunkedContent,
        cap_token: &CapabilityToken,
    ) -> Result<Vec<u8>, CasError> {
        let mut data = Vec::with_capacity(manifest.total_size);

        for chunk_ref in &manifest.chunks {
            let chunk_data = self.retrieve(chunk_ref.addr, cap_token)?;
            data.extend_from_slice(&chunk_data);
        }

        Ok(data)
    }
}

/// Reference to a chunk.
#[derive(Debug, Clone)]
pub struct ChunkRef {
    /// Content address of the chunk.
    pub addr: ContentAddress,
    /// Size of the chunk.
    pub size: usize,
}

/// Manifest for chunked content.
#[derive(Debug, Clone)]
pub struct ChunkedContent {
    /// Total size of the original content.
    pub total_size: usize,
    /// List of chunk references.
    pub chunks: Vec<ChunkRef>,
}

impl ChunkedContent {
    /// Computes the content address of the manifest itself.
    pub fn manifest_addr(&self) -> ContentAddress {
        // Serialize chunk addresses to compute manifest hash
        let mut data = Vec::new();
        data.extend_from_slice(&(self.total_size as u64).to_le_bytes());
        for chunk in &self.chunks {
            data.extend_from_slice(chunk.addr.as_bytes());
            data.extend_from_slice(&(chunk.size as u64).to_le_bytes());
        }
        ContentAddress::from_data(&data)
    }
}

/// CAS statistics.
#[derive(Debug, Clone)]
pub struct CasStats {
    pub blob_count: usize,
    pub total_bytes: usize,
}

/// CAS errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasError {
    /// Content not found.
    NotFound,
    /// Blob exceeds size limit.
    BlobTooLarge,
    /// Content verification failed.
    VerificationFailed,
    /// Storage is full.
    StorageFull,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken::new([1, 2, 3, 4])
    }

    #[test]
    fn test_store_and_retrieve() {
        let cas = ContentAddressedStore::new(CasConfig::default());
        let token = dummy_token();

        let data = b"Hello, CAS!".to_vec();
        let addr = cas.store(data.clone(), &token).unwrap();

        let retrieved = cas.retrieve(addr, &token).unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn test_deduplication() {
        let cas = ContentAddressedStore::new(CasConfig::default());
        let token = dummy_token();

        let data = b"duplicate content".to_vec();
        let addr1 = cas.store(data.clone(), &token).unwrap();
        let addr2 = cas.store(data.clone(), &token).unwrap();

        // Same content, same address
        assert_eq!(addr1, addr2);

        // Refcount should be 2
        assert_eq!(cas.refcount(addr1), Some(2));

        // Only one blob stored
        assert_eq!(cas.stats().blob_count, 1);
    }

    #[test]
    fn test_verify() {
        let cas = ContentAddressedStore::new(CasConfig::default());
        let token = dummy_token();

        let data = b"verify me".to_vec();
        let addr = cas.store(data, &token).unwrap();

        assert!(cas.verify(addr, &token).unwrap());
    }

    #[test]
    fn test_refcount_release() {
        let cas = ContentAddressedStore::new(CasConfig::default());
        let token = dummy_token();

        let data = b"refcount test".to_vec();
        let addr = cas.store(data.clone(), &token).unwrap();
        let _ = cas.store(data, &token).unwrap(); // Refcount = 2

        cas.release(addr, &token).unwrap(); // Refcount = 1
        assert_eq!(cas.refcount(addr), Some(1));

        cas.release(addr, &token).unwrap(); // Refcount = 0, removed
        assert!(!cas.exists(addr));
    }

    #[test]
    fn test_chunked_storage() {
        let config = CasConfig {
            chunk_size: 10, // Small chunks for testing
            ..Default::default()
        };
        let cas = ContentAddressedStore::new(config);
        let token = dummy_token();

        let data = b"This is a longer message that will be chunked into multiple pieces.";
        let manifest = cas.store_chunked(data, &token).unwrap();

        assert!(manifest.chunks.len() > 1);

        let retrieved = cas.retrieve_chunked(&manifest, &token).unwrap();
        assert_eq!(retrieved, data);
    }
}

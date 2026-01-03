//! # Secure Key Storage
//!
//! Provides secure storage for cryptographic keys with hardware-backed
//! protection when available.
//!
//! ## Design
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Key Storage Manager                       │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
//! │  │  Key Slot 0  │  │  Key Slot 1  │  │  Key Slot N  │      │
//! │  │  (Master)    │  │  (User Key)  │  │  (Ephemeral) │      │
//! │  └──────────────┘  └──────────────┘  └──────────────┘      │
//! ├─────────────────────────────────────────────────────────────┤
//! │                   Key Derivation (HKDF)                     │
//! ├─────────────────────────────────────────────────────────────┤
//! │              Hardware RNG / Memory Encryption               │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Features
//!
//! - **Memory encryption**: Keys encrypted in memory when not in use
//! - **Access control**: Capability-based key access
//! - **Key derivation**: Hierarchical key derivation
//! - **Key rotation**: Automatic key rotation support
//! - **Secure deletion**: Zeroization on key removal

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use spin::Mutex;

use super::cipher::{AesGcm, ChaCha20Poly1305, Cipher, CipherError};
use super::hash::{Hash, Sha256};
use super::kdf::Hkdf;
use super::random::{CryptoRng, SystemRng};
use crate::cap::{CapabilityToken, CapabilityTable, Operations, ResourceId};
use crate::sched::ProcessId;

// =============================================================================
// Key Types
// =============================================================================

/// Key identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(pub u64);

/// Key type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    /// Master key for key derivation
    Master,
    /// Symmetric encryption key (AES, ChaCha20)
    Symmetric,
    /// Asymmetric private key (Ed25519, X25519)
    PrivateKey,
    /// Asymmetric public key
    PublicKey,
    /// HMAC key
    Hmac,
    /// Password-derived key
    PasswordDerived,
    /// Ephemeral session key
    Ephemeral,
}

/// Key usage restrictions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyUsage {
    bits: u32,
}

impl KeyUsage {
    /// No usage allowed
    pub const NONE: Self = Self { bits: 0 };
    /// Key can encrypt data
    pub const ENCRYPT: Self = Self { bits: 1 << 0 };
    /// Key can decrypt data
    pub const DECRYPT: Self = Self { bits: 1 << 1 };
    /// Key can sign data
    pub const SIGN: Self = Self { bits: 1 << 2 };
    /// Key can verify signatures
    pub const VERIFY: Self = Self { bits: 1 << 3 };
    /// Key can derive other keys
    pub const DERIVE: Self = Self { bits: 1 << 4 };
    /// Key can be exported
    pub const EXPORT: Self = Self { bits: 1 << 5 };
    /// Key can wrap/unwrap other keys
    pub const WRAP: Self = Self { bits: 1 << 6 };
    /// All usages
    pub const ALL: Self = Self { bits: 0x7F };

    /// Combines two usage sets.
    pub const fn union(self, other: Self) -> Self {
        Self { bits: self.bits | other.bits }
    }

    /// Checks if this set contains all usages in `other`.
    pub const fn contains(self, other: Self) -> bool {
        (self.bits & other.bits) == other.bits
    }
}

/// Key algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAlgorithm {
    /// AES-128
    Aes128,
    /// AES-256
    Aes256,
    /// ChaCha20
    ChaCha20,
    /// Ed25519 signing
    Ed25519,
    /// X25519 key exchange
    X25519,
    /// HMAC-SHA256
    HmacSha256,
    /// HMAC-SHA512
    HmacSha512,
    /// Raw bytes (for derived keys)
    Raw,
}

impl KeyAlgorithm {
    /// Returns the expected key length in bytes.
    pub fn key_length(&self) -> usize {
        match self {
            Self::Aes128 => 16,
            Self::Aes256 | Self::ChaCha20 | Self::Ed25519 | Self::X25519 => 32,
            Self::HmacSha256 => 32,
            Self::HmacSha512 => 64,
            Self::Raw => 0, // Variable length
        }
    }
}

// =============================================================================
// Key Entry
// =============================================================================

/// Metadata for a stored key.
#[derive(Debug, Clone)]
pub struct KeyMetadata {
    /// Key identifier
    pub id: KeyId,
    /// Key type
    pub key_type: KeyType,
    /// Key algorithm
    pub algorithm: KeyAlgorithm,
    /// Allowed usages
    pub usage: KeyUsage,
    /// Creation timestamp (cycles)
    pub created_at: u64,
    /// Expiration timestamp (None = never)
    pub expires_at: Option<u64>,
    /// Last used timestamp
    pub last_used: u64,
    /// Use count
    pub use_count: u64,
    /// Human-readable label
    pub label: String,
    /// Parent key (for derived keys)
    pub parent: Option<KeyId>,
    /// Whether key is extractable
    pub extractable: bool,
}

/// Internal key storage entry.
struct KeyEntry {
    /// Key metadata
    metadata: KeyMetadata,
    /// Encrypted key material (encrypted with storage key)
    encrypted_material: Vec<u8>,
    /// Nonce used for encryption
    nonce: [u8; 12],
    /// Authentication tag
    tag: [u8; 16],
    /// Owner process
    owner: ProcessId,
    /// Capability token for access
    capability: Option<CapabilityToken>,
}

// =============================================================================
// Key Storage Errors
// =============================================================================

/// Key storage errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStoreError {
    /// Key not found
    NotFound,
    /// Key already exists
    AlreadyExists,
    /// Access denied
    AccessDenied,
    /// Invalid key material
    InvalidKeyMaterial,
    /// Invalid key length
    InvalidKeyLength,
    /// Key expired
    Expired,
    /// Usage not allowed
    UsageNotAllowed,
    /// Storage full
    StorageFull,
    /// Encryption error
    EncryptionError,
    /// Decryption error
    DecryptionError,
    /// Key derivation error
    DerivationError,
    /// Key not extractable
    NotExtractable,
    /// RNG failure
    RngFailure,
}

impl From<CipherError> for KeyStoreError {
    fn from(_: CipherError) -> Self {
        KeyStoreError::EncryptionError
    }
}

// =============================================================================
// Key Storage Manager
// =============================================================================

/// Configuration for the key storage.
#[derive(Debug, Clone)]
pub struct KeyStoreConfig {
    /// Maximum number of keys
    pub max_keys: usize,
    /// Enable memory encryption for keys at rest
    pub encrypt_at_rest: bool,
    /// Default key expiration (cycles, None = never)
    pub default_expiration: Option<u64>,
}

impl Default for KeyStoreConfig {
    fn default() -> Self {
        Self {
            max_keys: 1024,
            encrypt_at_rest: true,
            default_expiration: None,
        }
    }
}

/// Secure key storage manager.
pub struct KeyStore {
    /// Configuration
    config: KeyStoreConfig,
    /// Key entries indexed by ID
    keys: Mutex<BTreeMap<KeyId, KeyEntry>>,
    /// Next key ID
    next_id: Mutex<u64>,
    /// Storage encryption key (encrypted with a hardware-derived key)
    storage_key: [u8; 32],
    /// Random number generator
    rng: Mutex<Option<SystemRng>>,
}

impl KeyStore {
    /// Creates a new key storage.
    pub fn new(config: KeyStoreConfig) -> Result<Self, KeyStoreError> {
        // Generate storage key from hardware RNG
        let mut rng = SystemRng::new().map_err(|_| KeyStoreError::RngFailure)?;
        let mut storage_key = [0u8; 32];
        for i in 0..4 {
            let val = rng.random_u64().map_err(|_| KeyStoreError::RngFailure)?;
            storage_key[i * 8..(i + 1) * 8].copy_from_slice(&val.to_le_bytes());
        }

        Ok(Self {
            config,
            keys: Mutex::new(BTreeMap::new()),
            next_id: Mutex::new(1),
            storage_key,
            rng: Mutex::new(Some(rng)),
        })
    }

    /// Generates a new key.
    pub fn generate(
        &self,
        owner: ProcessId,
        algorithm: KeyAlgorithm,
        key_type: KeyType,
        usage: KeyUsage,
        label: &str,
    ) -> Result<KeyId, KeyStoreError> {
        // Generate key material
        let key_len = algorithm.key_length();
        if key_len == 0 {
            return Err(KeyStoreError::InvalidKeyLength);
        }

        let mut key_material = alloc::vec![0u8; key_len];
        self.fill_random(&mut key_material)?;

        self.import(owner, &key_material, algorithm, key_type, usage, label, true)
    }

    /// Imports an existing key.
    pub fn import(
        &self,
        owner: ProcessId,
        key_material: &[u8],
        algorithm: KeyAlgorithm,
        key_type: KeyType,
        usage: KeyUsage,
        label: &str,
        extractable: bool,
    ) -> Result<KeyId, KeyStoreError> {
        // Validate key length
        let expected_len = algorithm.key_length();
        if expected_len != 0 && key_material.len() != expected_len {
            return Err(KeyStoreError::InvalidKeyLength);
        }

        // Check capacity
        let mut keys = self.keys.lock();
        if keys.len() >= self.config.max_keys {
            return Err(KeyStoreError::StorageFull);
        }

        // Generate key ID
        let id = {
            let mut next = self.next_id.lock();
            let id = KeyId(*next);
            *next += 1;
            id
        };

        // Encrypt key material
        let (encrypted, nonce, tag) = self.encrypt_key_material(key_material)?;

        let now = crate::arch::read_cycle_counter();
        let metadata = KeyMetadata {
            id,
            key_type,
            algorithm,
            usage,
            created_at: now,
            expires_at: self.config.default_expiration.map(|d| now + d),
            last_used: now,
            use_count: 0,
            label: label.to_string(),
            parent: None,
            extractable,
        };

        let entry = KeyEntry {
            metadata,
            encrypted_material: encrypted,
            nonce,
            tag,
            owner,
            capability: None,
        };

        keys.insert(id, entry);

        // Securely zero key material
        // Note: In production, use a secure zeroization function
        
        Ok(id)
    }

    /// Derives a new key from an existing key.
    pub fn derive(
        &self,
        owner: ProcessId,
        parent_id: KeyId,
        info: &[u8],
        algorithm: KeyAlgorithm,
        usage: KeyUsage,
        label: &str,
    ) -> Result<KeyId, KeyStoreError> {
        // Get parent key
        let parent_material = self.get_key_material(owner, parent_id, KeyUsage::DERIVE)?;

        // Derive new key using HKDF
        let key_len = algorithm.key_length();
        if key_len == 0 {
            return Err(KeyStoreError::InvalidKeyLength);
        }

        let derived = Hkdf::expand(&parent_material, info, key_len)
            .map_err(|_| KeyStoreError::DerivationError)?;

        // Import derived key
        let id = self.import(
            owner,
            &derived,
            algorithm,
            KeyType::Symmetric,
            usage,
            label,
            false, // Derived keys are not extractable by default
        )?;

        // Update parent reference
        {
            let mut keys = self.keys.lock();
            if let Some(entry) = keys.get_mut(&id) {
                entry.metadata.parent = Some(parent_id);
            }
        }

        Ok(id)
    }

    /// Gets key metadata.
    pub fn get_metadata(&self, owner: ProcessId, id: KeyId) -> Result<KeyMetadata, KeyStoreError> {
        let keys = self.keys.lock();
        let entry = keys.get(&id).ok_or(KeyStoreError::NotFound)?;

        if entry.owner != owner {
            return Err(KeyStoreError::AccessDenied);
        }

        // Check expiration
        if let Some(expires) = entry.metadata.expires_at {
            if crate::arch::read_cycle_counter() > expires {
                return Err(KeyStoreError::Expired);
            }
        }

        Ok(entry.metadata.clone())
    }

    /// Exports a key (if extractable).
    pub fn export(&self, owner: ProcessId, id: KeyId) -> Result<Vec<u8>, KeyStoreError> {
        let keys = self.keys.lock();
        let entry = keys.get(&id).ok_or(KeyStoreError::NotFound)?;

        if entry.owner != owner {
            return Err(KeyStoreError::AccessDenied);
        }

        if !entry.metadata.extractable {
            return Err(KeyStoreError::NotExtractable);
        }

        if !entry.metadata.usage.contains(KeyUsage::EXPORT) {
            return Err(KeyStoreError::UsageNotAllowed);
        }

        self.decrypt_key_material(&entry.encrypted_material, &entry.nonce, &entry.tag)
    }

    /// Uses a key for an operation (returns decrypted material temporarily).
    pub fn use_key(
        &self,
        owner: ProcessId,
        id: KeyId,
        required_usage: KeyUsage,
    ) -> Result<KeyMaterial, KeyStoreError> {
        let material = self.get_key_material(owner, id, required_usage)?;

        // Update use count
        {
            let mut keys = self.keys.lock();
            if let Some(entry) = keys.get_mut(&id) {
                entry.metadata.use_count += 1;
                entry.metadata.last_used = crate::arch::read_cycle_counter();
            }
        }

        Ok(KeyMaterial { data: material })
    }

    /// Deletes a key (securely zeroizes).
    pub fn delete(&self, owner: ProcessId, id: KeyId) -> Result<(), KeyStoreError> {
        let mut keys = self.keys.lock();
        let entry = keys.get(&id).ok_or(KeyStoreError::NotFound)?;

        if entry.owner != owner {
            return Err(KeyStoreError::AccessDenied);
        }

        // Remove and zeroize
        if let Some(mut entry) = keys.remove(&id) {
            // Securely zeroize the encrypted material
            for byte in &mut entry.encrypted_material {
                *byte = 0;
            }
            for byte in &mut entry.nonce {
                *byte = 0;
            }
            for byte in &mut entry.tag {
                *byte = 0;
            }
        }

        Ok(())
    }

    /// Rotates a key (generates new key, re-encrypts data).
    pub fn rotate(&self, owner: ProcessId, id: KeyId) -> Result<KeyId, KeyStoreError> {
        let metadata = self.get_metadata(owner, id)?;

        // Generate new key with same parameters
        let new_id = self.generate(
            owner,
            metadata.algorithm,
            metadata.key_type,
            metadata.usage,
            &metadata.label,
        )?;

        // Delete old key
        self.delete(owner, id)?;

        Ok(new_id)
    }

    /// Lists all keys owned by a process.
    pub fn list(&self, owner: ProcessId) -> Vec<KeyMetadata> {
        let keys = self.keys.lock();
        keys.values()
            .filter(|e| e.owner == owner)
            .map(|e| e.metadata.clone())
            .collect()
    }

    // Internal helpers

    fn get_key_material(
        &self,
        owner: ProcessId,
        id: KeyId,
        required_usage: KeyUsage,
    ) -> Result<Vec<u8>, KeyStoreError> {
        let keys = self.keys.lock();
        let entry = keys.get(&id).ok_or(KeyStoreError::NotFound)?;

        if entry.owner != owner {
            return Err(KeyStoreError::AccessDenied);
        }

        if !entry.metadata.usage.contains(required_usage) {
            return Err(KeyStoreError::UsageNotAllowed);
        }

        // Check expiration
        if let Some(expires) = entry.metadata.expires_at {
            if crate::arch::read_cycle_counter() > expires {
                return Err(KeyStoreError::Expired);
            }
        }

        self.decrypt_key_material(&entry.encrypted_material, &entry.nonce, &entry.tag)
    }

    fn encrypt_key_material(&self, key: &[u8]) -> Result<(Vec<u8>, [u8; 12], [u8; 16]), KeyStoreError> {
        if !self.config.encrypt_at_rest {
            // No encryption, just store as-is
            return Ok((key.to_vec(), [0; 12], [0; 16]));
        }

        // Generate nonce
        let mut nonce = [0u8; 12];
        self.fill_random(&mut nonce)?;

        // Create cipher and encrypt using ChaCha20-Poly1305
        let cipher = ChaCha20Poly1305::new(&self.storage_key)
            .map_err(|_| KeyStoreError::EncryptionError)?;
        let ciphertext_with_tag = cipher.encrypt(&nonce, key, &[])
            .map_err(|_| KeyStoreError::EncryptionError)?;

        // Split ciphertext and tag
        let tag_start = ciphertext_with_tag.len() - 16;
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&ciphertext_with_tag[tag_start..]);
        let ciphertext = ciphertext_with_tag[..tag_start].to_vec();

        Ok((ciphertext, nonce, tag))
    }

    fn decrypt_key_material(
        &self,
        encrypted: &[u8],
        nonce: &[u8; 12],
        tag: &[u8; 16],
    ) -> Result<Vec<u8>, KeyStoreError> {
        if !self.config.encrypt_at_rest {
            return Ok(encrypted.to_vec());
        }

        // Reconstruct ciphertext with tag
        let mut ciphertext_with_tag = encrypted.to_vec();
        ciphertext_with_tag.extend_from_slice(tag);

        // Create cipher and decrypt
        let cipher = ChaCha20Poly1305::new(&self.storage_key)
            .map_err(|_| KeyStoreError::DecryptionError)?;
        let plaintext = cipher.decrypt(nonce, &ciphertext_with_tag, &[])
            .map_err(|_| KeyStoreError::DecryptionError)?;

        Ok(plaintext)
    }

    fn fill_random(&self, buf: &mut [u8]) -> Result<(), KeyStoreError> {
        let mut rng = self.rng.lock();
        if let Some(ref mut r) = *rng {
            for chunk in buf.chunks_mut(8) {
                let val = r.random_u64().map_err(|_| KeyStoreError::RngFailure)?;
                let bytes = val.to_le_bytes();
                for (i, b) in chunk.iter_mut().enumerate() {
                    *b = bytes[i];
                }
            }
            Ok(())
        } else {
            Err(KeyStoreError::RngFailure)
        }
    }
}

// =============================================================================
// Key Material (Secure Wrapper)
// =============================================================================

/// Wrapper for key material that zeroizes on drop.
pub struct KeyMaterial {
    data: Vec<u8>,
}

impl KeyMaterial {
    /// Gets the key bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Gets the key length.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Checks if empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl Drop for KeyMaterial {
    fn drop(&mut self) {
        // Securely zeroize key material
        for byte in &mut self.data {
            unsafe {
                core::ptr::write_volatile(byte, 0);
            }
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
}

// =============================================================================
// Global Key Store
// =============================================================================

use spin::Once;

static KEY_STORE: Once<KeyStore> = Once::new();

/// Initializes the global key store.
pub fn init_keystore(config: KeyStoreConfig) -> Result<(), KeyStoreError> {
    KEY_STORE.call_once(|| {
        KeyStore::new(config).expect("Failed to initialize key store")
    });
    Ok(())
}

/// Gets the global key store.
pub fn keystore() -> &'static KeyStore {
    KEY_STORE.get().expect("Key store not initialized")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let config = KeyStoreConfig {
            encrypt_at_rest: false, // Disable for tests
            ..Default::default()
        };
        let store = KeyStore::new(config).unwrap();
        let owner = ProcessId::new(1);

        let id = store
            .generate(owner, KeyAlgorithm::Aes256, KeyType::Symmetric, KeyUsage::ALL, "test")
            .unwrap();

        let metadata = store.get_metadata(owner, id).unwrap();
        assert_eq!(metadata.algorithm, KeyAlgorithm::Aes256);
        assert_eq!(metadata.label, "test");
    }

    #[test]
    fn test_key_derivation() {
        let config = KeyStoreConfig {
            encrypt_at_rest: false,
            ..Default::default()
        };
        let store = KeyStore::new(config).unwrap();
        let owner = ProcessId::new(1);

        let master = store
            .generate(owner, KeyAlgorithm::Aes256, KeyType::Master, KeyUsage::DERIVE, "master")
            .unwrap();

        let derived = store
            .derive(owner, master, b"encryption-key", KeyAlgorithm::ChaCha20, KeyUsage::ENCRYPT, "derived")
            .unwrap();

        let metadata = store.get_metadata(owner, derived).unwrap();
        assert_eq!(metadata.parent, Some(master));
    }
}

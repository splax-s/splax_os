//! # S-CAP: Capability Token Library
//!
//! This crate provides a shared CapabilityToken implementation for all Splax OS
//! userspace services. Capability tokens are cryptographic proofs of access rights
//! that enable the capability-based security model.
//!
//! ## Security Properties
//!
//! - **Unforgeability**: Tokens are derived using SHA-256 based HMAC
//! - **Validation**: Tokens include integrity checks via cryptographic hashing
//! - **Revocability**: Tokens can be invalidated by the capability manager
//! - **Attenuation**: Derived tokens can only have equal or fewer rights
//!
//! ## Usage
//!
//! ```ignore
//! use splax_cap::{CapabilityToken, Operations};
//!
//! // Generate a new token
//! let token = CapabilityToken::generate(
//!     resource_id,
//!     Operations::READ.union(Operations::WRITE),
//!     &secret_key,
//! );
//!
//! // Validate a token
//! if token.validate(&secret_key) {
//!     // Token is valid, proceed with operation
//! }
//! ```

#![no_std]

extern crate alloc;

/// A cryptographic capability token.
///
/// Tokens are 256-bit values that prove access rights. They are generated
/// using a SHA-256 based construction combining:
/// - A unique token ID
/// - The resource identifier
/// - Permitted operations
/// - A secret key known only to the capability manager
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityToken {
    /// The cryptographic token value (256 bits)
    value: [u64; 4],
}

impl CapabilityToken {
    /// Creates a new capability token with the specified raw value.
    ///
    /// This is the primary constructor for capability tokens.
    pub const fn new(value: [u64; 4]) -> Self {
        Self { value }
    }

    /// Alias for `new` - creates a capability token from raw values.
    pub const fn from_raw(value: [u64; 4]) -> Self {
        Self::new(value)
    }

    /// Creates a null/empty capability token (no permissions).
    pub const fn null() -> Self {
        Self { value: [0; 4] }
    }

    /// Generates a new capability token using cryptographic derivation.
    ///
    /// The token is derived as: SHA256(token_id || resource_id || operations || secret)
    ///
    /// # Arguments
    /// * `token_id` - Unique identifier for this token
    /// * `resource_id` - The resource this token grants access to
    /// * `operations` - Permitted operations (read, write, execute, grant)
    /// * `secret` - Secret key for HMAC-like construction
    pub fn generate(token_id: u64, resource_id: u64, operations: Operations, secret: &[u8]) -> Self {
        // Build input for hash: token_id || resource_id || operations || secret
        let mut input = [0u8; 256];
        let mut offset = 0;

        // Add token_id
        input[offset..offset + 8].copy_from_slice(&token_id.to_le_bytes());
        offset += 8;

        // Add resource_id
        input[offset..offset + 8].copy_from_slice(&resource_id.to_le_bytes());
        offset += 8;

        // Add operations
        input[offset..offset + 4].copy_from_slice(&operations.bits().to_le_bytes());
        offset += 4;

        // Add secret (truncate or pad to fit)
        let secret_len = secret.len().min(256 - offset);
        input[offset..offset + secret_len].copy_from_slice(&secret[..secret_len]);
        offset += secret_len;

        // Compute SHA-256 hash
        let hash = sha256(&input[..offset]);

        // Convert to [u64; 4]
        let mut value = [0u64; 4];
        for i in 0..4 {
            let bytes: [u8; 8] = hash[i * 8..(i + 1) * 8].try_into().unwrap_or([0u8; 8]);
            value[i] = u64::from_le_bytes(bytes);
        }

        Self { value }
    }

    /// Validates the token against the expected parameters.
    ///
    /// This recomputes the token and checks if it matches.
    pub fn validate(
        &self,
        token_id: u64,
        resource_id: u64,
        operations: Operations,
        secret: &[u8],
    ) -> bool {
        let expected = Self::generate(token_id, resource_id, operations, secret);
        self.constant_time_eq(&expected)
    }

    /// Validates that this token is not null/empty.
    pub fn is_valid(&self) -> bool {
        !self.is_null()
    }

    /// Checks if this is a null token (all zeros).
    pub fn is_null(&self) -> bool {
        self.value == [0; 4]
    }

    /// Returns the raw token value.
    pub fn raw(&self) -> &[u64; 4] {
        &self.value
    }

    /// Returns the token as bytes (32 bytes / 256 bits).
    pub fn as_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        for (i, &v) in self.value.iter().enumerate() {
            bytes[i * 8..(i + 1) * 8].copy_from_slice(&v.to_le_bytes());
        }
        bytes
    }

    /// Creates a token from bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut value = [0u64; 4];
        for i in 0..4 {
            let chunk: [u8; 8] = bytes[i * 8..(i + 1) * 8].try_into().unwrap_or([0u8; 8]);
            value[i] = u64::from_le_bytes(chunk);
        }
        Self { value }
    }

    /// Constant-time equality comparison to prevent timing attacks.
    fn constant_time_eq(&self, other: &Self) -> bool {
        let mut diff = 0u64;
        for i in 0..4 {
            diff |= self.value[i] ^ other.value[i];
        }
        diff == 0
    }

    /// Check if token has a specific permission.
    ///
    /// Note: This only checks if the token is non-null. Full permission
    /// checking requires validating against the capability manager.
    pub fn has_permission(&self, _perm: Permission) -> bool {
        !self.is_null()
    }
}

impl Default for CapabilityToken {
    fn default() -> Self {
        Self::null()
    }
}

/// Operations that can be authorized by a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operations {
    bits: u32,
}

impl Operations {
    /// No operations allowed.
    pub const NONE: Self = Self { bits: 0 };
    /// Read operation.
    pub const READ: Self = Self { bits: 1 << 0 };
    /// Write operation.
    pub const WRITE: Self = Self { bits: 1 << 1 };
    /// Execute operation.
    pub const EXECUTE: Self = Self { bits: 1 << 2 };
    /// Grant (delegate) the capability to others.
    pub const GRANT: Self = Self { bits: 1 << 3 };
    /// Revoke derived capabilities.
    pub const REVOKE: Self = Self { bits: 1 << 4 };
    /// All operations.
    pub const ALL: Self = Self { bits: 0x1F };

    /// Combines two operation sets (union).
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

    /// Returns the raw bits.
    pub const fn bits(self) -> u32 {
        self.bits
    }

    /// Creates from raw bits.
    pub const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }
}

/// Permission types for capability tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Permission to bind to network ports.
    NetworkBind,
    /// Permission to create network connections.
    NetworkConnect,
    /// Permission to access file system.
    FileAccess,
    /// Permission to spawn processes.
    ProcessSpawn,
    /// Permission to read resources.
    Read,
    /// Permission to write resources.
    Write,
    /// Permission to execute.
    Execute,
    /// Permission to grant capabilities to others.
    Grant,
}

// =============================================================================
// SHA-256 Implementation (Pure Rust, no_std compatible)
// =============================================================================

/// SHA-256 hash function.
///
/// This is a pure Rust implementation suitable for no_std environments.
fn sha256(data: &[u8]) -> [u8; 32] {
    // SHA-256 constants (first 32 bits of fractional parts of cube roots of first 64 primes)
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    // Initial hash values (first 32 bits of fractional parts of square roots of first 8 primes)
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    // Pre-processing: adding padding bits
    let ml = (data.len() as u64) * 8; // Message length in bits
    let mut padded = alloc::vec::Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80); // Append bit '1' to message

    // Pad to 448 mod 512 bits (56 mod 64 bytes)
    while (padded.len() % 64) != 56 {
        padded.push(0x00);
    }

    // Append original length as 64-bit big-endian
    padded.extend_from_slice(&ml.to_be_bytes());

    // Process each 512-bit (64-byte) chunk
    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];

        // Copy chunk into first 16 words
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }

        // Extend the first 16 words into the remaining 48 words
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        // Initialize working variables
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        // Compression function main loop
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        // Add compressed chunk to current hash value
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    // Produce the final hash value (big-endian)
    let mut result = [0u8; 32];
    for (i, &val) in h.iter().enumerate() {
        result[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
    }
    result
}

/// Compute SHA-256 hash of data (public interface).
pub fn compute_sha256(data: &[u8]) -> [u8; 32] {
    sha256(data)
}

// =============================================================================
// Random Token Generation
// =============================================================================

/// Token generator for creating new capability tokens.
///
/// Uses a combination of system state for entropy.
pub struct TokenGenerator {
    counter: u64,
    seed: [u8; 32],
}

impl TokenGenerator {
    /// Creates a new token generator with the given seed.
    pub fn new(seed: &[u8]) -> Self {
        let mut seed_arr = [0u8; 32];
        let len = seed.len().min(32);
        seed_arr[..len].copy_from_slice(&seed[..len]);

        Self {
            counter: 0,
            seed: seed_arr,
        }
    }

    /// Generates a new unique token ID.
    pub fn next_token_id(&mut self) -> u64 {
        self.counter = self.counter.wrapping_add(1);

        // Mix counter with seed using hash
        let mut input = [0u8; 40];
        input[..32].copy_from_slice(&self.seed);
        input[32..40].copy_from_slice(&self.counter.to_le_bytes());

        let hash = sha256(&input);

        // Extract 64 bits from hash
        u64::from_le_bytes(hash[..8].try_into().unwrap_or([0u8; 8]))
    }

    /// Generates a new capability token for the given resource.
    pub fn generate_token(
        &mut self,
        resource_id: u64,
        operations: Operations,
        secret: &[u8],
    ) -> CapabilityToken {
        let token_id = self.next_token_id();
        CapabilityToken::generate(token_id, resource_id, operations, secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_generation() {
        let secret = b"test_secret_key";
        let token = CapabilityToken::generate(1, 100, Operations::READ, secret);
        assert!(!token.is_null());
    }

    #[test]
    fn test_token_validation() {
        let secret = b"test_secret_key";
        let token = CapabilityToken::generate(1, 100, Operations::READ, secret);
        assert!(token.validate(1, 100, Operations::READ, secret));
        assert!(!token.validate(2, 100, Operations::READ, secret)); // Wrong token_id
        assert!(!token.validate(1, 101, Operations::READ, secret)); // Wrong resource_id
    }

    #[test]
    fn test_null_token() {
        let token = CapabilityToken::null();
        assert!(token.is_null());
        assert!(!token.is_valid());
    }

    #[test]
    fn test_operations() {
        let ops = Operations::READ.union(Operations::WRITE);
        assert!(ops.contains(Operations::READ));
        assert!(ops.contains(Operations::WRITE));
        assert!(!ops.contains(Operations::EXECUTE));
    }
}

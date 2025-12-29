//! # Message Authentication Codes
//!
//! HMAC implementations for message authentication.
//!
//! ## Implementations
//!
//! - **HMAC-SHA1**: HMAC with SHA-1 (legacy, for WPA)
//! - **HMAC-SHA256**: HMAC with SHA-256
//! - **HMAC-SHA512**: HMAC with SHA-512

use alloc::vec::Vec;
use super::hash::{Hash, Sha1, Sha256, Sha512};

/// MAC trait
pub trait Mac {
    /// Output size in bytes
    const OUTPUT_SIZE: usize;

    /// Creates a new MAC with the given key
    fn new(key: &[u8]) -> Self;

    /// Updates the MAC with data
    fn update(&mut self, data: &[u8]);

    /// Finalizes and returns the MAC tag
    fn finalize(self) -> Vec<u8>;

    /// One-shot MAC function
    fn mac(key: &[u8], data: &[u8]) -> Vec<u8>
    where
        Self: Sized,
    {
        let mut mac = Self::new(key);
        mac.update(data);
        mac.finalize()
    }

    /// Verifies a MAC tag (constant-time)
    fn verify(key: &[u8], data: &[u8], tag: &[u8]) -> bool
    where
        Self: Sized,
    {
        let computed = Self::mac(key, data);
        super::constant_time_eq(&computed, tag)
    }
}

// ============================================================================
// HMAC-SHA256
// ============================================================================

/// HMAC-SHA256
pub struct HmacSha256 {
    inner: Sha256,
    outer_key_pad: [u8; 64],
}

impl HmacSha256 {
    const BLOCK_SIZE: usize = 64;
}

impl Mac for HmacSha256 {
    const OUTPUT_SIZE: usize = 32;

    fn new(key: &[u8]) -> Self {
        // If key > block size, hash it
        let key_block = if key.len() > Self::BLOCK_SIZE {
            let hash = Sha256::hash(key);
            let mut block = [0u8; 64];
            block[..32].copy_from_slice(&hash);
            block
        } else {
            let mut block = [0u8; 64];
            block[..key.len()].copy_from_slice(key);
            block
        };

        // Inner key pad (key XOR ipad)
        let mut inner_key_pad = [0x36u8; 64];
        for (i, byte) in key_block.iter().enumerate() {
            inner_key_pad[i] ^= byte;
        }

        // Outer key pad (key XOR opad)
        let mut outer_key_pad = [0x5cu8; 64];
        for (i, byte) in key_block.iter().enumerate() {
            outer_key_pad[i] ^= byte;
        }

        // Start inner hash
        let mut inner = Sha256::new();
        inner.update(&inner_key_pad);

        Self {
            inner,
            outer_key_pad,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize(self) -> Vec<u8> {
        // Inner hash
        let inner_hash = self.inner.finalize();

        // Outer hash
        let mut outer = Sha256::new();
        outer.update(&self.outer_key_pad);
        outer.update(&inner_hash);
        outer.finalize()
    }
}

// ============================================================================
// HMAC-SHA1 (legacy, required for WPA)
// ============================================================================

/// HMAC-SHA1 (legacy, for WPA compatibility)
pub struct HmacSha1 {
    inner: Sha1,
    outer_key_pad: [u8; 64],
}

impl HmacSha1 {
    const BLOCK_SIZE: usize = 64;
    
    /// Compute MAC in one shot (for simple_prf compatibility)
    pub fn compute(&self, data: &[u8]) -> Vec<u8> {
        // Clone the inner state and outer key pad
        let mut new_inner = Sha1::new();
        new_inner.update(&self.inner_data());
        new_inner.update(data);
        let inner_hash = new_inner.finalize();
        
        let mut outer = Sha1::new();
        outer.update(&self.outer_key_pad);
        outer.update(&inner_hash);
        outer.finalize()
    }
    
    /// Get the current accumulated inner data
    fn inner_data(&self) -> Vec<u8> {
        // Return the inner key padding (what was used to initialize)
        let mut ipad = [0u8; 64];
        for i in 0..64 {
            ipad[i] = self.outer_key_pad[i] ^ 0x36 ^ 0x5c; // XOR to get back ipad from opad
        }
        ipad.to_vec()
    }
}

impl Mac for HmacSha1 {
    const OUTPUT_SIZE: usize = 20;

    fn new(key: &[u8]) -> Self {
        let key_block = if key.len() > Self::BLOCK_SIZE {
            let hash = Sha1::hash(key);
            let mut block = [0u8; 64];
            let copy_len = core::cmp::min(hash.len(), 64);
            block[..copy_len].copy_from_slice(&hash[..copy_len]);
            block
        } else {
            let mut block = [0u8; 64];
            block[..key.len()].copy_from_slice(key);
            block
        };

        let mut inner_key_pad = [0x36u8; 64];
        for (i, byte) in key_block.iter().enumerate() {
            inner_key_pad[i] ^= byte;
        }

        let mut outer_key_pad = [0x5cu8; 64];
        for (i, byte) in key_block.iter().enumerate() {
            outer_key_pad[i] ^= byte;
        }

        let mut inner = Sha1::new();
        inner.update(&inner_key_pad);

        Self {
            inner,
            outer_key_pad,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize(self) -> Vec<u8> {
        let inner_hash = self.inner.finalize();

        let mut outer = Sha1::new();
        outer.update(&self.outer_key_pad);
        outer.update(&inner_hash);
        outer.finalize()
    }
}

// ============================================================================
// HMAC-SHA512
// ============================================================================

/// HMAC-SHA512
pub struct HmacSha512 {
    inner: Sha512,
    outer_key_pad: [u8; 128],
}

impl HmacSha512 {
    const BLOCK_SIZE: usize = 128;
}

impl Mac for HmacSha512 {
    const OUTPUT_SIZE: usize = 64;

    fn new(key: &[u8]) -> Self {
        // If key > block size, hash it
        let key_block = if key.len() > Self::BLOCK_SIZE {
            let hash = Sha512::hash(key);
            let mut block = [0u8; 128];
            block[..64].copy_from_slice(&hash);
            block
        } else {
            let mut block = [0u8; 128];
            block[..key.len()].copy_from_slice(key);
            block
        };

        // Inner key pad (key XOR ipad)
        let mut inner_key_pad = [0x36u8; 128];
        for (i, byte) in key_block.iter().enumerate() {
            inner_key_pad[i] ^= byte;
        }

        // Outer key pad (key XOR opad)
        let mut outer_key_pad = [0x5cu8; 128];
        for (i, byte) in key_block.iter().enumerate() {
            outer_key_pad[i] ^= byte;
        }

        // Start inner hash
        let mut inner = Sha512::new();
        inner.update(&inner_key_pad);

        Self {
            inner,
            outer_key_pad,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn finalize(self) -> Vec<u8> {
        // Inner hash
        let inner_hash = self.inner.finalize();

        // Outer hash
        let mut outer = Sha512::new();
        outer.update(&self.outer_key_pad);
        outer.update(&inner_hash);
        outer.finalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hmac_sha256() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let mac = HmacSha256::mac(key, data);
        assert_eq!(mac.len(), 32);
        
        // Verify should succeed with correct tag
        assert!(HmacSha256::verify(key, data, &mac));
        
        // Verify should fail with wrong tag
        let mut bad_tag = mac.clone();
        bad_tag[0] ^= 0xff;
        assert!(!HmacSha256::verify(key, data, &bad_tag));
    }

    #[test]
    fn test_hmac_sha512() {
        let key = b"key";
        let data = b"The quick brown fox jumps over the lazy dog";
        let mac = HmacSha512::mac(key, data);
        assert_eq!(mac.len(), 64);
        
        // Verify should succeed with correct tag
        assert!(HmacSha512::verify(key, data, &mac));
    }
}

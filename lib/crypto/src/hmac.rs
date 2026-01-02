//! HMAC Implementation
//!
//! HMAC (Hash-based Message Authentication Code) using SHA-256 and SHA-512.

use crate::sha256::Sha256;
use crate::sha512::Sha512;

/// HMAC-SHA256.
pub struct HmacSha256 {
    inner: Sha256,
    outer_key_pad: [u8; 64],
}

impl HmacSha256 {
    /// Create a new HMAC-SHA256 with the given key.
    pub fn new(key: &[u8]) -> Self {
        let mut key_block = [0u8; 64];

        if key.len() > 64 {
            // Hash long keys
            key_block[..32].copy_from_slice(&Sha256::hash_bytes(key));
        } else {
            key_block[..key.len()].copy_from_slice(key);
        }

        // Inner key = key XOR 0x36
        let mut inner_key_pad = [0x36u8; 64];
        for i in 0..64 {
            inner_key_pad[i] ^= key_block[i];
        }

        // Outer key = key XOR 0x5c
        let mut outer_key_pad = [0x5cu8; 64];
        for i in 0..64 {
            outer_key_pad[i] ^= key_block[i];
        }

        let mut inner = Sha256::new();
        inner.update(&inner_key_pad);

        Self {
            inner,
            outer_key_pad,
        }
    }

    /// Update with message data.
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Finalize and return the 32-byte MAC.
    pub fn finalize(self) -> [u8; 32] {
        let inner_hash = self.inner.finalize();

        let mut outer = Sha256::new();
        outer.update(&self.outer_key_pad);
        outer.update(&inner_hash);
        outer.finalize()
    }

    /// One-shot HMAC.
    pub fn mac(key: &[u8], data: &[u8]) -> [u8; 32] {
        let mut hmac = Self::new(key);
        hmac.update(data);
        hmac.finalize()
    }
}

/// HMAC-SHA512.
pub struct HmacSha512 {
    inner: Sha512,
    outer_key_pad: [u8; 128],
}

impl HmacSha512 {
    /// Create a new HMAC-SHA512 with the given key.
    pub fn new(key: &[u8]) -> Self {
        let mut key_block = [0u8; 128];

        if key.len() > 128 {
            key_block[..64].copy_from_slice(&Sha512::hash_bytes(key));
        } else {
            key_block[..key.len()].copy_from_slice(key);
        }

        let mut inner_key_pad = [0x36u8; 128];
        for i in 0..128 {
            inner_key_pad[i] ^= key_block[i];
        }

        let mut outer_key_pad = [0x5cu8; 128];
        for i in 0..128 {
            outer_key_pad[i] ^= key_block[i];
        }

        let mut inner = Sha512::new();
        inner.update(&inner_key_pad);

        Self {
            inner,
            outer_key_pad,
        }
    }

    /// Update with message data.
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Finalize and return the 64-byte MAC.
    pub fn finalize(self) -> [u8; 64] {
        let inner_hash = self.inner.finalize();

        let mut outer = Sha512::new();
        outer.update(&self.outer_key_pad);
        outer.update(&inner_hash);
        outer.finalize()
    }

    /// One-shot HMAC.
    pub fn mac(key: &[u8], data: &[u8]) -> [u8; 64] {
        let mut hmac = Self::new(key);
        hmac.update(data);
        hmac.finalize()
    }
}

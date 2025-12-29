//! # Key Derivation Functions
//!
//! Functions for deriving cryptographic keys from passwords or other keying material.
//!
//! ## Implementations
//!
//! - **HKDF**: HMAC-based Key Derivation Function (RFC 5869)
//! - **PBKDF2**: Password-Based Key Derivation Function 2 (RFC 8018)

use alloc::vec::Vec;
use super::mac::{Mac, HmacSha1, HmacSha256, HmacSha512};

// ============================================================================
// HKDF (RFC 5869)
// ============================================================================

/// HKDF using HMAC-SHA256
pub struct Hkdf;

impl Hkdf {
    /// HKDF-Extract: Extract a fixed-length pseudorandom key from input keying material
    pub fn extract(salt: &[u8], ikm: &[u8]) -> Vec<u8> {
        // If salt is empty, use zeroes
        let salt = if salt.is_empty() {
            &[0u8; 32][..]
        } else {
            salt
        };

        HmacSha256::mac(salt, ikm)
    }

    /// HKDF-Expand: Expand a pseudorandom key to the desired length
    pub fn expand(prk: &[u8], info: &[u8], length: usize) -> Result<Vec<u8>, HkdfError> {
        if length > 255 * 32 {
            return Err(HkdfError::OutputTooLong);
        }

        let n = (length + 31) / 32;
        let mut okm = Vec::with_capacity(length);
        let mut t = Vec::new();

        for i in 1..=n {
            let mut hmac = HmacSha256::new(prk);
            hmac.update(&t);
            hmac.update(info);
            hmac.update(&[i as u8]);
            t = hmac.finalize();
            okm.extend_from_slice(&t);
        }

        okm.truncate(length);
        Ok(okm)
    }

    /// HKDF: Full extract-then-expand
    pub fn derive(
        salt: &[u8],
        ikm: &[u8],
        info: &[u8],
        length: usize,
    ) -> Result<Vec<u8>, HkdfError> {
        let prk = Self::extract(salt, ikm);
        Self::expand(&prk, info, length)
    }
}

/// HKDF errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HkdfError {
    /// Requested output length too long
    OutputTooLong,
}

// ============================================================================
// HKDF-SHA512
// ============================================================================

/// HKDF using HMAC-SHA512
pub struct HkdfSha512;

impl HkdfSha512 {
    /// HKDF-Extract with SHA-512
    pub fn extract(salt: &[u8], ikm: &[u8]) -> Vec<u8> {
        let salt = if salt.is_empty() {
            &[0u8; 64][..]
        } else {
            salt
        };

        HmacSha512::mac(salt, ikm)
    }

    /// HKDF-Expand with SHA-512
    pub fn expand(prk: &[u8], info: &[u8], length: usize) -> Result<Vec<u8>, HkdfError> {
        if length > 255 * 64 {
            return Err(HkdfError::OutputTooLong);
        }

        let n = (length + 63) / 64;
        let mut okm = Vec::with_capacity(length);
        let mut t = Vec::new();

        for i in 1..=n {
            let mut hmac = HmacSha512::new(prk);
            hmac.update(&t);
            hmac.update(info);
            hmac.update(&[i as u8]);
            t = hmac.finalize();
            okm.extend_from_slice(&t);
        }

        okm.truncate(length);
        Ok(okm)
    }

    /// HKDF: Full extract-then-expand with SHA-512
    pub fn derive(
        salt: &[u8],
        ikm: &[u8],
        info: &[u8],
        length: usize,
    ) -> Result<Vec<u8>, HkdfError> {
        let prk = Self::extract(salt, ikm);
        Self::expand(&prk, info, length)
    }
}

// ============================================================================
// PBKDF2 (RFC 8018)
// ============================================================================

/// PBKDF2 using HMAC-SHA256
pub struct Pbkdf2;

impl Pbkdf2 {
    /// Minimum recommended iteration count
    pub const MIN_ITERATIONS: u32 = 10_000;

    /// Derives a key from a password using PBKDF2-HMAC-SHA256
    pub fn derive(
        password: &[u8],
        salt: &[u8],
        iterations: u32,
        key_length: usize,
    ) -> Result<Vec<u8>, Pbkdf2Error> {
        if iterations == 0 {
            return Err(Pbkdf2Error::InvalidIterations);
        }

        if key_length == 0 {
            return Err(Pbkdf2Error::InvalidKeyLength);
        }

        // Maximum output length for SHA-256: (2^32 - 1) * 32 bytes
        let max_len = (u32::MAX as usize) * 32;
        if key_length > max_len {
            return Err(Pbkdf2Error::KeyTooLong);
        }

        let hash_len = 32; // SHA-256 output
        let block_count = (key_length + hash_len - 1) / hash_len;

        let mut derived_key = Vec::with_capacity(key_length);

        for block_num in 1..=block_count {
            let block = Self::f(password, salt, iterations, block_num as u32);
            derived_key.extend_from_slice(&block);
        }

        derived_key.truncate(key_length);
        Ok(derived_key)
    }

    /// F function: XOR of all HMAC iterations
    fn f(password: &[u8], salt: &[u8], iterations: u32, block_num: u32) -> [u8; 32] {
        // U_1 = HMAC(password, salt || block_num)
        let mut u = {
            let mut hmac = HmacSha256::new(password);
            hmac.update(salt);
            hmac.update(&block_num.to_be_bytes());
            let result = hmac.finalize();
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&result);
            arr
        };

        let mut result = u;

        // U_2 to U_c
        for _ in 1..iterations {
            u = {
                let result = HmacSha256::mac(password, &u);
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&result);
                arr
            };

            // XOR into result
            for (r, u_byte) in result.iter_mut().zip(u.iter()) {
                *r ^= u_byte;
            }
        }

        result
    }

    /// Derives a key from a password using PBKDF2-HMAC-SHA1 (for WPA)
    pub fn derive_sha1(
        password: &[u8],
        salt: &[u8],
        iterations: u32,
        key_length: usize,
    ) -> Result<Vec<u8>, Pbkdf2Error> {
        if iterations == 0 {
            return Err(Pbkdf2Error::InvalidIterations);
        }

        if key_length == 0 {
            return Err(Pbkdf2Error::InvalidKeyLength);
        }

        let hash_len = 20; // SHA-1 output
        let block_count = (key_length + hash_len - 1) / hash_len;

        let mut derived_key = Vec::with_capacity(key_length);

        for block_num in 1..=block_count {
            let block = Self::f_sha1(password, salt, iterations, block_num as u32);
            derived_key.extend_from_slice(&block);
        }

        derived_key.truncate(key_length);
        Ok(derived_key)
    }

    /// F function for SHA1: XOR of all HMAC iterations
    fn f_sha1(password: &[u8], salt: &[u8], iterations: u32, block_num: u32) -> [u8; 20] {
        let mut u = {
            let mut hmac = HmacSha1::new(password);
            hmac.update(salt);
            hmac.update(&block_num.to_be_bytes());
            let result = hmac.finalize();
            let mut arr = [0u8; 20];
            let copy_len = core::cmp::min(result.len(), 20);
            arr[..copy_len].copy_from_slice(&result[..copy_len]);
            arr
        };

        let mut result = u;

        for _ in 1..iterations {
            u = {
                let mac_result = HmacSha1::mac(password, &u);
                let mut arr = [0u8; 20];
                let copy_len = core::cmp::min(mac_result.len(), 20);
                arr[..copy_len].copy_from_slice(&mac_result[..copy_len]);
                arr
            };

            for (r, u_byte) in result.iter_mut().zip(u.iter()) {
                *r ^= u_byte;
            }
        }

        result
    }
}

/// PBKDF2-HMAC-SHA512 variant
pub struct Pbkdf2Sha512;

impl Pbkdf2Sha512 {
    /// Derives a key from a password using PBKDF2-HMAC-SHA512
    pub fn derive(
        password: &[u8],
        salt: &[u8],
        iterations: u32,
        key_length: usize,
    ) -> Result<Vec<u8>, Pbkdf2Error> {
        if iterations == 0 {
            return Err(Pbkdf2Error::InvalidIterations);
        }

        if key_length == 0 {
            return Err(Pbkdf2Error::InvalidKeyLength);
        }

        let hash_len = 64; // SHA-512 output
        let block_count = (key_length + hash_len - 1) / hash_len;

        let mut derived_key = Vec::with_capacity(key_length);

        for block_num in 1..=block_count {
            let block = Self::f(password, salt, iterations, block_num as u32);
            derived_key.extend_from_slice(&block);
        }

        derived_key.truncate(key_length);
        Ok(derived_key)
    }

    fn f(password: &[u8], salt: &[u8], iterations: u32, block_num: u32) -> [u8; 64] {
        let mut u = {
            let mut hmac = HmacSha512::new(password);
            hmac.update(salt);
            hmac.update(&block_num.to_be_bytes());
            let result = hmac.finalize();
            let mut arr = [0u8; 64];
            arr.copy_from_slice(&result);
            arr
        };

        let mut result = u;

        for _ in 1..iterations {
            u = {
                let result = HmacSha512::mac(password, &u);
                let mut arr = [0u8; 64];
                arr.copy_from_slice(&result);
                arr
            };

            for (r, u_byte) in result.iter_mut().zip(u.iter()) {
                *r ^= u_byte;
            }
        }

        result
    }
}

/// PBKDF2 errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pbkdf2Error {
    /// Iteration count must be > 0
    InvalidIterations,
    /// Key length must be > 0
    InvalidKeyLength,
    /// Requested key length too long
    KeyTooLong,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hkdf_basic() {
        let ikm = b"input keying material";
        let salt = b"salt";
        let info = b"context";

        let key = Hkdf::derive(salt, ikm, info, 32).unwrap();
        assert_eq!(key.len(), 32);

        // Same inputs should produce same output
        let key2 = Hkdf::derive(salt, ikm, info, 32).unwrap();
        assert_eq!(key, key2);

        // Different info should produce different output
        let key3 = Hkdf::derive(salt, ikm, b"other", 32).unwrap();
        assert_ne!(key, key3);
    }

    #[test]
    fn test_hkdf_long_output() {
        let ikm = b"input";
        let key = Hkdf::derive(&[], ikm, &[], 64).unwrap();
        assert_eq!(key.len(), 64);
    }

    #[test]
    fn test_pbkdf2_basic() {
        let password = b"password";
        let salt = b"salt";

        let key = Pbkdf2::derive(password, salt, 1000, 32).unwrap();
        assert_eq!(key.len(), 32);

        // Same inputs should produce same output
        let key2 = Pbkdf2::derive(password, salt, 1000, 32).unwrap();
        assert_eq!(key, key2);

        // More iterations should produce different output
        let key3 = Pbkdf2::derive(password, salt, 2000, 32).unwrap();
        assert_ne!(key, key3);
    }

    #[test]
    fn test_pbkdf2_errors() {
        assert!(Pbkdf2::derive(b"pass", b"salt", 0, 32).is_err());
        assert!(Pbkdf2::derive(b"pass", b"salt", 1000, 0).is_err());
    }
}

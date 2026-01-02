//! # Splax Cryptographic Primitives
//!
//! This crate provides pure Rust cryptographic primitives that work in both
//! kernel (no_std) and userspace (std) environments.
//!
//! ## Algorithms
//!
//! - **SHA-256**: Secure hash function, 256-bit output
//! - **SHA-512**: Secure hash function, 512-bit output
//! - **HMAC-SHA256**: Message authentication code
//!
//! ## Design
//!
//! - Pure Rust, no dependencies
//! - Constant-time where security-critical
//! - No dynamic allocation in core operations

#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

pub mod sha256;
pub mod sha512;
pub mod hmac;

pub use sha256::Sha256;
pub use sha512::Sha512;
pub use hmac::{HmacSha256, HmacSha512};

/// Hash trait for consistent interface.
pub trait Hash {
    /// Output size in bytes.
    const OUTPUT_SIZE: usize;

    /// Create a new hasher.
    fn new() -> Self;

    /// Update the hasher with data.
    fn update(&mut self, data: &[u8]);

    /// Finalize and return the hash as a fixed-size array.
    fn finalize_array(self) -> [u8; 64];

    /// One-shot hash to fixed array.
    fn hash_to_array(data: &[u8]) -> [u8; 64]
    where
        Self: Sized,
    {
        let mut hasher = Self::new();
        hasher.update(data);
        hasher.finalize_array()
    }

    /// One-shot hash to Vec.
    #[cfg(feature = "alloc")]
    fn hash(data: &[u8]) -> Vec<u8>
    where
        Self: Sized,
    {
        Self::hash_to_array(data)[..Self::OUTPUT_SIZE].to_vec()
    }
}

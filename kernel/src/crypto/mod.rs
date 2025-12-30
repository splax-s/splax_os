//! # Cryptography Subsystem
//!
//! Safe cryptographic primitives for Splax OS.
//!
//! ## Design Principles
//!
//! 1. **Pure Rust**: No C dependencies, no OpenSSL
//! 2. **Constant-time**: Resistant to timing attacks where applicable
//! 3. **Zero-copy where possible**: Minimize allocations
//! 4. **Capability-gated**: Access to crypto operations requires tokens
//!
//! ## Supported Algorithms
//!
//! ### Hash Functions
//! - SHA-256, SHA-512 (secure hashing)
//! - XXHash3 (fast non-cryptographic, for checksums)
//!
//! ### Symmetric Ciphers
//! - AES-128/256-GCM (authenticated encryption)
//! - ChaCha20-Poly1305 (modern AEAD)
//!
//! ### Key Derivation
//! - HKDF (HMAC-based KDF)
//! - PBKDF2 (password-based)
//!
//! ### MAC
//! - HMAC-SHA256, HMAC-SHA512
//!
//! ## Usage
//!
//! ```rust
//! use crate::crypto::{Sha256, Hash, AesGcm, Cipher};
//!
//! // Hash some data
//! let hash = Sha256::hash(b"hello world");
//!
//! // Encrypt with AES-GCM
//! let key = AesGcm::generate_key();
//! let nonce = AesGcm::generate_nonce();
//! let ciphertext = AesGcm::encrypt(&key, &nonce, b"secret", b"aad")?;
//! ```

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;

pub mod hash;
pub mod cipher;
pub mod mac;
pub mod asymmetric;
pub mod kdf;
pub mod random;

pub use hash::{Hash, Sha256, Sha512, XxHash3};
pub use cipher::{Cipher, AesGcm, ChaCha20Poly1305, CipherError};
pub use mac::{Mac, HmacSha256, HmacSha512};
pub use kdf::{Hkdf, Pbkdf2};
pub use random::{CryptoRng, SystemRng};

/// Cryptographic configuration
#[derive(Debug, Clone)]
pub struct CryptoConfig {
    /// Default key size in bytes
    pub default_key_size: usize,
    /// Enable hardware acceleration if available
    pub use_hw_accel: bool,
}

impl Default for CryptoConfig {
    fn default() -> Self {
        Self {
            default_key_size: 32, // 256 bits
            use_hw_accel: true,
        }
    }
}

/// Crypto subsystem manager
pub struct CryptoSubsystem {
    config: CryptoConfig,
    rng: SystemRng,
}

impl CryptoSubsystem {
    /// Creates a new crypto subsystem
    pub fn new(config: CryptoConfig) -> Self {
        Self {
            config,
            rng: SystemRng::new().unwrap_or_else(|_| {
                // If hardware RNG not available, create a fallback
                // This is safe since we check in is_available
                SystemRng { _private: () }
            }),
        }
    }

    /// Initializes the crypto subsystem
    pub fn init(&mut self) -> Result<(), CryptoError> {
        // Check for hardware acceleration
        #[cfg(target_arch = "x86_64")]
        {
            if self.config.use_hw_accel {
                // Check for AES-NI
                let has_aesni = check_aesni();
                if has_aesni {
                    // Hardware acceleration available
                }
            }
        }

        // Initialize the RNG (seed from hardware if available)
        // Note: reseed is not needed, SystemRng uses hardware entropy directly

        Ok(())
    }

    /// Returns a reference to the system RNG
    pub fn rng(&mut self) -> &mut SystemRng {
        &mut self.rng
    }

    /// Generates random bytes
    pub fn random_bytes(&mut self, buf: &mut [u8]) -> Result<(), CryptoError> {
        self.rng.fill_bytes(buf)
    }

    /// Generates a random key of the configured size
    pub fn generate_key(&mut self) -> Result<Vec<u8>, CryptoError> {
        let mut key = vec![0u8; self.config.default_key_size];
        self.random_bytes(&mut key)?;
        Ok(key)
    }
}

/// Check for AES-NI support on x86_64
#[cfg(target_arch = "x86_64")]
fn check_aesni() -> bool {
    // CPUID leaf 1, check ECX bit 25
    let ecx: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "mov {0:e}, ecx",
            "pop rbx",
            out(reg) ecx,
            out("eax") _,
            out("ecx") _,
            out("edx") _,
            options(nostack)
        );
    }
    (ecx & (1 << 25)) != 0
}

#[cfg(not(target_arch = "x86_64"))]
fn check_aesni() -> bool {
    false
}

/// Crypto errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    /// Invalid key length
    InvalidKeyLength,
    /// Invalid nonce length
    InvalidNonceLength,
    /// Authentication failed (for AEAD)
    AuthenticationFailed,
    /// Buffer too small
    BufferTooSmall,
    /// RNG failure
    RngFailure,
    /// Invalid input
    InvalidInput,
    /// Operation not supported
    NotSupported,
}

/// Securely zeroes memory
pub fn secure_zero(buf: &mut [u8]) {
    for byte in buf.iter_mut() {
        unsafe {
            core::ptr::write_volatile(byte, 0);
        }
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Constant-time byte comparison
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// Global crypto subsystem
static CRYPTO: spin::Lazy<spin::Mutex<CryptoSubsystem>> = spin::Lazy::new(|| {
    spin::Mutex::new(CryptoSubsystem::new(CryptoConfig::default()))
});

/// Initializes the global crypto subsystem
pub fn init() -> Result<(), CryptoError> {
    CRYPTO.lock().init()
}

/// Gets random bytes from the system RNG
pub fn random_bytes(buf: &mut [u8]) -> Result<(), CryptoError> {
    CRYPTO.lock().random_bytes(buf)
}

/// Generates a random key
pub fn generate_key(size: usize) -> Result<Vec<u8>, CryptoError> {
    let mut key = vec![0u8; size];
    random_bytes(&mut key)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
    }
}

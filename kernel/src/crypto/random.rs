//! # Random Number Generation
//!
//! Cryptographically secure random number generation.
//!
//! ## Implementations
//!
//! - **SystemRng**: Hardware RNG (RDRAND on x86_64, RNDR on AArch64)
//! - **ChaChaRng**: Software CSPRNG based on ChaCha20

use alloc::vec;
use alloc::vec::Vec;
use super::CryptoError;

/// Trait for cryptographic random number generators
pub trait CryptoRng {
    /// Fills a buffer with random bytes
    fn fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), CryptoError>;

    /// Generates random bytes and returns as Vec
    fn random_bytes(&mut self, len: usize) -> Result<Vec<u8>, CryptoError> {
        let mut buf = vec![0u8; len];
        self.fill_bytes(&mut buf)?;
        Ok(buf)
    }

    /// Generates a random u64
    fn random_u64(&mut self) -> Result<u64, CryptoError> {
        let mut buf = [0u8; 8];
        self.fill_bytes(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    /// Generates a random u32
    fn random_u32(&mut self) -> Result<u32, CryptoError> {
        let mut buf = [0u8; 4];
        self.fill_bytes(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    /// Generates a random value in range [0, max)
    fn random_range(&mut self, max: u64) -> Result<u64, CryptoError> {
        if max == 0 {
            return Err(CryptoError::InvalidInput);
        }

        // Rejection sampling to avoid modulo bias
        let threshold = u64::MAX - (u64::MAX % max);
        loop {
            let val = self.random_u64()?;
            if val < threshold {
                return Ok(val % max);
            }
        }
    }
}

// ============================================================================
// SystemRng - Hardware RNG
// ============================================================================

/// System hardware random number generator
///
/// Uses platform-specific hardware RNG:
/// - x86_64: RDRAND instruction
/// - AArch64: RNDR register
/// - RISC-V: Seed CSR (if available)
pub struct SystemRng {
    pub(crate) _private: (),
}

impl SystemRng {
    /// Creates a new SystemRng
    pub fn new() -> Result<Self, CryptoError> {
        // Check if hardware RNG is available
        if !Self::is_available() {
            return Err(CryptoError::RngFailure);
        }

        Ok(Self { _private: () })
    }

    /// Checks if hardware RNG is available
    pub fn is_available() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            Self::check_rdrand()
        }

        #[cfg(target_arch = "aarch64")]
        {
            // AArch64 RNDR availability check
            true // Assume available on ARMv8.5+
        }

        #[cfg(target_arch = "riscv64")]
        {
            // RISC-V Zkr extension check
            false // Conservative default
        }

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "riscv64")))]
        {
            false
        }
    }

    #[cfg(target_arch = "x86_64")]
    fn check_rdrand() -> bool {
        // CPUID to check RDRAND support (ECX bit 30)
        let result: u32;
        unsafe {
            core::arch::asm!(
                "push rbx",
                "mov eax, 1",
                "cpuid",
                "mov {0:e}, ecx",
                "pop rbx",
                out(reg) result,
                out("eax") _,
                out("ecx") _,
                out("edx") _,
                options(nostack)
            );
        }
        (result & (1 << 30)) != 0
    }

    #[cfg(target_arch = "x86_64")]
    fn rdrand_u64() -> Result<u64, CryptoError> {
        // RDRAND with retry
        for _ in 0..10 {
            let value: u64;
            let success: u8;
            
            unsafe {
                core::arch::asm!(
                    "rdrand {0}",
                    "setc {1}",
                    out(reg) value,
                    out(reg_byte) success,
                    options(nostack)
                );
            }

            if success != 0 {
                return Ok(value);
            }
        }

        Err(CryptoError::RngFailure)
    }

    #[cfg(target_arch = "aarch64")]
    fn rndr_u64() -> Result<u64, CryptoError> {
        let value: u64;
        let status: u64;

        unsafe {
            core::arch::asm!(
                "mrs {0}, s3_3_c2_c4_0", // RNDR
                "cset {1}, ne",
                out(reg) value,
                out(reg) status,
                options(nostack)
            );
        }

        if status != 0 {
            Ok(value)
        } else {
            Err(CryptoError::RngFailure)
        }
    }

    #[cfg(target_arch = "riscv64")]
    fn seed_u64() -> Result<u64, CryptoError> {
        // RISC-V Zkr extension - read from seed CSR
        // This is a simplified implementation
        Err(CryptoError::NotSupported)
    }

    fn hardware_random_u64() -> Result<u64, CryptoError> {
        #[cfg(target_arch = "x86_64")]
        {
            Self::rdrand_u64()
        }

        #[cfg(target_arch = "aarch64")]
        {
            Self::rndr_u64()
        }

        #[cfg(target_arch = "riscv64")]
        {
            Self::seed_u64()
        }

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "riscv64")))]
        {
            Err(CryptoError::NotSupported)
        }
    }
}

impl Default for SystemRng {
    fn default() -> Self {
        Self::new().expect("Hardware RNG not available")
    }
}

impl CryptoRng for SystemRng {
    fn fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), CryptoError> {
        let mut offset = 0;

        while offset < dest.len() {
            let random = Self::hardware_random_u64()?;
            let bytes = random.to_le_bytes();

            let remaining = dest.len() - offset;
            let to_copy = core::cmp::min(8, remaining);

            dest[offset..offset + to_copy].copy_from_slice(&bytes[..to_copy]);
            offset += to_copy;
        }

        Ok(())
    }
}

// ============================================================================
// Convenience Functions
// ============================================================================

/// Simple LFSR-based fallback RNG state for when hardware RNG is unavailable.
static LFSR_STATE: spin::Mutex<u64> = spin::Mutex::new(0xDEAD_BEEF_CAFE_BABEu64);

/// Generates N random bytes and returns them as a fixed-size array.
///
/// Uses hardware RNG if available, otherwise falls back to a simple LFSR.
///
/// # Example
/// ```
/// let cookie: [u8; 16] = random_bytes::<16>();
/// let key: [u8; 32] = random_bytes::<32>();
/// ```
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buffer = [0u8; N];
    fill_random_bytes(&mut buffer);
    buffer
}

/// Fills a buffer with random bytes.
///
/// Uses hardware RNG if available, otherwise falls back to a simple LFSR.
pub fn fill_random_bytes(buffer: &mut [u8]) {
    // Try hardware RNG first
    if let Ok(mut rng) = SystemRng::new() {
        if rng.fill_bytes(buffer).is_ok() {
            return;
        }
    }

    // Fallback: simple LFSR-based PRNG
    let mut state = LFSR_STATE.lock();
    for byte in buffer.iter_mut() {
        // Xorshift64 algorithm
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        *byte = *state as u8;
    }
}

// ============================================================================
// ChaChaRng - Software CSPRNG
// ============================================================================

/// ChaCha20-based CSPRNG
///
/// Uses ChaCha20 as a cryptographically secure PRNG.
/// Should be seeded from hardware RNG when possible.
pub struct ChaChaRng {
    state: [u32; 16],
    buffer: [u8; 64],
    buffer_pos: usize,
}

impl ChaChaRng {
    const CONSTANTS: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];

    /// Creates a new ChaChaRng with the given seed
    pub fn from_seed(seed: [u8; 32]) -> Self {
        let mut state = [0u32; 16];

        // Constants
        state[0] = Self::CONSTANTS[0];
        state[1] = Self::CONSTANTS[1];
        state[2] = Self::CONSTANTS[2];
        state[3] = Self::CONSTANTS[3];

        // Key from seed
        for i in 0..8 {
            state[4 + i] = u32::from_le_bytes([
                seed[i * 4],
                seed[i * 4 + 1],
                seed[i * 4 + 2],
                seed[i * 4 + 3],
            ]);
        }

        // Counter
        state[12] = 0;
        state[13] = 0;

        // Nonce (zeros for RNG use)
        state[14] = 0;
        state[15] = 0;

        Self {
            state,
            buffer: [0; 64],
            buffer_pos: 64, // Force refill on first use
        }
    }

    /// Creates a new ChaChaRng seeded from SystemRng
    pub fn from_system() -> Result<Self, CryptoError> {
        let mut rng = SystemRng::new()?;
        let mut seed = [0u8; 32];
        rng.fill_bytes(&mut seed)?;
        Ok(Self::from_seed(seed))
    }

    fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
        state[a] = state[a].wrapping_add(state[b]);
        state[d] ^= state[a];
        state[d] = state[d].rotate_left(16);

        state[c] = state[c].wrapping_add(state[d]);
        state[b] ^= state[c];
        state[b] = state[b].rotate_left(12);

        state[a] = state[a].wrapping_add(state[b]);
        state[d] ^= state[a];
        state[d] = state[d].rotate_left(8);

        state[c] = state[c].wrapping_add(state[d]);
        state[b] ^= state[c];
        state[b] = state[b].rotate_left(7);
    }

    fn generate_block(&mut self) {
        let mut working = self.state;

        // 20 rounds
        for _ in 0..10 {
            // Column rounds
            Self::quarter_round(&mut working, 0, 4, 8, 12);
            Self::quarter_round(&mut working, 1, 5, 9, 13);
            Self::quarter_round(&mut working, 2, 6, 10, 14);
            Self::quarter_round(&mut working, 3, 7, 11, 15);
            // Diagonal rounds
            Self::quarter_round(&mut working, 0, 5, 10, 15);
            Self::quarter_round(&mut working, 1, 6, 11, 12);
            Self::quarter_round(&mut working, 2, 7, 8, 13);
            Self::quarter_round(&mut working, 3, 4, 9, 14);
        }

        // Add original state
        for i in 0..16 {
            working[i] = working[i].wrapping_add(self.state[i]);
        }

        // Serialize to buffer
        for i in 0..16 {
            let bytes = working[i].to_le_bytes();
            self.buffer[i * 4] = bytes[0];
            self.buffer[i * 4 + 1] = bytes[1];
            self.buffer[i * 4 + 2] = bytes[2];
            self.buffer[i * 4 + 3] = bytes[3];
        }

        // Increment counter
        self.state[12] = self.state[12].wrapping_add(1);
        if self.state[12] == 0 {
            self.state[13] = self.state[13].wrapping_add(1);
        }

        self.buffer_pos = 0;
    }
}

impl CryptoRng for ChaChaRng {
    fn fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), CryptoError> {
        let mut offset = 0;

        while offset < dest.len() {
            if self.buffer_pos >= 64 {
                self.generate_block();
            }

            let remaining_buffer = 64 - self.buffer_pos;
            let remaining_dest = dest.len() - offset;
            let to_copy = core::cmp::min(remaining_buffer, remaining_dest);

            dest[offset..offset + to_copy]
                .copy_from_slice(&self.buffer[self.buffer_pos..self.buffer_pos + to_copy]);

            self.buffer_pos += to_copy;
            offset += to_copy;
        }

        Ok(())
    }
}

// ============================================================================
// Entropy Pool (for mixing multiple entropy sources)
// ============================================================================

/// Entropy pool for mixing multiple entropy sources
pub struct EntropyPool {
    pool: [u8; 32],
    count: usize,
}

impl EntropyPool {
    /// Creates a new empty entropy pool
    pub const fn new() -> Self {
        Self {
            pool: [0; 32],
            count: 0,
        }
    }

    /// Adds entropy to the pool
    pub fn add_entropy(&mut self, data: &[u8]) {
        use super::hash::{Hash, Sha256};

        // Mix new entropy with existing pool using SHA-256
        let mut hasher = Sha256::new();
        hasher.update(&self.pool);
        hasher.update(data);
        hasher.update(&(self.count as u64).to_le_bytes());

        let hash = hasher.finalize();
        self.pool.copy_from_slice(&hash);
        self.count = self.count.wrapping_add(1);
    }

    /// Extracts entropy from the pool
    pub fn extract(&mut self, dest: &mut [u8]) {
        use super::hash::{Hash, Sha256};

        // Generate output
        let mut hasher = Sha256::new();
        hasher.update(&self.pool);
        hasher.update(&[0x01]); // Domain separation
        hasher.update(&(self.count as u64).to_le_bytes());

        let output = hasher.finalize();

        let to_copy = core::cmp::min(dest.len(), 32);
        dest[..to_copy].copy_from_slice(&output[..to_copy]);

        // Update pool state
        let mut hasher = Sha256::new();
        hasher.update(&self.pool);
        hasher.update(&[0x02]); // Different domain
        hasher.update(&(self.count as u64).to_le_bytes());

        let new_pool = hasher.finalize();
        self.pool.copy_from_slice(&new_pool);
        self.count = self.count.wrapping_add(1);
    }

    /// Returns true if pool has been seeded
    pub fn is_seeded(&self) -> bool {
        self.count > 0
    }
}

impl Default for EntropyPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chacha_rng() {
        let seed = [0u8; 32];
        let mut rng = ChaChaRng::from_seed(seed);

        let mut buf1 = [0u8; 64];
        let mut buf2 = [0u8; 64];

        rng.fill_bytes(&mut buf1).unwrap();
        rng.fill_bytes(&mut buf2).unwrap();

        // Different blocks should be different
        assert_ne!(buf1, buf2);
    }

    #[test]
    fn test_chacha_rng_deterministic() {
        let seed = [0u8; 32];
        let mut rng1 = ChaChaRng::from_seed(seed);
        let mut rng2 = ChaChaRng::from_seed(seed);

        let mut buf1 = [0u8; 64];
        let mut buf2 = [0u8; 64];

        rng1.fill_bytes(&mut buf1).unwrap();
        rng2.fill_bytes(&mut buf2).unwrap();

        // Same seed should produce same output
        assert_eq!(buf1, buf2);
    }

    #[test]
    fn test_entropy_pool() {
        let mut pool = EntropyPool::new();
        assert!(!pool.is_seeded());

        pool.add_entropy(b"entropy1");
        assert!(pool.is_seeded());

        pool.add_entropy(b"entropy2");

        let mut out1 = [0u8; 32];
        let mut out2 = [0u8; 32];

        pool.extract(&mut out1);
        pool.extract(&mut out2);

        // Different extractions should produce different output
        assert_ne!(out1, out2);
    }
}

//! # Hash Functions
//!
//! Cryptographic and non-cryptographic hash functions.
//!
//! ## Implementations
//!
//! - **SHA-256**: Secure hash, 256-bit output
//! - **SHA-512**: Secure hash, 512-bit output  
//! - **XXHash3**: Fast non-cryptographic hash (for checksums)

use alloc::vec::Vec;

/// Hash function trait
pub trait Hash {
    /// Output size in bytes
    const OUTPUT_SIZE: usize;

    /// Creates a new hasher
    fn new() -> Self;

    /// Updates the hasher with data
    fn update(&mut self, data: &[u8]);

    /// Finalizes and returns the hash
    fn finalize(self) -> Vec<u8>;

    /// One-shot hash function
    fn hash(data: &[u8]) -> Vec<u8>
    where
        Self: Sized,
    {
        let mut hasher = Self::new();
        hasher.update(data);
        hasher.finalize()
    }
}

// ============================================================================
// SHA-256
// ============================================================================

/// SHA-256 hasher
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_len: u64,
}

impl Sha256 {
    /// Initial hash values (first 32 bits of fractional parts of square roots of first 8 primes)
    const H: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    /// Round constants (first 32 bits of fractional parts of cube roots of first 64 primes)
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    fn compress(&mut self, block: &[u8]) {
        debug_assert_eq!(block.len(), 64);

        // Parse block into 16 32-bit words
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }

        // Extend to 64 words
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        // Initialize working variables
        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        // 64 rounds
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(Self::K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        // Update state
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

impl Hash for Sha256 {
    const OUTPUT_SIZE: usize = 32;

    fn new() -> Self {
        Self {
            state: Self::H,
            buffer: [0; 64],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        let mut offset = 0;

        // Fill buffer if partially full
        if self.buffer_len > 0 {
            let to_copy = core::cmp::min(64 - self.buffer_len, data.len());
            self.buffer[self.buffer_len..self.buffer_len + to_copy]
                .copy_from_slice(&data[..to_copy]);
            self.buffer_len += to_copy;
            offset += to_copy;

            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            }
        }

        // Process full blocks
        while offset + 64 <= data.len() {
            self.compress(&data[offset..offset + 64]);
            offset += 64;
        }

        // Buffer remaining
        if offset < data.len() {
            let remaining = data.len() - offset;
            self.buffer[..remaining].copy_from_slice(&data[offset..]);
            self.buffer_len = remaining;
        }

        self.total_len += data.len() as u64;
    }

    fn finalize(mut self) -> Vec<u8> {
        // Padding
        let total_bits = self.total_len * 8;

        // Append 1 bit
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        // Zero padding
        if self.buffer_len > 56 {
            // Need extra block
            for i in self.buffer_len..64 {
                self.buffer[i] = 0;
            }
            let block = self.buffer;
            self.compress(&block);
            self.buffer_len = 0;
        }

        for i in self.buffer_len..56 {
            self.buffer[i] = 0;
        }

        // Append length (big endian)
        self.buffer[56..64].copy_from_slice(&total_bits.to_be_bytes());

        let block = self.buffer;
        self.compress(&block);

        // Output
        let mut output = Vec::with_capacity(32);
        for word in &self.state {
            output.extend_from_slice(&word.to_be_bytes());
        }
        output
    }
}

// ============================================================================
// SHA-512
// ============================================================================

/// SHA-512 hasher
pub struct Sha512 {
    state: [u64; 8],
    buffer: [u8; 128],
    buffer_len: usize,
    total_len: u128,
}

impl Sha512 {
    const H: [u64; 8] = [
        0x6a09e667f3bcc908, 0xbb67ae8584caa73b,
        0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1,
        0x510e527fade682d1, 0x9b05688c2b3e6c1f,
        0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
    ];

    const K: [u64; 80] = [
        0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc,
        0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
        0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
        0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
        0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
        0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
        0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
        0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
        0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
        0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
        0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30,
        0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
        0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8,
        0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
        0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
        0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
        0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178,
        0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
        0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c,
        0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817,
    ];

    fn compress(&mut self, block: &[u8]) {
        debug_assert_eq!(block.len(), 128);

        let mut w = [0u64; 80];
        for (i, chunk) in block.chunks(8).enumerate() {
            w[i] = u64::from_be_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3],
                chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
        }

        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];

        for i in 0..80 {
            let s1 = e.rotate_right(14) ^ e.rotate_right(18) ^ e.rotate_right(41);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(Self::K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(28) ^ a.rotate_right(34) ^ a.rotate_right(39);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

impl Hash for Sha512 {
    const OUTPUT_SIZE: usize = 64;

    fn new() -> Self {
        Self {
            state: Self::H,
            buffer: [0; 128],
            buffer_len: 0,
            total_len: 0,
        }
    }

    fn update(&mut self, data: &[u8]) {
        let mut offset = 0;

        if self.buffer_len > 0 {
            let to_copy = core::cmp::min(128 - self.buffer_len, data.len());
            self.buffer[self.buffer_len..self.buffer_len + to_copy]
                .copy_from_slice(&data[..to_copy]);
            self.buffer_len += to_copy;
            offset += to_copy;

            if self.buffer_len == 128 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            }
        }

        while offset + 128 <= data.len() {
            self.compress(&data[offset..offset + 128]);
            offset += 128;
        }

        if offset < data.len() {
            let remaining = data.len() - offset;
            self.buffer[..remaining].copy_from_slice(&data[offset..]);
            self.buffer_len = remaining;
        }

        self.total_len += data.len() as u128;
    }

    fn finalize(mut self) -> Vec<u8> {
        let total_bits = self.total_len * 8;

        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        if self.buffer_len > 112 {
            for i in self.buffer_len..128 {
                self.buffer[i] = 0;
            }
            let block = self.buffer;
            self.compress(&block);
            self.buffer_len = 0;
        }

        for i in self.buffer_len..112 {
            self.buffer[i] = 0;
        }

        self.buffer[112..128].copy_from_slice(&total_bits.to_be_bytes());

        let block = self.buffer;
        self.compress(&block);

        let mut output = Vec::with_capacity(64);
        for word in &self.state {
            output.extend_from_slice(&word.to_be_bytes());
        }
        output
    }
}

// ============================================================================
// XXHash3 (Non-cryptographic, fast)
// ============================================================================

/// XXHash3-64 hasher (fast non-cryptographic hash)
pub struct XxHash3 {
    acc: [u64; 8],
    buffer: [u8; 256],
    buffer_len: usize,
    total_len: u64,
    seed: u64,
}

impl XxHash3 {
    const PRIME64_1: u64 = 0x9E3779B185EBCA87;
    const PRIME64_2: u64 = 0xC2B2AE3D27D4EB4F;
    const PRIME64_3: u64 = 0x165667B19E3779F9;
    const PRIME64_4: u64 = 0x85EBCA77C2B2AE63;
    const PRIME64_5: u64 = 0x27D4EB2F165667C5;

    /// Creates a new hasher with a seed
    pub fn with_seed(seed: u64) -> Self {
        Self {
            acc: [
                Self::PRIME64_1.wrapping_add(Self::PRIME64_2),
                Self::PRIME64_2,
                0,
                Self::PRIME64_1.wrapping_neg(),
                Self::PRIME64_1.wrapping_add(Self::PRIME64_2),
                Self::PRIME64_2,
                0,
                Self::PRIME64_1.wrapping_neg(),
            ],
            buffer: [0; 256],
            buffer_len: 0,
            total_len: 0,
            seed,
        }
    }

    /// One-shot hash with seed
    pub fn hash_with_seed(data: &[u8], seed: u64) -> u64 {
        let mut hasher = Self::with_seed(seed);
        hasher.update(data);
        hasher.finalize_u64()
    }

    /// Returns hash as u64
    pub fn finalize_u64(self) -> u64 {
        let bytes = self.finalize();
        u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
            bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }

    fn avalanche(mut h: u64) -> u64 {
        h ^= h >> 33;
        h = h.wrapping_mul(Self::PRIME64_2);
        h ^= h >> 29;
        h = h.wrapping_mul(Self::PRIME64_3);
        h ^= h >> 32;
        h
    }
}

impl Hash for XxHash3 {
    const OUTPUT_SIZE: usize = 8;

    fn new() -> Self {
        Self::with_seed(0)
    }

    fn update(&mut self, data: &[u8]) {
        // Simplified: just buffer and process at end
        // Full XXH3 has streaming support
        let remaining = self.buffer.len() - self.buffer_len;
        let to_copy = core::cmp::min(remaining, data.len());
        self.buffer[self.buffer_len..self.buffer_len + to_copy]
            .copy_from_slice(&data[..to_copy]);
        self.buffer_len += to_copy;
        self.total_len += data.len() as u64;
    }

    fn finalize(self) -> Vec<u8> {
        // Simplified short input hash
        let len = self.total_len;
        let input = &self.buffer[..self.buffer_len];

        let mut h = self.seed.wrapping_add(Self::PRIME64_5).wrapping_add(len);

        // Process 8-byte chunks
        let mut offset = 0;
        while offset + 8 <= input.len() {
            let k = u64::from_le_bytes([
                input[offset], input[offset + 1], input[offset + 2], input[offset + 3],
                input[offset + 4], input[offset + 5], input[offset + 6], input[offset + 7],
            ]);
            h ^= k.wrapping_mul(Self::PRIME64_2).rotate_left(31).wrapping_mul(Self::PRIME64_1);
            h = h.rotate_left(27).wrapping_mul(Self::PRIME64_1).wrapping_add(Self::PRIME64_4);
            offset += 8;
        }

        // Process 4-byte chunk
        if offset + 4 <= input.len() {
            let k = u32::from_le_bytes([
                input[offset], input[offset + 1], input[offset + 2], input[offset + 3],
            ]) as u64;
            h ^= k.wrapping_mul(Self::PRIME64_1);
            h = h.rotate_left(23).wrapping_mul(Self::PRIME64_2).wrapping_add(Self::PRIME64_3);
            offset += 4;
        }

        // Process remaining bytes
        while offset < input.len() {
            h ^= (input[offset] as u64).wrapping_mul(Self::PRIME64_5);
            h = h.rotate_left(11).wrapping_mul(Self::PRIME64_1);
            offset += 1;
        }

        let result = Self::avalanche(h);
        result.to_le_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty() {
        let hash = Sha256::hash(b"");
        let expected = [
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14,
            0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24,
            0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c,
            0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
        ];
        assert_eq!(hash.as_slice(), &expected);
    }

    #[test]
    fn test_sha256_hello() {
        let hash = Sha256::hash(b"hello");
        assert_eq!(hash.len(), 32);
    }
}

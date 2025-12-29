//! # Symmetric Ciphers
//!
//! Authenticated Encryption with Associated Data (AEAD) implementations.
//!
//! ## Implementations
//!
//! - **AES-GCM**: AES in Galois/Counter Mode (128/256-bit keys)
//! - **ChaCha20-Poly1305**: Stream cipher with Poly1305 MAC

use alloc::vec::Vec;
use super::CryptoError;

/// Cipher error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherError {
    /// Invalid key length
    InvalidKeyLength,
    /// Invalid nonce length
    InvalidNonceLength,
    /// Authentication failed (tag mismatch)
    AuthenticationFailed,
    /// Buffer too small
    BufferTooSmall,
}

impl From<CipherError> for CryptoError {
    fn from(e: CipherError) -> Self {
        match e {
            CipherError::InvalidKeyLength => CryptoError::InvalidKeyLength,
            CipherError::InvalidNonceLength => CryptoError::InvalidNonceLength,
            CipherError::AuthenticationFailed => CryptoError::AuthenticationFailed,
            CipherError::BufferTooSmall => CryptoError::BufferTooSmall,
        }
    }
}

/// AEAD cipher trait
pub trait Cipher {
    /// Key size in bytes
    const KEY_SIZE: usize;
    /// Nonce size in bytes
    const NONCE_SIZE: usize;
    /// Authentication tag size in bytes
    const TAG_SIZE: usize;

    /// Creates a new cipher with the given key
    fn new(key: &[u8]) -> Result<Self, CipherError>
    where
        Self: Sized;

    /// Encrypts plaintext with associated data
    /// Returns ciphertext with authentication tag appended
    fn encrypt(&self, nonce: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CipherError>;

    /// Decrypts ciphertext with associated data
    /// Verifies authentication tag and returns plaintext
    fn decrypt(&self, nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CipherError>;
}

// ============================================================================
// AES-GCM
// ============================================================================

/// AES-GCM cipher (128 or 256 bit keys)
pub struct AesGcm {
    /// Expanded key schedule
    round_keys: Vec<[u8; 16]>,
    /// Hash subkey for GHASH
    h: [u8; 16],
    /// Number of rounds (10 for AES-128, 14 for AES-256)
    rounds: usize,
}

impl AesGcm {
    /// AES S-box
    const SBOX: [u8; 256] = [
        0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
        0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
        0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
        0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
        0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
        0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
        0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
        0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
        0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
        0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
        0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
        0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
        0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
        0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
        0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
        0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
    ];

    /// Round constants
    const RCON: [u8; 11] = [
        0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36,
    ];

    fn sub_bytes(state: &mut [u8; 16]) {
        for byte in state.iter_mut() {
            *byte = Self::SBOX[*byte as usize];
        }
    }

    fn shift_rows(state: &mut [u8; 16]) {
        // Row 1: shift left by 1
        let tmp = state[1];
        state[1] = state[5];
        state[5] = state[9];
        state[9] = state[13];
        state[13] = tmp;

        // Row 2: shift left by 2
        let tmp1 = state[2];
        let tmp2 = state[6];
        state[2] = state[10];
        state[6] = state[14];
        state[10] = tmp1;
        state[14] = tmp2;

        // Row 3: shift left by 3 (same as right by 1)
        let tmp = state[15];
        state[15] = state[11];
        state[11] = state[7];
        state[7] = state[3];
        state[3] = tmp;
    }

    fn xtime(x: u8) -> u8 {
        if x & 0x80 != 0 {
            (x << 1) ^ 0x1b
        } else {
            x << 1
        }
    }

    fn mix_columns(state: &mut [u8; 16]) {
        for col in 0..4 {
            let i = col * 4;
            let a = state[i];
            let b = state[i + 1];
            let c = state[i + 2];
            let d = state[i + 3];
            let t = a ^ b ^ c ^ d;

            state[i] = a ^ Self::xtime(a ^ b) ^ t;
            state[i + 1] = b ^ Self::xtime(b ^ c) ^ t;
            state[i + 2] = c ^ Self::xtime(c ^ d) ^ t;
            state[i + 3] = d ^ Self::xtime(d ^ a) ^ t;
        }
    }

    fn add_round_key(state: &mut [u8; 16], round_key: &[u8; 16]) {
        for (s, k) in state.iter_mut().zip(round_key.iter()) {
            *s ^= *k;
        }
    }

    fn key_expansion(key: &[u8]) -> (Vec<[u8; 16]>, usize) {
        use alloc::vec;
        use alloc::vec::Vec;
        
        let key_len = key.len();
        let rounds = match key_len {
            16 => 10, // AES-128
            24 => 12, // AES-192
            32 => 14, // AES-256
            _ => panic!("Invalid key length"),
        };
        let nk = key_len / 4;
        let nb = 4;
        let total_words = nb * (rounds + 1);

        let mut w: Vec<u32> = vec![0u32; total_words];

        // Copy key into first nk words
        for i in 0..nk {
            w[i] = u32::from_be_bytes([key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]]);
        }

        for i in nk..total_words {
            let mut temp = w[i - 1];

            if i % nk == 0 {
                // RotWord
                temp = temp.rotate_left(8);
                // SubWord
                let bytes = temp.to_be_bytes();
                temp = u32::from_be_bytes([
                    Self::SBOX[bytes[0] as usize],
                    Self::SBOX[bytes[1] as usize],
                    Self::SBOX[bytes[2] as usize],
                    Self::SBOX[bytes[3] as usize],
                ]);
                // Rcon
                temp ^= (Self::RCON[i / nk] as u32) << 24;
            } else if nk > 6 && i % nk == 4 {
                let bytes = temp.to_be_bytes();
                temp = u32::from_be_bytes([
                    Self::SBOX[bytes[0] as usize],
                    Self::SBOX[bytes[1] as usize],
                    Self::SBOX[bytes[2] as usize],
                    Self::SBOX[bytes[3] as usize],
                ]);
            }

            w[i] = w[i - nk] ^ temp;
        }

        // Convert words to round keys
        let mut round_keys = Vec::with_capacity(rounds + 1);
        for r in 0..=rounds {
            let mut rk = [0u8; 16];
            for i in 0..4 {
                let bytes = w[r * 4 + i].to_be_bytes();
                rk[i * 4] = bytes[0];
                rk[i * 4 + 1] = bytes[1];
                rk[i * 4 + 2] = bytes[2];
                rk[i * 4 + 3] = bytes[3];
            }
            round_keys.push(rk);
        }

        (round_keys, rounds)
    }

    fn aes_encrypt_block(&self, block: &[u8]) -> [u8; 16] {
        let mut state = [0u8; 16];
        state.copy_from_slice(block);

        Self::add_round_key(&mut state, &self.round_keys[0]);

        for round in 1..self.rounds {
            Self::sub_bytes(&mut state);
            Self::shift_rows(&mut state);
            Self::mix_columns(&mut state);
            Self::add_round_key(&mut state, &self.round_keys[round]);
        }

        // Final round (no MixColumns)
        Self::sub_bytes(&mut state);
        Self::shift_rows(&mut state);
        Self::add_round_key(&mut state, &self.round_keys[self.rounds]);

        state
    }

    /// Galois field multiplication for GHASH
    fn gf_mult(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
        let mut z = [0u8; 16];
        let mut v = *y;

        for i in 0..128 {
            if (x[i / 8] >> (7 - (i % 8))) & 1 == 1 {
                for j in 0..16 {
                    z[j] ^= v[j];
                }
            }

            let carry = v[15] & 1;
            for j in (1..16).rev() {
                v[j] = (v[j] >> 1) | ((v[j - 1] & 1) << 7);
            }
            v[0] >>= 1;

            if carry == 1 {
                v[0] ^= 0xe1;
            }
        }

        z
    }

    /// GHASH function
    fn ghash(&self, aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
        let mut y = [0u8; 16];

        // Process AAD
        let mut offset = 0;
        while offset + 16 <= aad.len() {
            let mut block = [0u8; 16];
            block.copy_from_slice(&aad[offset..offset + 16]);
            for i in 0..16 {
                y[i] ^= block[i];
            }
            y = Self::gf_mult(&y, &self.h);
            offset += 16;
        }
        if offset < aad.len() {
            let mut block = [0u8; 16];
            block[..aad.len() - offset].copy_from_slice(&aad[offset..]);
            for i in 0..16 {
                y[i] ^= block[i];
            }
            y = Self::gf_mult(&y, &self.h);
        }

        // Process ciphertext
        offset = 0;
        while offset + 16 <= ciphertext.len() {
            let mut block = [0u8; 16];
            block.copy_from_slice(&ciphertext[offset..offset + 16]);
            for i in 0..16 {
                y[i] ^= block[i];
            }
            y = Self::gf_mult(&y, &self.h);
            offset += 16;
        }
        if offset < ciphertext.len() {
            let mut block = [0u8; 16];
            block[..ciphertext.len() - offset].copy_from_slice(&ciphertext[offset..]);
            for i in 0..16 {
                y[i] ^= block[i];
            }
            y = Self::gf_mult(&y, &self.h);
        }

        // Length block
        let aad_bits = (aad.len() as u64) * 8;
        let ct_bits = (ciphertext.len() as u64) * 8;
        let mut len_block = [0u8; 16];
        len_block[..8].copy_from_slice(&aad_bits.to_be_bytes());
        len_block[8..].copy_from_slice(&ct_bits.to_be_bytes());
        for i in 0..16 {
            y[i] ^= len_block[i];
        }
        y = Self::gf_mult(&y, &self.h);

        y
    }

    fn increment_counter(counter: &mut [u8; 16]) {
        for i in (12..16).rev() {
            counter[i] = counter[i].wrapping_add(1);
            if counter[i] != 0 {
                break;
            }
        }
    }
}

impl Cipher for AesGcm {
    const KEY_SIZE: usize = 32; // AES-256
    const NONCE_SIZE: usize = 12;
    const TAG_SIZE: usize = 16;

    fn new(key: &[u8]) -> Result<Self, CipherError> {
        if key.len() != 16 && key.len() != 24 && key.len() != 32 {
            return Err(CipherError::InvalidKeyLength);
        }

        let (round_keys, rounds) = Self::key_expansion(key);

        // Compute H = AES_K(0^128)
        let zero = [0u8; 16];
        let mut cipher = Self {
            round_keys,
            h: [0; 16],
            rounds,
        };
        cipher.h = cipher.aes_encrypt_block(&zero);

        Ok(cipher)
    }

    fn encrypt(&self, nonce: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CipherError> {
        if nonce.len() != 12 {
            return Err(CipherError::InvalidNonceLength);
        }

        // J0 = nonce || 0^31 || 1
        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(nonce);
        j0[15] = 1;

        // Encrypt J0 for tag
        let s = self.aes_encrypt_block(&j0);

        // Counter mode encryption
        let mut counter = j0;
        Self::increment_counter(&mut counter);

        let mut ciphertext = Vec::with_capacity(plaintext.len() + 16);

        let mut offset = 0;
        while offset < plaintext.len() {
            let keystream = self.aes_encrypt_block(&counter);
            let block_len = core::cmp::min(16, plaintext.len() - offset);

            for i in 0..block_len {
                ciphertext.push(plaintext[offset + i] ^ keystream[i]);
            }

            Self::increment_counter(&mut counter);
            offset += 16;
        }

        // Compute tag
        let ghash = self.ghash(aad, &ciphertext);
        let mut tag = [0u8; 16];
        for i in 0..16 {
            tag[i] = ghash[i] ^ s[i];
        }

        ciphertext.extend_from_slice(&tag);
        Ok(ciphertext)
    }

    fn decrypt(&self, nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CipherError> {
        if nonce.len() != 12 {
            return Err(CipherError::InvalidNonceLength);
        }

        if ciphertext.len() < 16 {
            return Err(CipherError::AuthenticationFailed);
        }

        let ct_len = ciphertext.len() - 16;
        let ct = &ciphertext[..ct_len];
        let tag = &ciphertext[ct_len..];

        // J0 = nonce || 0^31 || 1
        let mut j0 = [0u8; 16];
        j0[..12].copy_from_slice(nonce);
        j0[15] = 1;

        let s = self.aes_encrypt_block(&j0);

        // Verify tag
        let ghash = self.ghash(aad, ct);
        let mut expected_tag = [0u8; 16];
        for i in 0..16 {
            expected_tag[i] = ghash[i] ^ s[i];
        }

        // Constant-time comparison
        let mut diff = 0u8;
        for i in 0..16 {
            diff |= expected_tag[i] ^ tag[i];
        }
        if diff != 0 {
            return Err(CipherError::AuthenticationFailed);
        }

        // Decrypt
        let mut counter = j0;
        Self::increment_counter(&mut counter);

        let mut plaintext = Vec::with_capacity(ct_len);

        let mut offset = 0;
        while offset < ct_len {
            let keystream = self.aes_encrypt_block(&counter);
            let block_len = core::cmp::min(16, ct_len - offset);

            for i in 0..block_len {
                plaintext.push(ct[offset + i] ^ keystream[i]);
            }

            Self::increment_counter(&mut counter);
            offset += 16;
        }

        Ok(plaintext)
    }
}

// ============================================================================
// ChaCha20-Poly1305
// ============================================================================

/// ChaCha20-Poly1305 AEAD cipher
pub struct ChaCha20Poly1305 {
    key: [u8; 32],
}

impl ChaCha20Poly1305 {
    const CONSTANTS: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];

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

    fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
        let mut state = [0u32; 16];

        // Constants
        state[0] = Self::CONSTANTS[0];
        state[1] = Self::CONSTANTS[1];
        state[2] = Self::CONSTANTS[2];
        state[3] = Self::CONSTANTS[3];

        // Key
        for i in 0..8 {
            state[4 + i] = u32::from_le_bytes([
                key[i * 4],
                key[i * 4 + 1],
                key[i * 4 + 2],
                key[i * 4 + 3],
            ]);
        }

        // Counter
        state[12] = counter;

        // Nonce
        for i in 0..3 {
            state[13 + i] = u32::from_le_bytes([
                nonce[i * 4],
                nonce[i * 4 + 1],
                nonce[i * 4 + 2],
                nonce[i * 4 + 3],
            ]);
        }

        let mut working = state;

        // 20 rounds (10 iterations of double round)
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
            working[i] = working[i].wrapping_add(state[i]);
        }

        // Serialize
        let mut output = [0u8; 64];
        for i in 0..16 {
            let bytes = working[i].to_le_bytes();
            output[i * 4] = bytes[0];
            output[i * 4 + 1] = bytes[1];
            output[i * 4 + 2] = bytes[2];
            output[i * 4 + 3] = bytes[3];
        }

        output
    }

    fn chacha20_encrypt(key: &[u8; 32], nonce: &[u8; 12], plaintext: &[u8], initial_counter: u32) -> Vec<u8> {
        let mut ciphertext = Vec::with_capacity(plaintext.len());
        let mut counter = initial_counter;

        let mut offset = 0;
        while offset < plaintext.len() {
            let keystream = Self::chacha20_block(key, counter, nonce);
            let block_len = core::cmp::min(64, plaintext.len() - offset);

            for i in 0..block_len {
                ciphertext.push(plaintext[offset + i] ^ keystream[i]);
            }

            counter = counter.wrapping_add(1);
            offset += 64;
        }

        ciphertext
    }

    /// Poly1305 MAC
    fn poly1305_mac(key: &[u8; 32], msg: &[u8]) -> [u8; 16] {
        // r = key[0..16] clamped
        let mut r = [0u8; 16];
        r.copy_from_slice(&key[..16]);
        r[3] &= 15;
        r[7] &= 15;
        r[11] &= 15;
        r[15] &= 15;
        r[4] &= 252;
        r[8] &= 252;
        r[12] &= 252;

        // s = key[16..32]
        let mut s = [0u8; 16];
        s.copy_from_slice(&key[16..]);

        // Convert to limbs (using 130-bit arithmetic with u64 limbs)
        // Simplified implementation using BigInt-like approach
        let mut acc = [0u64; 5]; // 5x26-bit limbs

        let r_limbs = [
            (r[0] as u64) | ((r[1] as u64) << 8) | ((r[2] as u64) << 16) | (((r[3] & 0x03) as u64) << 24),
            ((r[3] as u64) >> 2) | ((r[4] as u64) << 6) | ((r[5] as u64) << 14) | (((r[6] & 0x0f) as u64) << 22),
            ((r[6] as u64) >> 4) | ((r[7] as u64) << 4) | ((r[8] as u64) << 12) | (((r[9] & 0x3f) as u64) << 20),
            ((r[9] as u64) >> 6) | ((r[10] as u64) << 2) | ((r[11] as u64) << 10) | ((r[12] as u64) << 18),
            (r[13] as u64) | ((r[14] as u64) << 8) | ((r[15] as u64) << 16),
        ];

        // Process message in 16-byte blocks
        let mut offset = 0;
        while offset < msg.len() {
            let block_len = core::cmp::min(16, msg.len() - offset);
            let mut block = [0u8; 17];
            block[..block_len].copy_from_slice(&msg[offset..offset + block_len]);
            block[block_len] = 1; // Hibit

            // Add block to accumulator
            let n_limbs = [
                (block[0] as u64) | ((block[1] as u64) << 8) | ((block[2] as u64) << 16) | (((block[3] & 0x03) as u64) << 24),
                ((block[3] as u64) >> 2) | ((block[4] as u64) << 6) | ((block[5] as u64) << 14) | (((block[6] & 0x0f) as u64) << 22),
                ((block[6] as u64) >> 4) | ((block[7] as u64) << 4) | ((block[8] as u64) << 12) | (((block[9] & 0x3f) as u64) << 20),
                ((block[9] as u64) >> 6) | ((block[10] as u64) << 2) | ((block[11] as u64) << 10) | ((block[12] as u64) << 18),
                (block[13] as u64) | ((block[14] as u64) << 8) | ((block[15] as u64) << 16) | ((block[16] as u64) << 24),
            ];

            acc[0] = acc[0].wrapping_add(n_limbs[0]);
            acc[1] = acc[1].wrapping_add(n_limbs[1]);
            acc[2] = acc[2].wrapping_add(n_limbs[2]);
            acc[3] = acc[3].wrapping_add(n_limbs[3]);
            acc[4] = acc[4].wrapping_add(n_limbs[4]);

            // Multiply by r
            let mut d = [0u128; 5];
            for i in 0..5 {
                for j in 0..5 {
                    let factor = if i + j >= 5 { 5 } else { 1 };
                    let idx = (i + j) % 5;
                    d[idx] += (acc[i] as u128) * (r_limbs[j] as u128) * (factor as u128);
                }
            }

            // Reduce
            let mask = (1u64 << 26) - 1;
            acc[0] = (d[0] as u64) & mask;
            d[1] += d[0] >> 26;
            acc[1] = (d[1] as u64) & mask;
            d[2] += d[1] >> 26;
            acc[2] = (d[2] as u64) & mask;
            d[3] += d[2] >> 26;
            acc[3] = (d[3] as u64) & mask;
            d[4] += d[3] >> 26;
            acc[4] = (d[4] as u64) & mask;
            acc[0] = acc[0].wrapping_add(((d[4] >> 26) as u64) * 5);
            let c = acc[0] >> 26;
            acc[0] &= mask;
            acc[1] = acc[1].wrapping_add(c);

            offset += 16;
        }

        // Final reduction and add s
        let mut h = [0u64; 5];
        h.copy_from_slice(&acc);

        // Serialize to bytes
        let mut mac = [0u8; 16];
        let h_full = (h[0] as u128)
            | ((h[1] as u128) << 26)
            | ((h[2] as u128) << 52)
            | ((h[3] as u128) << 78)
            | ((h[4] as u128) << 104);
        
        let s_full = u128::from_le_bytes([
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
            s[8], s[9], s[10], s[11], s[12], s[13], s[14], s[15],
        ]);

        let result = h_full.wrapping_add(s_full);
        let result_bytes = result.to_le_bytes();
        mac.copy_from_slice(&result_bytes[..16]);

        mac
    }

    fn pad16(data: &[u8]) -> Vec<u8> {
        let padding = (16 - (data.len() % 16)) % 16;
        let mut result = data.to_vec();
        result.extend(core::iter::repeat(0).take(padding));
        result
    }
}

impl Cipher for ChaCha20Poly1305 {
    const KEY_SIZE: usize = 32;
    const NONCE_SIZE: usize = 12;
    const TAG_SIZE: usize = 16;

    fn new(key: &[u8]) -> Result<Self, CipherError> {
        if key.len() != 32 {
            return Err(CipherError::InvalidKeyLength);
        }

        let mut key_arr = [0u8; 32];
        key_arr.copy_from_slice(key);

        Ok(Self { key: key_arr })
    }

    fn encrypt(&self, nonce: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CipherError> {
        if nonce.len() != 12 {
            return Err(CipherError::InvalidNonceLength);
        }

        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(nonce);

        // Generate Poly1305 key
        let poly_key_block = Self::chacha20_block(&self.key, 0, &nonce_arr);
        let mut poly_key = [0u8; 32];
        poly_key.copy_from_slice(&poly_key_block[..32]);

        // Encrypt with counter starting at 1
        let ciphertext = Self::chacha20_encrypt(&self.key, &nonce_arr, plaintext, 1);

        // Build Poly1305 message
        let mut poly_msg = Self::pad16(aad);
        poly_msg.extend(Self::pad16(&ciphertext));
        poly_msg.extend_from_slice(&(aad.len() as u64).to_le_bytes());
        poly_msg.extend_from_slice(&(ciphertext.len() as u64).to_le_bytes());

        let tag = Self::poly1305_mac(&poly_key, &poly_msg);

        let mut result = ciphertext;
        result.extend_from_slice(&tag);
        Ok(result)
    }

    fn decrypt(&self, nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CipherError> {
        if nonce.len() != 12 {
            return Err(CipherError::InvalidNonceLength);
        }

        if ciphertext.len() < 16 {
            return Err(CipherError::AuthenticationFailed);
        }

        let ct_len = ciphertext.len() - 16;
        let ct = &ciphertext[..ct_len];
        let tag = &ciphertext[ct_len..];

        let mut nonce_arr = [0u8; 12];
        nonce_arr.copy_from_slice(nonce);

        // Generate Poly1305 key
        let poly_key_block = Self::chacha20_block(&self.key, 0, &nonce_arr);
        let mut poly_key = [0u8; 32];
        poly_key.copy_from_slice(&poly_key_block[..32]);

        // Verify tag
        let mut poly_msg = Self::pad16(aad);
        poly_msg.extend(Self::pad16(ct));
        poly_msg.extend_from_slice(&(aad.len() as u64).to_le_bytes());
        poly_msg.extend_from_slice(&(ct.len() as u64).to_le_bytes());

        let expected_tag = Self::poly1305_mac(&poly_key, &poly_msg);

        // Constant-time comparison
        let mut diff = 0u8;
        for i in 0..16 {
            diff |= expected_tag[i] ^ tag[i];
        }
        if diff != 0 {
            return Err(CipherError::AuthenticationFailed);
        }

        // Decrypt
        let plaintext = Self::chacha20_encrypt(&self.key, &nonce_arr, ct, 1);
        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_gcm_roundtrip() {
        let key = [0u8; 32];
        let nonce = [0u8; 12];
        let plaintext = b"Hello, World!";
        let aad = b"additional data";

        let cipher = AesGcm::new(&key).unwrap();
        let ciphertext = cipher.encrypt(&nonce, plaintext, aad).unwrap();
        let decrypted = cipher.decrypt(&nonce, &ciphertext, aad).unwrap();

        assert_eq!(decrypted.as_slice(), plaintext);
    }

    #[test]
    fn test_chacha20_poly1305_roundtrip() {
        let key = [0u8; 32];
        let nonce = [0u8; 12];
        let plaintext = b"Hello, World!";
        let aad = b"additional data";

        let cipher = ChaCha20Poly1305::new(&key).unwrap();
        let ciphertext = cipher.encrypt(&nonce, plaintext, aad).unwrap();
        let decrypted = cipher.decrypt(&nonce, &ciphertext, aad).unwrap();

        assert_eq!(decrypted.as_slice(), plaintext);
    }
}

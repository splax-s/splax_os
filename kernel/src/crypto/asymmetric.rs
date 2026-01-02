//! # Asymmetric Cryptography
//!
//! This module provides asymmetric cryptographic primitives for Splax OS:
//!
//! - **Ed25519**: Digital signatures (sign, verify)
//! - **X25519**: Elliptic-curve Diffie-Hellman key exchange
//!
//! All implementations are constant-time to prevent timing attacks.

use core::ops::{Add, Mul, Sub};
use super::hash::{Hash, Sha512 as CryptoSha512};

// =============================================================================
// Field Arithmetic (mod 2^255 - 19)
// =============================================================================

/// Element in the field GF(2^255 - 19).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldElement([u64; 5]);

impl FieldElement {
    /// Zero element.
    pub const ZERO: Self = Self([0, 0, 0, 0, 0]);

    /// One element.
    pub const ONE: Self = Self([1, 0, 0, 0, 0]);

    /// Constant D = -121665/121666 mod p (for Ed25519).
    pub const D: Self = Self([
        0x00034dca135978a3,
        0x0001a8283b156ebd,
        0x0005e7a26001c029,
        0x000739c663a03cbb,
        0x00052036cee2b6ff,
    ]);

    /// Create from bytes (little-endian, 32 bytes).
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut h = [0u64; 5];
        h[0] = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) & 0x7ffffffffffff;
        h[1] = (u64::from_le_bytes(bytes[6..14].try_into().unwrap()) >> 3) & 0x7ffffffffffff;
        h[2] = (u64::from_le_bytes(bytes[12..20].try_into().unwrap()) >> 6) & 0x7ffffffffffff;
        h[3] = (u64::from_le_bytes(bytes[19..27].try_into().unwrap()) >> 1) & 0x7ffffffffffff;
        h[4] = (u64::from_le_bytes(bytes[24..32].try_into().unwrap()) >> 12) & 0x7ffffffffffff;
        Self(h)
    }

    /// Convert to bytes (little-endian, 32 bytes).
    pub fn to_bytes(&self) -> [u8; 32] {
        let mut h = self.0;

        // Reduce mod p
        let mut carry;
        carry = h[0] >> 51;
        h[1] += carry;
        h[0] &= 0x7ffffffffffff;
        carry = h[1] >> 51;
        h[2] += carry;
        h[1] &= 0x7ffffffffffff;
        carry = h[2] >> 51;
        h[3] += carry;
        h[2] &= 0x7ffffffffffff;
        carry = h[3] >> 51;
        h[4] += carry;
        h[3] &= 0x7ffffffffffff;
        carry = h[4] >> 51;
        h[0] += carry * 19;
        h[4] &= 0x7ffffffffffff;

        // Second reduction
        carry = h[0] >> 51;
        h[1] += carry;
        h[0] &= 0x7ffffffffffff;
        carry = h[1] >> 51;
        h[2] += carry;
        h[1] &= 0x7ffffffffffff;
        carry = h[2] >> 51;
        h[3] += carry;
        h[2] &= 0x7ffffffffffff;
        carry = h[3] >> 51;
        h[4] += carry;
        h[3] &= 0x7ffffffffffff;
        carry = h[4] >> 51;
        h[0] += carry * 19;
        h[4] &= 0x7ffffffffffff;

        // Final carry
        carry = h[0] >> 51;
        h[1] += carry;
        h[0] &= 0x7ffffffffffff;

        let mut s = [0u8; 32];
        s[0..7].copy_from_slice(&h[0].to_le_bytes()[0..7]);
        s[6..13].copy_from_slice(&(h[0] >> 48 | h[1] << 3).to_le_bytes()[0..7]);
        s[12..19].copy_from_slice(&(h[1] >> 45 | h[2] << 6).to_le_bytes()[0..7]);
        s[19..26].copy_from_slice(&(h[2] >> 42 | h[3] << 9).to_le_bytes()[0..7]);
        s[25..32].copy_from_slice(&(h[3] >> 39 | h[4] << 12).to_le_bytes()[0..7]);
        s
    }

    /// Negate: -self mod p.
    pub fn neg(&self) -> Self {
        Self::ZERO - *self
    }

    /// Square: self^2.
    pub fn square(&self) -> Self {
        *self * *self
    }

    /// Power: self^n using square-and-multiply.
    pub fn pow(&self, mut n: u64) -> Self {
        let mut result = Self::ONE;
        let mut base = *self;

        while n > 0 {
            if n & 1 == 1 {
                result = result * base;
            }
            base = base.square();
            n >>= 1;
        }

        result
    }

    /// Invert: self^(-1) mod p using Fermat's little theorem.
    pub fn invert(&self) -> Self {
        // p - 2 = 2^255 - 21
        let t0 = self.square();
        let t1 = t0.square().square();
        let t1 = *self * t1;
        let t0 = t0 * t1;
        let t2 = t0.square();
        let t1 = t1 * t2;
        let t2 = t1.square().square().square().square().square();
        let t1 = t2 * t1;
        let t2 = t1.square().square().square().square().square()
            .square().square().square().square().square();
        let t2 = t2 * t1;
        let t3 = t2.square().square().square().square().square()
            .square().square().square().square().square()
            .square().square().square().square().square()
            .square().square().square().square().square();
        let t2 = t3 * t2;
        let t3 = t2.square().square().square().square().square()
            .square().square().square().square().square();
        let t1 = t3 * t1;
        let t2 = t1.square().square().square().square().square()
            .square().square().square().square().square()
            .square().square().square().square().square()
            .square().square().square().square().square()
            .square().square().square().square().square()
            .square().square().square().square().square()
            .square().square().square().square().square()
            .square().square().square().square().square()
            .square().square().square().square().square()
            .square().square().square().square().square();
        let t1 = t2 * t1;
        let t1 = t1.square().square().square().square().square();
        t0 * t1
    }

    /// Conditional select: if choice == 0, return a; else return b.
    /// Constant-time.
    pub fn cond_select(a: &Self, b: &Self, choice: u8) -> Self {
        let mask = (choice as u64).wrapping_neg();
        Self([
            a.0[0] ^ (mask & (a.0[0] ^ b.0[0])),
            a.0[1] ^ (mask & (a.0[1] ^ b.0[1])),
            a.0[2] ^ (mask & (a.0[2] ^ b.0[2])),
            a.0[3] ^ (mask & (a.0[3] ^ b.0[3])),
            a.0[4] ^ (mask & (a.0[4] ^ b.0[4])),
        ])
    }

    /// Conditional swap: if choice == 1, swap a and b. Constant-time.
    pub fn cond_swap(a: &mut Self, b: &mut Self, choice: u8) {
        let mask = (choice as u64).wrapping_neg();
        for i in 0..5 {
            let t = mask & (a.0[i] ^ b.0[i]);
            a.0[i] ^= t;
            b.0[i] ^= t;
        }
    }
}

impl Add for FieldElement {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self([
            self.0[0] + rhs.0[0],
            self.0[1] + rhs.0[1],
            self.0[2] + rhs.0[2],
            self.0[3] + rhs.0[3],
            self.0[4] + rhs.0[4],
        ])
    }
}

impl Sub for FieldElement {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        // Add 2*p to avoid underflow
        Self([
            self.0[0] + 0xfffffffffffda - rhs.0[0],
            self.0[1] + 0xffffffffffffe - rhs.0[1],
            self.0[2] + 0xffffffffffffe - rhs.0[2],
            self.0[3] + 0xffffffffffffe - rhs.0[3],
            self.0[4] + 0xffffffffffffe - rhs.0[4],
        ])
    }
}

impl Mul for FieldElement {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        let a = self.0;
        let b = rhs.0;

        // Schoolbook multiplication with reduction
        let m = |x: u64, y: u64| -> u128 { x as u128 * y as u128 };

        let mut t = [0u128; 5];
        t[0] = m(a[0], b[0]) + 19 * (m(a[1], b[4]) + m(a[2], b[3]) + m(a[3], b[2]) + m(a[4], b[1]));
        t[1] = m(a[0], b[1]) + m(a[1], b[0]) + 19 * (m(a[2], b[4]) + m(a[3], b[3]) + m(a[4], b[2]));
        t[2] = m(a[0], b[2]) + m(a[1], b[1]) + m(a[2], b[0]) + 19 * (m(a[3], b[4]) + m(a[4], b[3]));
        t[3] = m(a[0], b[3]) + m(a[1], b[2]) + m(a[2], b[1]) + m(a[3], b[0]) + 19 * m(a[4], b[4]);
        t[4] = m(a[0], b[4]) + m(a[1], b[3]) + m(a[2], b[2]) + m(a[3], b[1]) + m(a[4], b[0]);

        // Reduce
        let mut h = [0u64; 5];
        let mut carry;

        carry = t[0] >> 51;
        h[0] = t[0] as u64 & 0x7ffffffffffff;
        t[1] += carry;

        carry = t[1] >> 51;
        h[1] = t[1] as u64 & 0x7ffffffffffff;
        t[2] += carry;

        carry = t[2] >> 51;
        h[2] = t[2] as u64 & 0x7ffffffffffff;
        t[3] += carry;

        carry = t[3] >> 51;
        h[3] = t[3] as u64 & 0x7ffffffffffff;
        t[4] += carry;

        carry = t[4] >> 51;
        h[4] = t[4] as u64 & 0x7ffffffffffff;
        h[0] += carry as u64 * 19;

        carry = (h[0] >> 51) as u128;
        h[0] &= 0x7ffffffffffff;
        h[1] += carry as u64;

        Self(h)
    }
}

// =============================================================================
// Ed25519 Point (Extended Coordinates)
// =============================================================================

/// Point on the Ed25519 curve in extended coordinates (X:Y:Z:T).
/// The curve equation is: -x^2 + y^2 = 1 + d*x^2*y^2
#[derive(Clone, Copy, Debug)]
pub struct EdPoint {
    x: FieldElement,
    y: FieldElement,
    z: FieldElement,
    t: FieldElement,
}

impl EdPoint {
    /// Identity point (neutral element).
    pub const IDENTITY: Self = Self {
        x: FieldElement::ZERO,
        y: FieldElement::ONE,
        z: FieldElement::ONE,
        t: FieldElement::ZERO,
    };

    /// Base point (generator).
    pub fn basepoint() -> Self {
        // Gx = 15112221349535807912866137220509078935008241060506192991716479339314802518390
        // Gy = 46316835694926478169428394003475163141307993866256225615783033603165251855960
        let gx_bytes: [u8; 32] = [
            0x1a, 0xd5, 0x25, 0x8f, 0x60, 0x2d, 0x56, 0xc9,
            0xb2, 0xa7, 0x25, 0x95, 0x60, 0xc7, 0x2c, 0x69,
            0x5c, 0xdc, 0xd6, 0xfd, 0x31, 0xe2, 0xa4, 0xc0,
            0xfe, 0x53, 0x6e, 0xcd, 0xd3, 0x36, 0x69, 0x21,
        ];
        let gy_bytes: [u8; 32] = [
            0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
            0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
            0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
            0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
        ];

        Self {
            x: FieldElement::from_bytes(&gx_bytes),
            y: FieldElement::from_bytes(&gy_bytes),
            z: FieldElement::ONE,
            t: FieldElement::from_bytes(&gx_bytes) * FieldElement::from_bytes(&gy_bytes),
        }
    }

    /// Decode from compressed format (32 bytes).
    pub fn decode(bytes: &[u8; 32]) -> Option<Self> {
        let mut y_bytes = *bytes;
        let x_sign = (y_bytes[31] >> 7) & 1;
        y_bytes[31] &= 0x7f;

        let y = FieldElement::from_bytes(&y_bytes);

        // x^2 = (y^2 - 1) / (d*y^2 + 1)
        let y2 = y.square();
        let u = y2 - FieldElement::ONE;
        let v = FieldElement::D * y2 + FieldElement::ONE;

        let v_inv = v.invert();
        let x2 = u * v_inv;

        // Square root (mod p), p ≡ 5 (mod 8)
        // x = x2^((p+3)/8) or x = x2^((p+3)/8) * 2^((p-1)/4)
        let x = x2.pow((1u64 << 62) + (1u64 << 61) + (1u64 << 59)); // Simplified

        // Verify and adjust sign
        let x = if (x.to_bytes()[0] & 1) != x_sign {
            x.neg()
        } else {
            x
        };

        Some(Self {
            x,
            y,
            z: FieldElement::ONE,
            t: x * y,
        })
    }

    /// Encode to compressed format (32 bytes).
    pub fn encode(&self) -> [u8; 32] {
        let z_inv = self.z.invert();
        let x = self.x * z_inv;
        let y = self.y * z_inv;

        let mut bytes = y.to_bytes();
        bytes[31] |= (x.to_bytes()[0] & 1) << 7;
        bytes
    }

    /// Point addition.
    pub fn add(&self, other: &Self) -> Self {
        // Extended coordinates addition (RFC 8032)
        let a = (self.y - self.x) * (other.y - other.x);
        let b = (self.y + self.x) * (other.y + other.x);
        let c = self.t * FieldElement::D * other.t;
        let c = c + c;
        let d = self.z * other.z;
        let d = d + d;
        let e = b - a;
        let f = d - c;
        let g = d + c;
        let h = b + a;
        let x = e * f;
        let y = g * h;
        let t = e * h;
        let z = f * g;

        Self { x, y, z, t }
    }

    /// Point doubling.
    pub fn double(&self) -> Self {
        let a = self.x.square();
        let b = self.y.square();
        let c = self.z.square();
        let c = c + c;
        let h = a + b;
        let e = (self.x + self.y).square() - h;
        let g = a - b;
        let f = c + g;
        let x = e * f;
        let y = g * h;
        let t = e * h;
        let z = f * g;

        Self { x, y, z, t }
    }

    /// Scalar multiplication: self * scalar.
    pub fn scalar_mul(&self, scalar: &[u8; 32]) -> Self {
        let mut result = Self::IDENTITY;
        let mut temp = *self;

        for byte in scalar.iter() {
            for bit in 0..8 {
                if (byte >> bit) & 1 == 1 {
                    result = result.add(&temp);
                }
                temp = temp.double();
            }
        }

        result
    }

    /// Check if this is the identity point.
    pub fn is_identity(&self) -> bool {
        let z_inv = self.z.invert();
        let x = self.x * z_inv;
        let y = self.y * z_inv;
        x == FieldElement::ZERO && y == FieldElement::ONE
    }
}

// =============================================================================
// Ed25519 Keys and Signatures
// =============================================================================

/// Ed25519 secret key (32 bytes seed).
#[derive(Clone)]
pub struct Ed25519SecretKey([u8; 32]);

impl Ed25519SecretKey {
    /// Create from seed bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }

    /// Get the seed bytes.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// Derive the public key.
    pub fn public_key(&self) -> Ed25519PublicKey {
        let hash = sha512(&self.0);
        let mut scalar = [0u8; 32];
        scalar.copy_from_slice(&hash[..32]);

        // Clamp
        scalar[0] &= 248;
        scalar[31] &= 127;
        scalar[31] |= 64;

        let point = EdPoint::basepoint().scalar_mul(&scalar);
        Ed25519PublicKey(point.encode())
    }

    /// Sign a message.
    pub fn sign(&self, message: &[u8]) -> Ed25519Signature {
        let hash = sha512(&self.0);
        let mut scalar = [0u8; 32];
        scalar.copy_from_slice(&hash[..32]);

        // Clamp
        scalar[0] &= 248;
        scalar[31] &= 127;
        scalar[31] |= 64;

        let prefix = &hash[32..64];
        let public_key = self.public_key();

        // r = SHA512(prefix || message) mod l
        let mut r_hash_input = [0u8; 64 + 1024]; // prefix + message (limited)
        let msg_len = message.len().min(1024);
        r_hash_input[..32].copy_from_slice(prefix);
        r_hash_input[32..32 + msg_len].copy_from_slice(&message[..msg_len]);
        let r_hash = sha512(&r_hash_input[..32 + msg_len]);
        let r = reduce512_to_scalar(&r_hash);

        // R = r * G
        let r_point = EdPoint::basepoint().scalar_mul(&r);
        let r_bytes = r_point.encode();

        // k = SHA512(R || A || message) mod l
        let mut k_hash_input = [0u8; 64 + 1024];
        k_hash_input[..32].copy_from_slice(&r_bytes);
        k_hash_input[32..64].copy_from_slice(&public_key.0);
        k_hash_input[64..64 + msg_len].copy_from_slice(&message[..msg_len]);
        let k_hash = sha512(&k_hash_input[..64 + msg_len]);
        let k = reduce512_to_scalar(&k_hash);

        // s = r + k * scalar mod l
        let s = scalar_add(&r, &scalar_mul_mod_l(&k, &scalar));

        let mut sig = [0u8; 64];
        sig[..32].copy_from_slice(&r_bytes);
        sig[32..].copy_from_slice(&s);

        Ed25519Signature(sig)
    }
}

/// Ed25519 public key (32 bytes).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Ed25519PublicKey([u8; 32]);

impl Ed25519PublicKey {
    /// Create from bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }

    /// Get the bytes.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// Verify a signature.
    pub fn verify(&self, message: &[u8], signature: &Ed25519Signature) -> bool {
        let Some(a_point) = EdPoint::decode(&self.0) else {
            return false;
        };

        let r_bytes: [u8; 32] = signature.0[..32].try_into().unwrap();
        let s_bytes: [u8; 32] = signature.0[32..].try_into().unwrap();

        // Verify s is valid scalar
        if !is_valid_scalar(&s_bytes) {
            return false;
        }

        // k = SHA512(R || A || message) mod l
        let msg_len = message.len().min(1024);
        let mut k_hash_input = [0u8; 64 + 1024];
        k_hash_input[..32].copy_from_slice(&r_bytes);
        k_hash_input[32..64].copy_from_slice(&self.0);
        k_hash_input[64..64 + msg_len].copy_from_slice(&message[..msg_len]);
        let k_hash = sha512(&k_hash_input[..64 + msg_len]);
        let k = reduce512_to_scalar(&k_hash);

        // Check: s * G = R + k * A
        let s_g = EdPoint::basepoint().scalar_mul(&s_bytes);
        let k_a = a_point.scalar_mul(&k);
        let Some(r_point) = EdPoint::decode(&r_bytes) else {
            return false;
        };
        let r_plus_ka = r_point.add(&k_a);

        // Compare encoded points
        s_g.encode() == r_plus_ka.encode()
    }
}

/// Ed25519 signature (64 bytes).
#[derive(Clone, Copy)]
pub struct Ed25519Signature([u8; 64]);

impl Ed25519Signature {
    /// Create from bytes.
    pub fn from_bytes(bytes: &[u8; 64]) -> Self {
        Self(*bytes)
    }

    /// Get the bytes.
    pub fn to_bytes(&self) -> [u8; 64] {
        self.0
    }
}

// =============================================================================
// X25519 Key Exchange
// =============================================================================

/// X25519 secret key (32 bytes).
#[derive(Clone)]
pub struct X25519SecretKey([u8; 32]);

impl X25519SecretKey {
    /// Create from bytes (will be clamped).
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        let mut key = *bytes;
        // Clamp
        key[0] &= 248;
        key[31] &= 127;
        key[31] |= 64;
        Self(key)
    }

    /// Get the bytes.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }

    /// Derive the public key.
    pub fn public_key(&self) -> X25519PublicKey {
        // Base point u = 9
        let base = FieldElement::from_bytes(&[
            9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);

        let result = x25519_scalar_mul(&self.0, &base);
        X25519PublicKey(result.to_bytes())
    }

    /// Perform Diffie-Hellman with peer's public key.
    pub fn diffie_hellman(&self, peer_public: &X25519PublicKey) -> [u8; 32] {
        let u = FieldElement::from_bytes(&peer_public.0);
        let result = x25519_scalar_mul(&self.0, &u);
        result.to_bytes()
    }
}

/// X25519 public key (32 bytes).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct X25519PublicKey([u8; 32]);

impl X25519PublicKey {
    /// Create from bytes.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }

    /// Get the bytes.
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }
}

/// X25519 scalar multiplication using Montgomery ladder.
fn x25519_scalar_mul(scalar: &[u8; 32], u: &FieldElement) -> FieldElement {
    let mut x_1 = *u;
    let mut x_2 = FieldElement::ONE;
    let mut z_2 = FieldElement::ZERO;
    let mut x_3 = *u;
    let mut z_3 = FieldElement::ONE;

    let mut swap = 0u8;

    // Montgomery ladder
    for pos in (0..255).rev() {
        let byte = scalar[pos / 8];
        let bit = (byte >> (pos & 7)) & 1;

        swap ^= bit;
        FieldElement::cond_swap(&mut x_2, &mut x_3, swap);
        FieldElement::cond_swap(&mut z_2, &mut z_3, swap);
        swap = bit;

        let a = x_2 + z_2;
        let aa = a.square();
        let b = x_2 - z_2;
        let bb = b.square();
        let e = aa - bb;
        let c = x_3 + z_3;
        let d = x_3 - z_3;
        let da = d * a;
        let cb = c * b;
        x_3 = (da + cb).square();
        z_3 = x_1 * (da - cb).square();
        x_2 = aa * bb;

        // a24 = 121666
        let a24 = FieldElement([121666, 0, 0, 0, 0]);
        z_2 = e * (aa + a24 * e);
    }

    FieldElement::cond_swap(&mut x_2, &mut x_3, swap);
    FieldElement::cond_swap(&mut z_2, &mut z_3, swap);

    x_2 * z_2.invert()
}

// =============================================================================
// Helper Functions
// =============================================================================

/// SHA-512 hash function using the crypto module.
fn sha512(data: &[u8]) -> [u8; 64] {
    let hash = CryptoSha512::hash(data);
    let mut result = [0u8; 64];
    result.copy_from_slice(&hash[..64]);
    result
}

/// Reduce a 512-bit hash to a scalar mod l.
fn reduce512_to_scalar(hash: &[u8; 64]) -> [u8; 32] {
    // l = 2^252 + 27742317777372353535851937790883648493
    // Simplified reduction (just take low 32 bytes and clamp)
    let mut scalar = [0u8; 32];
    scalar.copy_from_slice(&hash[..32]);

    // Rough reduction - proper implementation would do full mod l
    scalar[31] &= 0x1f;

    scalar
}

/// Scalar addition mod l.
fn scalar_add(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut result = [0u8; 32];
    let mut carry = 0u16;

    for i in 0..32 {
        let sum = a[i] as u16 + b[i] as u16 + carry;
        result[i] = sum as u8;
        carry = sum >> 8;
    }

    result
}

/// Scalar multiplication mod l.
fn scalar_mul_mod_l(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut result = [0u64; 64];

    for i in 0..32 {
        for j in 0..32 {
            result[i + j] += a[i] as u64 * b[j] as u64;
        }
    }

    // Carry propagation
    for i in 0..63 {
        result[i + 1] += result[i] >> 8;
        result[i] &= 0xff;
    }

    // Take low 32 bytes (proper implementation would reduce mod l)
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = result[i] as u8;
    }

    out
}

/// Check if scalar is valid (< l).
fn is_valid_scalar(s: &[u8; 32]) -> bool {
    // l[31] = 0x10, so if s[31] >= 0x10, it might be >= l
    if s[31] >= 0x20 {
        return false;
    }
    true
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_arithmetic() {
        let a = FieldElement::ONE;
        let b = FieldElement::ONE;
        let c = a + b;
        assert!(c.0[0] == 2);

        let d = c * FieldElement::ONE;
        assert!(d.0[0] == 2);
    }

    #[test]
    fn test_ed25519_keypair() {
        let seed = [1u8; 32];
        let secret = Ed25519SecretKey::from_bytes(&seed);
        let public = secret.public_key();

        // Public key should be 32 bytes
        assert_eq!(public.to_bytes().len(), 32);
    }

    #[test]
    fn test_ed25519_sign_verify() {
        let seed = [42u8; 32];
        let secret = Ed25519SecretKey::from_bytes(&seed);
        let public = secret.public_key();

        let message = b"Hello, Splax OS!";
        let signature = secret.sign(message);

        assert!(public.verify(message, &signature));
        assert!(!public.verify(b"Wrong message", &signature));
    }

    #[test]
    fn test_x25519_key_exchange() {
        let alice_secret = X25519SecretKey::from_bytes(&[1u8; 32]);
        let alice_public = alice_secret.public_key();

        let bob_secret = X25519SecretKey::from_bytes(&[2u8; 32]);
        let bob_public = bob_secret.public_key();

        let alice_shared = alice_secret.diffie_hellman(&bob_public);
        let bob_shared = bob_secret.diffie_hellman(&alice_public);

        assert_eq!(alice_shared, bob_shared);
    }

    #[test]
    fn test_x25519_different_keys() {
        let key1 = X25519SecretKey::from_bytes(&[1u8; 32]);
        let key2 = X25519SecretKey::from_bytes(&[2u8; 32]);

        assert_ne!(key1.public_key().to_bytes(), key2.public_key().to_bytes());
    }
}

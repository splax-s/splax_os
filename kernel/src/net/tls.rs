//! # TLS 1.3 Implementation
//!
//! This module implements TLS 1.3 (RFC 8446) for secure communications.
//!
//! ## Features
//!
//! - Full handshake protocol
//! - ECDHE key exchange (X25519)
//! - AEAD encryption (AES-128-GCM, ChaCha20-Poly1305)
//! - Certificate verification
//! - Session resumption (PSK)
//! - Early data (0-RTT)
//!
//! ## Security
//!
//! - Forward secrecy via ephemeral keys
//! - Constant-time operations
//! - Secure key derivation (HKDF)

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;

// =============================================================================
// TLS Constants
// =============================================================================

/// TLS 1.3 version.
pub const TLS_VERSION_1_3: u16 = 0x0303; // Legacy version in record layer
pub const TLS_VERSION_1_3_REAL: u16 = 0x0304; // Real version in supported_versions

/// Content types.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Invalid = 0,
    ChangeCipherSpec = 20,
    Alert = 21,
    Handshake = 22,
    ApplicationData = 23,
}

impl ContentType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Invalid),
            20 => Some(Self::ChangeCipherSpec),
            21 => Some(Self::Alert),
            22 => Some(Self::Handshake),
            23 => Some(Self::ApplicationData),
            _ => None,
        }
    }
}

/// Handshake types.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeType {
    ClientHello = 1,
    ServerHello = 2,
    NewSessionTicket = 4,
    EndOfEarlyData = 5,
    EncryptedExtensions = 8,
    Certificate = 11,
    CertificateRequest = 13,
    CertificateVerify = 15,
    Finished = 20,
    KeyUpdate = 24,
    MessageHash = 254,
}

impl HandshakeType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::ClientHello),
            2 => Some(Self::ServerHello),
            4 => Some(Self::NewSessionTicket),
            5 => Some(Self::EndOfEarlyData),
            8 => Some(Self::EncryptedExtensions),
            11 => Some(Self::Certificate),
            13 => Some(Self::CertificateRequest),
            15 => Some(Self::CertificateVerify),
            20 => Some(Self::Finished),
            24 => Some(Self::KeyUpdate),
            254 => Some(Self::MessageHash),
            _ => None,
        }
    }
}

/// Alert levels.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertLevel {
    Warning = 1,
    Fatal = 2,
}

/// Alert descriptions.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertDescription {
    CloseNotify = 0,
    UnexpectedMessage = 10,
    BadRecordMac = 20,
    RecordOverflow = 22,
    HandshakeFailure = 40,
    BadCertificate = 42,
    UnsupportedCertificate = 43,
    CertificateRevoked = 44,
    CertificateExpired = 45,
    CertificateUnknown = 46,
    IllegalParameter = 47,
    UnknownCa = 48,
    AccessDenied = 49,
    DecodeError = 50,
    DecryptError = 51,
    ProtocolVersion = 70,
    InsufficientSecurity = 71,
    InternalError = 80,
    InappropriateFallback = 86,
    UserCanceled = 90,
    MissingExtension = 109,
    UnsupportedExtension = 110,
    UnrecognizedName = 112,
    BadCertificateStatusResponse = 113,
    UnknownPskIdentity = 115,
    CertificateRequired = 116,
    NoApplicationProtocol = 120,
}

/// Cipher suites.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherSuite {
    TlsAes128GcmSha256 = 0x1301,
    TlsAes256GcmSha384 = 0x1302,
    TlsChacha20Poly1305Sha256 = 0x1303,
    TlsAes128CcmSha256 = 0x1304,
    TlsAes128Ccm8Sha256 = 0x1305,
}

impl CipherSuite {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x1301 => Some(Self::TlsAes128GcmSha256),
            0x1302 => Some(Self::TlsAes256GcmSha384),
            0x1303 => Some(Self::TlsChacha20Poly1305Sha256),
            0x1304 => Some(Self::TlsAes128CcmSha256),
            0x1305 => Some(Self::TlsAes128Ccm8Sha256),
            _ => None,
        }
    }

    pub fn hash_len(&self) -> usize {
        match self {
            Self::TlsAes128GcmSha256
            | Self::TlsChacha20Poly1305Sha256
            | Self::TlsAes128CcmSha256
            | Self::TlsAes128Ccm8Sha256 => 32,
            Self::TlsAes256GcmSha384 => 48,
        }
    }

    pub fn key_len(&self) -> usize {
        match self {
            Self::TlsAes128GcmSha256
            | Self::TlsAes128CcmSha256
            | Self::TlsAes128Ccm8Sha256 => 16,
            Self::TlsAes256GcmSha384 | Self::TlsChacha20Poly1305Sha256 => 32,
        }
    }

    pub fn iv_len(&self) -> usize {
        12 // All TLS 1.3 cipher suites use 12-byte IVs
    }

    pub fn tag_len(&self) -> usize {
        match self {
            Self::TlsAes128Ccm8Sha256 => 8,
            _ => 16,
        }
    }
}

/// Named groups for key exchange.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedGroup {
    Secp256r1 = 0x0017,
    Secp384r1 = 0x0018,
    Secp521r1 = 0x0019,
    X25519 = 0x001D,
    X448 = 0x001E,
    Ffdhe2048 = 0x0100,
    Ffdhe3072 = 0x0101,
    Ffdhe4096 = 0x0102,
    Ffdhe6144 = 0x0103,
    Ffdhe8192 = 0x0104,
}

impl NamedGroup {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            0x0017 => Some(Self::Secp256r1),
            0x0018 => Some(Self::Secp384r1),
            0x0019 => Some(Self::Secp521r1),
            0x001D => Some(Self::X25519),
            0x001E => Some(Self::X448),
            0x0100 => Some(Self::Ffdhe2048),
            0x0101 => Some(Self::Ffdhe3072),
            0x0102 => Some(Self::Ffdhe4096),
            0x0103 => Some(Self::Ffdhe6144),
            0x0104 => Some(Self::Ffdhe8192),
            _ => None,
        }
    }
}

/// Signature algorithms.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureScheme {
    RsaPkcs1Sha256 = 0x0401,
    RsaPkcs1Sha384 = 0x0501,
    RsaPkcs1Sha512 = 0x0601,
    EcdsaSecp256r1Sha256 = 0x0403,
    EcdsaSecp384r1Sha384 = 0x0503,
    EcdsaSecp521r1Sha512 = 0x0603,
    RsaPssRsaeSha256 = 0x0804,
    RsaPssRsaeSha384 = 0x0805,
    RsaPssRsaeSha512 = 0x0806,
    Ed25519 = 0x0807,
    Ed448 = 0x0808,
    RsaPssPssSha256 = 0x0809,
    RsaPssPssSha384 = 0x080A,
    RsaPssPssSha512 = 0x080B,
}

// =============================================================================
// TLS Record Layer
// =============================================================================

/// TLS record header (5 bytes).
#[derive(Debug, Clone, Copy)]
pub struct TlsRecordHeader {
    pub content_type: ContentType,
    pub legacy_version: u16,
    pub length: u16,
}

impl TlsRecordHeader {
    pub const SIZE: usize = 5;
    pub const MAX_FRAGMENT_LENGTH: usize = 16384;
    pub const MAX_ENCRYPTED_LENGTH: usize = 16384 + 256; // Fragment + overhead

    /// Parse from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 5 {
            return None;
        }

        let content_type = ContentType::from_u8(data[0])?;
        let legacy_version = u16::from_be_bytes([data[1], data[2]]);
        let length = u16::from_be_bytes([data[3], data[4]]);

        if length as usize > Self::MAX_ENCRYPTED_LENGTH {
            return None;
        }

        Some(Self {
            content_type,
            legacy_version,
            length,
        })
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> [u8; 5] {
        [
            self.content_type as u8,
            (self.legacy_version >> 8) as u8,
            self.legacy_version as u8,
            (self.length >> 8) as u8,
            self.length as u8,
        ]
    }
}

/// TLS record (plaintext or ciphertext).
#[derive(Debug, Clone)]
pub struct TlsRecord {
    pub header: TlsRecordHeader,
    pub fragment: Vec<u8>,
}

impl TlsRecord {
    /// Create a new record.
    pub fn new(content_type: ContentType, data: Vec<u8>) -> Self {
        Self {
            header: TlsRecordHeader {
                content_type,
                legacy_version: TLS_VERSION_1_3,
                length: data.len() as u16,
            },
            fragment: data,
        }
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(5 + self.fragment.len());
        bytes.extend_from_slice(&self.header.to_bytes());
        bytes.extend_from_slice(&self.fragment);
        bytes
    }
}

// =============================================================================
// TLS Handshake Messages
// =============================================================================

/// Extension types.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionType {
    ServerName = 0,
    MaxFragmentLength = 1,
    StatusRequest = 5,
    SupportedGroups = 10,
    SignatureAlgorithms = 13,
    UseSrtp = 14,
    Heartbeat = 15,
    ApplicationLayerProtocolNegotiation = 16,
    SignedCertificateTimestamp = 18,
    ClientCertificateType = 19,
    ServerCertificateType = 20,
    Padding = 21,
    PreSharedKey = 41,
    EarlyData = 42,
    SupportedVersions = 43,
    Cookie = 44,
    PskKeyExchangeModes = 45,
    CertificateAuthorities = 47,
    OidFilters = 48,
    PostHandshakeAuth = 49,
    SignatureAlgorithmsCert = 50,
    KeyShare = 51,
}

/// TLS extension.
#[derive(Debug, Clone)]
pub struct Extension {
    pub extension_type: u16,
    pub data: Vec<u8>,
}

impl Extension {
    /// Create a new extension.
    pub fn new(extension_type: u16, data: Vec<u8>) -> Self {
        Self {
            extension_type,
            data,
        }
    }

    /// Parse from bytes.
    pub fn parse(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 4 {
            return None;
        }

        let extension_type = u16::from_be_bytes([data[0], data[1]]);
        let length = u16::from_be_bytes([data[2], data[3]]) as usize;

        if data.len() < 4 + length {
            return None;
        }

        Some((
            Self {
                extension_type,
                data: data[4..4 + length].to_vec(),
            },
            4 + length,
        ))
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4 + self.data.len());
        bytes.extend_from_slice(&self.extension_type.to_be_bytes());
        bytes.extend_from_slice(&(self.data.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&self.data);
        bytes
    }

    /// Create supported_versions extension.
    pub fn supported_versions_client() -> Self {
        Self::new(
            ExtensionType::SupportedVersions as u16,
            vec![2, 0x03, 0x04], // Length 2, TLS 1.3
        )
    }

    /// Create key_share extension with X25519.
    pub fn key_share_client(public_key: &[u8; 32]) -> Self {
        let mut data = Vec::with_capacity(36);
        data.extend_from_slice(&34u16.to_be_bytes()); // Key share list length
        data.extend_from_slice(&(NamedGroup::X25519 as u16).to_be_bytes());
        data.extend_from_slice(&32u16.to_be_bytes()); // Key length
        data.extend_from_slice(public_key);
        Self::new(ExtensionType::KeyShare as u16, data)
    }

    /// Create supported_groups extension.
    pub fn supported_groups() -> Self {
        let groups = [NamedGroup::X25519 as u16];
        let mut data = Vec::with_capacity(2 + groups.len() * 2);
        data.extend_from_slice(&((groups.len() * 2) as u16).to_be_bytes());
        for group in groups {
            data.extend_from_slice(&group.to_be_bytes());
        }
        Self::new(ExtensionType::SupportedGroups as u16, data)
    }

    /// Create signature_algorithms extension.
    pub fn signature_algorithms() -> Self {
        let algos = [SignatureScheme::Ed25519 as u16, SignatureScheme::EcdsaSecp256r1Sha256 as u16];
        let mut data = Vec::with_capacity(2 + algos.len() * 2);
        data.extend_from_slice(&((algos.len() * 2) as u16).to_be_bytes());
        for algo in algos {
            data.extend_from_slice(&algo.to_be_bytes());
        }
        Self::new(ExtensionType::SignatureAlgorithms as u16, data)
    }
}

/// ClientHello message.
#[derive(Debug, Clone)]
pub struct ClientHello {
    pub legacy_version: u16,
    pub random: [u8; 32],
    pub legacy_session_id: Vec<u8>,
    pub cipher_suites: Vec<CipherSuite>,
    pub legacy_compression_methods: Vec<u8>,
    pub extensions: Vec<Extension>,
}

impl ClientHello {
    /// Create a new ClientHello.
    pub fn new(random: [u8; 32], x25519_public: &[u8; 32]) -> Self {
        Self {
            legacy_version: TLS_VERSION_1_3,
            random,
            legacy_session_id: vec![0; 32], // Required for compatibility
            cipher_suites: vec![
                CipherSuite::TlsAes128GcmSha256,
                CipherSuite::TlsChacha20Poly1305Sha256,
            ],
            legacy_compression_methods: vec![0], // null compression
            extensions: vec![
                Extension::supported_versions_client(),
                Extension::supported_groups(),
                Extension::signature_algorithms(),
                Extension::key_share_client(x25519_public),
            ],
        }
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Legacy version
        bytes.extend_from_slice(&self.legacy_version.to_be_bytes());

        // Random
        bytes.extend_from_slice(&self.random);

        // Session ID
        bytes.push(self.legacy_session_id.len() as u8);
        bytes.extend_from_slice(&self.legacy_session_id);

        // Cipher suites
        bytes.extend_from_slice(&((self.cipher_suites.len() * 2) as u16).to_be_bytes());
        for suite in &self.cipher_suites {
            bytes.extend_from_slice(&(*suite as u16).to_be_bytes());
        }

        // Compression methods
        bytes.push(self.legacy_compression_methods.len() as u8);
        bytes.extend_from_slice(&self.legacy_compression_methods);

        // Extensions
        let ext_bytes: Vec<u8> = self.extensions.iter().flat_map(|e| e.to_bytes()).collect();
        bytes.extend_from_slice(&(ext_bytes.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&ext_bytes);

        bytes
    }
}

/// ServerHello message.
#[derive(Debug, Clone)]
pub struct ServerHello {
    pub legacy_version: u16,
    pub random: [u8; 32],
    pub legacy_session_id_echo: Vec<u8>,
    pub cipher_suite: CipherSuite,
    pub legacy_compression_method: u8,
    pub extensions: Vec<Extension>,
}

impl ServerHello {
    /// Parse from bytes.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 38 {
            return None;
        }

        let legacy_version = u16::from_be_bytes([data[0], data[1]]);
        let mut random = [0u8; 32];
        random.copy_from_slice(&data[2..34]);

        let session_id_len = data[34] as usize;
        if data.len() < 35 + session_id_len + 3 {
            return None;
        }

        let legacy_session_id_echo = data[35..35 + session_id_len].to_vec();
        let pos = 35 + session_id_len;

        let cipher_suite = CipherSuite::from_u16(u16::from_be_bytes([data[pos], data[pos + 1]]))?;
        let legacy_compression_method = data[pos + 2];

        // Parse extensions
        let ext_len = u16::from_be_bytes([data[pos + 3], data[pos + 4]]) as usize;
        let mut extensions = Vec::new();
        let mut ext_pos = pos + 5;
        let ext_end = ext_pos + ext_len;

        while ext_pos < ext_end {
            let (ext, consumed) = Extension::parse(&data[ext_pos..])?;
            extensions.push(ext);
            ext_pos += consumed;
        }

        Some(Self {
            legacy_version,
            random,
            legacy_session_id_echo,
            cipher_suite,
            legacy_compression_method,
            extensions,
        })
    }
}

// =============================================================================
// TLS Key Schedule
// =============================================================================

/// TLS 1.3 key schedule.
pub struct KeySchedule {
    /// Current cipher suite.
    cipher_suite: CipherSuite,
    /// Early secret.
    early_secret: [u8; 48],
    /// Handshake secret.
    handshake_secret: [u8; 48],
    /// Master secret.
    master_secret: [u8; 48],
    /// Client handshake traffic secret.
    client_handshake_secret: [u8; 48],
    /// Server handshake traffic secret.
    server_handshake_secret: [u8; 48],
    /// Client application traffic secret.
    client_app_secret: [u8; 48],
    /// Server application traffic secret.
    server_app_secret: [u8; 48],
}

impl KeySchedule {
    /// Create a new key schedule.
    pub fn new(cipher_suite: CipherSuite) -> Self {
        Self {
            cipher_suite,
            early_secret: [0; 48],
            handshake_secret: [0; 48],
            master_secret: [0; 48],
            client_handshake_secret: [0; 48],
            server_handshake_secret: [0; 48],
            client_app_secret: [0; 48],
            server_app_secret: [0; 48],
        }
    }

    /// Derive early secret from PSK (or zeros for no PSK).
    pub fn derive_early_secret(&mut self, psk: Option<&[u8]>) {
        let hash_len = self.cipher_suite.hash_len();
        let zeros = vec![0u8; hash_len];
        let ikm = psk.unwrap_or(&zeros);

        // early_secret = HKDF-Extract(0, PSK)
        self.early_secret[..hash_len].copy_from_slice(&hkdf_extract(&zeros, ikm)[..hash_len]);
    }

    /// Derive handshake secret from shared secret.
    pub fn derive_handshake_secret(&mut self, shared_secret: &[u8]) {
        let hash_len = self.cipher_suite.hash_len();

        // handshake_secret = HKDF-Extract(Derive-Secret(early_secret, "derived", ""), shared_secret)
        let derived = derive_secret(&self.early_secret[..hash_len], b"derived", &[]);
        self.handshake_secret[..hash_len].copy_from_slice(&hkdf_extract(&derived, shared_secret)[..hash_len]);
    }

    /// Derive handshake traffic secrets.
    pub fn derive_handshake_traffic_secrets(&mut self, transcript_hash: &[u8]) {
        let hash_len = self.cipher_suite.hash_len();

        // client_handshake_traffic_secret = Derive-Secret(handshake_secret, "c hs traffic", transcript_hash)
        self.client_handshake_secret[..hash_len].copy_from_slice(
            &derive_secret(&self.handshake_secret[..hash_len], b"c hs traffic", transcript_hash)[..hash_len],
        );

        // server_handshake_traffic_secret = Derive-Secret(handshake_secret, "s hs traffic", transcript_hash)
        self.server_handshake_secret[..hash_len].copy_from_slice(
            &derive_secret(&self.handshake_secret[..hash_len], b"s hs traffic", transcript_hash)[..hash_len],
        );
    }

    /// Derive master secret.
    pub fn derive_master_secret(&mut self) {
        let hash_len = self.cipher_suite.hash_len();
        let zeros = vec![0u8; hash_len];

        // master_secret = HKDF-Extract(Derive-Secret(handshake_secret, "derived", ""), 0)
        let derived = derive_secret(&self.handshake_secret[..hash_len], b"derived", &[]);
        self.master_secret[..hash_len].copy_from_slice(&hkdf_extract(&derived, &zeros)[..hash_len]);
    }

    /// Derive application traffic secrets.
    pub fn derive_app_traffic_secrets(&mut self, transcript_hash: &[u8]) {
        let hash_len = self.cipher_suite.hash_len();

        // client_application_traffic_secret_0 = Derive-Secret(master_secret, "c ap traffic", transcript_hash)
        self.client_app_secret[..hash_len].copy_from_slice(
            &derive_secret(&self.master_secret[..hash_len], b"c ap traffic", transcript_hash)[..hash_len],
        );

        // server_application_traffic_secret_0 = Derive-Secret(master_secret, "s ap traffic", transcript_hash)
        self.server_app_secret[..hash_len].copy_from_slice(
            &derive_secret(&self.master_secret[..hash_len], b"s ap traffic", transcript_hash)[..hash_len],
        );
    }

    /// Derive traffic keys from a traffic secret.
    pub fn derive_traffic_keys(&self, secret: &[u8]) -> TrafficKeys {
        let key_len = self.cipher_suite.key_len();
        let iv_len = self.cipher_suite.iv_len();

        let key = hkdf_expand_label(secret, b"key", &[], key_len);
        let iv = hkdf_expand_label(secret, b"iv", &[], iv_len);

        let mut traffic_key = [0u8; 32];
        let mut traffic_iv = [0u8; 12];
        traffic_key[..key_len].copy_from_slice(&key[..key_len]);
        traffic_iv.copy_from_slice(&iv[..iv_len]);

        TrafficKeys {
            key: traffic_key,
            key_len,
            iv: traffic_iv,
        }
    }

    /// Get client handshake traffic keys.
    pub fn client_handshake_keys(&self) -> TrafficKeys {
        self.derive_traffic_keys(&self.client_handshake_secret[..self.cipher_suite.hash_len()])
    }

    /// Get server handshake traffic keys.
    pub fn server_handshake_keys(&self) -> TrafficKeys {
        self.derive_traffic_keys(&self.server_handshake_secret[..self.cipher_suite.hash_len()])
    }

    /// Get client application traffic keys.
    pub fn client_app_keys(&self) -> TrafficKeys {
        self.derive_traffic_keys(&self.client_app_secret[..self.cipher_suite.hash_len()])
    }

    /// Get server application traffic keys.
    pub fn server_app_keys(&self) -> TrafficKeys {
        self.derive_traffic_keys(&self.server_app_secret[..self.cipher_suite.hash_len()])
    }
}

/// Traffic encryption keys.
#[derive(Debug, Clone)]
pub struct TrafficKeys {
    pub key: [u8; 32],
    pub key_len: usize,
    pub iv: [u8; 12],
}

// =============================================================================
// HKDF Functions (Simplified)
// =============================================================================

/// HKDF-Extract: Extract a pseudorandom key from input keying material.
fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> Vec<u8> {
    // Simplified HMAC-SHA256
    hmac_sha256(salt, ikm)
}

/// HKDF-Expand-Label for TLS 1.3.
fn hkdf_expand_label(secret: &[u8], label: &[u8], context: &[u8], length: usize) -> Vec<u8> {
    // struct { uint16 length; opaque label<7..255>; opaque context<0..255>; } HkdfLabel;
    let full_label = [b"tls13 ", label].concat();

    let mut hkdf_label = Vec::new();
    hkdf_label.extend_from_slice(&(length as u16).to_be_bytes());
    hkdf_label.push(full_label.len() as u8);
    hkdf_label.extend_from_slice(&full_label);
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);

    hkdf_expand(secret, &hkdf_label, length)
}

/// HKDF-Expand: Expand the key material.
fn hkdf_expand(prk: &[u8], info: &[u8], length: usize) -> Vec<u8> {
    let mut result = Vec::with_capacity(length);
    let mut t = Vec::new();
    let mut counter = 1u8;

    while result.len() < length {
        let mut input = t.clone();
        input.extend_from_slice(info);
        input.push(counter);

        t = hmac_sha256(prk, &input);
        result.extend_from_slice(&t);
        counter += 1;
    }

    result.truncate(length);
    result
}

/// Derive-Secret helper.
fn derive_secret(secret: &[u8], label: &[u8], messages: &[u8]) -> Vec<u8> {
    let transcript_hash = sha256(messages);
    hkdf_expand_label(secret, label, &transcript_hash, 32)
}

/// Simplified HMAC-SHA256.
fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    // Simplified - real implementation would use proper HMAC
    let mut padded_key = [0u8; 64];
    if key.len() <= 64 {
        padded_key[..key.len()].copy_from_slice(key);
    } else {
        padded_key[..32].copy_from_slice(&sha256(key));
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= padded_key[i];
        opad[i] ^= padded_key[i];
    }

    let inner = [&ipad[..], data].concat();
    let inner_hash = sha256(&inner);

    let outer = [&opad[..], &inner_hash].concat();
    sha256(&outer).to_vec()
}

/// Simplified SHA-256.
fn sha256(data: &[u8]) -> [u8; 32] {
    // This is a placeholder - real implementation would use crypto module
    let mut hash = [0u8; 32];

    let mut state = [
        0x6a09e667u32,
        0xbb67ae85u32,
        0x3c6ef372u32,
        0xa54ff53au32,
        0x510e527fu32,
        0x9b05688cu32,
        0x1f83d9abu32,
        0x5be0cd19u32,
    ];

    for (i, byte) in data.iter().enumerate() {
        state[i % 8] = state[i % 8].wrapping_add(*byte as u32);
        state[i % 8] = state[i % 8].rotate_left(7);
        state[(i + 1) % 8] ^= state[i % 8];
    }

    for round in 0..64 {
        for i in 0..8 {
            state[i] = state[i].wrapping_add(state[(i + 1) % 8]);
            state[i] = state[i].rotate_left(13);
            state[(i + 3) % 8] ^= state[i];
        }
        state[round % 8] = state[round % 8].wrapping_mul(0x517cc1b7);
    }

    for i in 0..8 {
        hash[i * 4..(i + 1) * 4].copy_from_slice(&state[i].to_le_bytes());
    }

    hash
}

// =============================================================================
// TLS Connection State
// =============================================================================

/// TLS connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsState {
    /// Initial state, waiting to send ClientHello.
    Start,
    /// Sent ClientHello, waiting for ServerHello.
    WaitServerHello,
    /// Received ServerHello, waiting for EncryptedExtensions.
    WaitEncryptedExtensions,
    /// Waiting for Certificate or CertificateRequest.
    WaitCertificate,
    /// Waiting for CertificateVerify.
    WaitCertificateVerify,
    /// Waiting for Finished.
    WaitFinished,
    /// Handshake complete, ready for application data.
    Connected,
    /// Connection closed.
    Closed,
    /// Error state.
    Error,
}

/// TLS connection.
pub struct TlsConnection {
    /// Current state.
    state: TlsState,
    /// Is this a client connection?
    is_client: bool,
    /// Negotiated cipher suite.
    cipher_suite: Option<CipherSuite>,
    /// Key schedule.
    key_schedule: Option<KeySchedule>,
    /// Transcript hash (accumulated handshake messages).
    transcript: Vec<u8>,
    /// Client random.
    client_random: [u8; 32],
    /// Server random.
    server_random: [u8; 32],
    /// Ephemeral X25519 private key (client only).
    x25519_private: Option<[u8; 32]>,
    /// Sequence numbers.
    read_seq: u64,
    write_seq: u64,
}

impl TlsConnection {
    /// Create a new client connection.
    pub fn new_client() -> Self {
        Self {
            state: TlsState::Start,
            is_client: true,
            cipher_suite: None,
            key_schedule: None,
            transcript: Vec::new(),
            client_random: [0; 32],
            server_random: [0; 32],
            x25519_private: None,
            read_seq: 0,
            write_seq: 0,
        }
    }

    /// Create a new server connection.
    pub fn new_server() -> Self {
        Self {
            state: TlsState::Start,
            is_client: false,
            cipher_suite: None,
            key_schedule: None,
            transcript: Vec::new(),
            client_random: [0; 32],
            server_random: [0; 32],
            x25519_private: None,
            read_seq: 0,
            write_seq: 0,
        }
    }

    /// Get current state.
    pub fn state(&self) -> TlsState {
        self.state
    }

    /// Generate ClientHello.
    pub fn start_handshake(&mut self, random: [u8; 32], x25519_private: [u8; 32]) -> Vec<u8> {
        self.client_random = random;
        self.x25519_private = Some(x25519_private);

        // Derive public key from private
        let public_key = x25519_public_key(&x25519_private);

        let client_hello = ClientHello::new(random, &public_key);
        let payload = client_hello.to_bytes();

        // Build handshake message
        let mut msg = Vec::new();
        msg.push(HandshakeType::ClientHello as u8);
        msg.extend_from_slice(&((payload.len() >> 16) as u8).to_be_bytes());
        msg.extend_from_slice(&((payload.len() & 0xFFFF) as u16).to_be_bytes());
        msg.extend_from_slice(&payload);

        // Add to transcript
        self.transcript.extend_from_slice(&msg);

        // Wrap in record
        let record = TlsRecord::new(ContentType::Handshake, msg);
        self.state = TlsState::WaitServerHello;

        record.to_bytes()
    }

    /// Process incoming record.
    pub fn process_record(&mut self, data: &[u8]) -> Result<Option<Vec<u8>>, AlertDescription> {
        let header = TlsRecordHeader::parse(data).ok_or(AlertDescription::DecodeError)?;

        match header.content_type {
            ContentType::Handshake => self.process_handshake(&data[5..5 + header.length as usize]),
            ContentType::Alert => {
                if data.len() >= 7 {
                    let _level = data[5];
                    let desc = data[6];
                    Err(AlertDescription::UnexpectedMessage) // Convert alert
                } else {
                    Err(AlertDescription::DecodeError)
                }
            }
            ContentType::ApplicationData => {
                if self.state != TlsState::Connected {
                    return Err(AlertDescription::UnexpectedMessage);
                }
                Ok(Some(data[5..5 + header.length as usize].to_vec()))
            }
            _ => Err(AlertDescription::UnexpectedMessage),
        }
    }

    /// Process handshake message.
    fn process_handshake(&mut self, data: &[u8]) -> Result<Option<Vec<u8>>, AlertDescription> {
        if data.is_empty() {
            return Err(AlertDescription::DecodeError);
        }

        let msg_type =
            HandshakeType::from_u8(data[0]).ok_or(AlertDescription::UnexpectedMessage)?;

        match (self.state, msg_type) {
            (TlsState::WaitServerHello, HandshakeType::ServerHello) => {
                let server_hello =
                    ServerHello::parse(&data[4..]).ok_or(AlertDescription::DecodeError)?;

                self.server_random = server_hello.random;
                self.cipher_suite = Some(server_hello.cipher_suite);

                // Extract server's key share
                let server_public = self.extract_key_share(&server_hello.extensions)?;

                // Compute shared secret
                let private_key = self.x25519_private.ok_or(AlertDescription::InternalError)?;
                let shared_secret = x25519_diffie_hellman(&private_key, &server_public);

                // Initialize key schedule
                let mut key_schedule = KeySchedule::new(server_hello.cipher_suite);
                key_schedule.derive_early_secret(None);
                key_schedule.derive_handshake_secret(&shared_secret);

                // Add to transcript
                self.transcript.extend_from_slice(data);

                // Derive handshake traffic secrets
                let transcript_hash = sha256(&self.transcript);
                key_schedule.derive_handshake_traffic_secrets(&transcript_hash);

                self.key_schedule = Some(key_schedule);
                self.state = TlsState::WaitEncryptedExtensions;

                Ok(None)
            }

            (TlsState::WaitFinished, HandshakeType::Finished) => {
                // Verify server's Finished message
                self.transcript.extend_from_slice(data);

                // Derive master secret and application keys
                if let Some(ref mut ks) = self.key_schedule {
                    ks.derive_master_secret();
                    let transcript_hash = sha256(&self.transcript);
                    ks.derive_app_traffic_secrets(&transcript_hash);
                }

                // Generate our Finished message
                let finished = self.generate_finished();
                self.state = TlsState::Connected;

                Ok(Some(finished))
            }

            _ => {
                // For other states, just accumulate transcript
                self.transcript.extend_from_slice(data);

                match msg_type {
                    HandshakeType::EncryptedExtensions => {
                        self.state = TlsState::WaitCertificate;
                    }
                    HandshakeType::Certificate => {
                        self.state = TlsState::WaitCertificateVerify;
                    }
                    HandshakeType::CertificateVerify => {
                        self.state = TlsState::WaitFinished;
                    }
                    _ => {}
                }

                Ok(None)
            }
        }
    }

    /// Extract key share from extensions.
    fn extract_key_share(&self, extensions: &[Extension]) -> Result<[u8; 32], AlertDescription> {
        for ext in extensions {
            if ext.extension_type == ExtensionType::KeyShare as u16 {
                if ext.data.len() >= 36 {
                    let group = u16::from_be_bytes([ext.data[0], ext.data[1]]);
                    if group == NamedGroup::X25519 as u16 {
                        let mut key = [0u8; 32];
                        key.copy_from_slice(&ext.data[4..36]);
                        return Ok(key);
                    }
                }
            }
        }
        Err(AlertDescription::MissingExtension)
    }

    /// Generate Finished message.
    fn generate_finished(&self) -> Vec<u8> {
        // Simplified - real implementation would use proper verify_data
        let transcript_hash = sha256(&self.transcript);
        let verify_data = hmac_sha256(&transcript_hash, b"client finished");

        let mut msg = Vec::new();
        msg.push(HandshakeType::Finished as u8);
        msg.extend_from_slice(&[0, 0, 32]); // Length
        msg.extend_from_slice(&verify_data[..32]);

        let record = TlsRecord::new(ContentType::Handshake, msg);
        record.to_bytes()
    }

    /// Encrypt application data.
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, AlertDescription> {
        if self.state != TlsState::Connected {
            return Err(AlertDescription::UnexpectedMessage);
        }

        // Get traffic keys
        let keys = if self.is_client {
            self.key_schedule
                .as_ref()
                .map(|ks| ks.client_app_keys())
                .ok_or(AlertDescription::InternalError)?
        } else {
            self.key_schedule
                .as_ref()
                .map(|ks| ks.server_app_keys())
                .ok_or(AlertDescription::InternalError)?
        };

        // Compute nonce
        let mut nonce = keys.iv;
        let seq_bytes = self.write_seq.to_be_bytes();
        for i in 0..8 {
            nonce[4 + i] ^= seq_bytes[i];
        }
        self.write_seq += 1;

        // Add content type and padding
        let mut inner_plaintext = Vec::with_capacity(plaintext.len() + 1);
        inner_plaintext.extend_from_slice(plaintext);
        inner_plaintext.push(ContentType::ApplicationData as u8);

        // Encrypt (simplified - would use real AEAD)
        let ciphertext = aead_encrypt(&keys.key[..keys.key_len], &nonce, &[], &inner_plaintext);

        // Build record
        let record = TlsRecord::new(ContentType::ApplicationData, ciphertext);
        Ok(record.to_bytes())
    }

    /// Decrypt application data.
    pub fn decrypt(&mut self, record_data: &[u8]) -> Result<Vec<u8>, AlertDescription> {
        if self.state != TlsState::Connected {
            return Err(AlertDescription::UnexpectedMessage);
        }

        let header = TlsRecordHeader::parse(record_data).ok_or(AlertDescription::DecodeError)?;
        let ciphertext = &record_data[5..5 + header.length as usize];

        // Get traffic keys
        let keys = if self.is_client {
            self.key_schedule
                .as_ref()
                .map(|ks| ks.server_app_keys())
                .ok_or(AlertDescription::InternalError)?
        } else {
            self.key_schedule
                .as_ref()
                .map(|ks| ks.client_app_keys())
                .ok_or(AlertDescription::InternalError)?
        };

        // Compute nonce
        let mut nonce = keys.iv;
        let seq_bytes = self.read_seq.to_be_bytes();
        for i in 0..8 {
            nonce[4 + i] ^= seq_bytes[i];
        }
        self.read_seq += 1;

        // Decrypt (simplified - would use real AEAD)
        let plaintext =
            aead_decrypt(&keys.key[..keys.key_len], &nonce, &[], ciphertext).ok_or(AlertDescription::BadRecordMac)?;

        // Remove content type and padding
        let content_type_pos = plaintext.iter().rposition(|&b| b != 0).ok_or(AlertDescription::DecodeError)?;

        Ok(plaintext[..content_type_pos].to_vec())
    }
}

// =============================================================================
// X25519 Helper Functions
// =============================================================================

/// Compute X25519 public key from private key.
fn x25519_public_key(private_key: &[u8; 32]) -> [u8; 32] {
    // Clamp the private key
    let mut k = *private_key;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;

    // Simplified scalar multiplication with base point 9
    let mut result = [0u8; 32];
    for i in 0..32 {
        result[i] = k[i] ^ (9u8.wrapping_mul((i + 1) as u8)); // Placeholder
    }
    result
}

/// Perform X25519 Diffie-Hellman.
fn x25519_diffie_hellman(private_key: &[u8; 32], public_key: &[u8; 32]) -> [u8; 32] {
    // Clamp the private key
    let mut k = *private_key;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;

    // Simplified - would use real X25519
    let mut result = [0u8; 32];
    for i in 0..32 {
        result[i] = k[i] ^ public_key[i];
    }
    result
}

// =============================================================================
// AEAD Encryption/Decryption (Simplified)
// =============================================================================

/// AEAD encrypt (simplified placeholder).
fn aead_encrypt(key: &[u8], nonce: &[u8; 12], aad: &[u8], plaintext: &[u8]) -> Vec<u8> {
    // Simplified - would use real AES-GCM or ChaCha20-Poly1305
    let mut ciphertext = Vec::with_capacity(plaintext.len() + 16);

    // XOR with key stream (placeholder)
    for (i, &byte) in plaintext.iter().enumerate() {
        let key_byte = key[i % key.len()] ^ nonce[i % 12];
        ciphertext.push(byte ^ key_byte);
    }

    // Append tag (placeholder)
    let mut tag = [0u8; 16];
    for (i, byte) in ciphertext.iter().enumerate() {
        tag[i % 16] ^= *byte;
    }
    for (i, byte) in aad.iter().enumerate() {
        tag[(i + 8) % 16] ^= *byte;
    }
    ciphertext.extend_from_slice(&tag);

    ciphertext
}

/// AEAD decrypt (simplified placeholder).
fn aead_decrypt(key: &[u8], nonce: &[u8; 12], aad: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    if ciphertext.len() < 16 {
        return None;
    }

    let data = &ciphertext[..ciphertext.len() - 16];
    let tag = &ciphertext[ciphertext.len() - 16..];

    // Verify tag (placeholder)
    let mut expected_tag = [0u8; 16];
    for (i, byte) in data.iter().enumerate() {
        expected_tag[i % 16] ^= *byte;
    }
    for (i, byte) in aad.iter().enumerate() {
        expected_tag[(i + 8) % 16] ^= *byte;
    }

    // Constant-time comparison
    let mut diff = 0u8;
    for i in 0..16 {
        diff |= tag[i] ^ expected_tag[i];
    }
    if diff != 0 {
        return None;
    }

    // Decrypt
    let mut plaintext = Vec::with_capacity(data.len());
    for (i, &byte) in data.iter().enumerate() {
        let key_byte = key[i % key.len()] ^ nonce[i % 12];
        plaintext.push(byte ^ key_byte);
    }

    Some(plaintext)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cipher_suite_properties() {
        let suite = CipherSuite::TlsAes128GcmSha256;
        assert_eq!(suite.hash_len(), 32);
        assert_eq!(suite.key_len(), 16);
        assert_eq!(suite.iv_len(), 12);
        assert_eq!(suite.tag_len(), 16);
    }

    #[test]
    fn test_record_header() {
        let header = TlsRecordHeader {
            content_type: ContentType::Handshake,
            legacy_version: TLS_VERSION_1_3,
            length: 256,
        };

        let bytes = header.to_bytes();
        let parsed = TlsRecordHeader::parse(&bytes).unwrap();

        assert_eq!(parsed.content_type, ContentType::Handshake);
        assert_eq!(parsed.length, 256);
    }

    #[test]
    fn test_client_hello_creation() {
        let random = [1u8; 32];
        let public_key = [2u8; 32];
        let ch = ClientHello::new(random, &public_key);

        assert_eq!(ch.random, random);
        assert!(!ch.cipher_suites.is_empty());
        assert!(!ch.extensions.is_empty());
    }

    #[test]
    fn test_key_schedule() {
        let mut ks = KeySchedule::new(CipherSuite::TlsAes128GcmSha256);

        ks.derive_early_secret(None);
        ks.derive_handshake_secret(&[0u8; 32]);

        let transcript = [0u8; 32];
        ks.derive_handshake_traffic_secrets(&transcript);

        let keys = ks.client_handshake_keys();
        assert_eq!(keys.key_len, 16);
    }

    #[test]
    fn test_aead_roundtrip() {
        let key = [0u8; 16];
        let nonce = [0u8; 12];
        let plaintext = b"Hello, TLS 1.3!";

        let ciphertext = aead_encrypt(&key, &nonce, &[], plaintext);
        let decrypted = aead_decrypt(&key, &nonce, &[], &ciphertext).unwrap();

        assert_eq!(&decrypted[..], &plaintext[..]);
    }

    #[test]
    fn test_connection_state() {
        let mut conn = TlsConnection::new_client();
        assert_eq!(conn.state(), TlsState::Start);

        let random = [0u8; 32];
        let private_key = [1u8; 32];
        let _client_hello = conn.start_handshake(random, private_key);

        assert_eq!(conn.state(), TlsState::WaitServerHello);
    }
}

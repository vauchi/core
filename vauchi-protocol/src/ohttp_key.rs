// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Signed, windowed OHTTP gateway key records (#288).
//!
//! The gateway publishes one KeyConfig per fixed UTC window and signs each
//! (window, KeyConfig) pair with an intermediate key; an offline anchor signs
//! the intermediate. This module holds the wire codec and the exact bytes
//! each signature covers; signing and verification live with the parties
//! that hold keys (relay and core), since this crate carries no crypto.
//!
//! Wire layout, all integers big-endian:
//!
//! ```text
//! version (1) | window (8) | key_config_len (2) | key_config
//! | signature (64)
//! | intermediate public key (32) | not_before (8) | not_after (8)
//! | anchor signature (64)
//! ```

/// Current record version.
pub const RECORD_VERSION: u8 = 1;

/// Window length: one UTC day (owner decision 2026-10-05).
pub const WINDOW_SECONDS: u64 = 86_400;

/// Upper bound on an encoded KeyConfig; a P-256/X25519 config is ~41 bytes.
pub const MAX_KEY_CONFIG_BYTES: usize = 512;

const SIGNATURE_BYTES: usize = 64;
const PUBLIC_KEY_BYTES: usize = 32;
const FIXED_HEAD: usize = 1 + 8 + 2;
/// An encoded [`IntermediateCert`]: public key, not_before, not_after,
/// anchor signature.
pub const INTERMEDIATE_CERT_BYTES: usize = PUBLIC_KEY_BYTES + 8 + 8 + SIGNATURE_BYTES;
const FIXED_TAIL: usize = SIGNATURE_BYTES + INTERMEDIATE_CERT_BYTES;

/// Upper bound on an encoded record, checked before parsing (DC-01).
pub const MAX_RECORD_BYTES: usize = FIXED_HEAD + MAX_KEY_CONFIG_BYTES + FIXED_TAIL;

const KEY_CONFIG_DOMAIN: &[u8] = b"vauchi-ohttp-keyconfig-v1";
const INTERMEDIATE_DOMAIN: &[u8] = b"vauchi-ohttp-intermediate-v1";

/// The window a Unix time falls in; clients and the gateway agree without
/// asking each other.
pub fn window_of(unix_seconds: u64) -> u64 {
    unix_seconds / WINDOW_SECONDS
}

/// The OHTTP key id the gateway uses for a window's key.
pub fn key_id_for_window(window: u64) -> u8 {
    (window % 256) as u8
}

/// The gateway's intermediate signing key, certified by the offline anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntermediateCert {
    pub public_key: [u8; PUBLIC_KEY_BYTES],
    /// Unix seconds; the intermediate is valid from here.
    pub not_before: u64,
    /// Unix seconds; the intermediate is valid until here.
    pub not_after: u64,
    /// The anchor's Ed25519 signature over [`Self::signing_message`].
    pub anchor_signature: [u8; SIGNATURE_BYTES],
}

impl IntermediateCert {
    /// The bytes the anchor signs.
    pub fn signing_message(
        public_key: &[u8; PUBLIC_KEY_BYTES],
        not_before: u64,
        not_after: u64,
    ) -> Vec<u8> {
        let mut message = Vec::with_capacity(INTERMEDIATE_DOMAIN.len() + PUBLIC_KEY_BYTES + 16);
        message.extend_from_slice(INTERMEDIATE_DOMAIN);
        message.extend_from_slice(public_key);
        message.extend_from_slice(&not_before.to_be_bytes());
        message.extend_from_slice(&not_after.to_be_bytes());
        message
    }

    /// The certificate's fixed-length encoding, as written after the anchor
    /// ceremony and as carried at the tail of every [`SignedKeyConfig`].
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(INTERMEDIATE_CERT_BYTES);
        out.extend_from_slice(&self.public_key);
        out.extend_from_slice(&self.not_before.to_be_bytes());
        out.extend_from_slice(&self.not_after.to_be_bytes());
        out.extend_from_slice(&self.anchor_signature);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, SignedKeyConfigError> {
        let mut reader = Reader(bytes);
        let cert = Self::read(&mut reader)?;
        if !reader.0.is_empty() {
            return Err(SignedKeyConfigError::TrailingBytes);
        }
        Ok(cert)
    }

    fn read(reader: &mut Reader<'_>) -> Result<Self, SignedKeyConfigError> {
        Ok(Self {
            public_key: reader.take::<PUBLIC_KEY_BYTES>()?,
            not_before: u64::from_be_bytes(reader.take::<8>()?),
            not_after: u64::from_be_bytes(reader.take::<8>()?),
            anchor_signature: reader.take::<SIGNATURE_BYTES>()?,
        })
    }
}

/// One window's gateway KeyConfig with its signature chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedKeyConfig {
    pub window: u64,
    /// An RFC 9458 KeyConfig, opaque here.
    pub key_config: Vec<u8>,
    /// The intermediate's Ed25519 signature over [`Self::signing_message`].
    pub signature: [u8; SIGNATURE_BYTES],
    pub intermediate: IntermediateCert,
}

/// Why a byte string is not a [`SignedKeyConfig`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignedKeyConfigError {
    #[error("record is {0} bytes, over the {MAX_RECORD_BYTES}-byte bound")]
    TooLarge(usize),
    #[error("record is truncated")]
    Truncated,
    #[error("record has trailing bytes")]
    TrailingBytes,
    #[error("unknown record version {0}")]
    UnknownVersion(u8),
    #[error("record carries an empty KeyConfig")]
    EmptyKeyConfig,
    #[error("KeyConfig is {0} bytes, over the {MAX_KEY_CONFIG_BYTES}-byte bound")]
    KeyConfigTooLarge(usize),
}

impl SignedKeyConfig {
    /// The bytes the intermediate signs.
    pub fn signing_message(window: u64, key_config: &[u8]) -> Vec<u8> {
        let mut message = Vec::with_capacity(KEY_CONFIG_DOMAIN.len() + 8 + key_config.len());
        message.extend_from_slice(KEY_CONFIG_DOMAIN);
        message.extend_from_slice(&window.to_be_bytes());
        message.extend_from_slice(key_config);
        message
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(FIXED_HEAD + self.key_config.len() + FIXED_TAIL);
        out.push(RECORD_VERSION);
        out.extend_from_slice(&self.window.to_be_bytes());
        // Encoding an over-bound KeyConfig would produce a record decode
        // rejects; the length field saturates rather than wrapping.
        let len = u16::try_from(self.key_config.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&self.key_config);
        out.extend_from_slice(&self.signature);
        out.extend_from_slice(&self.intermediate.encode());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, SignedKeyConfigError> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(SignedKeyConfigError::TooLarge(bytes.len()));
        }
        let mut reader = Reader(bytes);
        let version = reader.take::<1>()?[0];
        if version != RECORD_VERSION {
            return Err(SignedKeyConfigError::UnknownVersion(version));
        }
        let window = u64::from_be_bytes(reader.take::<8>()?);
        let key_config_len = usize::from(u16::from_be_bytes(reader.take::<2>()?));
        if key_config_len == 0 {
            return Err(SignedKeyConfigError::EmptyKeyConfig);
        }
        if key_config_len > MAX_KEY_CONFIG_BYTES {
            return Err(SignedKeyConfigError::KeyConfigTooLarge(key_config_len));
        }
        let key_config = reader.take_slice(key_config_len)?.to_vec();
        let signature = reader.take::<SIGNATURE_BYTES>()?;
        let intermediate = IntermediateCert::read(&mut reader)?;
        if !reader.0.is_empty() {
            return Err(SignedKeyConfigError::TrailingBytes);
        }
        Ok(Self {
            window,
            key_config,
            signature,
            intermediate,
        })
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take_slice(&mut self, len: usize) -> Result<&'a [u8], SignedKeyConfigError> {
        if self.0.len() < len {
            return Err(SignedKeyConfigError::Truncated);
        }
        let (head, rest) = self.0.split_at(len);
        self.0 = rest;
        Ok(head)
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], SignedKeyConfigError> {
        let slice = self.take_slice(N)?;
        let mut out = [0u8; N];
        out.copy_from_slice(slice);
        Ok(out)
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A gateway that signs its window keys under a test anchor (#288), for the
//! client tests: each window gets a real OHTTP key config whose key id names
//! the window, signed through anchor → intermediate.

use std::collections::HashMap;
use std::sync::Mutex;

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use vauchi_protocol::ohttp_key::{
    AnchorRollover, IntermediateCert, SignedKeyConfig, backup_commitment_message,
    encode_rollover_chain, key_id_for_window,
};

use super::mock_relay::CannedResponse;

pub const DAY: u64 = 86_400;
/// A minute into window 20_367 (2025-10-06).
pub const NOW: u64 = 20_367 * DAY + 60;

pub struct SignedGateway {
    anchor: SigningKey,
    intermediate: SigningKey,
    /// One key config per window, as a gateway holds: generating it anew on
    /// each call would hand out a different key for the same window.
    keys: Mutex<HashMap<u64, Vec<u8>>>,
}

impl SignedGateway {
    pub fn new(seed: u8) -> Self {
        Self {
            anchor: SigningKey::from_bytes(&[seed; 32]),
            intermediate: SigningKey::from_bytes(&[seed.wrapping_add(1); 32]),
            keys: Mutex::default(),
        }
    }

    pub fn anchor(&self) -> [u8; 32] {
        self.anchor.verifying_key().to_bytes()
    }

    /// What a client holds to accept this gateway's anchor as the backup
    /// of another (decision 0.13).
    pub fn commitment(&self) -> [u8; 32] {
        Sha256::digest(backup_commitment_message(&self.anchor())).into()
    }

    /// The rollover record in which this gateway's anchor takes over and
    /// commits to `next` as its own backup.
    pub fn takeover(&self, next: [u8; 32]) -> AnchorRollover {
        let new_anchor = self.anchor();
        AnchorRollover {
            new_anchor,
            next_commitment: next,
            signature: self
                .anchor
                .sign(&AnchorRollover::signing_message(&new_anchor, &next))
                .to_bytes(),
        }
    }

    /// The OHTTP key config this gateway serves for `window`.
    pub fn key_config(&self, window: u64) -> Vec<u8> {
        self.keys
            .lock()
            .unwrap()
            .entry(window)
            .or_insert_with(|| new_key_config(window))
            .clone()
    }

    /// `window`'s record with `key_config`, under a certificate valid
    /// around [`NOW`].
    pub fn record_for(&self, window: u64, key_config: Vec<u8>) -> SignedKeyConfig {
        let public_key = self.intermediate.verifying_key().to_bytes();
        let (not_before, not_after) = (NOW - 30 * DAY, NOW + 60 * DAY);
        let intermediate = IntermediateCert {
            public_key,
            not_before,
            not_after,
            anchor_signature: self
                .anchor
                .sign(&IntermediateCert::signing_message(
                    &public_key,
                    not_before,
                    not_after,
                ))
                .to_bytes(),
        };
        SignedKeyConfig {
            window,
            signature: self
                .intermediate
                .sign(&SignedKeyConfig::signing_message(window, &key_config))
                .to_bytes(),
            key_config,
            intermediate,
        }
    }

    pub fn record(&self, window: u64) -> SignedKeyConfig {
        self.record_for(window, self.key_config(window))
    }

    /// `GET /v2/ohttp-key-signed` answering with `window`'s record.
    pub fn response(&self, window: u64) -> CannedResponse {
        signed_response(self.record(window).encode())
    }
}

fn new_key_config(window: u64) -> Vec<u8> {
    use ohttp::{KeyConfig, SymmetricSuite, hpke};
    KeyConfig::new(
        key_id_for_window(window),
        hpke::Kem::X25519Sha256,
        vec![SymmetricSuite::new(
            hpke::Kdf::HkdfSha256,
            hpke::Aead::ChaCha20Poly1305,
        )],
    )
    .expect("KeyConfig::new")
    .encode()
    .expect("encode key config")
}

pub fn signed_response(body: Vec<u8>) -> CannedResponse {
    CannedResponse {
        status: 200,
        headers: vec![(
            "Content-Type".into(),
            "application/vnd.vauchi.ohttp-key-signed".into(),
        )],
        body,
    }
}

/// `GET /v2/ohttp-anchor-rollover` answering with `chain`.
pub fn rollover_response(chain: &[AnchorRollover]) -> CannedResponse {
    CannedResponse {
        status: 200,
        headers: vec![(
            "Content-Type".into(),
            "application/vnd.vauchi.ohttp-anchor-rollover".into(),
        )],
        body: encode_rollover_chain(chain),
    }
}

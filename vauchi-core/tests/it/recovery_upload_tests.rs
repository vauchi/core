// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Pins the upload/recipient contract without requiring service binaries.

use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use vauchi_core::crypto::{PublicKey, Signature};
use vauchi_core::identity::Identity;
use vauchi_core::recovery::{RecoveryVoucher, VerificationResult};
use vauchi_core::{Vauchi, VauchiConfig, VauchiError};

use crate::common::mock_relay::{CannedResponse, MockRelay};

/// A replacement identity holding three vouchers for `old`, talking to a
/// relay that accepts one proof upload.
fn replacement_ready_to_upload(relay: &MockRelay, old: &Identity) -> (Vauchi, tempfile::TempDir) {
    relay.queue(
        "recovery_store",
        CannedResponse::ok_json(br#"{"status":"ok"}"#.to_vec()),
    );
    let mut config = VauchiConfig::default().with_relay_url(relay.url());
    config.ohttp.allow_direct = true;
    config.relay.connect_timeout_ms = 2_000;
    let dir = tempfile::tempdir().unwrap();
    config.storage_path = dir.path().join("replacement.db");
    let mut replacement = Vauchi::new(config).unwrap();
    replacement.create_identity("Alice replacement").unwrap();
    let claim = replacement
        .create_recovery_claim(old.signing_public_key())
        .unwrap();
    for name in ["Bob", "Carol", "Dave"] {
        let helper = Identity::create(name, 0);
        let voucher = RecoveryVoucher::create_from_claim(
            &claim,
            helper.signing_keypair(),
            None,
            replacement.clock().unix_seconds(),
        )
        .unwrap();
        replacement
            .add_recovery_voucher(&voucher.to_bytes())
            .unwrap();
    }
    (replacement, dir)
}

// @scenario: contact_recovery :: Upload recovery proof to relay
#[test]
fn recovery_upload_uses_the_recipient_proof_codec() {
    let relay = MockRelay::start();
    let old = Identity::create("Alice", 0);
    let (replacement, _dir) = replacement_ready_to_upload(&relay, &old);
    replacement.upload_recovery_proof().unwrap();
    let requests = relay.received();
    assert_eq!(requests.len(), 1);
    let request: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    let bytes = STANDARD
        .decode(request["proof_data"].as_str().unwrap())
        .unwrap();
    let recipient = Vauchi::in_memory().unwrap();
    let (proof, verdict) = recipient.verify_recovery_proof(&bytes).unwrap();
    assert_eq!(proof.old_pk().as_bytes(), old.signing_public_key());
    assert_eq!(
        proof.new_pk().as_bytes(),
        replacement.identity().unwrap().signing_public_key()
    );
    assert_eq!(proof.threshold(), 3);
    assert_eq!(proof.voucher_count(), 3);
    assert!(matches!(
        verdict,
        VerificationResult::LowConfidence { total_vouchers: 3 }
    ));
    assert!(replacement.get_recovery_progress().unwrap().is_none());
}

// @internal
#[test]
fn recovery_upload_rejects_a_future_start_time_without_losing_progress() {
    let mut replacement = Vauchi::in_memory().unwrap();
    replacement.create_identity("Alice replacement").unwrap();
    let claim = replacement.create_recovery_claim(&[7; 32]).unwrap();
    for name in ["Bob", "Carol", "Dave"] {
        let helper = Identity::create(name, 0);
        let voucher = RecoveryVoucher::create_from_claim(
            &claim,
            helper.signing_keypair(),
            None,
            replacement.clock().unix_seconds(),
        )
        .unwrap();
        replacement
            .add_recovery_voucher(&voucher.to_bytes())
            .unwrap();
    }
    let mut progress = replacement.get_recovery_progress().unwrap().unwrap();
    progress.started_at = u64::MAX;
    replacement
        .storage()
        .recovery()
        .save_recovery_progress(&progress)
        .unwrap();
    match replacement.upload_recovery_proof().unwrap_err() {
        VauchiError::InvalidState(detail) => {
            assert_eq!(detail, "Recovery start time is in the future")
        }
        other => panic!("expected invalid recovery time, got {other}"),
    }
    assert_eq!(
        replacement
            .get_recovery_progress()
            .unwrap()
            .unwrap()
            .started_at,
        u64::MAX
    );
}

// The relay erases a proof on its owner's purge (#711 D3), so the upload
// names the identity it came from and proves it holds that key.
// @internal
#[test]
fn recovery_upload_is_signed_by_the_replacement_identity() {
    let relay = MockRelay::start();
    let old = Identity::create("Alice", 0);
    let (replacement, _dir) = replacement_ready_to_upload(&relay, &old);
    let before = replacement.clock().unix_seconds();

    replacement.upload_recovery_proof().unwrap();

    let request: serde_json::Value = serde_json::from_slice(&relay.received()[0].body).unwrap();
    let owner_pk = *replacement.identity().unwrap().signing_public_key();
    assert_eq!(request["owner_pk"], hex::encode(owner_pk));
    let timestamp = request["timestamp"].as_u64().unwrap();
    assert!(timestamp >= before && timestamp <= replacement.clock().unix_seconds());
    let key_hash = hex::decode(request["key_hash"].as_str().unwrap()).unwrap();
    let proof = STANDARD
        .decode(request["proof_data"].as_str().unwrap())
        .unwrap();
    let mut message = b"recovery-store".to_vec();
    message.extend_from_slice(&key_hash);
    message.extend_from_slice(&Sha256::digest(&proof));
    message.extend_from_slice(&timestamp.to_be_bytes());
    let signature: [u8; 64] = hex::decode(request["signature"].as_str().unwrap())
        .unwrap()
        .try_into()
        .unwrap();
    let signature = Signature::from_bytes(signature);
    assert!(PublicKey::from_bytes(owner_pk).verify(&message, &signature));
    let mut other_proof = message.clone();
    other_proof[b"recovery-store".len() + 32] ^= 1;
    assert!(!PublicKey::from_bytes(owner_pk).verify(&other_proof, &signature));
}

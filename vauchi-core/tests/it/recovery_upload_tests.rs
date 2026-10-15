// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pins the upload/recipient contract without requiring service binaries.

use base64::{Engine, engine::general_purpose::STANDARD};
use vauchi_core::identity::Identity;
use vauchi_core::recovery::{RecoveryVoucher, VerificationResult};
use vauchi_core::{Vauchi, VauchiConfig, VauchiError};

use crate::common::mock_relay::{CannedResponse, MockRelay};

// @scenario: contact_recovery :: Upload recovery proof to relay
#[test]
fn recovery_upload_uses_the_recipient_proof_codec() {
    let relay = MockRelay::start();
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
    let old = Identity::create("Alice", 0);
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

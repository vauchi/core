// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the recovery domain methods on `PlatformAppEngine`.
//!
//! Phase B2 of the [collapse-vauchi-platform-into-app-engine plan][plan]:
//! these wrap-and-test the 9 recovery methods that previously only existed
//! on `VauchiPlatform`.
//!
//! [plan]: https://gitlab.com/vauchi/private/-/blob/main/docs/problems/2026-04-28-collapse-vauchi-platform-into-app-engine/implementation-plan.md

use std::sync::Arc;

use vauchi_platform::{DomainCommand, DomainCommandResult, PlatformAppEngine};

// ── Helpers ──────────────────────────────────────────────────────────

/// Create a `PlatformAppEngine` with a temp directory, drive it through
/// the onboarding flow, and return the engine + tempdir.
fn create_engine_with_identity() -> (Arc<PlatformAppEngine>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");

    drive_onboarding(&engine);

    (engine, dir)
}

/// Drive through the 6-step onboarding flow so an identity exists.
fn drive_onboarding(engine: &PlatformAppEngine) {
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "create_new"}}"#.into())
        .expect("create_new");
    engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Alice"}}"#.into(),
        )
        .expect("display_name");
    for _ in 0..3 {
        engine
            .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
            .expect("continue");
    }
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "start_app"}}"#.into())
        .expect("start_app");
}

/// Hex-encode a fake 32-byte public key. The "old PK" in a recovery
/// claim is the public key of the lost identity — for the create/parse
/// path it doesn't have to be a real key, just a 32-byte hex string.
fn fake_old_pk_hex() -> String {
    hex::encode([0x42u8; 32])
}

// ── create_recovery_claim ────────────────────────────────────────────

// @internal
#[test]
fn create_recovery_claim_returns_claim_with_supplied_old_pk() {
    let (engine, _dir) = create_engine_with_identity();
    let old_pk_hex = fake_old_pk_hex();

    let claim = engine
        .create_recovery_claim(old_pk_hex.clone())
        .expect("create_recovery_claim");

    assert_eq!(claim.old_public_key, old_pk_hex);
    assert!(!claim.claim_data.is_empty(), "claim_data must be base64");
    assert!(!claim.is_expired, "fresh claim must not be expired");
    // new_public_key is the active identity's signing key — non-empty hex
    assert_eq!(
        claim.new_public_key.len(),
        64,
        "new_public_key hex == 64 chars"
    );
}

// @internal
#[test]
fn create_recovery_claim_rejects_invalid_hex() {
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.create_recovery_claim("not-hex".into());
    assert!(result.is_err(), "invalid hex must error");
}

// @internal
#[test]
fn create_recovery_claim_rejects_wrong_length() {
    let (engine, _dir) = create_engine_with_identity();
    // 31 bytes = 62 hex chars
    let result = engine.create_recovery_claim(hex::encode([0u8; 31]));
    assert!(result.is_err(), "wrong-length pk must error");
}

// ── parse_recovery_claim ─────────────────────────────────────────────

// @internal
#[test]
fn parse_recovery_claim_round_trips_create() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let old_pk_hex = fake_old_pk_hex();

    let original = engine
        .create_recovery_claim(old_pk_hex.clone())
        .expect("create");
    let parse_result = engine
        .dispatch_domain_command(DomainCommand::ParseRecoveryClaim {
            claim_b64: original.claim_data.clone(),
        })
        .expect("dispatch ParseRecoveryClaim");
    let DomainCommandResult::RecoveryClaim { claim: parsed } = parse_result else {
        panic!("ParseRecoveryClaim: unexpected result variant {parse_result:?}");
    };

    assert_eq!(parsed.old_public_key, original.old_public_key);
    assert_eq!(parsed.new_public_key, original.new_public_key);
    assert_eq!(parsed.claim_data, original.claim_data);
    assert_eq!(parsed.is_expired, original.is_expired);
}

// @internal
#[test]
fn parse_recovery_claim_rejects_invalid_base64() {
    use vauchi_platform::DomainCommand;
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.dispatch_domain_command(DomainCommand::ParseRecoveryClaim {
        claim_b64: "not!valid!base64!".into(),
    });
    assert!(result.is_err(), "invalid base64 must error");
}

// ── get_recovery_status ──────────────────────────────────────────────

// @internal
#[test]
fn get_recovery_status_is_none_when_no_recovery_in_progress() {
    let (engine, _dir) = create_engine_with_identity();
    let status = engine.get_recovery_status().expect("get_recovery_status");
    assert!(status.is_none(), "no recovery in progress → None");
}

// @internal
#[test]
fn get_recovery_status_reflects_create_recovery_claim() {
    let (engine, _dir) = create_engine_with_identity();
    engine
        .create_recovery_claim(fake_old_pk_hex())
        .expect("create");

    let status = engine
        .get_recovery_status()
        .expect("get_recovery_status")
        .expect("recovery in progress");

    assert_eq!(status.vouchers_collected, 0);
    assert!(status.vouchers_needed >= 1, "threshold must be ≥ 1");
    assert!(!status.is_complete, "no vouchers added yet");
}

// ── get_recovery_proof ───────────────────────────────────────────────

// @internal
#[test]
fn get_recovery_proof_is_none_when_no_recovery_in_progress() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let result = engine
        .dispatch_domain_command(DomainCommand::GetRecoveryProof)
        .expect("dispatch GetRecoveryProof");
    let DomainCommandResult::StringOpt { value: proof } = result else {
        panic!("GetRecoveryProof: unexpected result variant {result:?}");
    };
    assert!(proof.is_none(), "no proof yet");
}

// @internal
#[test]
fn get_recovery_proof_is_none_when_threshold_not_met() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    engine
        .create_recovery_claim(fake_old_pk_hex())
        .expect("create");

    let result = engine
        .dispatch_domain_command(DomainCommand::GetRecoveryProof)
        .expect("dispatch GetRecoveryProof");
    let DomainCommandResult::StringOpt { value: proof } = result else {
        panic!("GetRecoveryProof: unexpected result variant {result:?}");
    };
    assert!(proof.is_none(), "0 vouchers < threshold → None");
}

// ── add_recovery_voucher ─────────────────────────────────────────────

// @internal
#[test]
fn add_recovery_voucher_rejects_invalid_base64() {
    let (engine, _dir) = create_engine_with_identity();
    engine
        .create_recovery_claim(fake_old_pk_hex())
        .expect("create");

    let result = engine.add_recovery_voucher("not!valid!".into());
    assert!(result.is_err(), "invalid base64 must error");
}

// @internal
#[test]
fn add_recovery_voucher_errors_when_no_recovery_in_progress() {
    let (engine, _dir) = create_engine_with_identity();
    // No create_recovery_claim first — so no proof file exists.
    // Feed it a syntactically-valid base64 but garbage voucher.
    use base64::Engine;
    let garbage = base64::engine::general_purpose::STANDARD.encode([0u8; 16]);
    let result = engine.add_recovery_voucher(garbage);
    assert!(
        result.is_err(),
        "no recovery in progress + invalid voucher → error"
    );
}

// ── create_recovery_voucher ──────────────────────────────────────────

// @internal
#[test]
fn create_recovery_voucher_rejects_invalid_base64() {
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.create_recovery_voucher("not!valid!".into());
    assert!(result.is_err(), "invalid base64 must error");
}

// @internal
#[test]
fn create_recovery_voucher_rejects_self_vouching() {
    // The active identity creates a claim binding `old_pk → its own pk`
    // and tries to vouch for it. Core rejects self-vouching as a
    // security guard; the wrapper must surface that error rather than
    // silently swallowing it.
    let (engine, _dir) = create_engine_with_identity();
    let claim = engine
        .create_recovery_claim(fake_old_pk_hex())
        .expect("create_claim");

    let result = engine.create_recovery_voucher(claim.claim_data);
    assert!(result.is_err(), "self-vouching must be rejected");
    let err_str = format!("{:?}", result.unwrap_err());
    assert!(
        err_str.contains("self-vouching") || err_str.contains("own recovery"),
        "error must mention self-vouching: {err_str}"
    );
}

// ── trust_contact_for_recovery / untrust / count ─────────────────────
//
// Slice 32g-B (2026-05-17): the direct PAE pub fns retired in favour
// of `DomainCommand::{TrustContactForRecovery, UntrustContactForRecovery,
// TrustedContactCount}`. Same storage semantics; the dispatch envelope
// is the only thing that changed.

// @internal
#[test]
fn trusted_contact_count_is_zero_when_no_contacts() {
    let (engine, _dir) = create_engine_with_identity();
    let result = engine
        .dispatch_domain_command(DomainCommand::TrustedContactCount)
        .expect("TrustedContactCount dispatch");
    match result {
        DomainCommandResult::Count { value } => assert_eq!(value, 0),
        other => panic!("expected DomainCommandResult::Count, got {other:?}"),
    }
}

// @internal
#[test]
fn trust_contact_for_recovery_errors_on_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.dispatch_domain_command(DomainCommand::TrustContactForRecovery {
        contact_id: "nonexistent-id".into(),
    });
    assert!(result.is_err(), "unknown contact must error");
}

// @internal
#[test]
fn untrust_contact_for_recovery_errors_on_unknown_contact() {
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.dispatch_domain_command(DomainCommand::UntrustContactForRecovery {
        contact_id: "nonexistent-id".into(),
    });
    assert!(result.is_err(), "unknown contact must error");
}

// ── Cache invalidation contract (CC-05 light) ────────────────────────

// @internal
#[test]
fn create_recovery_claim_invalidates_recovery_screen_cache() {
    // After a write through the engine, the next current_screen_json()
    // must rebuild the affected screen rather than serve stale data.
    // Smoke-level: assert no panic on read-after-write across the
    // affected screens.
    let (engine, _dir) = create_engine_with_identity();
    engine
        .create_recovery_claim(fake_old_pk_hex())
        .expect("create");

    // Reading the current screen (whatever it is) must succeed without
    // a stale-engine panic. The Recovery screen is reachable via
    // settings, so we just exercise the cache by invalidating + reading.
    engine
        .invalidate_all()
        .expect("invalidate_all does not panic");
    let _ = engine
        .current_screen_json()
        .expect("current_screen_json after recovery write");
}

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

use vauchi_platform::{DomainCommand, DomainCommandResult, MobileRecoveryClaim, PlatformAppEngine};

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

/// Drive through the full onboarding flow via the canonical envelope.
///
/// Every step reads the Core-minted interaction and binding ids from the
/// current command batch — exactly what a real shell renders — and
/// dispatches generic events back. No retired action/screen seams.
fn drive_onboarding(engine: &PlatformAppEngine) {
    fn primary_interaction(batch: &serde_json::Value) -> (String, String) {
        let bar = batch["commands"]
            .as_array()
            .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
            .expect("command batch must carry a context bar");
        (
            bar["surface_id"]
                .as_str()
                .expect("bar surface id")
                .to_owned(),
            bar["bar"]["primary"]["interaction_id"]
                .as_str()
                .expect("primary interaction id")
                .to_owned(),
        )
    }

    fn dispatch_primary(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
    ) -> serde_json::Value {
        let (surface_id, interaction_id) = primary_interaction(batch);
        let event = serde_json::json!({
            "ActionActivated": { "surface_id": surface_id, "interaction_id": interaction_id }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch primary activation"),
        )
        .expect("parse command batch")
    }

    fn find_input<'v>(nodes: &'v [serde_json::Value]) -> Option<&'v serde_json::Value> {
        nodes.iter().find_map(|node| {
            if let Some(input) = node.get("Input") {
                Some(input)
            } else {
                node["Group"]["children"]
                    .as_array()
                    .and_then(|children| find_input(children))
            }
        })
    }

    fn set_text_input(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
        text: &str,
    ) -> serde_json::Value {
        let (surface_id, nodes) = batch["commands"]
            .as_array()
            .and_then(|commands| {
                commands.iter().find_map(|c| {
                    let surface = &c["ReplaceSurface"]["surface"];
                    surface
                        .is_object()
                        .then(|| (surface["surface_id"].clone(), surface["nodes"].clone()))
                })
            })
            .expect("command batch must replace a surface");
        let nodes: Vec<serde_json::Value> =
            serde_json::from_value(nodes).expect("surface nodes array");
        let input = find_input(&nodes).expect("surface must carry a text input");
        let event = serde_json::json!({
            "ValueChanged": {
                "surface_id": surface_id,
                "binding_id": input["binding_id"],
                "value": { "text": text },
            }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch text input"),
        )
        .expect("parse command batch")
    }

    let mut batch: serde_json::Value = serde_json::from_str(
        &engine
            .initial_commands_json()
            .expect("initial onboarding commands"),
    )
    .expect("parse initial batch");

    batch = dispatch_primary(engine, &batch); // identity_check → default_name
    batch = set_text_input(engine, &batch, "Alice"); // enter display name
    batch = dispatch_primary(engine, &batch); // default_name → groups_setup
    batch = dispatch_primary(engine, &batch); // groups_setup → contact_info
    batch = dispatch_primary(engine, &batch); // contact_info → what_next
    let _ = dispatch_primary(engine, &batch); // what_next → complete → home
}

/// Hex-encode a fake 32-byte public key. The "old PK" in a recovery
/// claim is the public key of the lost identity — for the create/parse
/// path it doesn't have to be a real key, just a 32-byte hex string.
fn fake_old_pk_hex() -> String {
    hex::encode([0x42u8; 32])
}

/// Dispatch `CreateRecoveryClaim` and unwrap the resulting claim.
/// Most recovery tests need an in-progress claim as setup; the typed
/// `create_recovery_claim` method was retired in Track B B4a-6.
fn create_claim(engine: &PlatformAppEngine, old_pk_hex: String) -> MobileRecoveryClaim {
    let result = engine
        .dispatch_domain_command(DomainCommand::CreateRecoveryClaim { old_pk_hex })
        .expect("dispatch CreateRecoveryClaim");
    let DomainCommandResult::RecoveryClaim { claim } = result else {
        panic!("CreateRecoveryClaim: unexpected result variant {result:?}");
    };
    claim
}

// ── create_recovery_claim ────────────────────────────────────────────

// @internal
#[test]
fn create_recovery_claim_returns_claim_with_supplied_old_pk() {
    let (engine, _dir) = create_engine_with_identity();
    let old_pk_hex = fake_old_pk_hex();

    let claim = create_claim(&engine, old_pk_hex.clone());

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
    let result = engine.dispatch_domain_command(DomainCommand::CreateRecoveryClaim {
        old_pk_hex: "not-hex".into(),
    });
    assert!(result.is_err(), "invalid hex must error");
}

// @internal
#[test]
fn create_recovery_claim_rejects_wrong_length() {
    let (engine, _dir) = create_engine_with_identity();
    // 31 bytes = 62 hex chars
    let result = engine.dispatch_domain_command(DomainCommand::CreateRecoveryClaim {
        old_pk_hex: hex::encode([0u8; 31]),
    });
    assert!(result.is_err(), "wrong-length pk must error");
}

// ── parse_recovery_claim ─────────────────────────────────────────────

// @internal
#[test]
fn parse_recovery_claim_round_trips_create() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let old_pk_hex = fake_old_pk_hex();

    let original = create_claim(&engine, old_pk_hex.clone());
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
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let result = engine
        .dispatch_domain_command(DomainCommand::GetRecoveryStatus)
        .expect("dispatch GetRecoveryStatus");
    let DomainCommandResult::OptionalRecoveryProgress { progress: status } = result else {
        panic!("GetRecoveryStatus: unexpected result variant {result:?}");
    };
    assert!(status.is_none(), "no recovery in progress → None");
}

// @internal
#[test]
fn get_recovery_status_reflects_create_recovery_claim() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    create_claim(&engine, fake_old_pk_hex());

    let result = engine
        .dispatch_domain_command(DomainCommand::GetRecoveryStatus)
        .expect("dispatch GetRecoveryStatus");
    let DomainCommandResult::OptionalRecoveryProgress { progress: status } = result else {
        panic!("GetRecoveryStatus: unexpected result variant {result:?}");
    };
    let status = status.expect("recovery in progress");

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
    create_claim(&engine, fake_old_pk_hex());

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
    use vauchi_platform::DomainCommand;
    let (engine, _dir) = create_engine_with_identity();
    create_claim(&engine, fake_old_pk_hex());

    let result = engine.dispatch_domain_command(DomainCommand::AddRecoveryVoucher {
        voucher_b64: "not!valid!".into(),
    });
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
    let result =
        engine.dispatch_domain_command(vauchi_platform::DomainCommand::AddRecoveryVoucher {
            voucher_b64: garbage,
        });
    assert!(
        result.is_err(),
        "no recovery in progress + invalid voucher → error"
    );
}

// ── create_recovery_voucher ──────────────────────────────────────────

// @internal
#[test]
fn create_recovery_voucher_rejects_invalid_base64() {
    use vauchi_platform::DomainCommand;
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.dispatch_domain_command(DomainCommand::CreateRecoveryVoucher {
        claim_b64: "not!valid!".into(),
    });
    assert!(result.is_err(), "invalid base64 must error");
}

// @internal
#[test]
fn create_recovery_voucher_rejects_self_vouching() {
    use vauchi_platform::DomainCommand;
    // The active identity creates a claim binding `old_pk → its own pk`
    // and tries to vouch for it. Core rejects self-vouching as a
    // security guard; the wrapper must surface that error rather than
    // silently swallowing it.
    let (engine, _dir) = create_engine_with_identity();
    let claim = create_claim(&engine, fake_old_pk_hex());

    let result = engine.dispatch_domain_command(DomainCommand::CreateRecoveryVoucher {
        claim_b64: claim.claim_data,
    });
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
    // After a write through the engine, the next initial_commands_json()
    // must rebuild the affected screen rather than serve stale data.
    // Smoke-level: assert no panic on read-after-write across the
    // affected screens.
    let (engine, _dir) = create_engine_with_identity();
    create_claim(&engine, fake_old_pk_hex());

    // Reading the current screen (whatever it is) must succeed without
    // a stale-engine panic. The Recovery screen is reachable via
    // settings, so we just exercise the cache by invalidating + reading.
    engine
        .dispatch_json(r#""PresentationInvalidated""#.into())
        .expect("presentation invalidation does not panic");
    let _ = engine
        .initial_commands_json()
        .expect("initial_commands_json after recovery write");
}

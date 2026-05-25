// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the link-mode responder engine bridge —
//! Phase 1.6 of `_private/docs/problems/2026-04-27-deep-link-responder-flow`.
//!
//! Covers `PlatformAppEngine::current_link_responder_session` and the
//! lazy auto-creation / cancel-on-leave lifecycle wired through
//! `after_screen_transition`. The companion in-isolation cycle-thread
//! tests live in `link_responder_session_tests.rs`.

use vauchi_platform::PlatformAppEngine;

fn create_engine() -> (std::sync::Arc<PlatformAppEngine>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    (engine, dir)
}

fn drive_onboarding(engine: &PlatformAppEngine) {
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "create_new"}}"#.into())
        .expect("create_new");
    engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Bob"}}"#.into(),
        )
        .expect("text changed");
    for _ in 0..3 {
        engine
            .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
            .expect("continue");
    }
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "start_app"}}"#.into())
        .expect("start_app");
}

fn fresh_link_url() -> String {
    let (init, _) = vauchi_core::exchange::link_mode::initiator_generate();
    init.url
}

/// Drive the engine all the way to `link_responder_waiting`: onboarding
/// → handle_deep_link_uri → grant → DeepLinkResponder.
fn drive_to_link_responder(engine: &PlatformAppEngine) {
    drive_onboarding(engine);
    engine
        .handle_deep_link_uri(fresh_link_url())
        .expect("deep link routes to consent");
    let consent_id = engine.current_screen_id().expect("screen id");
    assert_eq!(consent_id, "deep_link_consent");
    let grant_json = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "grant"}}"#.into())
        .expect("grant action");
    let post_id = engine.current_screen_id().expect("screen id after grant");
    assert_eq!(
        post_id, "link_responder_waiting",
        "grant must route to the responder waiting screen — action returned {grant_json}",
    );
}

// @internal
#[test]
fn current_link_responder_session_returns_none_off_screen() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    // Post-onboarding we land on `my_info`, not the responder screen.
    let session = engine
        .current_link_responder_session()
        .expect("getter must succeed off the responder screen");
    assert!(
        session.is_none(),
        "off the responder screen the engine must not hand out a session",
    );
}

// @internal
#[test]
fn current_link_responder_session_lazily_creates_session_on_screen() {
    let (engine, _dir) = create_engine();
    drive_to_link_responder(&engine);

    let session = engine
        .current_link_responder_session()
        .expect("getter must succeed on the responder screen")
        .expect("session must be created when on the responder screen");
    let gate = session.gate_hash_bytes();
    assert!(
        !gate.is_empty(),
        "session must expose a non-empty gate_hash — confirms the DH + key-derive ran",
    );
}

// @internal
#[test]
fn current_link_responder_session_returns_same_session_on_repeat_calls() {
    let (engine, _dir) = create_engine();
    drive_to_link_responder(&engine);

    let first = engine
        .current_link_responder_session()
        .expect("first call ok")
        .expect("first call returns session");
    let second = engine
        .current_link_responder_session()
        .expect("second call ok")
        .expect("second call returns session");
    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "the engine must cache the session — frontends fetch the same handle each time",
    );
}

// @internal
#[test]
fn navigate_back_from_responder_drops_session() {
    let (engine, _dir) = create_engine();
    drive_to_link_responder(&engine);

    // Force creation (lazily) so we have something to drop.
    let _ = engine
        .current_link_responder_session()
        .expect("getter ok")
        .expect("session created");

    engine
        .navigate_back_json()
        .expect("navigate back away from responder");
    assert_ne!(
        engine.current_screen_id().expect("screen id post back"),
        "link_responder_waiting",
        "navigate_back must leave the responder screen",
    );

    let session_after_back = engine
        .current_link_responder_session()
        .expect("getter ok off-screen");
    assert!(
        session_after_back.is_none(),
        "leaving the responder screen must drop the cached session",
    );
}

// ─────────────────────────────────────────────────────────────────────
// Slice 32l Phase 2 — screen-driven responder (T2.1 RED)
//
// Design: `_private/docs/designs/2026-05-25-slice-32l-phase-2-responder-
// screen-driven-design.md`. The engine must OWN the `LinkResponder` and
// reflect its terminal state on the screen, driven by `handle_hardware_event`
// — the frontend pulls NO session object. These fail RED until T2.2 wires
// the engine-owned responder and adds the `link_responder_failed` /
// `link_responder_completed` screens.
//
// RED scaffold: `gate_hash` is bootstrapped here via the (retiring)
// `current_link_responder_session` getter. T2.2 GREEN replaces that with
// `poll_link_responder_commands()` (the `RelayEscrowDeposit` command carries
// `gate_hash`), then retires the getter. NOTE: getting the session only
// builds it (DH + key derive) — it does NOT call `.start()`, so no cycle
// thread is spawned.
//
// Deferred to T2.2 (need a valid encrypted blob fixture / the command
// drain, out of scope for this RED): the success path
// (`RelayEscrowBlobReceived` with a valid card → `link_responder_completed`
// + `import_received_link_card` persistence) and the deposit-command
// assertions.

/// Bootstrap the responder's `gate_hash` for event construction.
fn responder_gate_hash(engine: &PlatformAppEngine) -> Vec<u8> {
    engine
        .current_link_responder_session()
        .expect("getter ok on screen")
        .expect("session built on the responder screen")
        .gate_hash_bytes()
}

// @internal
#[test]
fn relay_deposit_failure_drives_engine_to_failed_screen() {
    let (engine, _dir) = create_engine();
    drive_to_link_responder(&engine);
    let gate_hash = responder_gate_hash(&engine);

    engine
        .handle_hardware_event(vauchi_platform::MobileEvent::RelayEscrowFailed {
            gate_hash,
            reason: "deposit_rejected".into(),
        })
        .expect("engine must accept the relay-failure hardware event");

    assert_eq!(
        engine.current_screen_id().expect("screen id"),
        "link_responder_failed",
        "a relay deposit failure must drive the engine-owned responder to the failed \
         screen — not leave it on link_responder_waiting (the frontend pulls no session)",
    );
}

// @internal
#[test]
fn undecryptable_relay_blob_drives_engine_to_failed_screen() {
    let (engine, _dir) = create_engine();
    drive_to_link_responder(&engine);
    let gate_hash = responder_gate_hash(&engine);

    // `RelayEscrowReady` advances Polling → Retrieving; an undecryptable blob
    // then fails the card decrypt, which must surface as the failed screen.
    engine
        .handle_hardware_event(vauchi_platform::MobileEvent::RelayEscrowReady {
            gate_hash: gate_hash.clone(),
        })
        .expect("engine must accept RelayEscrowReady");
    engine
        .handle_hardware_event(vauchi_platform::MobileEvent::RelayEscrowBlobReceived {
            gate_hash,
            blob: vec![0xde, 0xad, 0xbe, 0xef],
        })
        .expect("engine must accept RelayEscrowBlobReceived");

    assert_eq!(
        engine.current_screen_id().expect("screen id"),
        "link_responder_failed",
        "an undecryptable relay blob must drive the engine-owned responder to the failed screen",
    );
}

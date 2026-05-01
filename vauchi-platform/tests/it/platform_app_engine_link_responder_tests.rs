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
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "grant"}}"#.into())
        .expect("grant action");
    let post_id = engine.current_screen_id().expect("screen id after grant");
    assert_eq!(
        post_id, "link_responder_waiting",
        "grant must route to the responder waiting screen",
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

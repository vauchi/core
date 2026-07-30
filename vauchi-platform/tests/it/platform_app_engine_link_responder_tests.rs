// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform-level entry into the engine-owned link-mode responder
//! (ADR-049). Onboarding → `UserAction::LinkOpened { uri }` → grant must
//! navigate to `link_responder_waiting`, and — now that core drives the
//! relay escrow round-trip itself (`AppEngine::advance_link_responder_session`),
//! since no frontend ever executed the `RelayEscrow*` commands — the grant
//! envelope must carry **no** escrow commands.
//!
//! The full deposit → gate-poll → ready → retrieve → terminal behaviour is
//! covered end-to-end against a mock relay in
//! `vauchi-core/tests/it/link_responder_poll_tests.rs`; the responder state
//! machine's event handling is covered in
//! `vauchi-core/tests/it/link_responder_tests.rs`. Those replace the four
//! prior envelope-/hand-fed-event tests, whose contract (the frontend
//! executing escrow commands and reporting events) was the dead path this
//! ADR retires.

use vauchi_platform::{PlatformAppEngine, PlatformAppEngineTestHelpers};

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

// @internal
fn current_screen_id(engine: &PlatformAppEngine) -> String {
    let json = engine.current_screen_json().expect("current_screen_json");
    let v: serde_json::Value = serde_json::from_str(&json).expect("parse screen json");
    v.get("screen_id")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string()
}

#[test]
fn grant_navigates_to_responder_with_no_frontend_escrow_commands() {
    let (engine, _dir) = create_engine();
    drive_onboarding(&engine);
    let link_url = fresh_link_url();
    engine
        .handle_action_json(format!(r#"{{"LinkOpened":{{"uri":"{link_url}"}}}}"#))
        .expect("LinkOpened routes exchange URI to consent");
    assert_eq!(current_screen_id(&engine), "deep_link_consent",);

    let grant_json = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "grant"}}"#.into())
        .expect("grant action");
    assert_eq!(
        current_screen_id(&engine),
        "link_responder_waiting",
        "grant must route to the responder waiting screen — action returned {grant_json}",
    );

    // ADR-049: the responder's escrow deposits stay in the engine-owned
    // machine and are driven by `advance_link_responder_session`; they must
    // NOT be surfaced to the frontend command envelope (the dead path).
    let value: serde_json::Value =
        serde_json::from_str(&grant_json).expect("envelope is valid JSON");
    let commands = value["commands"]
        .as_array()
        .expect("envelope carries a commands array");
    let escrow_commands: Vec<&serde_json::Value> = commands
        .iter()
        .filter(|command| {
            command.get("RelayEscrowDeposit").is_some()
                || command.get("RelayEscrowCheck").is_some()
                || command.get("RelayEscrowRetrieve").is_some()
        })
        .collect();
    assert!(
        escrow_commands.is_empty(),
        "escrow is core-driven (ADR-049); the grant envelope must carry no \
         RelayEscrow* commands, got: {grant_json}",
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the engine-owned link-mode responder
//! (slice 32l Phase 2). The `AppEngine` owns the `LinkResponderSession`
//! while on `AppScreen::DeepLinkResponder`: its deposit commands ride out
//! in the grant action's `{action_result, commands}` envelope, and
//! `RelayEscrow*` hardware events drive it to a terminal screen via
//! `handle_hardware_event` (ADR-021/043 Humble UI; ADR-031 command/event).
//! The frontend pulls no session object — the retired cycle-thread
//! wrapper (`MobileLinkResponderSession`) and its
//! `current_link_responder_session` getter are gone.

use vauchi_platform::{MobileEvent, PlatformAppEngine};

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

/// Drive the engine to `link_responder_waiting` (onboarding →
/// `handle_deep_link_uri` → grant) and return the responder's
/// `gate_hash`, read from the `RelayEscrowCheck` deposit the grant
/// action emits into its envelope. This is the engine-owned
/// `LinkResponderSession`'s listen gate; hardware events must carry it
/// to be accepted (the per-build ephemeral key makes it
/// non-deterministic, so it MUST come from the live machine's own
/// deposit — not a second responder build).
fn drive_to_link_responder(engine: &PlatformAppEngine) -> Vec<u8> {
    drive_onboarding(engine);
    engine
        .handle_deep_link_uri(fresh_link_url())
        .expect("deep link routes to consent");
    assert_eq!(
        engine.current_screen_id().expect("screen id"),
        "deep_link_consent",
    );
    let grant_json = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "grant"}}"#.into())
        .expect("grant action");
    assert_eq!(
        engine.current_screen_id().expect("screen id after grant"),
        "link_responder_waiting",
        "grant must route to the responder waiting screen — action returned {grant_json}",
    );
    gate_hash_from_envelope(&grant_json)
}

/// Pull the responder gate_hash out of the `RelayEscrowCheck` command in
/// an action-result envelope's `commands` array. Confirms the engine
/// emitted the responder's escrow deposits on screen entry.
fn gate_hash_from_envelope(envelope_json: &str) -> Vec<u8> {
    let value: serde_json::Value =
        serde_json::from_str(envelope_json).expect("envelope is valid JSON");
    let commands = value["commands"]
        .as_array()
        .expect("envelope carries a commands array");
    for command in commands {
        if let Some(check) = command.get("RelayEscrowCheck") {
            let bytes = check["gate_hash"]
                .as_array()
                .expect("RelayEscrowCheck carries a gate_hash byte array");
            return bytes
                .iter()
                .map(|b| u8::try_from(b.as_u64().expect("gate_hash byte")).expect("byte in range"))
                .collect();
        }
    }
    panic!(
        "grant envelope must carry a RelayEscrowCheck with the responder gate_hash; got: \
         {envelope_json}"
    );
}

// @internal
#[test]
fn responder_screen_entry_emits_escrow_deposits() {
    let (engine, _dir) = create_engine();
    // `drive_to_link_responder` panics unless the grant envelope carried
    // a RelayEscrowCheck — i.e. the engine built + drove the responder on
    // screen entry and surfaced its deposits via ActionResult::Commands.
    let gate_hash = drive_to_link_responder(&engine);
    assert!(
        !gate_hash.is_empty(),
        "the responder must expose a non-empty gate_hash — confirms the DH + key-derive ran",
    );
}

// @internal
#[test]
fn relay_deposit_failure_drives_engine_to_failed_screen() {
    let (engine, _dir) = create_engine();
    let gate_hash = drive_to_link_responder(&engine);

    engine
        .handle_hardware_event(MobileEvent::RelayEscrowFailed {
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
    let gate_hash = drive_to_link_responder(&engine);

    // `RelayEscrowReady` advances Polling → Retrieving; an undecryptable
    // blob then fails the card decrypt, which must surface as the failed
    // screen.
    engine
        .handle_hardware_event(MobileEvent::RelayEscrowReady {
            gate_hash: gate_hash.clone(),
        })
        .expect("engine must accept RelayEscrowReady");
    engine
        .handle_hardware_event(MobileEvent::RelayEscrowBlobReceived {
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

// @internal
#[test]
fn relay_event_for_foreign_gate_leaves_responder_waiting() {
    let (engine, _dir) = create_engine();
    let _gate_hash = drive_to_link_responder(&engine);

    // A relay failure for an unrelated gate must be ignored — the
    // machine only reacts to events carrying its own gate_hash.
    engine
        .handle_hardware_event(MobileEvent::RelayEscrowFailed {
            gate_hash: vec![0u8; 32],
            reason: "deposit_rejected".into(),
        })
        .expect("engine must accept the event");

    assert_eq!(
        engine.current_screen_id().expect("screen id"),
        "link_responder_waiting",
        "an event for a foreign gate must not transition the responder",
    );
}

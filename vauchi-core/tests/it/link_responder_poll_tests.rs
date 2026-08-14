// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end tests for the core-driven link-responder escrow poll
//! (ADR-049 Phase 1 T3). Drives a real `AppEngine` — onboarded, pointed
//! at the in-process `MockRelay` — to the `DeepLinkResponder` screen, then
//! calls `poll_notifications` and asserts the engine advances the escrow
//! exchange itself (deposit → gate poll → ready → retrieve → terminal)
//! with no frontend command execution. This is the integration the
//! per-layer unit tests structurally could not cover — and which caught
//! the build-responder deposit-routing gap.
//!
//! The responder's escrow `card_key` is symmetric (escrow.rs), so the
//! peer blob the success test replays is the responder's own card deposit
//! captured off the mock — the only key-free way to mint a decryptable
//! blob now that the session keys are encapsulated. Poll 1 holds the gate
//! half-full (so the deposit can be captured); poll 2 returns `Count{2}`
//! and the blob, and the Ready cascades through the Retrieve in one tick.

#![cfg(all(feature = "network-http", feature = "storage"))]

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::VauchiConfig;
use vauchi_core::api::vauchi::VauchiBuilder;
use vauchi_core::exchange::link_mode::{initiator_generate, parse_exchange_deep_link};
use vauchi_protocol::escrow::EscrowResponse;

use crate::common::app_engine_helpers::drive_onboarding;
use crate::common::helpers::assert_contact_count;
use crate::common::mock_relay::{CannedResponse, MockRelay};

fn canned(resp: &EscrowResponse) -> CannedResponse {
    CannedResponse::ok_json(serde_json::to_vec(resp).expect("serialize EscrowResponse"))
}

/// Build an onboarded engine using the explicit testing-only direct seam and
/// isolated temp storage. Production action transports remain fail-closed.
fn onboarded_engine_at(mock: &MockRelay) -> (AppEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("vauchi.db");
    let mut config = VauchiConfig::with_storage_path(&db_path).with_relay_url(mock.url());
    config.ohttp.allow_direct = true;
    let vauchi = VauchiBuilder::new()
        .config(config)
        .build()
        .expect("build vauchi");
    let mut engine = AppEngine::new(vauchi);
    let _ = drive_onboarding(&mut engine);
    (engine, dir)
}

/// Navigate to the consent gate for a fresh link URL and grant it, landing
/// on the responder waiting screen.
fn grant_into_responder(engine: &mut AppEngine) {
    let (init, _) = initiator_generate();
    let payload = parse_exchange_deep_link(&init.url).expect("parse deep link");
    engine.navigate_to(AppScreen::DeepLinkConsent { payload });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "grant".to_string(),
    });
    assert_eq!(
        engine.current_screen().screen_id,
        "link_responder_waiting",
        "grant must land on the responder waiting screen"
    );
}

/// The blob of the `n`-th `Put` the engine has sent to the mock so far.
/// Deposit 0 is the handshake (epk); deposit 1 is the encrypted card.
fn nth_put_blob(mock: &MockRelay, n: usize) -> String {
    let puts: Vec<String> = mock
        .received()
        .iter()
        .filter_map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).ok())
        .filter(|b| b["escrow_action"] == "Put")
        .filter_map(|b| b["blob"].as_str().map(str::to_string))
        .collect();
    puts.get(n)
        .unwrap_or_else(|| panic!("expected at least {} Put(s), saw {}", n + 1, puts.len()))
        .clone()
}

// @internal
#[test]
fn poll_drives_retrieve_then_rejects_a_self_bootstrap() {
    // The escrow `card_key` is symmetric, so the only key-free blob the test
    // can mint is the responder's *own* card deposit, captured off the mock.
    // Under the v1 import this round-tripped into a (self-)contact; under the
    // v2 symmetric exchange (ADR-050 T5b) completing your own signed
    // bootstrap is a degenerate self-exchange — `complete_link_exchange`
    // rejects it *after* a successful AEAD decrypt + retrieve. This test
    // still exercises the full poll cascade (deposit → gate → Ready →
    // Retrieve → decrypt → completion) in one tick; the rejection at the
    // completion step is distinct from the AEAD-decrypt failure below.
    // A genuine two-party persist (different identities) is covered by
    // `complete_link_exchange_tests` + `link_two_party_exchange_tests`.
    let mock = MockRelay::start();
    // Poll 1: deposits + Check all see a half-full gate → stays Polling.
    mock.set_default(canned(&EscrowResponse::Count { count: 1 }));

    let (mut engine, _dir) = onboarded_engine_at(&mock);
    grant_into_responder(&mut engine);
    assert_contact_count(engine.vauchi(), 0);

    engine.poll_notifications();
    assert_eq!(engine.current_screen().screen_id, "link_responder_waiting");

    // Replay the responder's own card deposit as the peer blob (symmetric
    // card_key → it decrypts). Queue the ready sequence for poll 2: the
    // active Check sees both slots, then the Retrieve fetches the blob.
    let card_blob = nth_put_blob(&mock, 1);
    mock.queue("escrow", canned(&EscrowResponse::Count { count: 2 }));
    mock.queue("escrow", canned(&EscrowResponse::Blob { blob: card_blob }));

    engine.poll_notifications();

    assert_eq!(
        engine.current_screen().screen_id,
        "link_responder_failed",
        "the cascade reaches completion, where a self-bootstrap is rejected",
    );
    assert_contact_count(engine.vauchi(), 0);

    // The blob decrypted — that is what reaching completion means. Naming
    // this a decryption failure sends the user, and anyone debugging, at
    // the wrong layer
    // (2026-08-14-link-responder-reports-every-completion-failure-as-decrypt-error).
    let detail = failed_detail(&engine);
    assert!(
        !detail.contains("could not be decrypted"),
        "a completion rejection must not be reported as a decryption failure — \
         the AEAD decrypt succeeded. Got: {detail}"
    );
    assert!(
        detail.contains("your own"),
        "a self-exchange must say so, so the user knows retrying cannot help. \
         Got: {detail}"
    );
}

/// The `StatusIndicator` detail rendered on `link_responder_failed` — the
/// only user-visible statement of *why* the exchange failed.
fn failed_detail(engine: &AppEngine) -> String {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            vauchi_app::ui::Component::StatusIndicator { detail, .. } => detail.clone(),
            _ => None,
        })
        .expect("the failed screen must render a StatusIndicator detail")
}

// @internal
#[test]
fn poll_drives_to_failed_on_undecryptable_blob() {
    let mock = MockRelay::start();
    mock.set_default(canned(&EscrowResponse::Count { count: 1 }));

    let (mut engine, _dir) = onboarded_engine_at(&mock);
    grant_into_responder(&mut engine);

    engine.poll_notifications();
    mock.queue("escrow", canned(&EscrowResponse::Count { count: 2 }));
    mock.queue(
        "escrow",
        canned(&EscrowResponse::Blob {
            blob: "Z2FyYmFnZQ".to_string(), // "garbage" — fails AEAD decrypt
        }),
    );
    engine.poll_notifications();

    assert_eq!(engine.current_screen().screen_id, "link_responder_failed");
    assert_contact_count(engine.vauchi(), 0);
}

// @internal
#[test]
fn poll_stays_waiting_until_the_gate_fills() {
    let mock = MockRelay::start();
    // The gate never fills — every Check sees one slot.
    mock.set_default(canned(&EscrowResponse::Count { count: 1 }));

    let (mut engine, _dir) = onboarded_engine_at(&mock);
    grant_into_responder(&mut engine);

    engine.poll_notifications();
    engine.poll_notifications();

    assert_eq!(
        engine.current_screen().screen_id,
        "link_responder_waiting",
        "a half-filled gate keeps the responder polling"
    );
    assert_contact_count(engine.vauchi(), 0);
}

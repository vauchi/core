// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! End-to-end tests for the core-driven link-mode *initiator* escrow poll
//! (ADR-049). Drives a real `AppEngine` on `AppScreen::LinkExchange`,
//! pointed at the in-process `MockRelay`, and asserts
//! `advance_link_initiator_session` drives the two-gate flow itself: it
//! deposits presence, polls the handshake gate, retrieves the responder's
//! deposit, derives the escrow keys, deposits the card, and polls the escrow
//! gate — with no frontend command execution.
//!
//! The machine's own state transitions (the deposit → Retrieving → escrow →
//! Finalized) are unit-tested in `link_initiator_tests.rs`; these cover the
//! relay-driving the poll adds. A real responder opens the link the screen
//! shows, with the code it shows.

#![cfg(all(feature = "network-http", feature = "storage"))]

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use vauchi_app::ui::{AppEngine, AppScreen, Component, WorkflowEngine};
use vauchi_core::Command;
use vauchi_core::api::VauchiConfig;
use vauchi_core::api::vauchi::VauchiBuilder;
use vauchi_core::exchange::link_mode::{parse_link_url, responder_open};
use vauchi_core::exchange::link_pake::LinkCode;
use vauchi_protocol::escrow::EscrowResponse;

use crate::common::app_engine_helpers::drive_onboarding;
use crate::common::helpers::assert_contact_count;
use crate::common::mock_relay::{CannedResponse, MockRelay};

fn canned(resp: &EscrowResponse) -> CannedResponse {
    CannedResponse::ok_json(serde_json::to_vec(resp).expect("serialize EscrowResponse"))
}

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

fn shown_text(engine: &AppEngine, wanted: &str) -> String {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::Text { id, content, .. } if id == wanted => Some(content.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the screen shows {wanted}"))
}

/// The handshake deposit of a responder who opened the link the screen shows,
/// with the code it shows.
fn responder_deposit_b64(engine: &AppEngine) -> String {
    let parsed = parse_link_url(&shown_text(engine, "link_url")).expect("link parses");
    let code = LinkCode::parse(&shown_text(engine, "link_code")).expect("code parses");
    match responder_open(&parsed, &code)
        .expect("link opens")
        .deposit_command()
    {
        Command::RelayEscrowDeposit { encrypted_card, .. } => {
            URL_SAFE_NO_PAD.encode(encrypted_card)
        }
        other => panic!("expected the deposit, got {other:?}"),
    }
}

// @internal
#[test]
fn initiator_poll_drives_handshake_then_polls_the_escrow_gate() {
    let mock = MockRelay::start();
    // Poll 1: presence deposit + handshake Check see a not-yet-ready gate.
    mock.set_default(canned(&EscrowResponse::Count { count: 1 }));

    let (mut engine, _dir) = onboarded_engine_at(&mock);
    engine.navigate_to(AppScreen::LinkExchange);
    assert_contact_count(engine.vauchi(), 0);

    engine.poll_notifications();

    // Poll 2: handshake gate fills → retrieve the responder's deposit
    // → derive keys + deposit card → poll the escrow gate (still pending).
    mock.queue("escrow", canned(&EscrowResponse::Count { count: 2 })); // handshake Check
    mock.queue(
        "escrow",
        canned(&EscrowResponse::Blob {
            blob: responder_deposit_b64(&engine),
        }),
    ); // handshake Retrieve → the responder's deposit
    mock.queue("escrow", canned(&EscrowResponse::Stored)); // card deposit
    mock.queue("escrow", canned(&EscrowResponse::Count { count: 1 })); // escrow Check (not ready)

    engine.poll_notifications();

    // Reaching the retrieving screen proves: the handshake gate was driven,
    // the deposit reached the machine (it ran the ECDH and PAKE and derived
    // keys), and the machine moved to the escrow phase.
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_link_retrieving",
        "handshake → escrow phase must be driven by the core poll",
    );
    assert_contact_count(engine.vauchi(), 0);
}

// @internal
#[test]
fn initiator_poll_drives_to_failed_on_relay_failure() {
    let mock = MockRelay::start();
    // The handshake gate reports a terminal failure (e.g. gate expired).
    mock.set_default(canned(&EscrowResponse::Count { count: 1 }));

    let (mut engine, _dir) = onboarded_engine_at(&mock);
    engine.navigate_to(AppScreen::LinkExchange);

    engine.poll_notifications(); // presence + handshake poll
    mock.queue("escrow", canned(&EscrowResponse::NotFound)); // handshake Check → failure
    engine.poll_notifications();

    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_link_failed",
        "a relay failure on the handshake gate must drive the initiator to the failed screen",
    );
}

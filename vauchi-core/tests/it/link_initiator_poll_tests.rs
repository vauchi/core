// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end tests for the core-driven link-mode *initiator* escrow poll
//! (ADR-049). Drives a real `AppEngine` on `AppScreen::LinkExchange`,
//! pointed at the in-process `MockRelay`, and asserts
//! `advance_link_initiator_session` drives the two-gate flow itself: it
//! deposits presence, polls the handshake gate, retrieves the responder's
//! epk (fed as `LinkOpened`, not `RelayEscrowBlobReceived`), derives the
//! escrow keys, deposits the card, and polls the escrow gate — with no
//! frontend command execution.
//!
//! The machine's own state transitions (LinkOpened → Retrieving → escrow →
//! Finalized) are unit-tested in `link_initiator_tests.rs`; these cover the
//! relay-driving + the handshake→LinkOpened conversion the poll adds. A
//! valid X25519 point (the base point) stands in for the responder epk, so
//! the handshake ECDH succeeds and the machine reaches `Retrieving`.

#![cfg(all(feature = "network-http", feature = "storage"))]

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use vauchi_app::ui::{AppEngine, AppScreen, WorkflowEngine};
use vauchi_core::api::VauchiConfig;
use vauchi_core::api::vauchi::VauchiBuilder;
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

/// A valid (non-small-order) X25519 point — the base point — usable as the
/// responder's ephemeral public key so the initiator's handshake ECDH
/// succeeds.
fn responder_epk_b64() -> String {
    let mut epk = [0u8; 32];
    epk[0] = 9;
    URL_SAFE_NO_PAD.encode(epk)
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

    // Poll 2: handshake gate fills → retrieve the responder epk → LinkOpened
    // → derive keys + deposit card → poll the escrow gate (still pending).
    mock.queue("escrow", canned(&EscrowResponse::Count { count: 2 })); // handshake Check
    mock.queue(
        "escrow",
        canned(&EscrowResponse::Blob {
            blob: responder_epk_b64(),
        }),
    ); // handshake Retrieve → epk
    mock.queue("escrow", canned(&EscrowResponse::Stored)); // card deposit
    mock.queue("escrow", canned(&EscrowResponse::Count { count: 1 })); // escrow Check (not ready)

    engine.poll_notifications();

    // Reaching the retrieving screen proves: the handshake gate was driven,
    // the epk blob was fed as LinkOpened (handle_link_opened ran the ECDH +
    // derived keys), and the machine moved to the escrow phase.
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_link_retrieving",
        "handshake → LinkOpened → escrow phase must be driven by the core poll",
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

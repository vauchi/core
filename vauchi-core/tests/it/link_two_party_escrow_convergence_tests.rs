// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Full two-party Link exchange over one shared escrow store.
//!
//! The per-side poll tests (`link_initiator_poll_tests`,
//! `link_responder_poll_tests`) canned the peer's escrow blobs. This drives
//! *both* engines against a single stateful `MockRelay` (`enable_escrow_store`)
//! so the initiator's `Put` becomes the responder's `Get` — the convergence
//! the TUI harness cannot reach because it drives its two terminals
//! sequentially
//! (`problems/2026-09-09-tui-cannot-ingest-peer-exchange-payload`).
//!
//! Both parties share the handshake gate: it is `H(initiator_nonce ||
//! "handshake")`, derived by the initiator from its own nonce and by the
//! responder from the same nonce carried in the share URL (see
//! `link_mode::derive_handshake_slot`, called from both `initiator_generate`
//! and `responder_open`). The card gate, derived from the DH secret and the
//! code, is likewise shared when both hold the code.
//! Convergence only requires the relay to release a slot's blob to the *other*
//! party — which `relay/src/escrow.rs::get` does, and which the stateful
//! `MockRelay` now mirrors. An earlier fixture bug returned the *requester's
//! own* slot, so each side read back only its own deposit and neither
//! exchanged epk or card; that is fixed, and this test now passes.

use vauchi_app::ui::{
    AppEngine, AppScreen, Component, LINK_RESPONDER_ACTION_CONFIRM_CODE,
    LINK_RESPONDER_CODE_INPUT_ID, UserAction, WorkflowEngine,
};
use vauchi_core::api::VauchiConfig;
use vauchi_core::api::vauchi::VauchiBuilder;
use vauchi_core::exchange::link_mode::parse_exchange_deep_link;

use crate::common::app_engine_helpers::drive_onboarding;
use crate::common::mock_relay::MockRelay;

fn onboarded_engine_at(mock: &MockRelay) -> (AppEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut config =
        VauchiConfig::with_storage_path(dir.path().join("vauchi.db")).with_relay_url(mock.url());
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
        .unwrap_or_else(|| panic!("the screen renders {wanted}"))
}

/// Read the exchange link the initiator's share screen renders.
fn share_url(engine: &AppEngine) -> String {
    shown_text(engine, "link_url")
}

fn open_link(engine: &mut AppEngine, link: &str) {
    let payload = parse_exchange_deep_link(link).expect("parse the link");
    engine.navigate_to(AppScreen::DeepLinkConsent { payload });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "grant".to_string(),
    });
    assert_eq!(engine.current_screen().screen_id, "link_responder_code");
}

fn type_code(engine: &mut AppEngine, code: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: LINK_RESPONDER_CODE_INPUT_ID.to_string(),
        value: code.to_string(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: LINK_RESPONDER_ACTION_CONFIRM_CODE.to_string(),
    });
}

fn escrow_puts(mock: &MockRelay) -> usize {
    mock.received()
        .iter()
        .filter_map(|r| serde_json::from_slice::<serde_json::Value>(&r.body).ok())
        .filter(|b| b["escrow_action"] == "Put")
        .count()
}

fn contact_count(engine: &AppEngine) -> usize {
    engine
        .vauchi()
        .list_contacts()
        .expect("list contacts")
        .len()
}

// @scenario: link_exchange :: Two parties converge over one relay escrow
#[test]
fn initiator_and_responder_converge_over_a_shared_escrow_store() {
    let mock = MockRelay::start();
    mock.enable_escrow_store();

    let (mut alice, _a) = onboarded_engine_at(&mock);
    let (mut bob, _b) = onboarded_engine_at(&mock);

    // Alice opens Link mode; her initiator generates the share URL and
    // begins depositing presence to the escrow gate.
    alice.navigate_to(AppScreen::LinkExchange);
    assert_eq!(alice.current_screen().screen_id, "exchange_share_url");
    let alice_link = share_url(&alice);
    assert!(alice_link.starts_with("https://vauchi.app/x#pk="));
    // The landing page names the sender from the fragment (#277): "Alice"
    // in base64url.
    assert!(
        alice_link.ends_with("&nm=QWxpY2U"),
        "the share link names Alice: {alice_link}"
    );

    // Bob opens Alice's link, grants and types the code she sent him another
    // way — the camera-less paste route lands here too, via `LinkOpened`.
    open_link(&mut bob, &alice_link);
    type_code(&mut bob, &shown_text(&alice, "link_code"));
    assert_eq!(bob.current_screen().screen_id, "link_responder_waiting");

    assert_eq!(contact_count(&alice), 0);
    assert_eq!(contact_count(&bob), 0);

    let mut converged = false;
    for _ in 0..40 {
        alice.poll_notifications();
        bob.poll_notifications();
        if contact_count(&alice) >= 1 && contact_count(&bob) >= 1 {
            converged = true;
            break;
        }
    }

    assert!(
        converged,
        "both parties must gain a contact over the shared escrow store; \
         alice={} bob={} (alice screen={}, bob screen={})",
        contact_count(&alice),
        contact_count(&bob),
        alice.current_screen().screen_id,
        bob.current_screen().screen_id,
    );
}

// Whoever holds the link and not the code gets no contact on either side, however
// long both keep polling (#727, SC-74).
// @claim: S9
// @scenario: link_exchange :: A wrong code makes no contact
#[test]
fn a_wrong_code_makes_no_contact_on_either_side() {
    let mock = MockRelay::start();
    mock.enable_escrow_store();
    let (mut alice, _a) = onboarded_engine_at(&mock);
    let (mut mallory, _m) = onboarded_engine_at(&mock);
    alice.navigate_to(AppScreen::LinkExchange);
    let shown = shown_text(&alice, "link_code");
    let wrong = if shown == "000 000" {
        "000 001"
    } else {
        "000 000"
    };

    open_link(&mut mallory, &share_url(&alice));
    type_code(&mut mallory, wrong);
    for _ in 0..40 {
        alice.poll_notifications();
        mallory.poll_notifications();
    }

    assert_eq!(contact_count(&alice), 0);
    assert_eq!(contact_count(&mallory), 0);
    assert_eq!(mallory.current_screen().screen_id, "link_responder_waiting");
}

// Opening a link and not entering a good code uses nothing up: the person it
// was meant for can still use it (#727).
// @scenario: link_exchange :: Opening a link without a code sends nothing
#[test]
fn nothing_reaches_the_relay_until_a_well_formed_code_is_typed() {
    let mock = MockRelay::start();
    mock.enable_escrow_store();
    let (mut bob, _b) = onboarded_engine_at(&mock);
    let (init, _) = vauchi_core::exchange::link_mode::initiator_generate();

    open_link(&mut bob, &init.url);
    bob.poll_notifications();
    assert_eq!(escrow_puts(&mock), 0);

    for malformed in ["", "12345", "1234567", "abcdef"] {
        type_code(&mut bob, malformed);
        bob.poll_notifications();
        let screen = bob.current_screen();
        assert_eq!(screen.screen_id, "link_responder_code", "{malformed:?}");
        let error = screen.components.iter().find_map(|c| match c {
            Component::TextInput {
                validation_error, ..
            } => validation_error.clone(),
            _ => None,
        });
        assert!(error.is_some(), "{malformed:?} must say what is wrong");
        assert_eq!(escrow_puts(&mock), 0, "{malformed:?}");
    }

    type_code(&mut bob, init.code.as_str());
    bob.poll_notifications();
    assert_eq!(bob.current_screen().screen_id, "link_responder_waiting");
    assert_eq!(escrow_puts(&mock), 1, "only the handshake deposit, no card");
}

// @internal
#[test]
fn the_code_stays_on_screen_after_sharing_but_never_reaches_the_share_sheet() {
    let mock = MockRelay::start();
    mock.enable_escrow_store();
    let (mut alice, _a) = onboarded_engine_at(&mock);
    alice.navigate_to(AppScreen::LinkExchange);
    let code = shown_text(&alice, "link_code");

    let result = alice.handle_action(UserAction::ActionPressed {
        action_id: "share".to_string(),
    });

    assert_eq!(alice.current_screen().screen_id, "exchange_link_waiting");
    assert_eq!(shown_text(&alice, "link_code"), code);
    let shared = format!("{result:?}");
    assert!(shared.contains("vauchi.app/x#"), "{shared}");
    assert!(!shared.contains(&code.replace(' ', "")), "{shared}");
}

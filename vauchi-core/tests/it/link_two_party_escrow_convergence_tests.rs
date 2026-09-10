// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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
//! and `responder_respond`). The DH-derived card gate is likewise shared.
//! Convergence only requires the relay to release a slot's blob to the *other*
//! party — which `relay/src/escrow.rs::get` does, and which the stateful
//! `MockRelay` now mirrors. An earlier fixture bug returned the *requester's
//! own* slot, so each side read back only its own deposit and neither
//! exchanged epk or card; that is fixed, and this test now passes.

use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
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

/// Read the `vauchi://exchange` URL the initiator's share screen renders.
fn share_url(engine: &AppEngine) -> String {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::Text { id, content, .. } if id == "link_url" => Some(content.clone()),
            _ => None,
        })
        .expect("the share screen renders the link URL")
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
    assert!(alice_link.starts_with("vauchi://exchange?pk="));

    // Bob opens Alice's link and grants — the camera-less paste route lands
    // here too, via `LinkOpened`.
    let payload = parse_exchange_deep_link(&alice_link).expect("parse Alice's link");
    bob.navigate_to(AppScreen::DeepLinkConsent { payload });
    let _ = bob.handle_action(UserAction::ActionPressed {
        action_id: "grant".to_string(),
    });
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

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A device without a camera (the TUI) still completes a Link exchange:
//! the peer's `vauchi://exchange` link is pasted on the share screen and
//! the app engine routes it exactly like an opened deep link.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn peer_link() -> String {
    let (initiation, _presence) = vauchi_core::exchange::link_mode::initiator_generate();
    initiation.url
}

fn engine_on_share_screen() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let entry = engine.navigate_to(AppScreen::LinkExchange);
    assert_eq!(entry.screen_id, "exchange_share_url");
    engine
}

// @scenario: link_exchange :: Pasting the peer's link opens the consent screen
#[test]
fn opening_a_pasted_peer_link_routes_to_deep_link_consent() {
    let mut engine = engine_on_share_screen();
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "peer_link_input".into(),
        value: peer_link(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "open_peer_link".into(),
    });
    assert!(
        matches!(
            engine.current_app_screen(),
            AppScreen::DeepLinkConsent { .. }
        ),
        "open_peer_link must route the typed link like an opened deep link, got {:?}",
        engine.current_app_screen()
    );
}

// @scenario: link_exchange :: An empty peer link does nothing
#[test]
fn opening_an_empty_peer_link_stays_on_the_share_screen() {
    let mut engine = engine_on_share_screen();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "open_peer_link".into(),
    });
    assert_eq!(*engine.current_app_screen(), AppScreen::LinkExchange);
}

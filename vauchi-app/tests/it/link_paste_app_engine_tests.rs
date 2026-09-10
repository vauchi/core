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

// The TUI reaches the same route through generic presentation events:
// the typed value arrives as ValueChanged and Return as InputSubmitted.
// @scenario: link_exchange :: Pasting the peer's link opens the consent screen
#[test]
fn submitting_the_peer_link_input_through_presentation_events_routes_to_consent() {
    use vauchi_core::{BindingId, Event, InputValue, SurfaceId};

    let mut engine = engine_on_share_screen();
    // Read the surface and the input's Core-minted binding from the batch
    // rather than constructing them (the shell never builds ids).
    let commands = engine.initial_commands().expect("initial commands");
    let (surface_id, binding_id): (SurfaceId, BindingId) = commands
        .iter()
        .find_map(|c| match c {
            vauchi_core::Command::ReplaceSurface { surface } => {
                surface.nodes.iter().find_map(|n| match n {
                    vauchi_core::PresentationNode::Input { binding_id, .. } => {
                        Some((surface.surface_id.clone(), binding_id.clone()))
                    }
                    _ => None,
                })
            }
            _ => None,
        })
        .expect("the share surface carries one input");
    // A terminal reports every keystroke; the binding must survive the
    // re-render each one triggers.
    let link = peer_link();
    for prefix in [&link[..10], &link[..20], link.as_str()] {
        engine
            .dispatch(Event::ValueChanged {
                surface_id: surface_id.clone(),
                binding_id: binding_id.clone(),
                value: InputValue::Text(prefix.to_string()),
            })
            .expect("value change dispatches");
    }
    engine
        .dispatch(Event::InputSubmitted {
            surface_id,
            binding_id,
        })
        .expect("submit dispatches");
    assert!(
        matches!(
            engine.current_app_screen(),
            AppScreen::DeepLinkConsent { .. }
        ),
        "Return in the peer-link field must open the consent screen, got {:?}",
        engine.current_app_screen()
    );
}

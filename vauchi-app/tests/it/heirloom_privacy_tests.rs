// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The paper heirloom export, reached from the privacy screen
//! (private#363). Traces to features/paper_heirloom.feature.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::SymmetricKey;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;

fn engine_with_bob() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let bob = Contact::from_exchange(
        [5u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    vauchi.add_contact(bob).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Privacy);
    engine
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

// @scenario: paper_heirloom :: The export is plaintext and leaves the encryption envelope
#[test]
fn heirloom_shows_the_plaintext_warning_before_exporting() {
    let mut engine = engine_with_bob();

    let screen = match press(&mut engine, "heirloom") {
        ActionResult::NavigateTo(screen) | ActionResult::UpdateScreen(screen) => screen,
        other => panic!("expected the warning screen, got {other:?}"),
    };

    assert_eq!(screen.screen_id, "confirm_heirloom");
    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::InfoPanel { id, .. } if id == "heirloom_warning")),
        "the plaintext warning is shown: {:?}",
        screen.components
    );
    let actions: Vec<&str> = screen
        .contextual_actions
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(actions, ["confirm_heirloom", "cancel"]);
}

// @scenario: paper_heirloom :: The export is plaintext and leaves the encryption envelope
#[test]
fn cancelling_the_warning_exports_nothing() {
    let mut engine = engine_with_bob();
    let _ = press(&mut engine, "heirloom");

    let result = press(&mut engine, "cancel");

    match result {
        ActionResult::NavigateTo(screen) | ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "privacy_settings")
        }
        other => panic!("expected the privacy overview, got {other:?}"),
    }
}

// @scenario: paper_heirloom :: Export my contacts as a printable document
#[test]
fn confirming_exports_the_heirloom_document() {
    let mut engine = engine_with_bob();
    let _ = press(&mut engine, "heirloom");

    let html = match press(&mut engine, "confirm_heirloom") {
        ActionResult::HeirloomExportComplete { html } => html,
        other => panic!("expected HeirloomExportComplete, got {other:?}"),
    };

    assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
    assert!(html.contains("<h2>Bob</h2>"), "{html}");
    assert!(html.contains("Alice</h1>"), "{html}");
}

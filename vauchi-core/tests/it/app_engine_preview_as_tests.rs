// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine preview-as navigation tests.
//!
//! Covers Task 12: preview_as(contact_id) sets transient state,
//! navigates MyInfo to PreviewAs mode, and exit-preview clears it.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

/// Helper: create a Vauchi with identity + one contact, return (vauchi, contact_id).
fn vauchi_with_contact() -> (Vauchi, String) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let card = ContactCard::new("Bob");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([3u8; 32], card, shared_key, 0);
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    (vauchi, id)
}

// ── preview_as() ──────────────────────────────────────────────────────

// @internal
#[test]
fn test_preview_as_navigates_to_my_info() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.preview_as(contact_id);
    assert_eq!(screen.screen_id, "my_info");
}

// @internal
#[test]
fn test_preview_as_shows_preview_mode_title() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.preview_as(contact_id);
    // Title in PreviewAs mode is "Viewing as {contact_name}"
    assert!(
        screen.title.starts_with("Viewing as"),
        "expected PreviewAs title starting with 'Viewing as', got: {:?}",
        screen.title
    );
}

// @internal
#[test]
fn test_preview_as_shows_banner_component() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.preview_as(contact_id);

    let has_banner = screen.components.iter().any(|c| {
        matches!(c, Component::Banner { text, ..
        } if text.contains("Viewing as"))
    });
    assert!(
        has_banner,
        "PreviewAs mode must show a Banner with 'Viewing as' text, components: {:?}",
        screen.components
    );
}

// @internal
#[test]
fn test_preview_as_banner_has_exit_preview_action_id() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.preview_as(contact_id);

    let banner = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::Banner { .. }));
    let Some(Component::Banner { action_id, .. }) = banner else {
        panic!("No Banner component found in PreviewAs screen");
    };
    assert_eq!(
        action_id, "exit-preview",
        "Banner action_id must be 'exit-preview'"
    );
}

// @internal
#[test]
fn test_preview_as_screen_has_exit_preview_action() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.preview_as(contact_id);

    let has_exit_action = screen.actions.iter().any(|a| a.id == "exit-preview");
    assert!(
        has_exit_action,
        "PreviewAs screen must include 'exit-preview' in screen actions, got: {:?}",
        screen.actions
    );
}

// @internal
#[test]
fn test_preview_as_shows_contact_name_in_banner() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.preview_as(contact_id);

    let banner_text = screen.components.iter().find_map(|c| {
        if let Component::Banner { text, .. } = c {
            Some(text.clone())
        } else {
            None
        }
    });
    let text = banner_text.expect("Banner component must be present");
    assert!(
        text.contains("Bob"),
        "Banner text must contain the contact name 'Bob', got: {:?}",
        text
    );
}

// ── exit-preview action ───────────────────────────────────────────────

// @internal
#[test]
fn test_exit_preview_returns_to_edit_mode() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);

    engine.preview_as(contact_id);
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "exit-preview".into(),
    });

    let screen = match result {
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s) => s,
        other => panic!("Expected UpdateScreen or NavigateTo, got: {other:?}"),
    };

    assert_eq!(screen.screen_id, "my_info");
}

// @internal
#[test]
fn test_exit_preview_removes_banner() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);

    engine.preview_as(contact_id);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "exit-preview".into(),
    });

    let screen = match result {
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s) => s,
        other => panic!("Expected UpdateScreen or NavigateTo, got: {other:?}"),
    };

    let has_banner = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::Banner { .. }));
    assert!(
        !has_banner,
        "After exit-preview, MyInfo should not show a Banner component"
    );
}

// @internal
#[test]
fn test_exit_preview_screen_has_normal_my_info_actions() {
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);

    engine.preview_as(contact_id);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "exit-preview".into(),
    });

    let screen = match result {
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s) => s,
        other => panic!("Expected UpdateScreen or NavigateTo, got: {other:?}"),
    };

    // Normal MyInfo has "add_field" and "toggle_view" actions, not "exit-preview"
    let has_add_field = screen.actions.iter().any(|a| a.id == "add_field");
    let has_exit_preview = screen.actions.iter().any(|a| a.id == "exit-preview");

    assert!(
        has_add_field,
        "Normal MyInfo must have 'add_field' action after exiting preview"
    );
    assert!(
        !has_exit_preview,
        "Normal MyInfo must NOT have 'exit-preview' action after exiting preview"
    );
}

// ── transient state (not serialized, machine-local) ──────────────────

// @internal
#[test]
fn test_preview_as_state_cleared_by_exit_preview_not_left_over() {
    // After exit-preview, a subsequent navigate to MyInfo must not re-enter preview mode.
    let (vauchi, contact_id) = vauchi_with_contact();
    let mut engine = AppEngine::new(vauchi);

    engine.preview_as(contact_id);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "exit-preview".into(),
    });

    // Navigate away and back — should remain in normal mode
    engine.navigate_to(AppScreen::Contacts);
    let screen = engine.navigate_to(AppScreen::MyInfo);

    let has_banner = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::Banner { .. }));
    assert!(
        !has_banner,
        "After exit-preview, subsequent MyInfo navigation must not re-enter preview mode"
    );

    let has_add_field = screen.actions.iter().any(|a| a.id == "add_field");
    assert!(
        has_add_field,
        "Normal MyInfo must have 'add_field' after clearing preview state"
    );
}

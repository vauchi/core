// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for AvatarEditor wiring in AppEngine.
//!
//! Tests that AppEngine correctly navigates to the AvatarEditor screen,
//! handles completion (persist avatar / cancel), routes hardware events,
//! and that MyInfo shows an AvatarPreview component.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::ExchangeHardwareEvent;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

// ── Navigation ─────────────────────────────────────────────────

// @scenario: avatar_editor_wiring :: AvatarEditor is a valid AppScreen
#[test]
fn avatar_editor_screen_id_roundtrip() {
    let screen = AppScreen::AvatarEditor;
    assert_eq!(screen.screen_id(), "avatar_editor");
    assert_eq!(
        AppScreen::from_screen_id("avatar_editor"),
        Some(AppScreen::AvatarEditor)
    );
}

// @scenario: avatar_editor_wiring :: Navigate to AvatarEditor shows source picker
#[test]
fn navigate_to_avatar_editor_shows_source_picker() {
    let mut engine = engine_with_identity();
    let screen = engine.navigate_to(AppScreen::AvatarEditor);
    assert_eq!(screen.screen_id, "avatar_editor");
    assert_eq!(screen.title, "Choose Avatar");
}

// @scenario: avatar_editor_wiring :: edit_avatar action on MyInfo navigates to AvatarEditor
#[test]
fn edit_avatar_action_navigates_to_avatar_editor() {
    let mut engine = engine_with_identity();
    // Navigate to MyInfo first
    let _ = engine.navigate_to(AppScreen::MyInfo);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit_avatar".into(),
    });
    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "avatar_editor");
        }
        other => panic!("expected NavigateTo avatar_editor, got {other:?}"),
    }
}

// ── Completion: save ───────────────────────────────────────────

// @scenario: avatar_editor_wiring :: Save avatar persists and navigates back
#[test]
fn save_avatar_persists_and_navigates_back() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::AvatarEditor);

    // Simulate image received
    let avatar_data = vauchi_core::avatar::generate_initials_avatar([0, 128, 255], 32);
    let _ =
        engine.handle_hardware_event(ExchangeHardwareEvent::ImageReceived { data: avatar_data });

    // Save
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });

    // Should navigate back (to MyInfo or wherever we came from)
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "save should navigate back, got {result:?}"
    );

    // Avatar should be persisted in own card
    let card = engine.vauchi().own_card().unwrap().unwrap();
    assert!(card.avatar().is_some(), "avatar should be persisted");
    assert_eq!(
        &card.avatar().unwrap()[0..4],
        b"RIFF",
        "persisted avatar must be WebP"
    );
}

// ── Completion: cancel ─────────────────────────────────────────

// @scenario: avatar_editor_wiring :: Cancel navigates back without persisting
#[test]
fn cancel_navigates_back_without_persisting() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::AvatarEditor);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "cancel should navigate back, got {result:?}"
    );

    // Avatar should NOT be persisted
    let card = engine.vauchi().own_card().unwrap().unwrap();
    assert!(card.avatar().is_none(), "cancel should not persist avatar");
}

// ── Hardware events ────────────────────────────────────────────

// @scenario: avatar_editor_wiring :: Hardware events routed to AvatarEditor
#[test]
fn hardware_events_routed_to_avatar_editor() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::AvatarEditor);

    let avatar_data = vauchi_core::avatar::generate_initials_avatar([255, 0, 0], 32);
    let result =
        engine.handle_hardware_event(ExchangeHardwareEvent::ImageReceived { data: avatar_data });

    assert!(result.is_some(), "hardware event should be handled");
    match result.unwrap() {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "avatar_editor");
            // Should now be in editing state with AvatarPreview
            let has_preview = screen
                .components
                .iter()
                .any(|c| matches!(c, Component::AvatarPreview { .. }));
            assert!(has_preview, "editing state should show AvatarPreview");
        }
        other => panic!("expected UpdateScreen, got {other:?}"),
    }
}

// ── MyInfo AvatarPreview ───────────────────────────────────────

// @scenario: avatar_editor_wiring :: MyInfo shows AvatarPreview component
#[test]
fn my_info_shows_avatar_preview() {
    let mut engine = engine_with_identity();
    let screen = engine.navigate_to(AppScreen::MyInfo);

    let has_avatar = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::AvatarPreview { .. }));
    assert!(
        has_avatar,
        "MyInfo should show an AvatarPreview component at the top"
    );
}

// @scenario: avatar_editor_wiring :: MyInfo AvatarPreview shows avatar data after save
#[test]
fn my_info_avatar_preview_shows_saved_avatar() {
    let mut engine = engine_with_identity();

    // Set avatar via the editor flow
    let _ = engine.navigate_to(AppScreen::AvatarEditor);
    let avatar_data = vauchi_core::avatar::generate_initials_avatar([0, 200, 100], 32);
    let _ =
        engine.handle_hardware_event(ExchangeHardwareEvent::ImageReceived { data: avatar_data });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });

    // Navigate to MyInfo and check the preview
    let screen = engine.navigate_to(AppScreen::MyInfo);
    let avatar_preview = screen.components.iter().find_map(|c| match c {
        Component::AvatarPreview { image_data, .. } => Some(image_data.clone()),
        _ => None,
    });
    assert!(
        avatar_preview.is_some(),
        "MyInfo should have AvatarPreview component"
    );
    assert!(
        avatar_preview.unwrap().is_some(),
        "AvatarPreview should have image data after avatar was saved"
    );
}

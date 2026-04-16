// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the AvatarEditorEngine (core-driven avatar editing screen).

use vauchi_app::ui::{ActionResult, AvatarEditorEngine, Component, UserAction, WorkflowEngine};
use vauchi_core::exchange::{ExchangeCommand, ExchangeHardwareEvent};

fn tiny_avatar() -> Vec<u8> {
    // Use core's avatar generation to produce valid image bytes
    vauchi_core::avatar::generate_initials_avatar([255, 0, 0], 32)
}

// ── Source picker state ─────────────────────────────────────────

// @scenario: avatar_editor :: Initial screen shows source picker
#[test]
fn initial_screen_shows_source_picker() {
    let engine = AvatarEditorEngine::new("Alice".into(), false);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "avatar_editor");
    assert!(!screen.components.is_empty());
    // Should have an ActionList with source options
    let has_action_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ActionList { .. }));
    assert!(has_action_list, "source picker needs an ActionList");
}

// @scenario: avatar_editor :: Camera action emits image capture command
#[test]
fn camera_action_emits_capture_command() {
    let mut engine = AvatarEditorEngine::new("Alice".into(), false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "source_camera".into(),
    });
    match result {
        ActionResult::ExchangeCommands { commands } => {
            assert!(
                commands
                    .iter()
                    .any(|c| matches!(c, ExchangeCommand::ImageCaptureFromCamera)),
                "expected ImageCaptureFromCamera command"
            );
        }
        other => panic!("expected ExchangeCommands, got {other:?}"),
    }
}

// @scenario: avatar_editor :: Photos action emits pick command
#[test]
fn photos_action_emits_pick_command() {
    let mut engine = AvatarEditorEngine::new("Alice".into(), false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "source_photos".into(),
    });
    match result {
        ActionResult::ExchangeCommands { commands } => {
            let has_pick = commands.iter().any(|c| {
                matches!(
                    c,
                    ExchangeCommand::ImagePickFromLibrary | ExchangeCommand::ImagePickFromFile
                )
            });
            assert!(has_pick, "expected image pick command");
        }
        other => panic!("expected ExchangeCommands, got {other:?}"),
    }
}

// @scenario: avatar_editor :: Generate action transitions to generator
#[test]
fn generate_action_transitions_to_generator() {
    let mut engine = AvatarEditorEngine::new("Alice".into(), false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "source_generate".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "expected UpdateScreen"
    );
    let screen = engine.current_screen();
    // Should have an AvatarPreview with image data (generated)
    let has_preview = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::AvatarPreview { .. }));
    assert!(has_preview, "generator state needs an AvatarPreview");
}

// ── Image received ──────────────────────────────────────────────

// @scenario: avatar_editor :: Image received transitions to editing
#[test]
fn image_received_transitions_to_editing() {
    let mut engine = AvatarEditorEngine::new("Alice".into(), false);
    let result = engine
        .handle_hardware_event(ExchangeHardwareEvent::ImageReceived {
            data: tiny_avatar(),
        })
        .expect("should handle image received");
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "expected UpdateScreen after image received"
    );
    let screen = engine.current_screen();
    let has_preview = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::AvatarPreview { .. }));
    assert!(has_preview, "editing state needs an AvatarPreview");
    let has_slider = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::Slider { .. }));
    assert!(has_slider, "editing state needs a brightness Slider");
}

// @scenario: avatar_editor :: Image pick cancelled stays on source picker
#[test]
fn image_pick_cancelled_stays_on_source_picker() {
    let mut engine = AvatarEditorEngine::new("Alice".into(), false);
    let result = engine
        .handle_hardware_event(ExchangeHardwareEvent::ImagePickCancelled)
        .expect("should handle cancel");
    assert!(matches!(result, ActionResult::UpdateScreen(_)));
    // Still on source picker — should have ActionList
    let screen = engine.current_screen();
    let has_action_list = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::ActionList { .. }));
    assert!(has_action_list, "should remain on source picker");
}

// ── Editing state ───────────────────────────────────────────────

// @scenario: avatar_editor :: Save in editing state completes
#[test]
fn save_in_editing_completes() {
    let mut engine = AvatarEditorEngine::new("Alice".into(), false);
    let _ = engine.handle_hardware_event(ExchangeHardwareEvent::ImageReceived {
        data: tiny_avatar(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "save should complete"
    );
    assert!(!engine.was_cancelled());
    assert!(
        engine.result_avatar().is_some(),
        "result avatar should be set after save"
    );
}

// @scenario: avatar_editor :: Cancel completes with cancelled flag
#[test]
fn cancel_completes_with_cancelled_flag() {
    let mut engine = AvatarEditorEngine::new("Alice".into(), false);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    assert!(matches!(result, ActionResult::Complete));
    assert!(engine.was_cancelled());
    assert!(engine.result_avatar().is_none());
}

// ── Generator state ─────────────────────────────────────────────

// @scenario: avatar_editor :: Mandelbrot regenerate bumps seed
#[test]
fn mandelbrot_regenerate_updates_preview() {
    let mut engine = AvatarEditorEngine::new("Alice".into(), false);
    // Enter generator
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "source_generate".into(),
    });
    // Switch to mandelbrot
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "gen_style".into(),
        item_id: "mandelbrot".into(),
    });
    let screen_before = engine.current_screen();
    // Regenerate
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "regenerate".into(),
    });
    let screen_after = engine.current_screen();
    // Preview data should differ (different seed)
    let get_preview_data = |screen: &vauchi_app::ui::ScreenModel| {
        screen.components.iter().find_map(|c| match c {
            Component::AvatarPreview { image_data, .. } => image_data.clone(),
            _ => None,
        })
    };
    assert_ne!(
        get_preview_data(&screen_before),
        get_preview_data(&screen_after),
        "regenerate should produce different preview"
    );
}

// @scenario: avatar_editor :: Save in generator state completes with avatar
#[test]
fn save_in_generator_completes_with_avatar() {
    let mut engine = AvatarEditorEngine::new("Alice".into(), false);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "source_generate".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "use".into(),
    });
    assert!(matches!(result, ActionResult::Complete));
    assert!(!engine.was_cancelled());
    let avatar = engine.result_avatar().expect("result avatar should be set");
    assert_eq!(&avatar[0..4], b"RIFF", "avatar must be WebP");
}

// @scenario: avatar_editor :: Brightness slider updates preview
#[test]
fn brightness_slider_updates_preview() {
    let mut engine = AvatarEditorEngine::new("Alice".into(), false);
    let _ = engine.handle_hardware_event(ExchangeHardwareEvent::ImageReceived {
        data: tiny_avatar(),
    });
    let result = engine.handle_action(UserAction::SliderChanged {
        component_id: "brightness".into(),
        value_milli: -200, // -0.2
    });
    assert!(matches!(result, ActionResult::UpdateScreen(_)));
    // Verify brightness is reflected in the preview component
    let screen = engine.current_screen();
    let brightness = screen.components.iter().find_map(|c| match c {
        Component::AvatarPreview { brightness, .. } => Some(*brightness),
        _ => None,
    });
    assert!(
        (brightness.unwrap_or(0.0) - (-0.2)).abs() < 0.01,
        "brightness should be -0.2"
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{
    CANCEL_ACTION_ID, DEVICE_NAME_INPUT_ID, DeviceLinkJoinEngine, JOIN_ACTION_ID, RETRY_ACTION_ID,
};
use crate::ui::{DeviceLinkJoinUpdate, EngineUpdate, UserAction, WorkflowEngine};

fn engine() -> DeviceLinkJoinEngine {
    DeviceLinkJoinEngine::new("My Phone".to_string())
}

fn screen_id(engine: &DeviceLinkJoinEngine) -> String {
    engine.current_screen().screen_id
}

// @internal
#[test]
fn new_starts_at_enter_name() {
    let engine = engine();
    assert_eq!(screen_id(&engine), "device_link_join");
    let actions: Vec<String> = engine
        .current_screen()
        .contextual_actions
        .iter()
        .map(|a| a.id.clone())
        .collect();
    assert!(actions.contains(&JOIN_ACTION_ID.to_string()));
    assert!(actions.contains(&CANCEL_ACTION_ID.to_string()));
}

// @internal
#[test]
fn text_changed_updates_name_and_join_enable_state() {
    let mut engine = engine();
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: DEVICE_NAME_INPUT_ID.into(),
        value: "".into(),
    });
    let join_enabled = engine
        .current_screen()
        .contextual_actions
        .iter()
        .find(|a| a.id == JOIN_ACTION_ID)
        .map(|a| a.enabled)
        .unwrap();
    // Contract change: an empty field is no longer "no name", it is
    // "use this device's own name" — the default moved from the field's
    // content to its placeholder so typing cannot concatenate the two.
    // A blank *default* is still a validation failure, which
    // `join_action_with_empty_name_returns_validation_error` pins.
    assert!(
        join_enabled,
        "clearing the field falls back to the device's own name, so join stays available"
    );
    assert_eq!(engine.device_name(), "My Phone");

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: DEVICE_NAME_INPUT_ID.into(),
        value: "Alice's Phone".into(),
    });
    let join_enabled = engine
        .current_screen()
        .contextual_actions
        .iter()
        .find(|a| a.id == JOIN_ACTION_ID)
        .map(|a| a.enabled)
        .unwrap();
    assert!(join_enabled, "join must be enabled when name is non-empty");
    assert_eq!(engine.device_name(), "Alice's Phone");
}

/// The field is pre-filled with the device's own name so the common case
/// is "just confirm". But a caret lands after existing text, so a user
/// who types without clearing gets the two concatenated — a device
/// registered as "New DeviceiPhone SE" was the observed result
/// (2026-08-14-device-link-host-never-shows-the-confirmation-code).
///
/// Offering the default as a placeholder keeps the convenience (leave it
/// blank and the default applies) without putting text in the way.
// @scenario: device_management :: the joiner's name field does not prepend a default
#[test]
fn the_device_name_field_offers_the_default_without_pre_filling_it() {
    let engine = DeviceLinkJoinEngine::new("Pixel 3a".to_string());
    let screen = engine.current_screen();
    let Some(crate::ui::Component::TextInput {
        value, placeholder, ..
    }) = screen.components.first()
    else {
        panic!("expected a device-name input, got {:?}", screen.components);
    };

    assert!(
        value.is_empty(),
        "typing must replace rather than append to a default, so the field \
         starts empty; got {value:?}"
    );
    assert_eq!(
        placeholder.as_deref(),
        Some("Pixel 3a"),
        "the default still has to be visible, as a placeholder"
    );
    assert_eq!(
        engine.device_name(),
        "Pixel 3a",
        "leaving it untouched must still register the device under its own name"
    );
}

// @internal
#[test]
fn join_action_with_empty_name_returns_validation_error() {
    let mut engine = DeviceLinkJoinEngine::new("".to_string());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: JOIN_ACTION_ID.into(),
    });
    assert!(
        matches!(result, crate::ui::ActionResult::ValidationError { ref component_id, .. } if component_id == DEVICE_NAME_INPUT_ID),
        "expected validation error for empty name, got {result:?}"
    );
}

// @internal
#[test]
fn join_action_moves_to_posting_request_and_emits_start() {
    let mut engine = engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: JOIN_ACTION_ID.into(),
    });
    assert_eq!(screen_id(&engine), "device_link_join_posting");
    assert!(
        matches!(result, crate::ui::ActionResult::DeviceLinkJoinStart { ref device_name } if device_name == "My Phone"),
        "expected DeviceLinkJoinStart, got {result:?}"
    );
}

// @internal
#[test]
fn updates_move_through_steps() {
    let mut engine = engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: JOIN_ACTION_ID.into(),
    });

    assert!(engine.apply_update(EngineUpdate::DeviceLinkJoin(
        DeviceLinkJoinUpdate::NameAccepted
    )));
    assert_eq!(screen_id(&engine), "device_link_join_posting");

    assert!(engine.apply_update(EngineUpdate::DeviceLinkJoin(
        DeviceLinkJoinUpdate::RequestPosted {
            confirmation_code: "123456".into(),
        }
    )));
    assert_eq!(screen_id(&engine), "device_link_join_confirm");
    let code_text = engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            crate::ui::Component::Text { id, content, .. } if id == "confirmation_code" => {
                Some(content.clone())
            }
            _ => None,
        });
    assert_eq!(code_text, Some("123456".into()));

    assert!(engine.apply_update(EngineUpdate::DeviceLinkJoin(
        DeviceLinkJoinUpdate::ResponseReady
    )));
    assert_eq!(screen_id(&engine), "device_link_join_completing");

    assert!(engine.apply_update(EngineUpdate::DeviceLinkJoin(
        DeviceLinkJoinUpdate::Completed
    )));
    assert_eq!(screen_id(&engine), "device_link_join_complete");
}

// @internal
#[test]
fn failed_update_renders_failed_screen_with_retry() {
    let mut engine = engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: JOIN_ACTION_ID.into(),
    });

    assert!(
        engine.apply_update(EngineUpdate::DeviceLinkJoin(DeviceLinkJoinUpdate::Failed(
            "relay_failed".into()
        )))
    );
    assert_eq!(screen_id(&engine), "device_link_join_failed");
    let actions: Vec<String> = engine
        .current_screen()
        .contextual_actions
        .iter()
        .map(|a| a.id.clone())
        .collect();
    assert!(actions.contains(&RETRY_ACTION_ID.to_string()));
    assert!(actions.contains(&CANCEL_ACTION_ID.to_string()));
}

// @internal
#[test]
fn retry_returns_to_enter_name() {
    let mut engine = engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: JOIN_ACTION_ID.into(),
    });
    let _ = engine.apply_update(EngineUpdate::DeviceLinkJoin(DeviceLinkJoinUpdate::Failed(
        "relay_failed".into(),
    )));

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: RETRY_ACTION_ID.into(),
    });
    assert_eq!(screen_id(&engine), "device_link_join");
    assert!(matches!(result, crate::ui::ActionResult::UpdateScreen(_)));
}

// @internal
#[test]
fn cancel_emits_complete() {
    let mut engine = engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: CANCEL_ACTION_ID.into(),
    });
    assert!(matches!(result, crate::ui::ActionResult::Complete));
}

// @internal
#[test]
fn foreign_update_is_rejected() {
    let mut engine = engine();
    assert!(!engine.apply_update(EngineUpdate::ConfirmPendingDelete));
    assert_eq!(screen_id(&engine), "device_link_join");
}

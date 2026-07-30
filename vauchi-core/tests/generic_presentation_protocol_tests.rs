// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::json;
use vauchi_core::{
    ActionSpec, ActionTone, Command, ContextBar, Event, InputMode, InteractionId, MotionPreference,
    OverlayKind, OverlaySpec, PresentationIdError, StandardShortcut, SurfaceId,
};

fn action(
    id: &str,
    label: &str,
    icon_token: &str,
    shortcut: Option<StandardShortcut>,
) -> ActionSpec {
    ActionSpec {
        interaction_id: InteractionId::new(id).expect("valid interaction id"),
        label: label.to_owned(),
        accessibility_label: label.to_owned(),
        icon_token: Some(icon_token.to_owned()),
        enabled: true,
        tone: ActionTone::Standard,
        shortcut,
    }
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Contextual controls expose four stable roles
// @scenario: generic_presentation_protocol.feature :: Contextual controls expose four stable roles
#[test]
fn test_context_bar_complete_roles_round_trip_without_domain_interpretation() {
    let surface_id = SurfaceId::new("main").expect("valid surface id");
    let primary_id = InteractionId::new("save").expect("valid interaction id");
    let command = Command::SetContextBar {
        surface_id: surface_id.clone(),
        revision: 7,
        bar: Box::new(ContextBar {
            back: Some(action(
                "back",
                "Back",
                "navigation.back",
                Some(StandardShortcut::Back),
            )),
            navigation: Some(action("navigate", "Navigate", "navigation.menu", None)),
            primary: Some(action(
                "save",
                "Save",
                "action.confirm",
                Some(StandardShortcut::ActivatePrimary),
            )),
            secondary: Some(action("more", "More actions", "action.more", None)),
        }),
    };

    let encoded = serde_json::to_value(&command).expect("serialize context bar");
    assert_eq!(
        encoded,
        json!({
            "SetContextBar": {
                "surface_id": "main",
                "revision": 7,
                "bar": {
                    "back": {
                        "interaction_id": "back",
                        "label": "Back",
                        "accessibility_label": "Back",
                        "icon_token": "navigation.back",
                        "enabled": true,
                        "shortcut": "back"
                    },
                    "navigation": {
                        "interaction_id": "navigate",
                        "label": "Navigate",
                        "accessibility_label": "Navigate",
                        "icon_token": "navigation.menu",
                        "enabled": true,
                        "shortcut": null
                    },
                    "primary": {
                        "interaction_id": "save",
                        "label": "Save",
                        "accessibility_label": "Save",
                        "icon_token": "action.confirm",
                        "enabled": true,
                        "shortcut": "activate_primary"
                    },
                    "secondary": {
                        "interaction_id": "more",
                        "label": "More actions",
                        "accessibility_label": "More actions",
                        "icon_token": "action.more",
                        "enabled": true,
                        "shortcut": null
                    }
                }
            }
        })
    );
    let decoded: Command = serde_json::from_value(encoded).expect("decode context bar");
    assert_eq!(decoded, command);

    let event = Event::ActionActivated {
        surface_id,
        interaction_id: primary_id,
    };
    assert_eq!(
        serde_json::to_value(event).expect("serialize activation"),
        json!({
            "ActionActivated": {
                "surface_id": "main",
                "interaction_id": "save"
            }
        })
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn test_presentation_ids_reject_invalid_values_on_creation_and_decode() {
    assert_eq!(
        SurfaceId::new("").expect_err("empty id must fail"),
        PresentationIdError::Empty
    );
    assert_eq!(
        InteractionId::new("a".repeat(129)).expect_err("oversized id must fail"),
        PresentationIdError::TooLong
    );
    assert_eq!(
        InteractionId::new("save\nnow").expect_err("control character must fail"),
        PresentationIdError::ControlCharacter
    );

    let decoded = serde_json::from_value::<SurfaceId>(json!(""));
    assert_eq!(
        decoded.expect_err("decoder must validate").to_string(),
        "presentation identifier must not be empty"
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Overlay kinds remain distinct with reduced motion
// @scenario: generic_presentation_protocol.feature :: Overlay kinds remain distinct with reduced motion
#[test]
fn test_overlay_commands_preserve_navigation_and_action_menu_kinds() {
    let surface_id = SurfaceId::new("main").expect("valid surface id");
    let navigation = Command::PresentOverlay {
        surface_id: surface_id.clone(),
        revision: 9,
        overlay: OverlaySpec {
            kind: OverlayKind::Navigation,
            title: Some("Navigate".to_owned()),
            items: vec![action("contacts", "Contacts", "navigation.contacts", None)],
        },
    };
    let action_menu = Command::PresentOverlay {
        surface_id,
        revision: 9,
        overlay: OverlaySpec {
            kind: OverlayKind::ActionMenu,
            title: Some("More actions".to_owned()),
            items: vec![action("archive", "Archive", "action.archive", None)],
        },
    };

    assert_eq!(
        serde_json::to_value(&navigation).expect("serialize navigation overlay"),
        json!({
            "PresentOverlay": {
                "surface_id": "main",
                "revision": 9,
                "overlay": {
                    "kind": "navigation",
                    "title": "Navigate",
                    "items": [{
                        "interaction_id": "contacts",
                        "label": "Contacts",
                        "accessibility_label": "Contacts",
                        "icon_token": "navigation.contacts",
                        "enabled": true,
                        "shortcut": null
                    }]
                }
            }
        })
    );
    assert_ne!(navigation, action_menu);
}

/// Feature: generic_presentation_protocol.feature
/// Scenario Outline: Available window drives structural composition
// @scenario: generic_presentation_protocol.feature :: Available window drives structural composition
#[test]
fn test_presentation_environment_event_carries_window_input_and_motion_facts() {
    let event = Event::PresentationEnvironmentChanged {
        available_width: 600,
        available_height: 900,
        input_modes: vec![InputMode::Touch, InputMode::Keyboard],
        motion: MotionPreference::Reduced,
    };

    assert_eq!(
        serde_json::to_value(event).expect("serialize presentation environment"),
        json!({
            "PresentationEnvironmentChanged": {
                "available_width": 600,
                "available_height": 900,
                "input_modes": ["touch", "keyboard"],
                "motion": "reduced"
            }
        })
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Interaction activates its visible pane first
// @scenario: generic_presentation_protocol.feature :: Interaction activates its visible pane first
#[test]
fn test_surface_activation_event_preserves_the_opaque_surface_id() {
    let event = Event::SurfaceActivated {
        surface_id: SurfaceId::new("detail").expect("valid surface id"),
    };

    assert_eq!(
        serde_json::to_value(event).expect("serialize surface activation"),
        json!({
            "SurfaceActivated": {
                "surface_id": "detail"
            }
        })
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Native lifecycle inputs use the same reducer boundary
// @internal
#[test]
fn test_native_lifecycle_events_preserve_raw_shell_facts() {
    assert_eq!(
        serde_json::to_value(Event::DeepLinkOpened {
            uri: "vauchi://contact/opaque".to_owned(),
        })
        .expect("serialize deep link"),
        json!({
            "DeepLinkOpened": {
                "uri": "vauchi://contact/opaque"
            }
        })
    );
    assert_eq!(
        serde_json::to_value(Event::AppBackgrounded).expect("serialize lifecycle event"),
        json!("AppBackgrounded")
    );
}

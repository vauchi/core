// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::json;
use vauchi_core::{
    Event, EventJsonError, MAX_EVENT_JSON_NESTING_DEPTH, OverlayKind, SurfaceId, event_from_json,
};

fn surface(id: &str) -> SurfaceId {
    SurfaceId::new(id).expect("valid surface id")
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: User interaction returns as an opaque event
// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn test_back_and_overlay_dismissal_events_remain_presentation_only() {
    let back = Event::BackRequested {
        surface_id: surface("detail"),
    };
    let dismissed = Event::OverlayDismissed {
        surface_id: surface("detail"),
        kind: OverlayKind::ActionMenu,
    };

    assert_eq!(
        serde_json::to_value(back).expect("serialize back request"),
        json!({
            "BackRequested": {
                "surface_id": "detail"
            }
        })
    );
    assert_eq!(
        serde_json::to_value(dismissed).expect("serialize overlay dismissal"),
        json!({
            "OverlayDismissed": {
                "surface_id": "detail",
                "kind": "action_menu"
            }
        })
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn test_event_json_rejects_excessive_nesting_before_deserialization() {
    let nested_value = format!(
        "{}null{}",
        "[".repeat(MAX_EVENT_JSON_NESTING_DEPTH + 1),
        "]".repeat(MAX_EVENT_JSON_NESTING_DEPTH + 1)
    );
    let event = format!(
        r#"{{"ActionActivated":{{"surface_id":"main","interaction_id":"save","extra":{nested_value}}}}}"#
    );

    assert_eq!(
        event_from_json(&event).expect_err("excessive nesting must be rejected"),
        EventJsonError::TooDeep
    );
}

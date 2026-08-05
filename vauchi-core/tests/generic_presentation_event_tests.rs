// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::json;
use vauchi_core::{
    Event, EventJsonError, MAX_EVENT_INPUT_VALUE_BYTES, MAX_EVENT_JSON_BYTES,
    MAX_EVENT_JSON_NESTING_DEPTH, OverlayKind, SurfaceId, event_from_json,
};

fn surface(id: &str) -> SurfaceId {
    SurfaceId::new(id).expect("valid surface id")
}

/// The shortest event that carries no payload, used to pad a document to an
/// exact byte length without also tripping the input-value ceiling.
const SMALLEST_EVENT: &str = r#""PresentationInvalidated""#;

/// Wrap a nested value in a syntactically valid event envelope, which adds two
/// levels of `{` before the nesting under test begins.
fn event_wrapping(nested_value: &str) -> String {
    format!(
        r#"{{"ActionActivated":{{"surface_id":"main","interaction_id":"save","extra":{nested_value}}}}}"#
    )
}

fn nested_arrays(depth: usize) -> String {
    format!("{}null{}", "[".repeat(depth), "]".repeat(depth))
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
    // The envelope is already two levels deep, so this is exactly one level
    // over the ceiling rather than merely "deep enough to fail".
    let event = event_wrapping(&nested_arrays(MAX_EVENT_JSON_NESTING_DEPTH - 1));

    assert_eq!(
        event_from_json(&event).expect_err("excessive nesting must be rejected"),
        EventJsonError::TooDeep
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn test_event_json_admits_nesting_at_the_depth_ceiling() {
    // Depth exactly at the ceiling must pass the scanner and reach serde,
    // which ignores the unknown field. Pairing this with the rejection above
    // pins the boundary: an off-by-one would reject legitimate documents.
    let event = event_wrapping(&nested_arrays(MAX_EVENT_JSON_NESTING_DEPTH - 2));

    assert_eq!(
        event_from_json(&event).expect("nesting at the ceiling must be accepted"),
        Event::ActionActivated {
            surface_id: surface("main"),
            interaction_id: vauchi_core::InteractionId::new("save").expect("valid interaction id"),
        }
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn test_event_json_admits_a_document_at_the_size_ceiling() {
    let padding = MAX_EVENT_JSON_BYTES - SMALLEST_EVENT.len();
    let event = format!("{SMALLEST_EVENT}{}", " ".repeat(padding));

    assert_eq!(event.len(), MAX_EVENT_JSON_BYTES);
    assert_eq!(
        event_from_json(&event).expect("a document at the ceiling must be accepted"),
        Event::PresentationInvalidated
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn test_event_json_rejects_a_document_one_byte_over_the_size_ceiling() {
    let padding = MAX_EVENT_JSON_BYTES - SMALLEST_EVENT.len() + 1;
    let event = format!("{SMALLEST_EVENT}{}", " ".repeat(padding));

    assert_eq!(event.len(), MAX_EVENT_JSON_BYTES + 1);
    assert_eq!(
        event_from_json(&event).expect_err("one byte over the ceiling must be rejected"),
        EventJsonError::TooLarge
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn test_event_json_does_not_count_brackets_escaped_inside_a_string() {
    // An escaped quote must not end the string literal. If it did, every
    // bracket after it would be counted as real nesting and this legitimate
    // value would be rejected as `TooDeep`.
    let text = format!("\"{}", "{".repeat(MAX_EVENT_JSON_NESTING_DEPTH * 2));
    let event = json!({
        "ValueChanged": {
            "surface_id": "main",
            "binding_id": "display-name",
            "value": {"text": text},
        }
    })
    .to_string();

    assert!(
        matches!(
            event_from_json(&event).expect("escaped brackets must not count as nesting"),
            Event::ValueChanged { .. }
        ),
        "escaped brackets must not count as nesting"
    );
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Invalid boundary input fails safely
// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn test_event_json_rejects_oversized_input_value() {
    let event = json!({
        "ValueChanged": {
            "surface_id": "main",
            "binding_id": "display-name",
            "value": {"text": "x".repeat(MAX_EVENT_INPUT_VALUE_BYTES + 1)},
        }
    })
    .to_string();

    assert_eq!(
        event_from_json(&event).expect_err("oversized input value must be rejected"),
        EventJsonError::InputValueTooLarge
    );
}

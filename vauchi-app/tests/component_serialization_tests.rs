// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serialization roundtrip tests for Component and its helper types.
//!
//! These tests verify that components can be serialized to JSON and
//! deserialized back to an identical value (serde roundtrip), and that
//! the JSON shape matches the expected externally-tagged format.

use vauchi_app::ui::{Component, DropdownOption};

// --- Dropdown ---

// @scenario: component_serialization.feature - Dropdown serializes and deserializes
// @internal
#[test]
fn dropdown_roundtrip_with_selection() {
    let component = Component::Dropdown {
        id: "theme_picker".to_string(),
        label: "Theme".to_string(),
        selected: Some("dark".to_string()),
        options: vec![
            DropdownOption {
                id: "light".to_string(),
                label: "Light".to_string(),
            },
            DropdownOption {
                id: "dark".to_string(),
                label: "Dark".to_string(),
            },
        ],
    };

    let json = serde_json::to_string(&component).expect("serialize Dropdown");
    let roundtrip: Component = serde_json::from_str(&json).expect("deserialize Dropdown");
    assert_eq!(component, roundtrip);
}

// @scenario: component_serialization.feature - Dropdown serializes and deserializes with no selection
// @internal
#[test]
fn dropdown_roundtrip_no_selection() {
    let component = Component::Dropdown {
        id: "lang_picker".to_string(),
        label: "Language".to_string(),
        selected: None,
        options: vec![DropdownOption {
            id: "en".to_string(),
            label: "English".to_string(),
        }],
    };

    let json = serde_json::to_string(&component).expect("serialize Dropdown");
    let roundtrip: Component = serde_json::from_str(&json).expect("deserialize Dropdown");
    assert_eq!(component, roundtrip);
}

// @scenario: component_serialization.feature - Dropdown JSON uses externally-tagged format
// @internal
#[test]
fn dropdown_json_shape() {
    let component = Component::Dropdown {
        id: "picker".to_string(),
        label: "Pick".to_string(),
        selected: None,
        options: vec![DropdownOption {
            id: "a".to_string(),
            label: "A".to_string(),
        }],
    };

    let json = serde_json::to_string(&component).expect("serialize Dropdown");
    // Externally tagged: {"Dropdown": {...}}
    assert!(
        json.starts_with(r#"{"Dropdown":"#),
        "expected externally-tagged JSON, got: {json}"
    );
    assert!(
        json.contains(r#""id":"picker""#),
        "id field missing: {json}"
    );
    assert!(
        json.contains(r#""label":"Pick""#),
        "label field missing: {json}"
    );
    assert!(
        json.contains(r#""options""#),
        "options field missing: {json}"
    );
}

// @scenario: component_serialization.feature - Dropdown with empty options list
// @internal
#[test]
fn dropdown_roundtrip_empty_options() {
    let component = Component::Dropdown {
        id: "empty".to_string(),
        label: "Empty".to_string(),
        selected: None,
        options: vec![],
    };

    let json = serde_json::to_string(&component).expect("serialize Dropdown");
    let roundtrip: Component = serde_json::from_str(&json).expect("deserialize Dropdown");
    assert_eq!(component, roundtrip);
}

// --- DropdownOption ---

// @scenario: component_serialization.feature - DropdownOption serializes and deserializes
// @internal
#[test]
fn dropdown_option_roundtrip() {
    let option = DropdownOption {
        id: "opt1".to_string(),
        label: "Option 1".to_string(),
    };

    let json = serde_json::to_string(&option).expect("serialize DropdownOption");
    let roundtrip: DropdownOption =
        serde_json::from_str(&json).expect("deserialize DropdownOption");
    assert_eq!(option, roundtrip);
}

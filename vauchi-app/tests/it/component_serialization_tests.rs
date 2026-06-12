// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serialization roundtrip tests for Component and its helper types.
//!
//! These tests verify that components can be serialized to JSON and
//! deserialized back to an identical value (serde roundtrip), and that
//! the JSON shape matches the expected externally-tagged format.

use vauchi_app::ui::{ActionListItem, Component, DropdownOption, IndicatorKind, Section};

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
        a11y: None,
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
        a11y: None,
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
        a11y: None,
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
        a11y: None,
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

// --- Indicator (chrome-positioned status, ADR-043 / shell-purity investigation) ---

// @internal
#[test]
fn indicator_roundtrip_with_action() {
    let component = Component::Indicator {
        id: "sync".to_string(),
        label: "Synced 15:47".to_string(),
        kind: IndicatorKind::Active,
        action_id: Some("sync_now".to_string()),
        a11y: None,
    };
    let json = serde_json::to_string(&component).expect("serialize Indicator");
    let roundtrip: Component = serde_json::from_str(&json).expect("deserialize Indicator");
    assert_eq!(component, roundtrip);
    // Wire shape check: action_id present on the wire, kind serialized as "Active".
    assert!(
        json.contains("\"action_id\":\"sync_now\""),
        "action_id present on wire: {json}"
    );
    assert!(
        json.contains("\"kind\":\"Active\""),
        "kind serialized as Active: {json}"
    );
}

// @internal
#[test]
fn indicator_roundtrip_display_only() {
    // Display-only indicator: action_id = None. Wire shape must omit
    // the field entirely via skip_serializing_if so frontends never see
    // an empty/null action_id they'd have to special-case.
    let component = Component::Indicator {
        id: "online".to_string(),
        label: "Offline".to_string(),
        kind: IndicatorKind::Error,
        action_id: None,
        a11y: None,
    };
    let json = serde_json::to_string(&component).expect("serialize Indicator");
    let roundtrip: Component = serde_json::from_str(&json).expect("deserialize Indicator");
    assert_eq!(component, roundtrip);
    assert!(
        !json.contains("action_id"),
        "display-only indicator must omit action_id: {json}"
    );
}

// @internal
#[test]
fn indicator_kind_covers_four_states() {
    // Exhaustive roundtrip across all IndicatorKind variants. Drift catcher:
    // if a future MR adds a 5th kind, this test must be updated to keep the
    // wire-shape coverage exhaustive.
    for kind in [
        IndicatorKind::Active,
        IndicatorKind::Error,
        IndicatorKind::Neutral,
        IndicatorKind::Busy,
    ] {
        let component = Component::Indicator {
            id: format!("k_{:?}", kind),
            label: format!("{:?}", kind),
            kind,
            action_id: None,
            a11y: None,
        };
        let json = serde_json::to_string(&component).expect("serialize");
        let roundtrip: Component = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(component, roundtrip, "kind {kind:?} must roundtrip");
    }
}

// --- SectionedActionList (grouped menu, ADR-043 / shell-purity investigation) ---

// @internal
#[test]
fn sectioned_action_list_roundtrip_with_multiple_sections() {
    let component = Component::SectionedActionList {
        id: "more_menu".to_string(),
        sections: vec![
            Section {
                id: "primary".to_string(),
                label: "Primary".to_string(),
                items: vec![
                    ActionListItem {
                        id: "settings".to_string(),
                        label: "Settings".to_string(),
                        icon: Some("gear".to_string()),
                        detail: None,
                        a11y: None,
                        info_key: None,
                    },
                    ActionListItem {
                        id: "help".to_string(),
                        label: "Help".to_string(),
                        icon: Some("questionmark.circle".to_string()),
                        detail: None,
                        a11y: None,
                        info_key: None,
                    },
                ],
            },
            Section {
                id: "legal".to_string(),
                label: "Legal".to_string(),
                items: vec![ActionListItem {
                    id: "privacy".to_string(),
                    label: "Privacy".to_string(),
                    icon: None,
                    detail: None,
                    a11y: None,
                    info_key: None,
                }],
            },
        ],
    };
    let json = serde_json::to_string(&component).expect("serialize SectionedActionList");
    let roundtrip: Component =
        serde_json::from_str(&json).expect("deserialize SectionedActionList");
    assert_eq!(component, roundtrip);
    // Section labels reach the wire so frontends can render the native
    // section idiom (SwiftUI Section, GTK4 group, Material header) without
    // a per-frontend label table.
    assert!(
        json.contains("\"label\":\"Primary\""),
        "section label on wire: {json}"
    );
    assert!(
        json.contains("\"label\":\"Legal\""),
        "section label on wire: {json}"
    );
}

// @internal
#[test]
fn sectioned_action_list_roundtrip_empty_sections() {
    // A SectionedActionList with no sections is technically valid wire shape.
    // Frontends render nothing; serves as the empty-state placeholder.
    let component = Component::SectionedActionList {
        id: "empty_menu".to_string(),
        sections: vec![],
    };
    let json = serde_json::to_string(&component).expect("serialize");
    let roundtrip: Component = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(component, roundtrip);
}

// --- List windowing (`2026-06-11-contacts-list-eager-render-anr` Track B) ---

// @internal
#[test]
fn list_unwindowed_keeps_pre_windowing_json_shape() {
    // Zero windowing fields must be skip-serialized so every unwindowed
    // emitter (group members, tags, places, …) keeps its exact current
    // wire bytes — hand-mirrored frontend decoders see no change.
    let component = Component::List {
        id: "members".to_string(),
        items: vec![],
        searchable: true,
        total_count: 0,
        offset: 0,
        window: 0,
    };
    let json = serde_json::to_value(&component).expect("serialize unwindowed List");
    assert_eq!(
        json,
        serde_json::json!({
            "List": { "id": "members", "items": [], "searchable": true }
        })
    );
}

// @internal
#[test]
fn list_without_window_fields_decodes_unwindowed() {
    let json = r#"{"List":{"id":"members","items":[],"searchable":true}}"#;
    let component: Component = serde_json::from_str(json).expect("decode pre-windowing List");
    let Component::List {
        total_count,
        offset,
        window,
        ..
    } = component
    else {
        panic!("expected List, got {component:?}");
    };
    assert_eq!((total_count, offset, window), (0, 0, 0));
}

// @internal
#[test]
fn list_windowed_roundtrip() {
    let component = Component::List {
        id: "contacts".to_string(),
        items: vec![],
        searchable: true,
        total_count: 500,
        offset: 200,
        window: 200,
    };
    let json = serde_json::to_string(&component).expect("serialize windowed List");
    assert!(
        json.contains("\"total_count\":500"),
        "windowed fields on wire: {json}"
    );
    let roundtrip: Component = serde_json::from_str(&json).expect("deserialize windowed List");
    assert_eq!(component, roundtrip);
}

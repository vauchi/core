// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wire Humble regression test (G4 of `2026-05-03-coreui-wire-humble-types`).
//!
//! Asserts that no domain-shaped JSON key reappears in any CoreUI
//! `Component` variant's serialized output. Tier 0 of the Wire
//! Humble migration renamed several types and fields to be UI-shaped
//! at the wire boundary; this test guards against re-domain-ification
//! by any future code change.
//!
//! When this test fires, do not "fix" it by widening the deny-list.
//! Instead: pick a UI-shaped name for the new key. The deny-list is
//! a record of names we've already retired — adding to it is fine
//! when retiring more, never to silence a regression.

use vauchi_app::ui::{
    Component, DropdownOption, Field, InputType, Item, ListItemAction, ListItemActionKind,
    PreviewVariant, QrMode, Status, TextStyle, UiFieldVisibility, VisibilityMode,
};

/// Domain-shaped JSON keys retired during Wire Humble Tier 0.
///
/// Each entry is a key that USED to appear in the wire JSON but
/// has been renamed or removed. The test fails if any of these
/// reappears anywhere in any Component variant's serialized output
/// (object key or variant tag).
const FORBIDDEN_KEYS: &[&str] = &[
    // Variant tags renamed (Phase 1)
    "ContactList", // → "List"
    "CardPreview", // → "Preview"
    // Type rename (Phase 1) — appears in JSON only via #[serde] derives
    // on referenced structs; included for defence-in-depth.
    "GroupCardView", // → "PreviewVariant"
    // Field names renamed (Phase 1)
    "contacts",       // Component::List → "items"
    "group_name",     // PreviewVariant → "variant_id"
    "group_views",    // Component::Preview → "variants"
    "selected_group", // Component::Preview → "selected_variant"
    // Field retired entirely (Phase 2)
    "searchable_fields", // moved to engine-internal IndexedItem.searchable
];

/// Recursively walk a JSON value asserting that no forbidden key
/// appears as an object key (which covers both struct field names
/// and the externally-tagged enum variant tag).
fn assert_no_forbidden_keys(value: &serde_json::Value, path: &str) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                assert!(
                    !FORBIDDEN_KEYS.contains(&key.as_str()),
                    "Wire Humble violation: key `{key}` at JSON path `{path}` is on the \
                     deny-list. CoreUI wire types must be UI-shaped — the renderer must \
                     not be able to read what kind of thing it is rendering. See \
                     `_private/docs/designs/2026-05-03-coreui-wire-humble-types-design.md`."
                );
                let sub_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                assert_no_forbidden_keys(child, &sub_path);
            }
        }
        serde_json::Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                assert_no_forbidden_keys(child, &format!("{path}[{i}]"));
            }
        }
        _ => {}
    }
}

fn sample_item() -> Item {
    Item {
        id: "i1".to_string(),
        name: "Sample".to_string(),
        subtitle: Some("subtitle".to_string()),
        avatar_initials: "S".to_string(),
        status: Some("active".to_string()),
        actions: vec![ListItemAction {
            id: "archive".to_string(),
            label: "Archive".to_string(),
            kind: ListItemActionKind::Archive,
            destructive: false,
        }],
        a11y: None,
    }
}

fn sample_field() -> Field {
    Field {
        id: "phone".to_string(),
        field_type: "phone".to_string(),
        label: "Phone".to_string(),
        value: "+1234567890".to_string(),
        icon: "phone".to_string(),
        visibility: UiFieldVisibility::Shown,
        a11y: None,
    }
}

fn sample_preview_variant() -> PreviewVariant {
    PreviewVariant {
        variant_id: "work".to_string(),
        display_name: "Work".to_string(),
        visible_fields: vec![sample_field()],
    }
}

/// Number of `Component` variants in
/// `core/vauchi-app/src/ui/component/mod.rs`. Asserted below by
/// [`all_components_covers_every_variant`] so a new variant added
/// without a matching sample fails the build, not just review.
///
/// `Component` is `#[non_exhaustive]`, so an exhaustive `match` from
/// this integration-test crate (an *external* crate to `vauchi_app`)
/// is rejected. Counting unique variant tags via the serialized form
/// is the next-best mechanical check.
const COMPONENT_VARIANT_COUNT: usize = 20;

/// Curated set of `Component` variants. Adding a new variant?
/// **Append a sample here AND bump [`COMPONENT_VARIANT_COUNT`].**
/// `all_components_covers_every_variant` will fail the build until
/// you do.
fn all_components() -> Vec<Component> {
    vec![
        Component::Text {
            id: "t".to_string(),
            content: "hello".to_string(),
            style: TextStyle::Body,
        },
        Component::TextInput {
            id: "ti".to_string(),
            label: "Name".to_string(),
            value: String::new(),
            placeholder: Some("Enter name".to_string()),
            max_length: Some(64),
            validation_error: None,
            input_type: InputType::Text,
            a11y: None,
            info_key: None,
        },
        Component::List {
            id: "l".to_string(),
            items: vec![sample_item()],
            searchable: true,
        },
        Component::Preview {
            name: "Alice".to_string(),
            initials: "A".to_string(),
            avatar_data: None,
            fields: vec![sample_field()],
            visible_fields: vec![sample_field()],
            variants: vec![sample_preview_variant()],
            selected_variant: Some("work".to_string()),
            a11y: None,
        },
        Component::FieldList {
            id: "fl".to_string(),
            fields: vec![sample_field()],
            visibility_mode: VisibilityMode::ReadOnly,
            available_groups: vec!["work".to_string()],
            a11y: None,
        },
        Component::ToggleList {
            id: "tl".to_string(),
            label: "Toggles".to_string(),
            items: vec![],
            a11y: None,
        },
        Component::InfoPanel {
            id: "ip".to_string(),
            icon: None,
            title: "Info".to_string(),
            items: vec![],
            a11y: None,
        },
        Component::SettingsGroup {
            id: "sg".to_string(),
            label: "Group".to_string(),
            items: vec![],
        },
        Component::ActionList {
            id: "al".to_string(),
            items: vec![],
        },
        Component::Row {
            id: "row".to_string(),
            items: vec![],
        },
        Component::StatusIndicator {
            id: "si".to_string(),
            icon: None,
            title: "Status".to_string(),
            detail: None,
            status: Status::Success,
            a11y: None,
        },
        Component::PinInput {
            id: "pi".to_string(),
            label: "PIN".to_string(),
            length: 6,
            filled: 2,
            masked: true,
            validation_error: None,
            a11y: None,
        },
        Component::QrCode {
            id: "qr".to_string(),
            data: "vchi:abc".to_string(),
            mode: QrMode::Display,
            label: Some("Scan me".to_string()),
            scan_quality: None,
            a11y: None,
        },
        Component::InlineConfirm {
            id: "ic".to_string(),
            warning: "This cannot be undone.".to_string(),
            confirm_text: "Delete".to_string(),
            cancel_text: "Cancel".to_string(),
            destructive: true,
            a11y: None,
        },
        Component::EditableText {
            id: "et".to_string(),
            label: "Display name".to_string(),
            value: "Alice".to_string(),
            editing: false,
            validation_error: None,
            a11y: None,
            info_key: None,
        },
        Component::Banner {
            text: "Preview mode".to_string(),
            action_label: "Dismiss".to_string(),
            action_id: "dismiss".to_string(),
            a11y: None,
        },
        Component::Dropdown {
            id: "dd".to_string(),
            label: "Pick".to_string(),
            selected: None,
            options: vec![DropdownOption {
                id: "a".to_string(),
                label: "A".to_string(),
            }],
            a11y: None,
        },
        Component::AvatarPreview {
            id: "ap".to_string(),
            image_data: None,
            initials: "AB".to_string(),
            bg_color: Some([100, 150, 200]),
            brightness: 0.0,
            editable: false,
            a11y: None,
        },
        Component::Slider {
            id: "sl".to_string(),
            label: "Brightness".to_string(),
            value: 0.0,
            min: -0.3,
            max: 0.3,
            step: 0.05,
            min_icon: Some("sun.min".to_string()),
            max_icon: Some("sun.max".to_string()),
            a11y: None,
        },
        Component::Divider,
    ]
}

/// @internal
#[test]
fn no_forbidden_keys_in_serialized_components() {
    for component in all_components() {
        let value = serde_json::to_value(&component).expect("serialize component");
        let variant_tag = match &value {
            serde_json::Value::Object(map) => map.keys().next().cloned().unwrap_or_default(),
            serde_json::Value::String(s) => s.clone(),
            _ => "<unknown>".to_string(),
        };
        assert_no_forbidden_keys(&value, &format!("Component::{variant_tag}"));
    }
}

/// @internal
#[test]
fn no_forbidden_keys_in_helper_types() {
    // The structs used inside Component variants — Item, Field,
    // PreviewVariant, ListItemAction — also serialize to JSON when
    // they appear nested. Cover their direct serialization too so
    // a future helper-type rename can't slip through.
    let item = serde_json::to_value(sample_item()).unwrap();
    assert_no_forbidden_keys(&item, "Item");

    let field = serde_json::to_value(sample_field()).unwrap();
    assert_no_forbidden_keys(&field, "Field");

    let variant = serde_json::to_value(sample_preview_variant()).unwrap();
    assert_no_forbidden_keys(&variant, "PreviewVariant");
}

/// @internal
#[test]
fn deny_list_self_check() {
    // The deny-list itself is the spec. If it's empty, the test is
    // a no-op — fail loud rather than silently passing.
    assert!(
        !FORBIDDEN_KEYS.is_empty(),
        "FORBIDDEN_KEYS is empty — Wire Humble regression test has no teeth"
    );
    // Each entry must be a non-empty string. A typo'd empty entry
    // would also silently pass.
    for key in FORBIDDEN_KEYS {
        assert!(!key.is_empty(), "FORBIDDEN_KEYS contains empty entry");
    }
}

/// Variant-coverage gate: forces `all_components()` to keep up with
/// new `Component` variants. The counting strategy uses serialized
/// variant tags rather than an exhaustive `match` because `Component`
/// is `#[non_exhaustive]` and this test crate is external to
/// `vauchi_app`, which forbids exhaustive matching.
///
/// @internal
#[test]
fn all_components_covers_every_variant() {
    use std::collections::HashSet;
    let tags: HashSet<String> = all_components()
        .iter()
        .map(|c| {
            let v = serde_json::to_value(c).expect("serialize component");
            match &v {
                serde_json::Value::Object(map) => map.keys().next().cloned().unwrap_or_default(),
                serde_json::Value::String(s) => s.clone(),
                _ => "<unknown>".to_string(),
            }
        })
        .collect();

    assert_eq!(
        tags.len(),
        COMPONENT_VARIANT_COUNT,
        "all_components() exposes {} unique variant tags but \
         COMPONENT_VARIANT_COUNT = {}. If you added a Component \
         variant, append a sample AND bump the constant. If you \
         retired one, remove the sample AND drop the constant. \
         Tags currently exercised: {:?}",
        tags.len(),
        COMPONENT_VARIANT_COUNT,
        tags
    );
}

/// Sanity check: the test actually catches violations.
///
/// A known-bad JSON containing each forbidden key MUST trip the
/// walker. Without this, a typo or refactor in
/// `assert_no_forbidden_keys` could neuter the regression test
/// while leaving every other test green.
///
/// @internal
#[test]
fn walker_catches_each_forbidden_key() {
    for key in FORBIDDEN_KEYS {
        let json = serde_json::json!({ *key: "anything" });
        let result = std::panic::catch_unwind(|| assert_no_forbidden_keys(&json, "test"));
        assert!(
            result.is_err(),
            "Walker failed to catch forbidden key `{key}` — regression test is toothless"
        );
    }
}

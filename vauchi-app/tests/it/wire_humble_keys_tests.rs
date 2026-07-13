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
    ActionListItem, ActionStyle, Component, DropdownOption, Field, IndicatorKind, InputType, Item,
    ListItemAction, ListItemActionKind, NativeWrapperHint, PreviewVariant, QrMode, ScreenAction,
    ScreenLayout, ScreenModel, ScreenPresentationKind, Section, Status, TabInfo, TextStyle,
    UiFieldVisibility, UserAction, VisibilityMode,
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
    // "avatar" domain word retired from the render wire vocab
    "AvatarPreview",   // Component variant tag → "ImageCircle"
    "avatar_data",     // Component::Preview field → "image_data"
    "avatar_initials", // Item field → "initials"
    // "group" domain word retired from the render wire vocab (Phase 3)
    "available_groups",  // Component::FieldList field → "available_scopes"
    "Groups",            // UiFieldVisibility::Groups variant tag → "Scopes"
    "GroupViewSelected", // UserAction variant tag → "VariantSelected"
];

/// Wire icon-token *field* names — the object keys whose string VALUE is
/// a glyph/SF-Symbol name the renderer looks up in its icon table. Every
/// `icon`-suffixed field on the render wire (`core/vauchi-app/src/ui`):
/// `icon` (InfoPanel, StatusIndicator, Field, ActionListItem, TabInfo),
/// `min_icon`/`max_icon` (Slider). The key-guard above cannot see these
/// values — it scans keys only — which is the gap this file closes.
const ICON_FIELD_KEYS: &[&str] = &["icon", "min_icon", "max_icon"];

/// Domain words that must NEVER appear as an icon-token *value*.
///
/// Icon values are glyph names (`folder`, `person`, `qrcode`, `shield`,
/// `devices`, `id_card`, …) the renderer maps to a native symbol. A
/// *domain* word here would let the renderer branch on what kind of thing
/// it is drawing — the exact leak Wire Humble forbids — through a channel
/// the key-only deny-list can't see. core!1359 (avatar) + core!1360
/// (group/icons) renamed the last domain icon tokens to glyphs; this list
/// locks that in.
///
/// Matched by EXACT token equality, never substring: `id_card` is a legit
/// glyph and must not trip on `card`; `devices`/`people`/`person` are legit
/// glyphs and stay off this list even though a domain lives next door.
const FORBIDDEN_ICON_VALUES: &[&str] = &[
    "avatar",
    "contact",
    "contacts",
    "card",
    "group",
    "groups",
    "members",
    "backup",
    "identity",
    "privacy",
    "consent",
    "recovery",
    "exchange",
    "duress",
    "fingerprint",
    "mailbox",
    "relay",
    "decoy",
    "emergency",
    "broadcast",
    "gdpr",
    "onboarding",
    "ratchet",
    "guardian",
    "shard",
    "reciprocity",
];

/// Recursively walk a JSON value asserting that no icon-token field carries
/// a domain word as its VALUE. Complements [`assert_no_forbidden_keys`]:
/// that guards object KEYS, this guards the string VALUE of the icon
/// fields — the channel a key-only scan is blind to (`"icon": "backup"`).
/// Exact token equality against [`FORBIDDEN_ICON_VALUES`], so glyph tokens
/// that merely embed a domain substring (`id_card`) are never flagged.
fn assert_no_domain_icon_values(value: &serde_json::Value, path: &str) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if ICON_FIELD_KEYS.contains(&key.as_str())
                    && let serde_json::Value::String(icon) = child
                {
                    assert!(
                        !FORBIDDEN_ICON_VALUES.contains(&icon.as_str()),
                        "Wire Humble violation: icon token `{icon}` at JSON path \
                         `{path}.{key}` is a domain word. Icon values must be \
                         glyph/SF-Symbol names the renderer looks up in a static \
                         icon table — never a domain word it could branch on. \
                         core!1359/core!1360 retired the domain icon tokens; pick a \
                         glyph name. See `.claude/rules/wire-humble.md`."
                    );
                }
                let sub_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                assert_no_domain_icon_values(child, &sub_path);
            }
        }
        serde_json::Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                assert_no_domain_icon_values(child, &format!("{path}[{i}]"));
            }
        }
        _ => {}
    }
}

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
        initials: "S".to_string(),
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

/// A field whose visibility carries data, so its serialized form emits the
/// `UiFieldVisibility` variant tag as an object key — the only way the
/// deny-list scan can see (and reject) that tag.
fn sample_scoped_field() -> Field {
    Field {
        visibility: UiFieldVisibility::Scopes(vec!["work".to_string()]),
        ..sample_field()
    }
}

fn sample_preview_variant() -> PreviewVariant {
    PreviewVariant {
        variant_id: "work".to_string(),
        display_name: "Work".to_string(),
        visible_fields: vec![sample_field()],
    }
}

/// Every `Component` variant tag in
/// `core/vauchi-app/src/ui/component/mod.rs`, spelled out explicitly so
/// [`all_components_covers_every_variant`] can assert the deny-list scan
/// serializes an instance of each. A plain count let two variants
/// (`Indicator`, `SectionedActionList`) go unscanned because the missing
/// samples and the stale count drifted together.
///
/// `Component` is `#[non_exhaustive]`, so an exhaustive `match` — the
/// compiler-enforced enumeration that would make this list redundant — is
/// rejected from this integration-test crate (external to `vauchi_app`).
/// That backstop lives in-crate on `ui::testing::screen_walker`'s
/// wildcard-free `match`, which fails to compile if a variant is added.
/// Here the explicit tag set is the next-best mechanical check: a variant
/// added without a sample fails the assertion by name, not just by count.
const EXPECTED_VARIANT_TAGS: &[&str] = &[
    "Text",
    "TextInput",
    "ToggleList",
    "FieldList",
    "Preview",
    "InfoPanel",
    "List",
    "SettingsGroup",
    "ActionList",
    "Row",
    "StatusIndicator",
    "PinInput",
    "QrCode",
    "InlineConfirm",
    "EditableText",
    "Divider",
    "Banner",
    "Dropdown",
    "ImageCircle",
    "Slider",
    "Indicator",
    "SectionedActionList",
];

/// Curated set of `Component` variants. Adding a new variant?
/// **Append a sample here AND add its tag to [`EXPECTED_VARIANT_TAGS`].**
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
            // Windowed sample so the new wire keys are walked by the
            // deny-list scan (zeros are skip-serialized).
            total_count: 3,
            offset: 1,
            window: 1,
        },
        Component::Preview {
            name: "Alice".to_string(),
            initials: "A".to_string(),
            image_data: None,
            fields: vec![sample_field()],
            visible_fields: vec![sample_field()],
            variants: vec![sample_preview_variant()],
            selected_variant: Some("work".to_string()),
            a11y: None,
        },
        Component::FieldList {
            id: "fl".to_string(),
            fields: vec![sample_field(), sample_scoped_field()],
            visibility_mode: VisibilityMode::ReadOnly,
            available_scopes: vec!["work".to_string()],
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
            status_label: "Success".to_string(),
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
            frames: vec!["vchi:abc-0".to_string(), "vchi:abc-1".to_string()],
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
        Component::ImageCircle {
            id: "ap".to_string(),
            image_data: None,
            initials: "AB".to_string(),
            bg_color: Some([100, 150, 200]),
            brightness: 0.0,
            editable: true,
            edit_action_id: Some("edit_avatar".to_string()),
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
        Component::Indicator {
            id: "ind".to_string(),
            label: "Offline".to_string(),
            kind: IndicatorKind::Neutral,
            action_id: Some("reconnect".to_string()),
            a11y: None,
        },
        Component::SectionedActionList {
            id: "sal".to_string(),
            sections: vec![Section {
                id: "primary".to_string(),
                label: "Primary".to_string(),
                items: vec![ActionListItem {
                    id: "import_contacts".to_string(),
                    label: "Import".to_string(),
                    icon: None,
                    detail: None,
                    a11y: None,
                    info_key: None,
                }],
            }],
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

/// The render surface beyond `Component`: the screen envelope
/// (`ScreenModel`), its per-screen affordances (`ScreenAction`), the tab
/// chrome (`TabInfo`), and the presentation enums. A domain-shaped key
/// leaking through any of these crosses the same core→renderer seam as a
/// `Component` key, so the deny-list must scan them too. `ScreenModel`
/// embeds every `Component` sample, so a re-domain-ified component key is
/// also caught here — a second, envelope-level detector.
///
/// Unit enum variants (`ScreenPresentationKind`, `ScreenLayout`,
/// `NativeWrapperHint`) serialize to bare JSON strings, so the key-walker
/// only guards them once a variant carries data (its tag becomes an object
/// key). Scanning them now records the intent and future-proofs that case.
///
/// @internal
#[test]
fn no_forbidden_keys_in_screen_surface() {
    let screen = ScreenModel {
        screen_id: "s".to_string(),
        title: "Screen".to_string(),
        components: all_components(),
        actions: vec![sample_screen_action()],
        presentation_kind: ScreenPresentationKind::Modal,
        layout: ScreenLayout::Fixed,
        native_wrapper_hint: NativeWrapperHint::MultiStageExchange,
        parent_screen_id: Some("contacts".to_string()),
        nav_tab_id: Some("more".to_string()),
        ..ScreenModel::default()
    };
    assert_no_forbidden_keys(&serde_json::to_value(&screen).unwrap(), "ScreenModel");

    assert_no_forbidden_keys(
        &serde_json::to_value(sample_screen_action()).unwrap(),
        "ScreenAction",
    );

    let tab = TabInfo {
        id: "contacts".to_string(),
        action_id: "nav_contacts".to_string(),
        label: "Contacts".to_string(),
        icon: "person.2".to_string(),
        badge_count: 0,
        is_home: false,
    };
    assert_no_forbidden_keys(&serde_json::to_value(&tab).unwrap(), "TabInfo");

    for kind in [
        ScreenPresentationKind::Page,
        ScreenPresentationKind::Modal,
        ScreenPresentationKind::Sheet,
    ] {
        assert_no_forbidden_keys(
            &serde_json::to_value(&kind).unwrap(),
            "ScreenPresentationKind",
        );
    }
    for layout in [
        ScreenLayout::Scroll,
        ScreenLayout::Fixed,
        ScreenLayout::Pinned,
    ] {
        assert_no_forbidden_keys(&serde_json::to_value(&layout).unwrap(), "ScreenLayout");
    }
    for hint in [
        NativeWrapperHint::None,
        NativeWrapperHint::MultiStageExchange,
        NativeWrapperHint::NfcExchange,
    ] {
        assert_no_forbidden_keys(&serde_json::to_value(&hint).unwrap(), "NativeWrapperHint");
    }
}

/// The action channel: `UserAction`s cross the same core↔renderer seam as
/// `Component`s (frontends construct them from affordances core minted), so a
/// domain-shaped variant tag or field name here re-domain-ifies that seam just
/// as a `Component` key would. Scans a representative spread — including the
/// preview-variant selection action, whose tag/field the deny-list guards.
///
/// @internal
#[test]
fn no_forbidden_keys_in_user_actions() {
    let actions = [
        UserAction::VariantSelected {
            variant_id: Some("work".to_string()),
        },
        UserAction::NavigateBack,
        UserAction::SearchChanged {
            component_id: "search".to_string(),
            query: "al".to_string(),
        },
        UserAction::ActionPressed {
            action_id: "save".to_string(),
        },
    ];
    for action in &actions {
        let value = serde_json::to_value(action).expect("serialize user action");
        let tag = match &value {
            serde_json::Value::Object(map) => map.keys().next().cloned().unwrap_or_default(),
            serde_json::Value::String(s) => s.clone(),
            _ => "<unknown>".to_string(),
        };
        assert_no_forbidden_keys(&value, &format!("UserAction::{tag}"));
    }
}

fn sample_screen_action() -> ScreenAction {
    ScreenAction {
        id: "save".to_string(),
        label: "Save".to_string(),
        style: ActionStyle::Primary,
        enabled: true,
        a11y: None,
    }
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

/// Variant-coverage gate: forces `all_components()` to serialize an
/// instance of every `Component` variant so the deny-list scan actually
/// walks each one. Compares the sampled tag set against the explicit
/// [`EXPECTED_VARIANT_TAGS`] rather than a bare count — a count check
/// passed while `Indicator` and `SectionedActionList` were unscanned,
/// because the missing samples and the stale count masked each other.
/// Set-equality names the exact missing (variant added, sample forgotten)
/// or extra (tag listed, sample removed) tag.
///
/// @internal
#[test]
fn all_components_covers_every_variant() {
    use std::collections::BTreeSet;
    let sampled: BTreeSet<String> = all_components()
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
    let expected: BTreeSet<String> = EXPECTED_VARIANT_TAGS
        .iter()
        .map(|s| s.to_string())
        .collect();

    let missing: Vec<&String> = expected.difference(&sampled).collect();
    let extra: Vec<&String> = sampled.difference(&expected).collect();

    assert!(
        missing.is_empty() && extra.is_empty(),
        "all_components() variant coverage drifted from EXPECTED_VARIANT_TAGS.\n\
         Missing samples (variant exists / listed, no serialized sample): {missing:?}\n\
         Extra samples (sampled tag not in EXPECTED_VARIANT_TAGS): {extra:?}\n\
         If you added a Component variant, append a sample AND its tag. If you \
         retired one, remove both."
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

/// Value-guard companion to [`no_forbidden_keys_in_screen_surface`]: walks
/// the same render-surface JSON asserting no icon-token field carries a
/// domain word as its VALUE. The key-only deny-list can't see icon values
/// (`"icon": "backup"`); this closes that gap. Green on current main
/// confirms core!1359/core!1360 renamed every domain icon token to a
/// glyph, and locks that cleanup against regression.
///
/// @internal
#[test]
fn no_domain_icon_values_in_render_surface() {
    let screen = ScreenModel {
        screen_id: "s".to_string(),
        title: "Screen".to_string(),
        components: all_components(),
        actions: vec![sample_screen_action()],
        presentation_kind: ScreenPresentationKind::Modal,
        layout: ScreenLayout::Fixed,
        native_wrapper_hint: NativeWrapperHint::MultiStageExchange,
        ..ScreenModel::default()
    };
    assert_no_domain_icon_values(&serde_json::to_value(&screen).unwrap(), "ScreenModel");

    let tab = TabInfo {
        id: "contacts".to_string(),
        action_id: "nav_contacts".to_string(),
        label: "Contacts".to_string(),
        icon: "person.2".to_string(),
        badge_count: 0,
        is_home: false,
    };
    assert_no_domain_icon_values(&serde_json::to_value(&tab).unwrap(), "TabInfo");
}

/// The blocklist is the spec. An empty list makes
/// [`no_domain_icon_values_in_render_surface`] a no-op — fail loud.
///
/// @internal
#[test]
fn forbidden_icon_values_self_check() {
    assert!(
        !FORBIDDEN_ICON_VALUES.is_empty(),
        "FORBIDDEN_ICON_VALUES is empty — icon-value guard has no teeth"
    );
    for value in FORBIDDEN_ICON_VALUES {
        assert!(
            !value.is_empty(),
            "FORBIDDEN_ICON_VALUES contains empty entry"
        );
    }
}

/// Sanity check: the icon-value guard actually catches violations.
///
/// Feeds a synthetic `Component` carrying a domain word as its `icon`
/// token value and asserts the walker trips — proving the guard is not
/// tautological (CC-03). Also checks each blocklist word directly, and
/// confirms a glyph token that merely embeds a domain substring
/// (`id_card`) is NOT flagged (exact-equality, not substring).
///
/// @internal
#[test]
fn icon_value_walker_catches_domain_word() {
    let bad = Component::InfoPanel {
        id: "ip".to_string(),
        icon: Some("group".to_string()),
        title: "Info".to_string(),
        items: vec![],
        a11y: None,
    };
    let value = serde_json::to_value(&bad).expect("serialize component");
    let result = std::panic::catch_unwind(|| assert_no_domain_icon_values(&value, "Component"));
    assert!(
        result.is_err(),
        "icon-value walker failed to catch domain icon token `group` — guard is toothless"
    );

    for value in FORBIDDEN_ICON_VALUES {
        let json = serde_json::json!({ "icon": *value });
        let result = std::panic::catch_unwind(|| assert_no_domain_icon_values(&json, "test"));
        assert!(
            result.is_err(),
            "icon-value walker failed to catch domain icon token `{value}`"
        );
    }

    // Exact-equality, not substring: a glyph embedding a domain word passes.
    let glyph = serde_json::json!({ "icon": "id_card" });
    assert_no_domain_icon_values(&glyph, "glyph");
}

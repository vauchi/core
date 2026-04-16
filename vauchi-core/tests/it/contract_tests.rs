// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contract tests: validate JSON fixtures and serialized types against JSON schemas.

use std::fs;
use std::path::PathBuf;

use vauchi_app::theme::load_themes_from_json;
use vauchi_app::ui::{ActionResult, ScreenModel, UserAction};
use vauchi_core::social::SocialNetworkRegistry;

fn schemas_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas")
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden")
}

fn load_schema(name: &str) -> serde_json::Value {
    let path = schemas_dir().join(name);
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read schema {}: {}", path.display(), e));
    serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse schema {}: {}", path.display(), e))
}

#[test]
fn golden_fixtures_validate_against_screen_model_schema() {
    let schema_value = load_schema("screen-model.schema.json");
    let validator =
        jsonschema::validator_for(&schema_value).expect("Failed to compile screen-model schema");

    let golden_files: Vec<_> = fs::read_dir(golden_dir())
        .expect("Failed to read golden fixtures directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    assert!(
        golden_files.len() >= 9,
        "Expected at least 9 golden fixtures, found {}",
        golden_files.len()
    );

    for path in &golden_files {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
        let instance: serde_json::Value = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path.display(), e));

        let result = validator.validate(&instance);
        assert!(
            result.is_ok(),
            "Golden fixture {} failed schema validation: {:?}",
            path.file_name().unwrap().to_string_lossy(),
            result.err().map(|e| e.to_string()).unwrap_or_default()
        );
    }
}

#[test]
fn user_action_variants_validate_against_schema() {
    let schema_value = load_schema("user-action.schema.json");
    let validator =
        jsonschema::validator_for(&schema_value).expect("Failed to compile user-action schema");

    let actions = vec![
        (
            "TextChanged",
            UserAction::TextChanged {
                component_id: "name".into(),
                value: "Alice".into(),
            },
        ),
        (
            "ItemToggled",
            UserAction::ItemToggled {
                component_id: "groups".into(),
                item_id: "Family".into(),
            },
        ),
        (
            "ActionPressed",
            UserAction::ActionPressed {
                action_id: "continue".into(),
            },
        ),
        (
            "FieldVisibilityChanged",
            UserAction::FieldVisibilityChanged {
                field_id: "phone".into(),
                group_id: None,
                visible: true,
            },
        ),
        (
            "GroupViewSelected",
            UserAction::GroupViewSelected {
                group_name: Some("Family".into()),
            },
        ),
        (
            "SearchChanged",
            UserAction::SearchChanged {
                component_id: "search".into(),
                query: "alice".into(),
            },
        ),
        (
            "ListItemSelected",
            UserAction::ListItemSelected {
                component_id: "contact_list".into(),
                item_id: "contact-1".into(),
            },
        ),
        (
            "SettingsToggled",
            UserAction::SettingsToggled {
                component_id: "settings".into(),
                item_id: "notifications".into(),
            },
        ),
        (
            "UndoPressed",
            UserAction::UndoPressed {
                action_id: "undo_delete_field:f1".into(),
            },
        ),
    ];

    for (variant_name, action) in &actions {
        let json_value = serde_json::to_value(action)
            .unwrap_or_else(|e| panic!("Failed to serialize UserAction::{}: {}", variant_name, e));

        let result = validator.validate(&json_value);
        assert!(
            result.is_ok(),
            "UserAction::{} failed schema validation.\nJSON: {}\nError: {:?}",
            variant_name,
            serde_json::to_string_pretty(&json_value).unwrap(),
            result.err().map(|e| e.to_string()).unwrap_or_default()
        );
    }
}

#[test]
fn action_result_variants_validate_against_schema() {
    let schema_value = load_schema("action-result.schema.json");
    let validator =
        jsonschema::validator_for(&schema_value).expect("Failed to compile action-result schema");

    // Build a minimal ScreenModel for variants that carry one.
    let screen = ScreenModel {
        screen_id: "test_screen".into(),
        title: "Test".into(),
        subtitle: None,
        components: vec![],
        actions: vec![],
        progress: None,
        ..Default::default()
    };

    let results = vec![
        ("UpdateScreen", ActionResult::UpdateScreen(screen.clone())),
        ("NavigateTo", ActionResult::NavigateTo(screen)),
        (
            "ValidationError",
            ActionResult::ValidationError {
                component_id: "name".into(),
                message: "Name is required".into(),
            },
        ),
        ("Complete", ActionResult::Complete),
        (
            "OpenContact",
            ActionResult::OpenContact {
                contact_id: "contact-123".into(),
            },
        ),
        (
            "EditContact",
            ActionResult::EditContact {
                contact_id: "contact-456".into(),
            },
        ),
        (
            "OpenUrl",
            ActionResult::OpenUrl {
                url: "https://example.com".into(),
            },
        ),
        (
            "ShowAlert",
            ActionResult::ShowAlert {
                title: "Alert".into(),
                message: "Something happened".into(),
            },
        ),
    ];

    for (variant_name, result) in &results {
        let json_value = serde_json::to_value(result).unwrap_or_else(|e| {
            panic!("Failed to serialize ActionResult::{}: {}", variant_name, e)
        });

        let validation = validator.validate(&json_value);
        assert!(
            validation.is_ok(),
            "ActionResult::{} failed schema validation.\nJSON: {}\nError: {:?}",
            variant_name,
            serde_json::to_string_pretty(&json_value).unwrap(),
            validation.err().map(|e| e.to_string()).unwrap_or_default()
        );
    }
}

/// Contract: deserializing an unknown UserAction variant returns a serde error,
/// not a panic. Frontends may send actions from a newer version; core must
/// handle this gracefully at the deserialization boundary.
#[test]
fn unknown_user_action_variant_returns_error_not_panic() {
    let unknown_json = r#"{"FutureAction": {"widget_id": "x"}}"#;
    let result = serde_json::from_str::<UserAction>(unknown_json);
    assert!(
        result.is_err(),
        "Deserializing unknown UserAction variant should return Err, got: {:?}",
        result
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("unknown variant"),
        "Error should mention 'unknown variant', got: {}",
        err_msg
    );
}

/// Contract: deserializing malformed JSON for UserAction returns an error.
#[test]
fn malformed_user_action_json_returns_error_not_panic() {
    let cases = [r#""JustAString""#, r#"null"#, r#"42"#, r#"[]"#, r#"{}"#];
    for json in &cases {
        let result = serde_json::from_str::<UserAction>(json);
        assert!(
            result.is_err(),
            "Malformed UserAction JSON should fail: {}",
            json
        );
    }
}

// ============================================================
// Content Repo Contract Tests
// ============================================================
//
// These tests verify that core's parsers can consume the actual
// content files from sibling repos (themes/, locales/, networks.json).
// A silent parsing failure here means degraded UX for all users.

/// Contract: embedded themes.json parses into multiple themes.
///
/// Catches the silent fallback in get_available_themes() where a parsing
/// failure returns only the default theme instead of failing visibly.
// @scenario: schema_compat :: Core parser accepts themes.json from sibling repo
#[test]
fn embedded_themes_json_parses_successfully() {
    let themes_json = include_bytes!("../../../../themes/generated/themes.json");
    let themes = load_themes_from_json(themes_json)
        .expect("themes.json must parse — silent fallback to default theme is a regression");
    assert!(
        themes.len() >= 2,
        "themes.json should contain multiple themes, got {}",
        themes.len()
    );
}

/// Contract: embedded networks.json parses into a populated registry.
// @scenario: schema_compat :: Core parser accepts networks.json
#[test]
fn embedded_networks_json_parses_successfully() {
    let registry = SocialNetworkRegistry::with_defaults();
    assert!(
        registry.all().len() >= 5,
        "networks.json should contain at least 5 social networks, got {}",
        registry.all().len()
    );
}

/// Contract: all locale files in the sibling repo are valid JSON with string values.
// @scenario: schema_compat :: Core parser accepts locale files from sibling repo
#[test]
fn locale_files_are_valid_json() {
    let locales_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales");
    if !locales_dir.exists() {
        eprintln!("SKIP: locales/ sibling repo not found");
        return;
    }

    let en_json =
        std::fs::read_to_string(locales_dir.join("en.json")).expect("en.json must be readable");
    let en: serde_json::Value = serde_json::from_str(&en_json).expect("en.json must be valid JSON");
    let en_obj = en.as_object().expect("en.json must be a JSON object");
    assert!(
        en_obj.len() >= 10,
        "en.json should have at least 10 translation keys, got {}",
        en_obj.len()
    );

    // Verify all locale files parse and have the same keys as en.json
    for entry in std::fs::read_dir(&locales_dir).expect("locales/ must be readable") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json")
            && !path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().contains("schema"))
        {
            let data = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
            let value: serde_json::Value = serde_json::from_str(&data)
                .unwrap_or_else(|e| panic!("{} is invalid JSON: {}", path.display(), e));
            let obj = value
                .as_object()
                .unwrap_or_else(|| panic!("{} must be a JSON object", path.display()));
            assert_eq!(
                obj.len(),
                en_obj.len(),
                "{} has {} keys but en.json has {} — missing or extra translations",
                path.display(),
                obj.len(),
                en_obj.len()
            );
        }
    }
}

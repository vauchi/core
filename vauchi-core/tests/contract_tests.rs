// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contract tests: validate JSON fixtures and serialized types against JSON schemas.

use std::fs;
use std::path::PathBuf;

use vauchi_core::ui::{ActionResult, ScreenModel, UserAction};

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

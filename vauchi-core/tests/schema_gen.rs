// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Schema generation tests.
//!
//! Generates JSON Schema files from core UI types and verifies they stay fresh.
//! Run with: cargo test --features schema-gen -p vauchi-core --test schema_gen

#![cfg(feature = "schema-gen")]

use schemars::schema_for;
use std::fs;
use std::path::PathBuf;
use vauchi_core::ui::{ActionResult, ScreenModel, UserAction};

fn schemas_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas")
}

fn generate_schema_json<T: schemars::JsonSchema>() -> String {
    let schema = schema_for!(T);
    serde_json::to_string_pretty(&schema).expect("schema serialization failed")
}

fn assert_schema_fresh<T: schemars::JsonSchema>(filename: &str) {
    let json = generate_schema_json::<T>();
    let path = schemas_dir().join(filename);

    assert!(
        path.exists(),
        "Schema file `{}` missing! Generate with:\n  cargo test --features schema-gen -p vauchi-core --test schema_gen -- regenerate --ignored",
        filename
    );

    let existing = fs::read_to_string(&path).unwrap();
    assert_eq!(
        existing.trim(),
        json.trim(),
        "Schema `{}` is stale! Regenerate with:\n  cargo test --features schema-gen -p vauchi-core --test schema_gen -- regenerate --ignored",
        filename
    );
}

#[test]
fn screen_model_schema_is_fresh() {
    let json = generate_schema_json::<ScreenModel>();
    assert!(!json.is_empty(), "ScreenModel schema must not be empty");
    assert_schema_fresh::<ScreenModel>("screen-model.schema.json");
}

#[test]
fn user_action_schema_is_fresh() {
    let json = generate_schema_json::<UserAction>();
    assert!(!json.is_empty(), "UserAction schema must not be empty");
    assert_schema_fresh::<UserAction>("user-action.schema.json");
}

#[test]
fn action_result_schema_is_fresh() {
    let json = generate_schema_json::<ActionResult>();
    assert!(!json.is_empty(), "ActionResult schema must not be empty");
    assert_schema_fresh::<ActionResult>("action-result.schema.json");
}

/// Regenerate all schemas. Run with `--ignored`:
/// `cargo test --features schema-gen -p vauchi-core --test schema_gen -- --ignored`
#[test]
#[ignore]
fn regenerate_all_schemas() {
    let dir = schemas_dir();
    fs::create_dir_all(&dir).unwrap();

    let schemas: &[(&str, String)] = &[
        (
            "screen-model.schema.json",
            generate_schema_json::<ScreenModel>(),
        ),
        (
            "user-action.schema.json",
            generate_schema_json::<UserAction>(),
        ),
        (
            "action-result.schema.json",
            generate_schema_json::<ActionResult>(),
        ),
    ];

    for (filename, json) in schemas {
        fs::write(dir.join(filename), json).unwrap();
        println!("Generated {}", filename);
    }

    assert_eq!(schemas.len(), 3, "expected 3 schemas to be generated");
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! An input value over `MAX_EVENT_INPUT_VALUE_BYTES` is rejected at the
//! boundary, but the rejection must reach the user as prepared
//! presentation rather than an error the shell can only log
//! (`2026-09-08-full-backup-restore-cannot-be-pasted`).

use std::sync::Arc;
use tempfile::TempDir;
use vauchi_core::MAX_EVENT_INPUT_VALUE_BYTES;
use vauchi_platform::PlatformAppEngine;

fn engine() -> (Arc<PlatformAppEngine>, TempDir) {
    let dir = TempDir::new().unwrap();
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
        key.as_bytes().to_vec(),
    )
    .expect("create PlatformAppEngine");
    (engine, dir)
}

fn value_changed(text: &str) -> String {
    serde_json::json!({
        "ValueChanged": {
            "surface_id": "surface",
            "binding_id": "binding",
            "value": { "text": text },
        }
    })
    .to_string()
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn oversized_input_value_is_explained_with_an_alert() {
    let (engine, _dir) = engine();
    let oversized = "a".repeat(MAX_EVENT_INPUT_VALUE_BYTES + 1);

    let batch: serde_json::Value = serde_json::from_str(
        &engine
            .dispatch_json(value_changed(&oversized))
            .expect("an oversized value is answered with commands, not an error"),
    )
    .expect("parse command batch");

    let commands = batch["commands"].as_array().expect("commands array");
    let alert = &commands[0]["PresentAlert"]["alert"];
    assert!(
        alert["title"].as_str().is_some_and(|t| !t.is_empty()),
        "alert must carry a prepared title, got {batch}"
    );
    assert!(
        alert["message"]
            .as_str()
            .is_some_and(|m| m.contains(&MAX_EVENT_INPUT_VALUE_BYTES.to_string())),
        "alert must name the bound so the user knows why, got {batch}"
    );
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn malformed_event_json_is_still_an_error() {
    let (engine, _dir) = engine();

    engine
        .dispatch_json("{not json".to_string())
        .expect_err("a malformed payload is a shell defect, not user input");
}

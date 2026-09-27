// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every event Core rejects — at the boundary decoder or while reducing
//! it — reaches the user as prepared presentation rather than an error
//! the shell can only log or print (`2026-09-08-full-backup-restore-
//! cannot-be-pasted`, `2026-08-07-ios-stale-overlay-and-raw-error-alert`).

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

/// The alert a rejection must produce: Core-prepared copy, never the
/// rejection's own text (ADR-045 Am1, DC-05 — `2026-08-07-ios-stale-
/// overlay-and-raw-error-alert` showed users a Rust `Debug` string).
fn assert_prepared_rejection_alert(envelope: &str, rejection_text: &str) {
    let batch: serde_json::Value = serde_json::from_str(envelope).expect("parse command batch");
    let alert = &batch["commands"][0]["PresentAlert"]["alert"];
    assert_eq!(alert["title"], "Error", "prepared title, got {batch}");
    assert_eq!(
        alert["message"], "Something went wrong",
        "prepared message, got {batch}"
    );
    assert!(
        !envelope.contains(rejection_text),
        "the rejection's own text must not reach the shell, got {batch}"
    );
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn malformed_event_json_is_explained_with_a_prepared_alert() {
    let (engine, _dir) = engine();

    let envelope = engine
        .dispatch_json("{not json".to_string())
        .expect("a refused payload is answered with commands, not an error");

    assert_prepared_rejection_alert(&envelope, "malformed");
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn event_for_an_inactive_surface_is_explained_with_a_prepared_alert() {
    let (engine, _dir) = engine();

    let envelope = engine
        .dispatch_json(value_changed("short"))
        .expect("a rejected event is answered with commands, not an error");

    assert_prepared_rejection_alert(&envelope, "surface");
}

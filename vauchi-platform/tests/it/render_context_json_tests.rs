// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the `PlatformAppEngine::set_render_context_json`
//! JSON shim.
//!
//! S2 of the settings-storage-by-sensitivity plan
//! (`_private/docs/planning/todo/2026-05-16-settings-storage-by-sensitivity-plan.md`).
//! Pins the JSON wire contract: valid shapes round-trip cleanly,
//! invalid shapes surface a parse error rather than silently
//! accepting garbage. AppEngine-side accessor correctness is
//! verified separately in
//! `core/vauchi-app/tests/it/render_context_tests.rs`.

use std::sync::Arc;

use vauchi_platform::PlatformAppEngine;

fn create_engine() -> (Arc<PlatformAppEngine>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    (engine, dir)
}

// @internal
#[test]
fn accepts_both_fields() {
    let (engine, _dir) = create_engine();
    let result =
        engine.set_render_context_json(r#"{"locale":"de","theme_id":"cyber"}"#.to_string());
    assert!(
        result.is_ok(),
        "both-fields JSON should round-trip cleanly: {result:?}"
    );
}

// @internal
#[test]
fn accepts_locale_only() {
    let (engine, _dir) = create_engine();
    let result = engine.set_render_context_json(r#"{"locale":"fr"}"#.to_string());
    assert!(
        result.is_ok(),
        "locale-only JSON should round-trip; theme_id defaults to None: {result:?}"
    );
}

// @internal
#[test]
fn accepts_theme_only() {
    let (engine, _dir) = create_engine();
    let result = engine.set_render_context_json(r#"{"theme_id":"classic"}"#.to_string());
    assert!(
        result.is_ok(),
        "theme-only JSON should round-trip; locale defaults to None: {result:?}"
    );
}

// @internal
#[test]
fn accepts_empty_object() {
    // Frontend boots before any preference has been picked — both
    // fields absent is a legitimate boot-time state. Must accept.
    let (engine, _dir) = create_engine();
    let result = engine.set_render_context_json("{}".to_string());
    assert!(
        result.is_ok(),
        "empty-object JSON should round-trip (boot-time defaults): {result:?}"
    );
}

// @internal
#[test]
fn accepts_explicit_nulls() {
    // `null` values are the explicit "no preference yet" wire form,
    // distinct from a literal string. Must accept (serde deserialises
    // `null` into `None` for `Option<String>` by default).
    let (engine, _dir) = create_engine();
    let result = engine.set_render_context_json(r#"{"locale":null,"theme_id":null}"#.to_string());
    assert!(
        result.is_ok(),
        "explicit-null JSON should map to None for both fields: {result:?}"
    );
}

// @internal
#[test]
fn rejects_invalid_json() {
    // Adversarial: non-JSON input must surface a parse error, not
    // silently accept. The frontend is the trust boundary; bad
    // input must fail loudly so the bug surfaces in frontend QA
    // rather than slipping into production with a stale render
    // context.
    let (engine, _dir) = create_engine();
    let result = engine.set_render_context_json("not json at all".to_string());
    assert!(result.is_err(), "non-JSON input must reject: {result:?}");
    let detail = format!("{:?}", result.unwrap_err());
    assert!(
        detail.contains("Invalid render context JSON"),
        "error message must mention render context parse failure: {detail}"
    );
}

// @internal
#[test]
fn rejects_wrong_field_type() {
    // Adversarial: `locale: 42` (integer instead of string) must
    // reject. Same trust-boundary discipline as `rejects_invalid_json`
    // — serde catches the type mismatch.
    let (engine, _dir) = create_engine();
    let result = engine.set_render_context_json(r#"{"locale":42}"#.to_string());
    assert!(
        result.is_err(),
        "integer in string field must reject: {result:?}"
    );
}

// @internal
#[test]
fn ignores_unknown_fields() {
    // Forward-compat: if a future frontend version pushes a JSON
    // payload with additional render-context fields not yet known
    // to this core version, the known fields must still parse.
    // Serde's default (`#[serde(deny_unknown_fields)]` is OFF) is
    // to ignore unknown fields — this test pins that behaviour as
    // intentional.
    let (engine, _dir) = create_engine();
    let result = engine.set_render_context_json(
        r#"{"locale":"de","theme_id":"cyber","future_field":"foo"}"#.to_string(),
    );
    assert!(
        result.is_ok(),
        "unknown future fields should be tolerated: {result:?}"
    );
}

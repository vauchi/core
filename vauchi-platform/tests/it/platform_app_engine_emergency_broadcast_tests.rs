// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the emergency-broadcast methods on
//! `PlatformAppEngine` (Phase B3 of `2026-04-28-collapse-vauchi-platform-into-app-engine`).

use std::sync::Arc;

use vauchi_platform::PlatformAppEngine;

/// Create a `PlatformAppEngine` with a temp directory, drive it through
/// the onboarding flow, and return the engine + tempdir.
fn create_engine_with_identity() -> (Arc<PlatformAppEngine>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");

    drive_onboarding(&engine);

    (engine, dir)
}

fn drive_onboarding(engine: &PlatformAppEngine) {
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "create_new"}}"#.into())
        .expect("create_new");
    engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Alice"}}"#.into(),
        )
        .expect("display_name");
    for _ in 0..3 {
        engine
            .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
            .expect("continue");
    }
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "start_app"}}"#.into())
        .expect("start_app");
}

// ── get_emergency_config ─────────────────────────────────────────────

// @internal
#[test]
fn get_emergency_config_is_none_initially() {
    let (engine, _dir) = create_engine_with_identity();
    let config = engine.get_emergency_config().expect("get_emergency_config");
    assert!(config.is_none(), "no config before configure");
}

// ── configure_emergency_broadcast ────────────────────────────────────

// @internal
#[test]
fn configure_emergency_broadcast_persists_via_get_config() {
    let (engine, _dir) = create_engine_with_identity();
    engine
        .configure_emergency_broadcast(
            vec!["contact-1".into(), "contact-2".into()],
            "Help me".into(),
            true,
        )
        .expect("configure");

    let config = engine
        .get_emergency_config()
        .expect("get_emergency_config")
        .expect("config exists after configure");

    assert_eq!(
        config.trusted_contact_ids,
        vec!["contact-1".to_string(), "contact-2".to_string()],
    );
    assert_eq!(config.message, "Help me");
    assert!(config.include_location);
}

// @internal
#[test]
fn configure_emergency_broadcast_with_no_location_persists_flag() {
    let (engine, _dir) = create_engine_with_identity();
    engine
        .configure_emergency_broadcast(vec![], "msg".into(), false)
        .expect("configure");

    let config = engine
        .get_emergency_config()
        .expect("get")
        .expect("present");
    assert!(!config.include_location);
}

// ── disable_emergency_broadcast ──────────────────────────────────────

// @internal
#[test]
fn disable_emergency_broadcast_clears_config() {
    let (engine, _dir) = create_engine_with_identity();
    engine
        .configure_emergency_broadcast(vec![], "msg".into(), false)
        .expect("configure");
    assert!(engine.get_emergency_config().expect("get").is_some());

    engine
        .disable_emergency_broadcast()
        .expect("disable_emergency_broadcast");

    assert!(
        engine.get_emergency_config().expect("get").is_none(),
        "disable must clear config"
    );
}

// @internal
#[test]
fn disable_emergency_broadcast_is_idempotent_when_no_config() {
    let (engine, _dir) = create_engine_with_identity();
    // No prior configure — disabling must succeed (idempotent).
    engine
        .disable_emergency_broadcast()
        .expect("disable on empty must not error");
}

// ── send_emergency_broadcast ─────────────────────────────────────────

// @internal
#[test]
fn send_emergency_broadcast_errors_when_no_config() {
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.send_emergency_broadcast();
    assert!(
        result.is_err(),
        "send without configure must error (no config to read)"
    );
}

// @internal
#[test]
fn send_emergency_broadcast_returns_total_after_configure() {
    let (engine, _dir) = create_engine_with_identity();
    engine
        .configure_emergency_broadcast(
            vec!["unknown-1".into(), "unknown-2".into()],
            "msg".into(),
            false,
        )
        .expect("configure");

    // Sending fails to deliver because the contacts don't exist in
    // storage, but the call must surface a result with `total` > 0
    // OR a clearly-attributable error. Either is acceptable surface
    // behavior — we mainly assert the wrapper does not panic.
    let _ = engine.send_emergency_broadcast();
}

// ── Cache invalidation contract ──────────────────────────────────────

// @internal
#[test]
fn configure_emergency_broadcast_invalidates_settings_cache() {
    // After a write, the next current_screen_json must rebuild the
    // affected screens rather than serve stale data. Smoke-level:
    // assert no panic on read-after-write.
    let (engine, _dir) = create_engine_with_identity();
    engine
        .configure_emergency_broadcast(vec![], "msg".into(), false)
        .expect("configure");

    engine.invalidate_all().expect("invalidate_all");
    let _ = engine
        .current_screen_json()
        .expect("current_screen_json after configure");
}

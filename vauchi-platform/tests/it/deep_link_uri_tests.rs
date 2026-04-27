// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for `PlatformAppEngine::handle_deep_link_uri`.
//!
//! Phase 1 T6 of `2026-04-25-deeplink-consent-orchestrator`. Covers
//! the UniFFI dispatch from raw URI string to the consent screen
//! JSON, plus the four typed-error variants surfaced as
//! `MobileError::InvalidInput { field, .. }`.

use vauchi_platform::{MobileError, PlatformAppEngine};

fn create_engine() -> (std::sync::Arc<PlatformAppEngine>, tempfile::TempDir) {
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

fn fresh_link_url() -> String {
    let (init, _) = vauchi_core::exchange::link_mode::initiator_generate();
    init.url
}

// @internal
#[test]
fn valid_uri_navigates_to_consent_screen() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    let json = engine
        .handle_deep_link_uri(url)
        .expect("canonical URL must dispatch");
    let v: serde_json::Value = serde_json::from_str(&json).expect("response is JSON");
    assert_eq!(v["screen_id"], "deep_link_consent");
    assert_eq!(v["title"], "Exchange Request");
    let actions = v["actions"].as_array().expect("actions is array");
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0]["id"], "grant");
    assert_eq!(actions[1]["id"], "deny");
}

// @internal
#[test]
fn invalid_scheme_returns_typed_invalid_input() {
    let (engine, _dir) = create_engine();
    let err = engine
        .handle_deep_link_uri("https://exchange?pk=AAAA&n=BBBB".into())
        .expect_err("non-vauchi scheme must reject");
    match err {
        MobileError::InvalidInput { field, .. } => assert_eq!(field, "deep_link_scheme"),
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

// @internal
#[test]
fn invalid_host_returns_typed_invalid_input() {
    let (engine, _dir) = create_engine();
    let err = engine
        .handle_deep_link_uri("vauchi://recover?pk=AAAA&n=BBBB".into())
        .expect_err("non-exchange host must reject");
    match err {
        MobileError::InvalidInput { field, .. } => assert_eq!(field, "deep_link_host"),
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

// @internal
#[test]
fn legacy_path_form_returns_typed_invalid_input() {
    let (engine, _dir) = create_engine();
    let err = engine
        .handle_deep_link_uri("vauchi://exchange/somepayload".into())
        .expect_err("legacy path form must reject");
    match err {
        MobileError::InvalidInput { field, .. } => assert_eq!(field, "deep_link_format"),
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

// @internal
#[test]
fn malformed_query_returns_typed_invalid_input() {
    let (engine, _dir) = create_engine();
    let err = engine
        .handle_deep_link_uri("vauchi://exchange?pk=AAAA".into())
        .expect_err("missing nonce must reject");
    match err {
        MobileError::InvalidInput { field, .. } => assert_eq!(field, "deep_link_format"),
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

// @internal
#[test]
fn after_dispatch_current_screen_is_consent_gate() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    engine.handle_deep_link_uri(url).expect("dispatch ok");
    let id = engine
        .current_screen_id()
        .expect("current_screen_id ok after dispatch");
    assert_eq!(id, "deep_link_consent");
}

// @internal
#[test]
fn grant_action_completes_consent_gate() {
    // Drive the full flow: handle_deep_link_uri → grant → engine
    // returns to a non-consent screen (Phase 1: AppEngine routes
    // ActionResult::Complete back to the default screen).
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    engine.handle_deep_link_uri(url).expect("dispatch ok");
    let result = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "grant"}}"#.into())
        .expect("grant action ok");
    let v: serde_json::Value = serde_json::from_str(&result).expect("parse result");
    // ActionResult::Complete is wrapped in a "Complete" tag or causes
    // navigation to a non-consent screen — either way, the consent
    // gate is no longer active.
    let screen_id_after = engine.current_screen_id().expect("screen id after grant");
    assert_ne!(
        screen_id_after, "deep_link_consent",
        "consent gate must release after grant; got result {v}"
    );
}

// @internal
#[test]
fn deny_action_completes_consent_gate() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    engine.handle_deep_link_uri(url).expect("dispatch ok");
    let _ = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "deny"}}"#.into())
        .expect("deny action ok");
    let screen_id_after = engine.current_screen_id().expect("screen id after deny");
    assert_ne!(
        screen_id_after, "deep_link_consent",
        "consent gate must release after deny"
    );
}

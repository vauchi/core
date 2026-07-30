// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for deep-link ingestion via the generic event protocol.
//!
//! Deep links no longer have a dedicated UniFFI/C-ABI surface; the frontend
//! forwards the raw URI as `DeepLinkOpened` and Core parses + routes it. This
//! file tests the routing contract for `vauchi://exchange` (exchange consent)
//! and `vauchi://device-link` (device-link join), plus the error shape for
//! unsupported links.

use serde_json::Value;
use vauchi_platform::{PlatformAppEngine, PlatformAppEngineTestHelpers};

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

fn link_opened_action(uri: &str) -> String {
    format!(r#"{{"LinkOpened": {{"uri": {}}}}}"#, serde_json::json!(uri))
}

// @internal
fn current_screen_id(engine: &PlatformAppEngine) -> String {
    let json = engine.current_screen_json().expect("current_screen_json");
    let v: Value = serde_json::from_str(&json).expect("parse screen json");
    v.get("screen_id")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string()
}

// @internal
fn action_result_from_envelope(envelope_json: &str) -> Value {
    let v: Value = serde_json::from_str(envelope_json).expect("parse envelope");
    v.get("action_result").cloned().unwrap_or(Value::Null)
}

#[test]
fn valid_exchange_uri_navigates_to_consent_screen() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    let event = serde_json::json!({"DeepLinkOpened": {"uri": url}});
    let result: Value = serde_json::from_str(
        &engine
            .dispatch_json(event.to_string())
            .expect("DeepLinkOpened must dispatch"),
    )
    .expect("command envelope");
    let commands = result["commands"].as_array().expect("commands array");
    let surface = commands
        .iter()
        .find_map(|command| command.get("ReplaceSurface"))
        .and_then(|command| command.get("surface"))
        .expect("consent surface command");
    assert_eq!(surface["surface_id"], "deep_link_consent");
    assert_eq!(surface["title"], "Exchange Request");

    let context_bar = commands
        .iter()
        .find_map(|command| command.get("SetContextBar"))
        .and_then(|command| command.get("bar"))
        .expect("consent context bar command");
    assert_eq!(context_bar["primary"]["label"], "Accept Exchange");
    assert_eq!(context_bar["secondary"]["label"], "Actions");
    assert!(
        surface.get("actions").is_none(),
        "legacy ScreenModel actions must not cross the presentation boundary"
    );
}

#[test]
fn invalid_scheme_returns_show_alert() {
    let (engine, _dir) = create_engine();
    let before = current_screen_id(&engine);
    let result = engine
        .handle_action_json(link_opened_action("https://exchange?pk=AAAA&n=BBBB"))
        .expect("LinkOpened must return envelope");
    let action_result = action_result_from_envelope(&result);
    assert!(
        action_result.get("ShowAlert").is_some(),
        "non-vauchi scheme must surface ShowAlert, got {result}"
    );
    assert_eq!(
        current_screen_id(&engine),
        before,
        "screen must not change on invalid link"
    );
}

#[test]
fn unsupported_host_returns_show_alert() {
    let (engine, _dir) = create_engine();
    let before = current_screen_id(&engine);
    let result = engine
        .handle_action_json(link_opened_action("vauchi://recover?pk=AAAA&n=BBBB"))
        .expect("LinkOpened must return envelope");
    let action_result = action_result_from_envelope(&result);
    assert!(
        action_result.get("ShowAlert").is_some(),
        "unsupported host must surface ShowAlert, got {result}"
    );
    assert_eq!(
        current_screen_id(&engine),
        before,
        "screen must not change on unsupported host"
    );
}

#[test]
fn legacy_path_form_returns_show_alert() {
    let (engine, _dir) = create_engine();
    let before = current_screen_id(&engine);
    let result = engine
        .handle_action_json(link_opened_action("vauchi://exchange/somepayload"))
        .expect("LinkOpened must return envelope");
    let action_result = action_result_from_envelope(&result);
    assert!(
        action_result.get("ShowAlert").is_some(),
        "legacy path form must surface ShowAlert, got {result}"
    );
    assert_eq!(
        current_screen_id(&engine),
        before,
        "screen must not change on legacy path form"
    );
}

#[test]
fn malformed_query_returns_show_alert() {
    let (engine, _dir) = create_engine();
    let before = current_screen_id(&engine);
    let result = engine
        .handle_action_json(link_opened_action("vauchi://exchange?pk=AAAA"))
        .expect("LinkOpened must return envelope");
    let action_result = action_result_from_envelope(&result);
    assert!(
        action_result.get("ShowAlert").is_some(),
        "malformed query must surface ShowAlert, got {result}"
    );
    assert_eq!(
        current_screen_id(&engine),
        before,
        "screen must not change on malformed query"
    );
}

#[test]
fn after_dispatch_current_screen_is_consent_gate() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    engine
        .handle_action_json(link_opened_action(&url))
        .expect("dispatch ok");
    let id = current_screen_id(&engine);
    assert_eq!(id, "deep_link_consent");
}

#[test]
fn grant_action_completes_consent_gate() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    engine
        .handle_action_json(link_opened_action(&url))
        .expect("dispatch ok");
    let result = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "grant"}}"#.into())
        .expect("grant action ok");
    let v: Value = serde_json::from_str(&result).expect("parse result");
    let screen_id_after = current_screen_id(&engine);
    assert_ne!(
        screen_id_after, "deep_link_consent",
        "consent gate must release after grant; got result {v}"
    );
}

#[test]
fn deny_action_completes_consent_gate() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    engine
        .handle_action_json(link_opened_action(&url))
        .expect("dispatch ok");
    let _ = engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "deny"}}"#.into())
        .expect("deny action ok");
    let screen_id_after = current_screen_id(&engine);
    assert_ne!(
        screen_id_after, "deep_link_consent",
        "consent gate must release after deny"
    );
}

// @internal
#[test]
fn valid_device_link_uri_navigates_to_join_screen() {
    let (engine, _dir) = create_engine();
    let result = engine
        .handle_action_json(link_opened_action(
            "vauchi://device-link?qr=d2hhdGV2ZXI&code=QlJPS0VSNDI",
        ))
        .expect("device-link URL must dispatch");
    let action_result = action_result_from_envelope(&result);
    let screen = action_result
        .get("NavigateTo")
        .expect("expected NavigateTo result");
    assert_eq!(screen["screen_id"], "device_link_join");
}

// @internal
fn add_test_contact(engine: &PlatformAppEngine, name: &str) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let contact = vauchi_core::Contact::from_exchange(
        [0xAB; 32],
        card,
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    engine
        .save_test_contact(&contact)
        .expect("save test contact");
    id
}

// @internal
#[test]
fn valid_contact_uri_navigates_to_contact_detail() {
    let (engine, _dir) = create_engine();
    let contact_id = add_test_contact(&engine, "Alice");
    let result = engine
        .handle_action_json(link_opened_action(&format!(
            "vauchi://contact/{contact_id}"
        )))
        .expect("contact URL must dispatch");
    let action_result = action_result_from_envelope(&result);
    let screen = action_result
        .get("NavigateTo")
        .expect("expected NavigateTo result");
    assert_eq!(screen["screen_id"], "contact_detail");
    assert_eq!(
        screen["title"], "Alice",
        "ContactDetail title should be the contact's display name"
    );
}

// @internal
#[test]
fn unknown_contact_uri_returns_show_alert() {
    let (engine, _dir) = create_engine();
    let before = current_screen_id(&engine);
    let result = engine
        .handle_action_json(link_opened_action("vauchi://contact/unknown-id-123"))
        .expect("contact URL must dispatch");
    let action_result = action_result_from_envelope(&result);
    assert!(
        action_result.get("ShowAlert").is_some(),
        "unknown contact id must surface ShowAlert, got {result}"
    );
    assert_eq!(
        current_screen_id(&engine),
        before,
        "screen must not change on unknown contact"
    );
}

// @internal
#[test]
fn malformed_contact_uri_returns_show_alert() {
    let (engine, _dir) = create_engine();
    let before = current_screen_id(&engine);
    let result = engine
        .handle_action_json(link_opened_action("vauchi://contact/abc/def"))
        .expect("contact URL must dispatch");
    let action_result = action_result_from_envelope(&result);
    assert!(
        action_result.get("ShowAlert").is_some(),
        "malformed contact URI must surface ShowAlert, got {result}"
    );
    assert_eq!(
        current_screen_id(&engine),
        before,
        "screen must not change on malformed contact URI"
    );
}

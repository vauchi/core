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

fn deep_link_event(uri: &str) -> String {
    serde_json::json!({"DeepLinkOpened": {"uri": uri}}).to_string()
}

/// Dispatch one canonical event and return the parsed command batch.
fn dispatch(engine: &PlatformAppEngine, event_json: String) -> Value {
    serde_json::from_str(
        &engine
            .dispatch_json(event_json)
            .expect("event must dispatch"),
    )
    .expect("command envelope")
}

/// Activate an interaction from a command batch (bar role or overlay item)
/// and return the next batch — the way a shell forwards a tap.
fn activate(engine: &PlatformAppEngine, surface_id: &str, interaction_id: &str) -> Value {
    let event = serde_json::json!({
        "ActionActivated": { "surface_id": surface_id, "interaction_id": interaction_id }
    });
    dispatch(engine, event.to_string())
}

/// The `(surface_id, interaction_id)` of a context-bar role in this batch.
fn bar_role(batch: &Value, role: &str) -> (String, String) {
    let bar = batch["commands"]
        .as_array()
        .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
        .expect("command batch must carry a context bar");
    (
        bar["surface_id"]
            .as_str()
            .expect("bar surface id")
            .to_owned(),
        bar["bar"][role]["interaction_id"]
            .as_str()
            .unwrap_or_else(|| panic!("bar must carry a {role} interaction"))
            .to_owned(),
    )
}

/// The Decline-style item of the action-menu overlay in this batch.
fn overlay_item(batch: &Value, label: &str) -> (String, String) {
    let overlay = batch["commands"]
        .as_array()
        .and_then(|commands| commands.iter().find_map(|c| c.get("PresentOverlay")))
        .expect("command batch must present an overlay");
    let item = overlay["overlay"]["items"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["label"].as_str() == Some(label))
        })
        .unwrap_or_else(|| panic!("overlay must carry a {label} item"));
    (
        overlay["surface_id"]
            .as_str()
            .expect("overlay surface id")
            .to_owned(),
        item["interaction_id"]
            .as_str()
            .expect("item interaction id")
            .to_owned(),
    )
}

/// First replaced surface id in a batch, when the batch replaces one.
fn batch_surface_id(batch: &Value) -> Option<String> {
    batch["commands"].as_array().and_then(|commands| {
        commands.iter().find_map(|c| {
            c["ReplaceSurface"]["surface"]["surface_id"]
                .as_str()
                .map(str::to_owned)
        })
    })
}

/// The alert of a PresentAlert command in this batch, when present.
fn batch_alert(batch: &Value) -> Option<Value> {
    batch["commands"].as_array().and_then(|commands| {
        commands.iter().find_map(|c| {
            let alert = &c["PresentAlert"]["alert"];
            alert.is_object().then(|| alert.clone())
        })
    })
}

// @internal
fn current_screen_id(engine: &PlatformAppEngine) -> String {
    // initial_commands re-composes the current presentation — the same
    // refresh path a shell hits on load — so the current surface id is
    // readable without the retired screen seam.
    let batch = dispatch(engine, r#""PresentationInvalidated""#.into());
    batch_surface_id(&batch).unwrap_or_default()
}

/// Dispatch a deep link that must be rejected and assert the generic
/// alert command plus that the visible surface did not change.
fn assert_invalid_link_alert(engine: &PlatformAppEngine, uri: &str, case: &str) {
    let before = current_screen_id(engine);
    let batch = dispatch(engine, deep_link_event(uri));
    assert!(
        batch_alert(&batch).is_some(),
        "{case} must surface a PresentAlert command, got {batch}"
    );
    assert_eq!(
        current_screen_id(engine),
        before,
        "surface must not change on {case}"
    );
}

#[test]
fn valid_exchange_uri_navigates_to_consent_screen() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    let result = dispatch(&engine, deep_link_event(&url));
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
    assert_invalid_link_alert(
        &engine,
        "https://exchange?pk=AAAA&n=BBBB",
        "non-vauchi scheme",
    );
}

#[test]
fn unsupported_host_returns_show_alert() {
    let (engine, _dir) = create_engine();
    assert_invalid_link_alert(
        &engine,
        "vauchi://recover?pk=AAAA&n=BBBB",
        "unsupported host",
    );
}

#[test]
fn legacy_path_form_returns_show_alert() {
    let (engine, _dir) = create_engine();
    assert_invalid_link_alert(&engine, "vauchi://exchange/somepayload", "legacy path form");
}

#[test]
fn malformed_query_returns_show_alert() {
    let (engine, _dir) = create_engine();
    assert_invalid_link_alert(&engine, "vauchi://exchange?pk=AAAA", "malformed query");
}

#[test]
fn after_dispatch_current_screen_is_consent_gate() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    let _ = dispatch(&engine, deep_link_event(&url));
    let id = current_screen_id(&engine);
    assert_eq!(id, "deep_link_consent");
}

#[test]
fn grant_action_completes_consent_gate() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    let batch = dispatch(&engine, deep_link_event(&url));
    let (surface_id, grant_id) = bar_role(&batch, "primary");
    let result = activate(&engine, &surface_id, &grant_id);
    let screen_id_after = current_screen_id(&engine);
    assert_ne!(
        screen_id_after, "deep_link_consent",
        "consent gate must release after grant; got result {result}"
    );
}

#[test]
fn deny_action_completes_consent_gate() {
    let (engine, _dir) = create_engine();
    let url = fresh_link_url();
    let batch = dispatch(&engine, deep_link_event(&url));
    // Decline lives in the secondary action menu: open it, then activate
    // the item — the same two taps a user makes.
    let (surface_id, secondary_id) = bar_role(&batch, "secondary");
    let overlay = activate(&engine, &surface_id, &secondary_id);
    let (overlay_surface, deny_id) = overlay_item(&overlay, "Decline");
    let _ = activate(&engine, &overlay_surface, &deny_id);
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
    let batch = dispatch(
        &engine,
        deep_link_event("vauchi://device-link?qr=d2hhdGV2ZXI&code=QlJPS0VSNDI"),
    );
    assert_eq!(
        batch_surface_id(&batch).as_deref(),
        Some("device_link_join"),
        "device-link URI should replace with the device_link_join surface, got: {batch}"
    );
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
    let batch = dispatch(
        &engine,
        deep_link_event(&format!("vauchi://contact/{contact_id}")),
    );
    let commands = batch["commands"].as_array().expect("commands array");
    // The contact detail opens as a pane of the contacts surface; the
    // retired "contact_detail" screen id was Core-internal vocabulary.
    // What a shell can observe is the Core-prepared title of the detail
    // pane: the contact's display name.
    let titles: Vec<&str> = commands
        .iter()
        .filter_map(|command| command["ReplaceSurface"]["surface"]["title"].as_str())
        .collect();
    assert!(
        titles.contains(&"Alice"),
        "ContactDetail pane title should be the contact's display name, got titles {titles:?}"
    );
}

// @internal
#[test]
fn unknown_contact_uri_returns_show_alert() {
    let (engine, _dir) = create_engine();
    assert_invalid_link_alert(
        &engine,
        "vauchi://contact/unknown-id-123",
        "unknown contact id",
    );
}

// @internal
#[test]
fn malformed_contact_uri_returns_show_alert() {
    let (engine, _dir) = create_engine();
    assert_invalid_link_alert(&engine, "vauchi://contact/abc/def", "malformed contact URI");
}

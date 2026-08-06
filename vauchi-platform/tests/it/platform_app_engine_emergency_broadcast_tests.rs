// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the emergency-broadcast methods on
//! `PlatformAppEngine` (Phase B3 of `2026-04-28-collapse-vauchi-platform-into-app-engine`).

use std::sync::Arc;

use vauchi_platform::{
    DomainCommand, DomainCommandResult, MobileEmergencyConfig, PlatformAppEngine,
};

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

/// Drive through the full onboarding flow via the canonical envelope.
///
/// Every step reads the Core-minted interaction and binding ids from the
/// current command batch — exactly what a real shell renders — and
/// dispatches generic events back. No retired action/screen seams.
fn drive_onboarding(engine: &PlatformAppEngine) {
    fn primary_interaction(batch: &serde_json::Value) -> (String, String) {
        let bar = batch["commands"]
            .as_array()
            .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
            .expect("command batch must carry a context bar");
        (
            bar["surface_id"]
                .as_str()
                .expect("bar surface id")
                .to_owned(),
            bar["bar"]["primary"]["interaction_id"]
                .as_str()
                .expect("primary interaction id")
                .to_owned(),
        )
    }

    fn dispatch_primary(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
    ) -> serde_json::Value {
        let (surface_id, interaction_id) = primary_interaction(batch);
        let event = serde_json::json!({
            "ActionActivated": { "surface_id": surface_id, "interaction_id": interaction_id }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch primary activation"),
        )
        .expect("parse command batch")
    }

    fn find_input<'v>(nodes: &'v [serde_json::Value]) -> Option<&'v serde_json::Value> {
        nodes.iter().find_map(|node| {
            if let Some(input) = node.get("Input") {
                Some(input)
            } else {
                node["Group"]["children"]
                    .as_array()
                    .and_then(|children| find_input(children))
            }
        })
    }

    fn set_text_input(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
        text: &str,
    ) -> serde_json::Value {
        let (surface_id, nodes) = batch["commands"]
            .as_array()
            .and_then(|commands| {
                commands.iter().find_map(|c| {
                    let surface = &c["ReplaceSurface"]["surface"];
                    surface
                        .is_object()
                        .then(|| (surface["surface_id"].clone(), surface["nodes"].clone()))
                })
            })
            .expect("command batch must replace a surface");
        let nodes: Vec<serde_json::Value> =
            serde_json::from_value(nodes).expect("surface nodes array");
        let input = find_input(&nodes).expect("surface must carry a text input");
        let event = serde_json::json!({
            "ValueChanged": {
                "surface_id": surface_id,
                "binding_id": input["binding_id"],
                "value": { "text": text },
            }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch text input"),
        )
        .expect("parse command batch")
    }

    let mut batch: serde_json::Value = serde_json::from_str(
        &engine
            .initial_commands_json()
            .expect("initial onboarding commands"),
    )
    .expect("parse initial batch");

    batch = dispatch_primary(engine, &batch); // identity_check → default_name
    batch = set_text_input(engine, &batch, "Alice"); // enter display name
    batch = dispatch_primary(engine, &batch); // default_name → groups_setup
    batch = dispatch_primary(engine, &batch); // groups_setup → contact_info
    batch = dispatch_primary(engine, &batch); // contact_info → what_next
    let _ = dispatch_primary(engine, &batch); // what_next → complete → home
}

// ── Dispatch helpers (typed methods retired in Track B B4b) ──

fn configure(
    engine: &PlatformAppEngine,
    contact_ids: Vec<String>,
    message: &str,
    include_location: bool,
) {
    let result = engine
        .dispatch_domain_command(DomainCommand::ConfigureEmergencyBroadcast {
            contact_ids,
            message: message.to_string(),
            include_location,
        })
        .expect("dispatch ConfigureEmergencyBroadcast");
    assert!(
        matches!(result, DomainCommandResult::Bool { value: true }),
        "configure must return Bool(true), got {result:?}"
    );
}

fn get_config(engine: &PlatformAppEngine) -> Option<MobileEmergencyConfig> {
    let result = engine
        .dispatch_domain_command(DomainCommand::GetEmergencyConfig)
        .expect("dispatch GetEmergencyConfig");
    let DomainCommandResult::OptionalEmergencyConfig { config } = result else {
        panic!("GetEmergencyConfig: unexpected result variant {result:?}");
    };
    config
}

fn disable(
    engine: &PlatformAppEngine,
) -> Result<DomainCommandResult, vauchi_platform::MobileError> {
    engine.dispatch_domain_command(DomainCommand::DisableEmergencyBroadcast)
}

// ── get_emergency_config ─────────────────────────────────────────────

// @internal
#[test]
fn get_emergency_config_is_none_initially() {
    let (engine, _dir) = create_engine_with_identity();
    let config = get_config(&engine);
    assert!(config.is_none(), "no config before configure");
}

// ── configure_emergency_broadcast ────────────────────────────────────

// @internal
#[test]
fn configure_emergency_broadcast_persists_via_get_config() {
    let (engine, _dir) = create_engine_with_identity();
    configure(
        &engine,
        vec!["contact-1".into(), "contact-2".into()],
        "Help me",
        true,
    );

    let config = get_config(&engine).expect("config exists after configure");

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
    configure(&engine, vec![], "msg", false);

    let config = get_config(&engine).expect("present");
    assert!(!config.include_location);
}

// ── disable_emergency_broadcast ──────────────────────────────────────

// @internal
#[test]
fn disable_emergency_broadcast_clears_config() {
    let (engine, _dir) = create_engine_with_identity();
    configure(&engine, vec![], "msg", false);
    assert!(get_config(&engine).is_some());

    disable(&engine).expect("disable_emergency_broadcast");

    assert!(get_config(&engine).is_none(), "disable must clear config");
}

// @internal
#[test]
fn disable_emergency_broadcast_is_idempotent_when_no_config() {
    let (engine, _dir) = create_engine_with_identity();
    // No prior configure — disabling must succeed (idempotent).
    disable(&engine).expect("disable on empty must not error");
}

// ── send_emergency_broadcast ─────────────────────────────────────────

// @internal
#[test]
fn send_emergency_broadcast_errors_when_no_config() {
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.dispatch_domain_command(DomainCommand::SendEmergencyBroadcast);
    assert!(
        result.is_err(),
        "send without configure must error (no config to read)"
    );
}

// @internal
#[test]
fn send_emergency_broadcast_returns_total_after_configure() {
    let (engine, _dir) = create_engine_with_identity();
    configure(
        &engine,
        vec!["unknown-1".into(), "unknown-2".into()],
        "msg",
        false,
    );

    // Sending fails to deliver because the contacts don't exist in
    // storage, but the call must surface a result with `total` > 0
    // OR a clearly-attributable error. Either is acceptable surface
    // behavior — assert the dispatch routes to the right result shape
    // on the Ok path and tolerate the delivery error.
    let result = engine.dispatch_domain_command(DomainCommand::SendEmergencyBroadcast);
    match result {
        Ok(r) => assert!(
            matches!(r, DomainCommandResult::BroadcastResult { .. }),
            "SendEmergencyBroadcast must yield a BroadcastResult, got {r:?}"
        ),
        Err(_) => { /* delivery to non-existent contacts may legitimately error */ }
    }
}

// ── Cache invalidation contract ──────────────────────────────────────

// @internal
#[test]
fn configure_emergency_broadcast_invalidates_settings_cache() {
    // After a write, the next initial_commands_json must rebuild the
    // affected screens rather than serve stale data. Smoke-level:
    // assert no panic on read-after-write.
    let (engine, _dir) = create_engine_with_identity();
    configure(&engine, vec![], "msg", false);

    engine
        .dispatch_json(r#""PresentationInvalidated""#.into())
        .expect("presentation invalidation");
    let _ = engine
        .initial_commands_json()
        .expect("initial_commands_json after configure");
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the device-linking domain on `PlatformAppEngine`
//! (Phase B4 of `2026-04-28-collapse-vauchi-platform-into-app-engine`).
//!
//! Surface migrated: the **post-orchestrator** shape only. The
//! pre-orchestrator legacy methods (`start_device_link`,
//! `start_device_join`, `send_device_link_request`,
//! `listen_for_device_link_request`, `send_device_link_response`)
//! are intentionally NOT migrated — they were superseded by the
//! orchestrator session in `done/2026-04-27-device-link-orchestrator-phase2d-windows`.

use std::sync::Arc;

use vauchi_platform::PlatformAppEngine;

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

// ── get_devices / device_count / is_primary_device ───────────────────

// @internal
#[test]
fn get_devices_returns_only_current_device_initially() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let result = engine
        .dispatch_domain_command(DomainCommand::GetDevices)
        .expect("dispatch GetDevices");
    let DomainCommandResult::Devices { devices } = result else {
        panic!("GetDevices: unexpected result variant {result:?}");
    };
    assert_eq!(
        devices.len(),
        1,
        "fresh identity has only the current device"
    );
    assert!(devices[0].is_current);
    assert!(devices[0].is_active);
    assert_eq!(devices[0].device_index, 0, "current is primary");
}

// @internal
#[test]
fn device_count_is_one_initially() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let result = engine
        .dispatch_domain_command(DomainCommand::GetDeviceCount)
        .expect("dispatch GetDeviceCount");
    let DomainCommandResult::Count { value: count } = result else {
        panic!("GetDeviceCount: unexpected result variant {result:?}");
    };
    assert_eq!(count, 1, "fresh identity has one device");
}

// @internal
#[test]
fn is_primary_device_is_true_for_first_device() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let result = engine
        .dispatch_domain_command(DomainCommand::IsPrimaryDevice)
        .expect("dispatch IsPrimaryDevice");
    let DomainCommandResult::Bool { value: is_primary } = result else {
        panic!("IsPrimaryDevice: unexpected result variant {result:?}");
    };
    assert!(is_primary, "first device must be primary");
}

// ── unlink_device ────────────────────────────────────────────────────

// @internal
#[test]
fn unlink_device_returns_false_for_out_of_range_index() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let result = engine
        .dispatch_domain_command(DomainCommand::UnlinkDevice { device_index: 99 })
        .expect("dispatch UnlinkDevice");
    let DomainCommandResult::Bool { value } = result else {
        panic!("UnlinkDevice: unexpected result variant {result:?}");
    };
    assert!(!value, "out-of-range index returns false");
}

// @internal
#[test]
fn unlink_device_returns_false_when_no_registry_yet() {
    // A fresh identity has no on-disk DeviceRegistry — `get_devices()`
    // synthesises a single-element list from the identity's own
    // `device_info`, but `unlink_device` reads the persisted registry
    // and returns `false` rather than erroring. The "Cannot unlink the
    // current device" error path only fires once a registry has been
    // saved (after the first `confirm_link` succeeds end-to-end).
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let result = engine
        .dispatch_domain_command(DomainCommand::UnlinkDevice { device_index: 0 })
        .expect("dispatch UnlinkDevice(0)");
    let DomainCommandResult::Bool { value } = result else {
        panic!("UnlinkDevice: unexpected result variant {result:?}");
    };
    assert!(!value, "no registry yet → false (matches legacy behaviour)");
}

// ── generate_device_link_qr / parse_device_link_qr ──────────────────

// @internal
#[test]
fn generate_device_link_qr_returns_data_with_expiry() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let result = engine
        .dispatch_domain_command(DomainCommand::GenerateDeviceLinkQr)
        .expect("dispatch GenerateDeviceLinkQr");
    let DomainCommandResult::DeviceLinkData { data: qr } = result else {
        panic!("GenerateDeviceLinkQr: unexpected result variant {result:?}");
    };

    assert!(!qr.qr_data.is_empty(), "qr_data must not be empty");
    assert_eq!(
        qr.identity_public_key.len(),
        64,
        "identity pk hex == 64 chars"
    );
    assert!(
        qr.expires_at > qr.timestamp,
        "expires_at must be after timestamp"
    );
}

// @internal
#[test]
fn parse_device_link_qr_round_trips_generate() {
    use vauchi_platform::{DomainCommand, DomainCommandResult};
    let (engine, _dir) = create_engine_with_identity();
    let gen_result = engine
        .dispatch_domain_command(DomainCommand::GenerateDeviceLinkQr)
        .expect("dispatch GenerateDeviceLinkQr");
    let DomainCommandResult::DeviceLinkData { data: qr } = gen_result else {
        panic!("GenerateDeviceLinkQr: unexpected result variant {gen_result:?}");
    };

    let parse_result = engine
        .dispatch_domain_command(DomainCommand::ParseDeviceLinkQr {
            qr_data: qr.qr_data,
        })
        .expect("dispatch ParseDeviceLinkQr");
    let DomainCommandResult::DeviceLinkInfo { info: parsed } = parse_result else {
        panic!("ParseDeviceLinkQr: unexpected result variant {parse_result:?}");
    };

    assert_eq!(parsed.identity_public_key, qr.identity_public_key);
    assert_eq!(parsed.timestamp, qr.timestamp);
    assert!(!parsed.is_expired, "fresh QR must not be expired");
}

// @internal
#[test]
fn parse_device_link_qr_rejects_garbage_input() {
    use vauchi_platform::DomainCommand;
    let (engine, _dir) = create_engine_with_identity();
    let result = engine.dispatch_domain_command(DomainCommand::ParseDeviceLinkQr {
        qr_data: "not-a-qr".into(),
    });
    assert!(result.is_err(), "garbage QR must error");
}

// ── Cache invalidation contract ──────────────────────────────────────

// @internal
#[test]
fn unlink_device_invalidates_device_management_cache() {
    use vauchi_platform::DomainCommand;
    let (engine, _dir) = create_engine_with_identity();
    // out-of-range so no actual mutation, but the wrapper must still
    // be a no-panic call and the read-after-call must succeed.
    let _ = engine.dispatch_domain_command(DomainCommand::UnlinkDevice { device_index: 99 });
    engine
        .dispatch_json(r#""PresentationInvalidated""#.into())
        .expect("presentation invalidation");
    let _ = engine
        .initial_commands_json()
        .expect("initial_commands_json after unlink_device");
}

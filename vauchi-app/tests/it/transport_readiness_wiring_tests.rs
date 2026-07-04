// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine wiring for the TransportReadiness ledger (Phase 2 T2.1b of
//! `2026-06-11-exchange-waits-forever-without-capabilities`).
//!
//! `Event::PermissionDenied` at the `handle_hardware_event` choke point feeds
//! the device-wide ledger up front (regardless of screen). Non-transport
//! labels — notably `"location"` (the ADR-051 capture-geolocation permission)
//! — are ignored, so declining a capture prompt never gates a BLE mode.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ScreenModel, UserAction, WorkflowEngine,
};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::PermissionState;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::DeviceRequirement;

fn engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Me").unwrap();
    AppEngine::new(vauchi)
}

// @internal
#[test]
fn permission_denied_event_updates_the_readiness_ledger() {
    let mut engine = engine();
    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Camera),
        PermissionState::Unknown,
        "ledger starts clean"
    );

    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });

    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Camera),
        PermissionState::Denied,
        "a camera PermissionDenied must be recorded device-wide"
    );
}

// @internal
#[test]
fn location_permission_denied_does_not_touch_any_transport() {
    // "location" is the ADR-051 capture-geolocation permission, not a
    // transport — it must not deny BLE (or anything) in the ledger.
    let mut engine = engine();

    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "location".into(),
    });

    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Ble),
        PermissionState::Unknown,
        "a location denial is the capture-geolocation permission, not BLE"
    );
}

// @internal
#[test]
fn ble_permission_denied_event_updates_the_ledger() {
    // iOS emits "BLE" (uppercase); exercise the real label through the
    // AppEngine choke point, not just the unit mapping.
    let mut engine = engine();

    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "BLE".into(),
    });

    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Ble),
        PermissionState::Denied
    );
}

/// Capabilities where Glance's transports (camera + BLE) are both present, so a
/// camera denial yields a present-but-denied *grant affordance*, not an
/// `Unavailable` (hardware-absent) row.
fn caps_camera_and_ble() -> DeviceCapabilities {
    DeviceCapabilities {
        has_camera: true,
        has_ble: true,
        ..Default::default()
    }
}

/// All `ActionList` item ids across a screen, in render order.
fn item_ids(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::ActionList { items, .. } => Some(items),
            _ => None,
        })
        .flatten()
        .map(|item| item.id.clone())
        .collect()
}

// @internal
#[test]
fn denied_camera_turns_glance_into_a_grant_affordance_on_the_live_picker() {
    let mut engine = engine();
    engine.set_device_capabilities(caps_camera_and_ble());
    let entry = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(entry.screen_id, "exchange");
    assert!(
        item_ids(&entry).iter().any(|id| id == "mode:glance"),
        "with camera + BLE present and no denial, Glance is selectable"
    );

    // Deny camera while the picker is the live screen: the ledger records it
    // AND `rebuild_exchange_engine` re-renders the picker in place (T2.2).
    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });
    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Camera),
        PermissionState::Denied
    );

    let ids = item_ids(&engine.current_screen());
    assert!(
        ids.iter().any(|id| id == "grant:glance:camera"),
        "a denied-but-present camera turns Glance into a grant affordance; got {ids:?}"
    );
    assert!(
        !ids.iter().any(|id| id == "mode:glance"),
        "the denied Glance must not also be a selectable mode; got {ids:?}"
    );
}

// @internal
#[test]
fn tapping_grant_affordance_relearns_permission_and_restores_the_mode() {
    let mut engine = engine();
    engine.set_device_capabilities(caps_camera_and_ble());
    let _ = engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });

    // Tap the grant affordance the picker now renders.
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "recommended".into(),
        item_id: "grant:glance:camera".into(),
    });

    // The device-wide ledger re-learns the permission as granted (no OS
    // "granted" event exists — ADR-030/031 — so the affordance is the seam).
    assert_eq!(
        engine
            .transport_readiness()
            .permission(DeviceRequirement::Camera),
        PermissionState::Granted
    );
    // ...and the re-rendered picker offers Glance as a selectable mode again.
    let screen = match result {
        ActionResult::UpdateScreen(s) => s,
        other => panic!("grant tap should return UpdateScreen, got {other:?}"),
    };
    let ids = item_ids(&screen);
    assert!(
        ids.iter().any(|id| id == "mode:glance"),
        "granting camera should restore the selectable mode:glance row; got {ids:?}"
    );
    assert!(
        !ids.iter().any(|id| id.starts_with("grant:glance:")),
        "a granted Glance must no longer be a grant affordance; got {ids:?}"
    );
}

// @internal
#[test]
fn record_symptom_camera_and_ble_denied_offers_grant_and_other_mode() {
    // T2.4 — automated proxy for the device-observed symptom (2026-06-11, Pixel
    // 3a): fresh install, Exchange → Glance, deny camera, BLE never granted →
    // "Searching…" forever. With both transports present-but-denied, the picker
    // must instead show a recoverable state: Glance (camera + BLE) becomes a
    // grant affordance, and a transport-independent mode (Link, internet-only)
    // stays selectable as an alternative — never a silent wait. The manual
    // device re-probe of this exact flow remains the acceptance test.
    let mut engine = engine();
    // Camera + BLE + internet present, so the denials are present-but-denied
    // (grant affordance), not hardware-absent (which has no grant path).
    engine.set_device_capabilities(DeviceCapabilities {
        has_camera: true,
        has_ble: true,
        has_internet: true,
        ..Default::default()
    });
    // The record's two simultaneous denials.
    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    });
    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "ble".into(),
    });
    let entry = engine.navigate_to(AppScreen::Exchange);
    assert_eq!(entry.screen_id, "exchange");

    let ids = item_ids(&engine.current_screen());
    // Glance (camera + BLE, both denied) → grant affordance, not a silent mode
    // row. With the M2 S3 hero+disclosure picker the denied hero itself renders
    // as the grant affordance — the recoverable state is front and center.
    assert!(
        ids.iter().any(|id| id.starts_with("grant:glance:")),
        "denied camera+BLE must turn Glance into a grant affordance; got {ids:?}"
    );
    assert!(
        !ids.iter().any(|id| id == "mode:glance"),
        "Glance must not stay a silently-selectable wait; got {ids:?}"
    );
    // A transport-independent fallback (Link, internet-only) stays selectable
    // as the alternative the record requires — one visible disclosure tap away
    // (M2 S3: "Other ways to connect").
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "more".into(),
        item_id: "show_other_modes".into(),
    });
    let ids = item_ids(&engine.current_screen());
    assert!(
        ids.iter().any(|id| id == "mode:link"),
        "an internet-only mode must remain selectable as an alternative; got {ids:?}"
    );
}

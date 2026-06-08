// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Capture-at-exchange seam (ADR-051, Phase 4c slice 3): `AppEngine`
//! emits `Command::LocationRequest` for a just-exchanged contact and
//! consumes the `Event::LocationResult` reply into
//! `Vauchi::set_exchange_location`.
//!
//! The multi-stage finalize path calls `request_exchange_location`; here
//! it is invoked directly (it is a public AppEngine operation) so the
//! emit→consume round-trip is tested without driving a full exchange.
//! Per-frontend native handlers that answer `LocationRequest` are separate.

use vauchi_app::ui::AppEngine;
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event};

fn engine_with_contact() -> (AppEngine, String) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Me").unwrap();
    vauchi
        .import_contacts_from_vcf(b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n")
        .unwrap();
    let cid = vauchi.list_contacts().unwrap()[0].id().to_string();
    (AppEngine::new(vauchi), cid)
}

// @internal
#[test]
fn request_emits_location_command_then_result_records_location() {
    let (mut engine, cid) = engine_with_contact();

    engine.request_exchange_location(cid.clone());

    // A LocationRequest is queued for the frontend.
    let cmds = engine.drain_pending_commands();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::LocationRequest { .. })),
        "request must queue Command::LocationRequest, got {cmds:?}"
    );

    // The frontend's reply records the location on the pending contact.
    let result = engine.handle_hardware_event(Event::LocationResult {
        latitude: 47.37,
        longitude: 8.54,
        accuracy_meters: Some(12.0),
    });
    assert!(result.is_none(), "LocationResult drives no screen change");

    let loc = engine
        .vauchi()
        .exchange_location(&cid)
        .unwrap()
        .expect("exchange location recorded");
    assert!((loc.latitude - 47.37).abs() < 1e-9);
    assert!((loc.longitude - 8.54).abs() < 1e-9);
    assert!(loc.place_id.is_none(), "captured location is unnamed");
}

// @internal
#[test]
fn location_permission_denied_drops_the_pending_capture() {
    let (mut engine, cid) = engine_with_contact();
    engine.request_exchange_location(cid.clone());

    // Peer declined the location permission → pending capture cleared.
    let _ = engine.handle_hardware_event(Event::PermissionDenied {
        transport: "location".into(),
    });
    // A late LocationResult must not resurrect the cleared capture.
    let _ = engine.handle_hardware_event(Event::LocationResult {
        latitude: 1.0,
        longitude: 2.0,
        accuracy_meters: None,
    });

    assert!(
        engine.vauchi().exchange_location(&cid).unwrap().is_none(),
        "denied capture leaves no recorded location"
    );
}

// @internal
#[test]
fn location_result_without_pending_is_a_noop() {
    let (mut engine, cid) = engine_with_contact();
    // No request_exchange_location first.
    let result = engine.handle_hardware_event(Event::LocationResult {
        latitude: 1.0,
        longitude: 2.0,
        accuracy_meters: None,
    });
    assert!(result.is_none());
    assert!(
        engine.vauchi().exchange_location(&cid).unwrap().is_none(),
        "a stray LocationResult records nothing"
    );
}

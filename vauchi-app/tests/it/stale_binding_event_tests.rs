// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A shell reports values against the surface it last rendered, so a
//! decode can arrive on a binding Core has just re-minted: on a Pixel 3a
//! Glance screen a BLE discovery moved the surface from revision 4 to 5
//! while the camera still had decodes in flight for `surface.4.binding.0`
//! (vauchi/private#9, GXP-8c). Such an event is stale, not malformed: Core
//! drops it and logs it. An id Core never minted still fails closed
//! (ADR-066).

#![cfg(feature = "testing")]

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::platform::{
    BindingId, Command, InputValue, PresentationNode, PresentationQrPurpose, SurfaceSpec,
};

fn on_glance_with_camera(name: &str) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity(name).expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        has_ble: true,
        has_camera: true,
        ..Default::default()
    });
    let _ = engine.navigate_to(AppScreen::Exchange);
    let _ = engine.navigate_to(AppScreen::BleExchange {
        mode: ExchangeMode::Glance,
    });
    engine
}

fn rendered(engine: &mut AppEngine) -> SurfaceSpec {
    engine
        .initial_commands()
        .expect("commands")
        .into_iter()
        .rev()
        .find_map(|c| match c {
            Command::ReplaceSurface { surface } => Some(surface),
            _ => None,
        })
        .expect("a surface to render")
}

fn qr(surface: &SurfaceSpec, wanted: PresentationQrPurpose) -> (BindingId, Vec<String>) {
    fn walk(
        nodes: &[PresentationNode],
        wanted: PresentationQrPurpose,
    ) -> Option<(BindingId, Vec<String>)> {
        nodes.iter().find_map(|node| match node {
            PresentationNode::Qr {
                id,
                purpose,
                payloads,
                ..
            } if *purpose == wanted => Some((id.clone(), payloads.clone())),
            PresentationNode::Group { children, .. } => walk(children, wanted),
            _ => None,
        })
    }
    walk(&surface.nodes, wanted).expect("QR node")
}

fn decoded(surface: &SurfaceSpec, binding: &BindingId, frame: &str) -> Event {
    Event::ValueChanged {
        surface_id: surface.surface_id.clone(),
        binding_id: binding.clone(),
        value: InputValue::Text(frame.to_string()),
    }
}

// @scenario: generic_presentation_protocol :: A value on a binding from an older revision is dropped
#[test]
fn a_decode_on_a_binding_re_minted_by_a_ble_discovery_is_dropped() {
    let mut alice = on_glance_with_camera("Alice");
    let mut bob = on_glance_with_camera("Bob");
    let alice_qr = qr(&rendered(&mut alice), PresentationQrPurpose::Display).1[0].clone();
    let before = rendered(&mut bob);
    let (old_binding, _) = qr(&before, PresentationQrPurpose::Capture);
    bob.dispatch(Event::BleDeviceDiscovered {
        id: "stranger".into(),
        rssi: -60,
        adv_data: vec![0xAB, 0xCD],
    })
    .expect("discovery handled");
    let (new_binding, _) = qr(&rendered(&mut bob), PresentationQrPurpose::Capture);
    assert_ne!(
        new_binding, old_binding,
        "the discovery must re-mint the capture binding"
    );

    let out = bob.dispatch(decoded(&before, &old_binding, &alice_qr));

    assert_eq!(
        out.expect("a stale decode is dropped, not an error"),
        Vec::new()
    );
    assert_eq!(
        qr(&rendered(&mut bob), PresentationQrPurpose::Capture).0,
        new_binding,
        "the dropped decode must not move the surface on"
    );
}

// @scenario: generic_presentation_protocol :: A value on a binding Core never minted fails closed
#[test]
fn a_value_on_an_unknown_binding_of_the_current_revision_still_fails() {
    let mut bob = on_glance_with_camera("Bob");
    let surface = rendered(&mut bob);
    let (binding, _) = qr(&surface, PresentationQrPurpose::Capture);
    let unknown = BindingId::new(format!("{}9", binding.as_str())).expect("binding id");

    let out = bob.dispatch(decoded(&surface, &unknown, "frame"));

    assert!(
        out.is_err(),
        "an unminted binding must fail closed, got {out:?}"
    );
}

// @scenario: generic_presentation_protocol :: A value on a binding Core never minted fails closed
#[test]
fn a_value_on_a_binding_from_a_future_revision_still_fails() {
    let mut bob = on_glance_with_camera("Bob");
    let surface = rendered(&mut bob);
    let future = BindingId::new("surface.999999.binding.0").expect("binding id");

    let out = bob.dispatch(decoded(&surface, &future, "frame"));

    assert!(
        out.is_err(),
        "a binding newer than the surface cannot be stale, got {out:?}"
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A shell can still be delivering events for the surface it showed a moment
//! ago when Core has already moved on: the keyboard closing after "Save"
//! reports focus ended on the editor Core just replaced, and a camera decode
//! lands after an exchange finished. Core ignores such an event and logs it.
//! Refusing it turned every such race into a "Something went wrong" alert
//! after an action that had succeeded (vauchi/private#438).

#![cfg(feature = "testing")]

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::platform::{
    BindingId, Command, InputValue, PresentationNode, PresentationQrPurpose, SurfaceSpec,
};

fn on_glance_with_camera() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
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

fn capture_binding(surface: &SurfaceSpec) -> BindingId {
    fn walk(nodes: &[PresentationNode]) -> Option<BindingId> {
        nodes.iter().find_map(|node| match node {
            PresentationNode::Qr { id, purpose, .. }
                if *purpose == PresentationQrPurpose::Capture =>
            {
                Some(id.clone())
            }
            PresentationNode::Group { children, .. } => walk(children),
            _ => None,
        })
    }
    walk(&surface.nodes).expect("capture QR node")
}

/// The Glance surface, rendered, then left for Contacts.
fn left_behind() -> (AppEngine, SurfaceSpec, SurfaceSpec) {
    let mut engine = on_glance_with_camera();
    let before = rendered(&mut engine);
    let _ = engine.navigate_to(AppScreen::Contacts);
    let now = rendered(&mut engine);
    assert_ne!(
        now.surface_id, before.surface_id,
        "navigation must change the surface"
    );
    (engine, before, now)
}

// @internal
#[test]
fn a_late_value_for_a_surface_left_behind_is_ignored() {
    let (mut engine, before, now) = left_behind();

    let out = engine.dispatch(Event::ValueChanged {
        surface_id: before.surface_id.clone(),
        binding_id: capture_binding(&before),
        value: InputValue::Text("a decode still in flight".to_string()),
    });

    assert_eq!(
        out.expect("a late event is ignored, not refused"),
        Vec::new()
    );
    assert_eq!(rendered(&mut engine).surface_id, now.surface_id);
}

// @internal
#[test]
fn a_late_focus_end_for_a_surface_left_behind_is_ignored() {
    let (mut engine, before, now) = left_behind();

    let out = engine.dispatch(Event::InputFocusEnded {
        surface_id: before.surface_id.clone(),
        binding_id: capture_binding(&before),
    });

    assert_eq!(
        out.expect("a late event is ignored, not refused"),
        Vec::new()
    );
    assert_eq!(rendered(&mut engine).surface_id, now.surface_id);
}

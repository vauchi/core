// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A shell can still be delivering events for the surface it showed a moment
//! ago when Core has already moved on: the keyboard closing after "Save"
//! reports focus ended on the editor Core just replaced, and a camera decode
//! lands after an exchange finished. Core ignores such an event and logs it.
//! Refusing it turned every such race into a "Something went wrong" alert
//! after an action that had succeeded (vauchi/private#438).

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::platform::{
    BindingId, Command, InputValue, PresentationNode, PresentationQrPurpose, SurfaceSpec,
};
use vauchi_core::{Event, InputMode, MotionPreference};

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

// @internal
#[test]
fn a_late_activation_of_a_surface_left_behind_is_ignored() {
    let (mut engine, before, now) = left_behind();
    engine
        .dispatch(Event::PresentationEnvironmentChanged {
            available_width: 390,
            available_height: 800,
            input_modes: vec![InputMode::Touch],
            motion: MotionPreference::Full,
        })
        .expect("phone environment");

    let out = engine.dispatch(Event::SurfaceActivated {
        surface_id: before.surface_id.clone(),
    });

    assert_eq!(
        out.expect("a late activation is ignored, not refused"),
        Vec::new()
    );
    assert_eq!(rendered(&mut engine).surface_id, now.surface_id);
}

fn on_hover_with_camera() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        has_camera: true,
        ..Default::default()
    });
    let _ = engine.navigate_to(AppScreen::Exchange);
    let _ = engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Hover,
    });
    engine
}

fn has_capture(surface: &SurfaceSpec) -> bool {
    fn walk(nodes: &[PresentationNode]) -> bool {
        nodes.iter().any(|node| match node {
            PresentationNode::Qr { purpose, .. } => *purpose == PresentationQrPurpose::Capture,
            PresentationNode::Group { children, .. } => walk(children),
            _ => false,
        })
    }
    walk(&surface.nodes)
}

fn replaced_surface(commands: Vec<Command>) -> SurfaceSpec {
    commands
        .into_iter()
        .rev()
        .find_map(|c| match c {
            Command::ReplaceSurface { surface } => Some(surface),
            _ => None,
        })
        .expect("the scan re-renders the surface")
}

/// A peer's FAIL frame for session id `[7; 16]`, as
/// `qr_codec::format_fail_qr` writes it. A literal, because that module is
/// public only under the `testing` feature and these tests must run in the
/// standard suite.
const PEER_FAIL_FRAME: &str = "FAIL:*0:*0:*0:*0:*0:*0:*0:*0";

/// The scan that ends an exchange (here the peer's FAIL frame; on device
/// the frame that finalizes) takes the camera off the surface it arrived
/// on.
fn hover_ended_by_a_scan() -> (AppEngine, SurfaceSpec, SurfaceSpec) {
    let mut engine = on_hover_with_camera();
    let scanning = rendered(&mut engine);
    let ended = replaced_surface(
        engine
            .dispatch(Event::ValueChanged {
                surface_id: scanning.surface_id.clone(),
                binding_id: capture_binding(&scanning),
                value: InputValue::Text(PEER_FAIL_FRAME.to_string()),
            })
            .expect("the peer's frame is accepted"),
    );
    assert!(
        !has_capture(&ended),
        "precondition: the ended exchange has no camera"
    );
    (engine, scanning, ended)
}

/// Shells drop a value whose binding belongs to an older revision. When a
/// scan removed the camera, the surface without it went out under the
/// revision that still had it, so nothing marked the camera's binding as
/// old. Device-observed as `UnknownBinding binding=surface.17.binding.0
/// stale=false engine_rev=17` in 10 of 12 Hover runs (2026-10-02, Pixel 3a,
/// vauchi/private#438).
// @internal
#[test]
fn a_scan_that_removes_the_camera_moves_the_surface_to_a_new_revision() {
    let (_engine, scanning, ended) = hover_ended_by_a_scan();

    assert_eq!(ended.surface_id, scanning.surface_id);
    assert!(
        ended.revision > scanning.revision,
        "the bindings changed, so the revision must: was {}, is {}",
        scanning.revision,
        ended.revision
    );
}

/// The camera delivers a few more decodes after the scan that ended the
/// exchange. Refusing them raised "Something went wrong" over the finished
/// exchange's last QR, which the peer then could not read: one phone saved
/// the contact and the other never finished (2026-10-02, run H14-p640-1).
// @internal
#[test]
fn a_decode_still_in_flight_when_a_scan_ended_the_exchange_is_ignored() {
    let (mut engine, scanning, ended) = hover_ended_by_a_scan();

    let out = engine.dispatch(Event::ValueChanged {
        surface_id: scanning.surface_id.clone(),
        binding_id: capture_binding(&scanning),
        value: InputValue::Text("a decode still in flight".to_string()),
    });

    assert_eq!(
        out.expect("a late decode is ignored, not refused"),
        Vec::new()
    );
    assert_eq!(rendered(&mut engine).revision, ended.revision);
}

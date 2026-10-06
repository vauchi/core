// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! On a short screen the exchange screen leaves out the mode row and the
//! heading over the code, so the camera under the code keeps a usable size
//! (#513: on an iPhone SE the Glance camera was 0 pt tall). Core decides
//! from the height the shell reports (ADR-066); the shell only draws.

use vauchi_app::ui::{AppEngine, AppScreen, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::platform::{
    Command, InputMode, MotionPreference, PresentationNode, PresentationQrPurpose,
    PresentationQrSize, SurfaceSpec,
};

const SHORT_WINDOW_HEIGHT: u32 = 647;
const TALL_WINDOW_HEIGHT: u32 = 760;

fn environment(available_height: u32) -> Event {
    Event::PresentationEnvironmentChanged {
        available_width: 375,
        available_height,
        input_modes: vec![InputMode::Touch],
        motion: MotionPreference::Full,
    }
}

fn engine_on_glance(available_height: u32) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory Vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        has_ble: true,
        has_camera: true,
        ..Default::default()
    });
    engine
        .dispatch(environment(available_height))
        .expect("environment accepted");
    engine.navigate_to(AppScreen::BleExchange {
        mode: ExchangeMode::Glance,
    });
    assert_eq!(engine.current_screen().screen_id, "exchange_ble_glance");
    engine
}

fn replaced_surface(commands: Vec<Command>) -> Option<SurfaceSpec> {
    commands
        .into_iter()
        .rev()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface),
            _ => None,
        })
}

fn presented(engine: &mut AppEngine) -> SurfaceSpec {
    replaced_surface(engine.initial_commands().expect("commands")).expect("a surface")
}

fn shows_mode_row(surface: &SurfaceSpec) -> bool {
    format!("{:?}", surface.nodes).contains("pictogram.exchange.glance")
}

/// The display code's visible heading and its spoken label.
fn own_code_labels(nodes: &[PresentationNode]) -> Option<(Option<String>, String)> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Qr {
            purpose: PresentationQrPurpose::Display,
            label,
            accessibility,
            ..
        } => Some((label.clone(), accessibility.label.clone())),
        PresentationNode::Group { children, .. } => own_code_labels(children),
        _ => None,
    })
}

fn own_code_size(nodes: &[PresentationNode]) -> Option<Option<PresentationQrSize>> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Qr {
            purpose: PresentationQrPurpose::Display,
            size,
            ..
        } => Some(*size),
        PresentationNode::Group { children, .. } => own_code_size(children),
        _ => None,
    })
}

// @internal
#[test]
fn a_tall_screen_keeps_the_mode_row_and_the_heading_over_the_code() {
    let surface = presented(&mut engine_on_glance(TALL_WINDOW_HEIGHT));

    assert!(shows_mode_row(&surface));
    let (heading, spoken) = own_code_labels(&surface.nodes).expect("the own code is shown");
    assert_eq!(heading.as_deref(), Some("Show this to exchange"));
    assert!(!spoken.is_empty());
    assert_eq!(own_code_size(&surface.nodes), Some(None));
    assert!(surface.subtitle.is_some());
}

// @internal
#[test]
fn a_short_screen_leaves_out_the_mode_row_and_the_heading_over_the_code() {
    let surface = presented(&mut engine_on_glance(SHORT_WINDOW_HEIGHT));

    assert!(!shows_mode_row(&surface));
    let (heading, spoken) = own_code_labels(&surface.nodes).expect("the own code is shown");
    assert_eq!(heading, None);
    assert_eq!(spoken, "Show this to exchange");
}

// Even without the mode row and the heading, a 320 pt code leaves an SE's
// camera 6 x 11 pt in its 474 pt surface (rig, 2026-10-06). Glance's code
// is a small bootstrap code read at arm's length, so it can be drawn
// smaller; the subtitle gives way too.
// @internal
#[test]
fn a_short_screen_asks_for_a_compact_code_and_drops_the_subtitle() {
    let surface = presented(&mut engine_on_glance(SHORT_WINDOW_HEIGHT));

    assert_eq!(
        own_code_size(&surface.nodes),
        Some(Some(PresentationQrSize::Compact))
    );
    assert_eq!(surface.subtitle, None);
}

// @internal
#[test]
fn a_screen_that_becomes_short_gets_the_compact_exchange_screen() {
    let mut engine = engine_on_glance(TALL_WINDOW_HEIGHT);
    let _ = presented(&mut engine);

    let commands = engine
        .dispatch(environment(SHORT_WINDOW_HEIGHT))
        .expect("environment accepted");

    let surface = replaced_surface(commands).expect("the exchange screen is sent again");
    assert!(!shows_mode_row(&surface));
}

fn engine_on_hover(available_height: u32) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory Vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine
        .dispatch(environment(available_height))
        .expect("environment accepted");
    engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(vauchi_app::ui::UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:hover".into(),
    });
    assert!(matches!(
        engine.current_app_screen(),
        AppScreen::MultiStageExchange { .. }
    ));
    engine
}

// Hover keeps the standard square even on a short window: at the compact
// square a Samsung S7's front camera, 8 cm away, decoded none of an iPhone
// SE's frames (0 of 5 exchanges), while the standard square completed 3 of
// 3 in the same placement (rig, 2026-10-06, #534).
// @internal
#[test]
fn hover_keeps_the_standard_code_on_a_short_screen() {
    let short = presented(&mut engine_on_hover(SHORT_WINDOW_HEIGHT));
    let tall = presented(&mut engine_on_hover(TALL_WINDOW_HEIGHT));

    assert_eq!(own_code_size(&short.nodes), Some(None));
    assert_eq!(own_code_size(&tall.nodes), Some(None));
}

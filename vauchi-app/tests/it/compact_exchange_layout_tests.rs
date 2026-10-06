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
    Command, InputMode, MotionPreference, PresentationNode, PresentationQrPurpose, SurfaceSpec,
};

const IPHONE_SE_HEIGHT: u32 = 647;
const PIXEL_3A_HEIGHT: u32 = 760;

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

// @internal
#[test]
fn a_tall_screen_keeps_the_mode_row_and_the_heading_over_the_code() {
    let surface = presented(&mut engine_on_glance(PIXEL_3A_HEIGHT));

    assert!(shows_mode_row(&surface));
    let (heading, spoken) = own_code_labels(&surface.nodes).expect("the own code is shown");
    assert_eq!(heading.as_deref(), Some("Show this to exchange"));
    assert!(!spoken.is_empty());
}

// @internal
#[test]
fn a_short_screen_leaves_out_the_mode_row_and_the_heading_over_the_code() {
    let surface = presented(&mut engine_on_glance(IPHONE_SE_HEIGHT));

    assert!(!shows_mode_row(&surface));
    let (heading, spoken) = own_code_labels(&surface.nodes).expect("the own code is shown");
    assert_eq!(heading, None);
    assert_eq!(spoken, "Show this to exchange");
}

// @internal
#[test]
fn a_screen_that_becomes_short_gets_the_compact_exchange_screen() {
    let mut engine = engine_on_glance(PIXEL_3A_HEIGHT);
    let _ = presented(&mut engine);

    let commands = engine
        .dispatch(environment(IPHONE_SE_HEIGHT))
        .expect("environment accepted");

    let surface = replaced_surface(commands).expect("the exchange screen is sent again");
    assert!(!shows_mode_row(&surface));
}

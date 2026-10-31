// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(feature = "testing")]

//! The camera belongs to the hardware shell (ADR-077). Once one is attached,
//! a camera request becomes a capture for the hardware shell and a viewport
//! for the display shell, joined by one opaque preview id; Core stops the
//! capture when the person leaves the screen that started it.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::clock::FakeClock;
use vauchi_core::{CameraPurpose, Command, CommandTarget, Event, Vauchi, event_from_json};

fn engine() -> AppEngine {
    let clock = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let mut vauchi = Vauchi::in_memory_with_clock(clock).unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);
    engine
}

fn attached() -> AppEngine {
    let mut engine = engine();
    engine
        .dispatch(
            event_from_json(
                r#"{"HardwareCapabilitiesReported":{"capabilities":{"has_camera":true}}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    engine
}

/// Shows the current screen with `camera` requested, as a scan button does.
fn request(engine: &mut AppEngine, camera: Command) -> (Vec<Command>, Vec<Command>) {
    engine.extend_pending_commands([camera]);
    let display = engine.initial_commands().unwrap();
    (display, engine.drain_hardware_commands())
}

fn viewport(display: &[Command]) -> Option<String> {
    display.iter().find_map(|command| match command {
        Command::CameraViewport { preview_id } => Some(preview_id.clone()),
        _ => None,
    })
}

fn capture(hardware: &[Command]) -> Option<(String, CameraPurpose)> {
    hardware.iter().find_map(|command| match command {
        Command::CameraCapture {
            preview_id,
            purpose,
        } => Some((preview_id.clone(), *purpose)),
        _ => None,
    })
}

fn stops(hardware: &[Command]) -> Vec<String> {
    hardware
        .iter()
        .filter_map(|command| match command {
            Command::CameraStop { preview_id } => Some(preview_id.clone()),
            _ => None,
        })
        .collect()
}

// @internal — today's shells keep receiving the camera request they know.
#[test]
fn without_a_hardware_shell_the_scan_request_is_unchanged() {
    let mut engine = engine();

    let (display, hardware) = request(&mut engine, Command::QrRequestScan);

    assert!(display.contains(&Command::QrRequestScan));
    assert_eq!(viewport(&display), None);
    assert_eq!(hardware, Vec::new());
}

// @internal — one preview id joins the display viewport and the hardware capture.
#[test]
fn a_scan_becomes_a_viewport_and_a_capture_with_one_preview_id() {
    let mut engine = attached();

    let (display, hardware) = request(&mut engine, Command::QrRequestScan);

    let id = viewport(&display).expect("a viewport for the display shell");
    assert_eq!(capture(&hardware), Some((id, CameraPurpose::QrScan)));
    assert!(!display.contains(&Command::QrRequestScan));
    assert!(!hardware.contains(&Command::QrRequestScan));
    assert!(display.iter().all(|c| c.target() == CommandTarget::Display));
    assert!(
        hardware
            .iter()
            .all(|c| c.target() == CommandTarget::Hardware)
    );
}

// @internal — privacy: the camera goes off when the person leaves the screen.
#[test]
fn leaving_the_screen_stops_the_capture() {
    let mut engine = attached();
    let (display, _) = request(&mut engine, Command::QrRequestScan);
    let id = viewport(&display).unwrap();

    engine.initial_commands().unwrap();
    let same_screen = engine.drain_hardware_commands();
    engine.navigate_to(AppScreen::Settings);
    engine.initial_commands().unwrap();
    let left = engine.drain_hardware_commands();

    assert_eq!(stops(&same_screen), Vec::<String>::new());
    assert_eq!(stops(&left), vec![id]);
}

// @internal — a photo is one shot: its answer stops the capture.
#[test]
fn a_photo_capture_stops_once_answered() {
    let mut engine = attached();
    let (display, hardware) = request(&mut engine, Command::ImageCaptureFromCamera);
    let id = viewport(&display).unwrap();
    assert_eq!(capture(&hardware), Some((id.clone(), CameraPurpose::Photo)));

    engine.dispatch(Event::ImagePickCancelled).unwrap();

    assert_eq!(stops(&engine.drain_hardware_commands()), vec![id]);
}

// @internal — a second request replaces the running capture.
#[test]
fn a_new_capture_stops_the_running_one_first() {
    let mut engine = attached();
    let (first, _) = request(&mut engine, Command::QrRequestScan);
    let first = viewport(&first).unwrap();

    let (second, hardware) = request(&mut engine, Command::QrRequestScan);

    let second = viewport(&second).unwrap();
    assert_ne!(first, second);
    assert_eq!(stops(&hardware), vec![first]);
}

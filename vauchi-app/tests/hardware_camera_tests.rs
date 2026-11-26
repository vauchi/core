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
use vauchi_core::exchange::mode::ExchangeMode;
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

fn attached_without_a_camera() -> AppEngine {
    let mut engine = engine();
    engine
        .dispatch(
            event_from_json(
                r#"{"HardwareCapabilitiesReported":{"capabilities":{"has_camera":false}}}"#,
            )
            .unwrap(),
        )
        .unwrap();
    engine
}

fn toasts(display: &[Command]) -> Vec<String> {
    display
        .iter()
        .filter_map(|command| match command {
            Command::ShowToast { toast } => Some(toast.message.clone()),
            _ => None,
        })
        .collect()
}

// @internal — private#1070: the hardware shell has no camera port to run a
// capture on, so Core starts none and says so, instead of waiting on a scan
// that never begins.
#[test]
fn without_a_camera_a_scan_starts_no_capture_and_says_why() {
    let mut engine = attached_without_a_camera();

    let (display, hardware) = request(&mut engine, Command::QrRequestScan);

    assert_eq!(viewport(&display), None);
    assert_eq!(capture(&hardware), None);
    assert!(!display.contains(&Command::QrRequestScan));
    assert_eq!(toasts(&display), ["Camera is not available on this device"]);
}

// @internal — private#1070, the same for a photo.
#[test]
fn without_a_camera_a_photo_starts_no_capture_and_says_why() {
    let mut engine = attached_without_a_camera();

    let (display, hardware) = request(&mut engine, Command::ImageCaptureFromCamera);

    assert_eq!(viewport(&display), None);
    assert_eq!(capture(&hardware), None);
    assert_eq!(toasts(&display), ["Camera is not available on this device"]);
}

/// Shows Glance, whose screen holds a QR code to capture.
fn show_glance(engine: &mut AppEngine) -> (Vec<Command>, Vec<Command>) {
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let display = engine.initial_commands().unwrap();
    (display, engine.drain_hardware_commands())
}

// @internal — private#772: a shell that runs its camera only when Core asks
// (ADR-077) gets the camera for a screen that offers to scan, as the
// phones' display shells open theirs for it.
#[test]
fn a_shown_scan_screen_starts_a_qr_capture() {
    let mut engine = attached();

    let (display, hardware) = show_glance(&mut engine);

    let id = viewport(&display).expect("a viewport for the display shell");
    assert_eq!(capture(&hardware), Some((id, CameraPurpose::QrScan)));
}

// @internal — phones whose display shells open the camera themselves are untouched.
#[test]
fn without_a_hardware_shell_a_scan_screen_starts_no_capture() {
    let mut engine = engine();

    let (display, hardware) = show_glance(&mut engine);

    assert_eq!(viewport(&display), None);
    assert!(!display.contains(&Command::QrRequestScan));
    assert_eq!(capture(&hardware), None);
}

// @internal — a screen that only offers to scan does not announce, each
// time it is shown, a camera the device does not have; a scan the person
// asks for still says so (private#1070).
#[test]
fn without_a_camera_a_scan_screen_starts_none_and_says_nothing() {
    let mut engine = attached_without_a_camera();

    let (display, hardware) = show_glance(&mut engine);

    assert_eq!(viewport(&display), None);
    assert_eq!(capture(&hardware), None);
    assert_eq!(toasts(&display), Vec::<String>::new());
}

// @internal — the scan screen redrawn keeps the camera it has.
#[test]
fn showing_the_scan_screen_again_keeps_its_capture() {
    let mut engine = attached();
    show_glance(&mut engine);

    engine.initial_commands().unwrap();
    let again = engine.drain_hardware_commands();

    assert_eq!(capture(&again), None);
    assert_eq!(stops(&again), Vec::<String>::new());
}

// @internal — the hardware shell releases the camera once it reads a code
// (windows-hw QrCamera, linux-hw camera.rs): the next showing of a scan
// screen starts a fresh capture instead of counting on the ended one.
#[test]
fn a_scanned_code_ends_the_capture() {
    let mut engine = attached();
    let (display, _) = show_glance(&mut engine);
    let first = viewport(&display).unwrap();

    let redrawn = engine
        .dispatch(event_from_json(r#"{"QrScanned":{"data":"not a vauchi code"}}"#).unwrap())
        .unwrap();
    let hardware = engine.drain_hardware_commands();

    let second = viewport(&redrawn).expect("a fresh viewport with the redrawn screen");
    assert_ne!(first, second);
    assert_eq!(capture(&hardware), Some((second, CameraPurpose::QrScan)));
    assert_eq!(
        stops(&hardware),
        Vec::<String>::new(),
        "the ended capture needs no stop"
    );
}

// @internal — privacy: leaving the scan screen turns the camera off.
#[test]
fn leaving_the_scan_screen_stops_its_capture() {
    let mut engine = attached();
    let (display, _) = show_glance(&mut engine);
    let id = viewport(&display).unwrap();

    engine.navigate_to(AppScreen::Settings);
    engine.initial_commands().unwrap();

    assert_eq!(stops(&engine.drain_hardware_commands()), vec![id]);
}

fn attached_with(capabilities: &str) -> AppEngine {
    let mut engine = engine();
    engine
        .dispatch(
            event_from_json(&format!(
                r#"{{"HardwareCapabilitiesReported":{{"capabilities":{capabilities}}}}}"#
            ))
            .unwrap(),
        )
        .unwrap();
    engine
}

/// Every command entering Glance sends, wherever Core routes it, as JSON.
fn entering_glance(engine: &mut AppEngine) -> (Vec<Command>, String) {
    engine.navigate_to(AppScreen::MultiStageExchange {
        mode: ExchangeMode::Glance,
    });
    let mut sent = engine.drain_pending_commands();
    sent.extend(engine.initial_commands().unwrap());
    sent.extend(engine.drain_hardware_commands());
    let json = serde_json::to_string(&sent).unwrap();
    (sent, json)
}

fn switches(sent: &[Command]) -> usize {
    sent.iter()
        .filter(|command| matches!(command, Command::SwitchCamera { .. }))
        .count()
}

const SWITCH_LABEL: &str = "Use Front Camera";

// @internal — native capability counts must not suppress Hover's initial front selector.
#[test]
fn a_single_camera_hover_still_prepares_its_front_selector_without_a_switch_button() {
    for mode in [ExchangeMode::Hover, ExchangeMode::TapHoverShake] {
        let mut engine = attached_with(r#"{"has_camera":true,"camera_count":1}"#);
        engine.navigate_to(AppScreen::MultiStageExchange { mode });
        let display = engine.initial_commands().unwrap();
        let hardware = engine.drain_hardware_commands();
        let selector = hardware
            .iter()
            .position(|command| matches!(command, Command::SwitchCamera { use_front: true }))
            .expect("Hover must prepare front selection before capture");
        if let Some(capture) = hardware
            .iter()
            .position(|command| matches!(command, Command::CameraCapture { .. }))
        {
            assert!(selector < capture, "select before acquiring the camera");
        }
        assert_eq!(switches(&hardware), 1);
        let json = serde_json::to_string(&display).unwrap();
        assert!(
            !json.contains("Use Back Camera"),
            "no camera switch: {json}"
        );
    }
}

// @internal — private#772: on a laptop with one camera there is nothing to
// switch to (the maintainer, Windows rig, 2026-11-23).
#[test]
fn a_device_with_one_camera_is_offered_no_switch() {
    let mut engine = attached_with(r#"{"has_camera":true,"camera_count":1}"#);

    let (sent, json) = entering_glance(&mut engine);

    assert_eq!(switches(&sent), 1, "one initial selection: {json}");
    assert!(sent.contains(&Command::SwitchCamera { use_front: false }));
    assert!(!json.contains(SWITCH_LABEL), "no switch button: {json}");
}

// @internal — a device that counted two cameras keeps the switch.
#[test]
fn a_device_with_two_cameras_keeps_the_switch() {
    let mut engine = attached_with(r#"{"has_camera":true,"camera_count":2}"#);

    let (sent, json) = entering_glance(&mut engine);

    assert_eq!(switches(&sent), 1, "{json}");
    assert!(json.contains(SWITCH_LABEL), "a switch button: {json}");
}

// @internal — a shell that does not count its cameras is offered what it was before.
#[test]
fn a_device_that_does_not_count_keeps_the_switch() {
    let mut engine = attached_with(r#"{"has_camera":true}"#);

    let (sent, json) = entering_glance(&mut engine);

    assert_eq!(switches(&sent), 1, "{json}");
    assert!(json.contains(SWITCH_LABEL), "a switch button: {json}");
}

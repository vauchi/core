// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A scan button asks for `Command::QrRequestScan`. With a hardware shell
//! attached (Linux), that command reaches the hardware camera. Without one
//! (the Android and iOS apps today), it reached display shells that have no
//! handler for it, and the button did nothing. Core then shows the
//! exchange's scanner in the screen instead, and a scanned value takes the
//! path a hardware scan takes (vauchi/private#1365, #1382; owner, 2026-11-24).

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, QrMode, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::DeviceCapabilities;
use vauchi_core::exchange::{DeviceLinkJoinInvitation, DeviceLinkQR};
use vauchi_core::{Command, Identity, event_from_json};

const SCANNER: &str = "in_screen_scanner";
const LEAVE: &str = "leave_scanner";

fn alices_invitation() -> String {
    let alice = Identity::create("Alice", 0);
    DeviceLinkJoinInvitation {
        qr_data: DeviceLinkQR::generate_with_timestamp(&alice, 1_700_000_000).to_data_string(),
        broker_code: "123456".into(),
        relay_url: None,
        local_rendezvous: Some("192.168.1.20:47101".into()),
    }
    .to_url()
}

/// A phone app: it reports a camera to Core but attaches no hardware shell.
fn phone(vauchi: Vauchi) -> AppEngine {
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        has_camera: true,
        ..Default::default()
    });
    engine
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn scan_in_screen(engine: &mut AppEngine, value: &str) -> ActionResult {
    engine.handle_action(UserAction::TextChanged {
        component_id: SCANNER.into(),
        value: value.into(),
    })
}

fn shows_the_scanner(engine: &AppEngine) -> bool {
    engine.current_screen().components.iter().any(|component| {
        matches!(component, Component::QrCode { id, mode: QrMode::Scan, .. } if id == SCANNER)
    })
}

fn asks_for_a_scan(result: &ActionResult) -> bool {
    matches!(result, ActionResult::Commands { commands } if commands.contains(&Command::QrRequestScan))
}

fn on_the_join_instructions(mut engine: AppEngine) -> AppEngine {
    engine.navigate_to(AppScreen::Onboarding);
    let _ = press(&mut engine, "link_device");
    assert_eq!(
        engine.current_screen().screen_id,
        "device_link_instructions"
    );
    engine
}

// @scenario: device_management :: Link new device via QR code
#[test]
fn on_a_phone_scan_qr_code_shows_the_scanner_and_a_scanned_invitation_opens_the_join() {
    let mut engine = on_the_join_instructions(phone(Vauchi::in_memory().unwrap()));

    let result = press(&mut engine, "scan_qr");

    assert!(
        !asks_for_a_scan(&result),
        "no shell handles the scan: {result:?}"
    );
    assert!(
        shows_the_scanner(&engine),
        "the scanner should be in the screen"
    );
    let ActionResult::NavigateTo(screen) = scan_in_screen(&mut engine, &alices_invitation()) else {
        panic!("a scanned invitation should open the join screen");
    };
    assert_eq!(screen.screen_id, "device_link_join");
}

// @internal — like a hardware camera, the scanner ignores a code it has no
// use for and keeps looking.
#[test]
fn on_a_phone_a_code_that_is_no_invitation_leaves_the_scanner_open() {
    let mut engine = on_the_join_instructions(phone(Vauchi::in_memory().unwrap()));
    let _ = press(&mut engine, "scan_qr");

    let _ = scan_in_screen(&mut engine, "https://example.org/not-an-invitation");

    assert_eq!(
        engine.current_screen().screen_id,
        "device_link_instructions"
    );
    assert!(shows_the_scanner(&engine));
}

// @internal
#[test]
fn leaving_the_scanner_returns_to_the_screen_without_it() {
    let mut engine = on_the_join_instructions(phone(Vauchi::in_memory().unwrap()));
    let _ = press(&mut engine, "scan_qr");
    assert!(shows_the_scanner(&engine));

    let _ = press(&mut engine, LEAVE);

    assert_eq!(
        engine.current_screen().screen_id,
        "device_link_instructions"
    );
    assert!(!shows_the_scanner(&engine));
}

// @internal — Linux keeps its hardware camera.
#[test]
fn with_a_hardware_shell_scan_qr_code_still_asks_it_for_the_scan() {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    let report = r#"{"HardwareCapabilitiesReported":{"capabilities":{"has_camera":true}}}"#;
    engine.dispatch(event_from_json(report).unwrap()).unwrap();
    let mut engine = on_the_join_instructions(engine);

    let result = press(&mut engine, "scan_qr");

    assert!(
        asks_for_a_scan(&result),
        "the hardware shell should get the scan: {result:?}"
    );
    assert!(!shows_the_scanner(&engine));
}

// @internal — a guardian vouching scans the recovering phone's claim.
#[test]
fn on_a_phone_the_guardians_scan_shows_the_scanner_and_takes_the_claim() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Bob").unwrap();
    let mut engine = phone(vauchi);
    engine.navigate_to(AppScreen::RecoveryHelp);
    let _ = press(&mut engine, "vouch");

    let result = press(&mut engine, "scan_claim");

    assert!(
        !asks_for_a_scan(&result),
        "no shell handles the scan: {result:?}"
    );
    assert!(shows_the_scanner(&engine));
    let _ = scan_in_screen(&mut engine, "not-a-claim");
    let claim_field = engine
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::TextInput { id, value, .. } if id == "claim_data" => Some(value),
            _ => None,
        });
    assert_eq!(claim_field.as_deref(), Some("not-a-claim"));
}

// @internal — the recovering phone scans each guardian's share.
#[test]
fn on_a_phone_the_recovering_phones_scan_shows_the_scanner_and_reads_the_share() {
    let alice = Identity::create("Alice", 0);
    let recovering = Vauchi::in_memory().unwrap();
    recovering
        .begin_recovery(&hex::encode(alice.signing_public_key()))
        .unwrap();
    let mut engine = phone(recovering);
    let _ = engine.initial_commands().unwrap();
    assert_eq!(engine.current_app_screen(), &AppScreen::Recovery);

    let result = press(&mut engine, "scan_share");

    assert!(
        !asks_for_a_scan(&result),
        "no shell handles the scan: {result:?}"
    );
    assert!(shows_the_scanner(&engine));
    let result = scan_in_screen(&mut engine, "not-a-share");
    assert!(
        matches!(result, ActionResult::ShowAlert { .. }),
        "a code that is no share should be refused, got {result:?}"
    );
}

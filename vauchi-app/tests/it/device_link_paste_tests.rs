// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device without a camera cannot scan the invitation of the device it
//! joins. Core then offers a field to paste it into, and the pasted text
//! takes the path a scan takes (vauchi/private#1233).

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ScreenModel, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::{DeviceLinkJoinInvitation, DeviceLinkQR};
use vauchi_core::{Identity, MAX_EVENT_INPUT_VALUE_BYTES, event_from_json};

const PASTE_FIELD: &str = "device_link_invitation";
const OPEN_PASTED: &str = "open_pasted_invitation";

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

fn fresh_device() -> AppEngine {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    engine.navigate_to(AppScreen::Onboarding);
    engine
}

fn report_hardware(engine: &mut AppEngine, has_camera: bool) {
    let report = format!(
        r#"{{"HardwareCapabilitiesReported":{{"capabilities":{{"has_camera":{has_camera}}}}}}}"#
    );
    engine.dispatch(event_from_json(&report).unwrap()).unwrap();
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn paste(engine: &mut AppEngine, text: &str) -> ActionResult {
    engine.handle_action(UserAction::TextChanged {
        component_id: PASTE_FIELD.into(),
        value: text.into(),
    })
}

fn link_this_device(engine: &mut AppEngine) -> ScreenModel {
    let ActionResult::NavigateTo(screen) = press(engine, "link_device") else {
        panic!("Link this device should open its instructions");
    };
    assert_eq!(screen.screen_id, "device_link_instructions");
    screen
}

fn paste_placeholder(screen: &ScreenModel) -> Option<String> {
    screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::TextInput {
                id, placeholder, ..
            } if id == PASTE_FIELD => placeholder.clone(),
            _ => None,
        })
}

fn actions(screen: &ScreenModel) -> Vec<(&str, bool)> {
    screen
        .contextual_actions
        .iter()
        .map(|action| (action.id.as_str(), action.enabled))
        .collect()
}

// @scenario: device_management :: Link new device via QR code
#[test]
fn without_a_camera_the_join_screen_takes_a_pasted_invitation() {
    let mut engine = fresh_device();
    report_hardware(&mut engine, false);

    let screen = link_this_device(&mut engine);
    assert_eq!(
        paste_placeholder(&screen).as_deref(),
        Some("Paste QR data here")
    );
    assert_eq!(actions(&screen), [(OPEN_PASTED, false), ("back", true)]);

    let invitation = alices_invitation();
    assert!(
        invitation.len() <= MAX_EVENT_INPUT_VALUE_BYTES,
        "a shell could not send an invitation of {} bytes as one input value",
        invitation.len()
    );
    let ActionResult::UpdateScreen(screen) = paste(&mut engine, &format!(" {invitation}\n")) else {
        panic!("typing into the field should redraw the screen");
    };
    assert_eq!(actions(&screen), [(OPEN_PASTED, true), ("back", true)]);

    let ActionResult::NavigateTo(screen) = press(&mut engine, OPEN_PASTED) else {
        panic!("a pasted invitation should open the join screen");
    };
    assert_eq!(screen.screen_id, "device_link_join");
}

// @internal — a wrong paste says so and loses nothing.
#[test]
fn pasted_text_that_is_no_invitation_is_refused_and_the_screen_stays() {
    let mut engine = fresh_device();
    report_hardware(&mut engine, false);
    link_this_device(&mut engine);
    assert!(matches!(
        paste(&mut engine, "https://example.org/not-an-invitation"),
        ActionResult::UpdateScreen(_)
    ));

    let ActionResult::ShowAlert { title, .. } = press(&mut engine, OPEN_PASTED) else {
        panic!("text that is no invitation should be refused with an alert");
    };

    assert_eq!(title, "Invalid Device Link");
    assert_eq!(
        engine.current_screen().screen_id,
        "device_link_instructions"
    );
}

// @internal — the scan button stays where a camera, or no report of one, exists.
#[test]
fn with_a_camera_or_no_hardware_report_the_join_screen_offers_the_scan() {
    let mut with_camera = fresh_device();
    report_hardware(&mut with_camera, true);
    let mut unreported = fresh_device();

    for engine in [&mut with_camera, &mut unreported] {
        let screen = link_this_device(engine);
        assert_eq!(paste_placeholder(&screen), None);
        assert_eq!(actions(&screen), [("scan_qr", true), ("back", true)]);
    }
}

// @internal — a hardware shell may attach while the screen is already open.
#[test]
fn a_report_of_no_camera_reaches_the_join_screen_already_open() {
    let mut engine = fresh_device();
    link_this_device(&mut engine);

    report_hardware(&mut engine, false);

    let screen = engine.current_screen();
    assert_eq!(
        paste_placeholder(&screen).as_deref(),
        Some("Paste QR data here")
    );
    assert_eq!(actions(&screen), [(OPEN_PASTED, false), ("back", true)]);
}

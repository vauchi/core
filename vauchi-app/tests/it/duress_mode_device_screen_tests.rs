// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! In duress mode the Devices screen shows only this device, and linking a
//! new one fails with the ordinary failure copy instead of producing a code
//! that would hand the identity to the coercer's phone (#469; owner
//! decision 2026-10-02).

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::identity::DeviceInfo;

const DURESS_PIN: &str = "135790";
const OTHER_SEED: [u8; 32] = [9u8; 32];

fn engine_in_duress_mode() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut registry = vauchi.identity().unwrap().initial_device_registry();
    registry
        .add_device_unsigned(
            DeviceInfo::derive(&OTHER_SEED, 1, "RealLaptop".into(), 0).to_registered(&OTHER_SEED),
        )
        .unwrap();
    vauchi
        .storage()
        .device()
        .save_device_registry(&registry)
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine
        .vauchi_mut()
        .setup_app_password("app-password-123")
        .unwrap();
    engine
        .vauchi_mut()
        .setup_duress_password(DURESS_PIN)
        .unwrap();
    let _ = engine.vauchi_mut().authenticate(DURESS_PIN).unwrap();
    engine
}

// @scenario: duress_mode :: Cannot access real contacts from duress mode
// @internal
#[test]
fn duress_mode_devices_screen_names_no_real_device() {
    let mut engine = engine_in_duress_mode();
    let screen = engine.navigate_to(AppScreen::DeviceManagement);
    let shown = format!("{:?}", screen.components);
    assert!(!shown.contains("RealLaptop"), "{shown}");
}

// The PIN step accepts the duress PIN (telling a coercer their own PIN is
// wrong would give the duress away); the link then fails like any other.
// @scenario: duress_mode :: Cannot access real contacts from duress mode
// @internal
// The link session exists only with these features (`device_link.rs`).
#[cfg(all(feature = "network-http", feature = "storage"))]
#[test]
fn duress_mode_device_link_fails_like_any_failed_link() {
    use vauchi_app::i18n::{Locale, get_string};
    use vauchi_app::ui::{UserAction, WorkflowEngine};

    let mut engine = engine_in_duress_mode();
    let screen = engine.navigate_to(AppScreen::DeviceLinking);
    assert_eq!(screen.screen_id, "link_confirm_pin");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: DURESS_PIN.into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_pin".into(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "link_failed");
    let generic = get_string(Locale::English, "device_link.failure_generic");
    assert!(
        format!("{:?}", screen.components).contains(&generic),
        "the ordinary failure copy, not a duress-specific one: {:?}",
        screen.components
    );
}

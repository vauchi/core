// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Under the biometric branch the lock screen's PIN counts only after a
//! biometric pass (ADR-032, amended 2026-11-10; vauchi/private#1117 task
//! 10 step 3). The lock screen looks the same on every install with
//! biometric unlock, and a PIN typed before the pass leaves it exactly as
//! a wrong PIN does.

use vauchi_app::ui::{AppEngine, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::{AuthMode, Vauchi};
use vauchi_core::exchange::capability::types::DeviceCapabilities;

const APP_PIN: &str = "2468";
const DURESS_PIN: &str = "654321";

fn locked_biometric_branch() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.vauchi_mut().setup_app_password(APP_PIN).unwrap();
    engine
        .vauchi_mut()
        .setup_duress_pin_with_biometric_unlock(DURESS_PIN)
        .unwrap();
    engine.set_device_capabilities(DeviceCapabilities {
        has_biometrics: true,
        ..DeviceCapabilities::default()
    });
    engine.bootstrap();
    assert_eq!(engine.current_screen().screen_id, "lock_screen");
    engine
}

fn unlock_with(engine: &mut AppEngine, pin: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: pin.into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });
}

fn lock_screen_after(pin: &str) -> String {
    let mut engine = locked_biometric_branch();
    unlock_with(&mut engine, pin);
    assert_eq!(
        engine.vauchi().auth_mode(),
        AuthMode::Unauthenticated,
        "{pin} before the pass must not unlock"
    );
    format!("{:?}", engine.current_screen())
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn a_pin_before_the_biometric_pass_leaves_the_screen_a_wrong_pin_leaves() {
    let wrong = lock_screen_after("1111");

    assert_eq!(lock_screen_after(APP_PIN), wrong);
    assert_eq!(lock_screen_after(DURESS_PIN), wrong);
}

// @scenario: duress_mode :: Credentials processed identically at UI level
#[test]
fn after_the_biometric_pass_the_pin_picks_the_profile() {
    for (pin, mode) in [(APP_PIN, AuthMode::Normal), (DURESS_PIN, AuthMode::Duress)] {
        let mut engine = locked_biometric_branch();
        let _ = engine.dispatch(Event::BiometricUnlockSucceeded).unwrap();

        unlock_with(&mut engine, pin);

        assert_eq!(engine.vauchi().auth_mode(), mode, "{pin}");
    }
}

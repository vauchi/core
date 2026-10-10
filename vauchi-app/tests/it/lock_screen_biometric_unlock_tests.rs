// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A successful biometric prompt on Core's lock screen is acted on by Core
//! (vauchi/private#591): without a duress PIN the app opens; with one, the
//! lock screen stays and asks for the password, so either PIN works on
//! every shell without a native app-password screen.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::{AuthMode, Vauchi};
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::{Command, Event};

const PASSWORD: &str = "app-password-123";
const DURESS_PIN: &str = "654321";

fn locked_engine(duress: bool) -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.vauchi_mut().setup_app_password(PASSWORD).unwrap();
    if duress {
        engine
            .vauchi_mut()
            .setup_duress_password(DURESS_PIN)
            .unwrap();
    }
    engine.set_device_capabilities(DeviceCapabilities {
        has_biometrics: true,
        ..DeviceCapabilities::default()
    });
    engine.bootstrap();
    assert_eq!(engine.current_screen().screen_id, "lock_screen");
    engine
}

fn replaced_surface_id(commands: &[Command]) -> Option<String> {
    commands.iter().find_map(|command| match command {
        Command::ReplaceSurface { surface } => Some(surface.surface_id.as_str().to_owned()),
        _ => None,
    })
}

fn offers_biometric_unlock(engine: &AppEngine) -> bool {
    engine
        .current_screen()
        .contextual_actions
        .iter()
        .any(|action| action.id == "unlock_biometric")
}

fn unlock_with(engine: &mut AppEngine, password: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: password.into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });
}

// @scenario: duress_mode.feature :: Biometric unlock without duress
#[test]
fn without_duress_a_biometric_unlock_opens_the_app() {
    let mut engine = locked_engine(false);
    let default_screen = engine.default_screen().screen_id();

    let commands = engine.dispatch(Event::BiometricUnlockSucceeded).unwrap();

    assert_eq!(engine.vauchi().auth_mode(), AuthMode::Normal);
    assert_ne!(engine.current_screen().screen_id, "lock_screen");
    assert_eq!(
        replaced_surface_id(&commands).as_deref(),
        Some(default_screen),
        "the shell must be handed the unlocked surface in the same batch",
    );
}

// @scenario: duress_mode.feature :: Biometric unlock with duress
#[test]
fn with_duress_a_biometric_unlock_asks_for_the_password_without_biometrics() {
    let mut engine = locked_engine(true);
    assert!(offers_biometric_unlock(&engine));

    let commands = engine.dispatch(Event::BiometricUnlockSucceeded).unwrap();

    assert_eq!(engine.vauchi().auth_mode(), AuthMode::Unauthenticated);
    assert_eq!(engine.current_screen().screen_id, "lock_screen");
    assert!(
        !offers_biometric_unlock(&engine),
        "biometrics already passed; offering them again loops"
    );
    assert_eq!(
        replaced_surface_id(&commands).as_deref(),
        Some(AppScreen::Lock.screen_id()),
    );
}

// @scenario: duress_mode.feature :: Biometric unlock with duress
#[test]
fn after_the_biometric_unlock_the_duress_pin_opens_the_same_screen_as_the_password() {
    let mut normal = locked_engine(true);
    let _ = normal.dispatch(Event::BiometricUnlockSucceeded).unwrap();
    unlock_with(&mut normal, PASSWORD);

    let mut duress = locked_engine(true);
    let _ = duress.dispatch(Event::BiometricUnlockSucceeded).unwrap();
    unlock_with(&mut duress, DURESS_PIN);

    assert_eq!(normal.vauchi().auth_mode(), AuthMode::Normal);
    assert_eq!(duress.vauchi().auth_mode(), AuthMode::Duress);
    assert_ne!(normal.current_screen().screen_id, "lock_screen");
    assert_eq!(
        duress.current_screen().screen_id,
        normal.current_screen().screen_id,
        "the duress unlock must look like a normal one (ADR-032)",
    );
}

// @scenario: duress_mode.feature :: Biometric unlock with duress
#[test]
fn the_offered_biometric_action_asks_the_shell_for_the_prompt() {
    let mut engine = locked_engine(true);

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock_biometric".into(),
    });

    assert!(
        format!("{result:?}").contains("RequestBiometricUnlock"),
        "the biometric action did not request the prompt: {result:?}",
    );
}

// @internal
#[test]
fn the_biometric_pass_is_logged_by_its_variant_name() {
    assert_eq!(
        vauchi_app::ui::EngineUpdate::BiometricUnlockPassed.name(),
        "BiometricUnlockPassed"
    );
}

// @scenario: duress_mode.feature :: Biometric unlock with duress
#[test]
fn a_forged_biometric_press_after_the_unlock_stays_inert() {
    let mut engine = locked_engine(true);
    let _ = engine.dispatch(Event::BiometricUnlockSucceeded).unwrap();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock_biometric".into(),
    });

    assert!(
        !format!("{result:?}").contains("RequestBiometricUnlock"),
        "the withdrawn biometric action still asked the shell for a prompt: {result:?}",
    );
    assert_eq!(engine.current_screen().screen_id, "lock_screen");
}

// @scenario: duress_mode.feature :: Biometric unlock without duress
#[test]
fn off_the_lock_screen_a_biometric_unlock_does_not_navigate() {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.bootstrap();
    let before = engine.current_screen().screen_id;

    let commands = engine.dispatch(Event::BiometricUnlockSucceeded).unwrap();

    assert_eq!(engine.current_screen().screen_id, before);
    assert_eq!(replaced_surface_id(&commands), None);
}

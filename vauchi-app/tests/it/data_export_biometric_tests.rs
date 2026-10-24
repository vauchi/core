// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! On a device with biometrics the secret is sealed behind biometry alone,
//! export needs an app password, and a re-enrolled finger never reaches
//! the first-password fallback, so a device password alone passes nothing
//! there (vauchi/private#778, owner decisions 2026-10-24). Devices without
//! biometrics keep the device credential (#712 D4).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::{BiometricType, DeviceCapabilities};
use vauchi_core::{AuthMode, Command, Event, UserPresenceBinding};

const SECRET: [u8; 32] = [42; 32];
const CHOSEN_PIN: &str = "chosen-pin-4821";
const APP_PIN: &str = "app-password-123";
const DURESS_PIN: &str = "913724";

fn privacy_screen(biometric_type: Option<BiometricType>) -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        can_seal_user_presence: true,
        biometric_type,
        ..Default::default()
    });
    engine.navigate_to(AppScreen::Privacy);
    let _ = engine.drain_pending_commands();
    engine
}

fn biometric_device() -> AppEngine {
    privacy_screen(Some(BiometricType::Fingerprint))
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn screen_id(engine: &AppEngine) -> String {
    engine.current_screen().screen_id
}

fn presence_request(engine: &mut AppEngine) -> Option<Command> {
    engine.drain_pending_commands().into_iter().find(|command| {
        matches!(
            command,
            Command::CreateUserPresenceSecret { .. } | Command::RevealUserPresenceSecret { .. }
        )
    })
}

fn seal_binding(engine: &mut AppEngine) -> Option<UserPresenceBinding> {
    match presence_request(engine) {
        Some(Command::CreateUserPresenceSecret { binding, .. }) => Some(binding),
        _ => None,
    }
}

fn submit_first_app_password(engine: &mut AppEngine) {
    engine.navigate_to(AppScreen::ChangePassword);
    for field in ["new_password", "confirm_password"] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: field.into(),
            value: CHOSEN_PIN.into(),
        });
    }
    let _ = press(engine, "submit");
}

/// A biometric device that sealed a secret while it had no app password:
/// an install from before #778.
fn enrolled_without_password() -> AppEngine {
    let engine = biometric_device();
    assert!(
        engine
            .vauchi()
            .enroll_user_presence_secret(&SECRET)
            .unwrap()
    );
    engine
}

// @internal
#[test]
fn a_biometric_device_seals_behind_biometry_and_others_behind_the_device_credential() {
    for (biometric_type, binding) in [
        (
            Some(BiometricType::Fingerprint),
            UserPresenceBinding::Biometry,
        ),
        (Some(BiometricType::FaceId), UserPresenceBinding::Biometry),
        (None, UserPresenceBinding::DeviceCredential),
    ] {
        let mut engine = privacy_screen(biometric_type.clone());

        submit_first_app_password(&mut engine);

        assert_eq!(
            seal_binding(&mut engine),
            Some(binding),
            "{biometric_type:?}"
        );
    }
}

// @internal
#[test]
fn a_biometric_device_without_an_app_password_asks_for_one_before_exporting() {
    let mut engine = biometric_device();

    let _ = press(&mut engine, "export");

    assert_eq!(screen_id(&engine), "export_needs_app_password");
    assert_eq!(presence_request(&mut engine), None);
}

// @internal
#[test]
fn a_biometric_device_sets_its_first_app_password_once_biometry_seals_a_secret() {
    let mut engine = biometric_device();
    submit_first_app_password(&mut engine);
    assert_eq!(
        seal_binding(&mut engine),
        Some(UserPresenceBinding::Biometry)
    );

    let _ = engine.handle_hardware_event(Event::UserPresenceSecretCreated {
        secret: SECRET.to_vec().into(),
    });

    assert!(engine.vauchi().is_password_enabled().unwrap());
}

// A new finger needs only the device password and drops the sealed item;
// passing "unavailable" or "missing" would hand a first password to
// whoever knows the device password. The way back is restore or re-link.
// @internal
#[test]
fn an_enrolled_biometric_device_sets_no_first_password_when_the_secret_is_gone() {
    for answer in [
        Event::HardwareUnavailable {
            transport: "user_presence".into(),
        },
        Event::UserPresenceSecretMissing,
    ] {
        let mut engine = enrolled_without_password();
        submit_first_app_password(&mut engine);
        assert!(matches!(
            presence_request(&mut engine),
            Some(Command::RevealUserPresenceSecret { .. })
        ));

        let result = engine.handle_hardware_event(answer.clone());

        assert!(
            !engine.vauchi().is_password_enabled().unwrap(),
            "{answer:?} must not set a first password"
        );
        let Some(ActionResult::ShowAlert { message, .. }) = result else {
            panic!("{answer:?} must explain the way back, got {result:?}");
        };
        assert!(
            message.contains("restore a backup") && message.contains("link this device"),
            "{message}"
        );
    }
}

// Without biometrics a lost secret keeps the #739 way back.
// @internal
#[test]
fn a_device_without_biometrics_still_sets_its_first_password_when_the_secret_is_gone() {
    let mut engine = privacy_screen(None);
    assert!(
        engine
            .vauchi()
            .enroll_user_presence_secret(&SECRET)
            .unwrap()
    );
    submit_first_app_password(&mut engine);
    let _ = presence_request(&mut engine);

    let _ = engine.handle_hardware_event(Event::UserPresenceSecretMissing);

    assert!(engine.vauchi().is_password_enabled().unwrap());
}

/// Exports with `pin` from a session opened with it, on a device with a
/// password, a duress PIN and an enrolled secret; returns the commands
/// the export emitted and whether the enrolment survived.
fn export_in_session(pin: &str) -> (Vec<&'static str>, bool) {
    let mut engine = enrolled_without_password();
    engine.vauchi_mut().setup_app_password(APP_PIN).unwrap();
    engine.vauchi_mut().authenticate(APP_PIN).unwrap();
    engine
        .vauchi_mut()
        .setup_duress_password(DURESS_PIN)
        .unwrap();
    let mode = engine.vauchi_mut().authenticate(pin).unwrap();
    assert_eq!(
        mode,
        if pin == DURESS_PIN {
            AuthMode::Duress
        } else {
            AuthMode::Normal
        }
    );
    engine.navigate_to(AppScreen::Privacy);
    let _ = engine.drain_pending_commands();

    let _ = press(&mut engine, "export");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: pin.into(),
    });
    let _ = press(&mut engine, "confirm_pin");

    let commands = engine
        .drain_pending_commands()
        .iter()
        .map(Command::variant_name)
        .collect();
    (
        commands,
        engine.vauchi().has_user_presence_secret().unwrap(),
    )
}

// @internal
#[test]
fn a_duress_export_leaves_the_enrolment_and_emits_what_a_normal_one_does() {
    let (normal_commands, _) = export_in_session(APP_PIN);
    let (duress_commands, duress_kept) = export_in_session(DURESS_PIN);

    assert_eq!(duress_commands, normal_commands);
    assert!(
        duress_kept,
        "a duress session must not clear the real enrolment"
    );
}

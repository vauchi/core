// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Without an app password, a device whose shell can seal a secret behind
//! the OS's own check (biometric or device credential) verifies the user
//! that way before each export. The shell hands back the sealed secret and
//! Core compares it with what it enrolled, so a forged "success" exports
//! nothing (#712 D2/D4; owner decisions 2026-10-20).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::DeviceCapabilities;
use vauchi_core::{AuthMode, Command, Event};

const SECRET: [u8; 32] = [42; 32];

fn privacy_screen(can_seal_user_presence: bool) -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        can_seal_user_presence,
        ..Default::default()
    });
    engine.navigate_to(AppScreen::Privacy);
    let _ = engine.drain_pending_commands();
    engine
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn screen_id(engine: &AppEngine) -> String {
    engine.current_screen().screen_id
}

/// The user-presence command the export asked the shell to run, if any.
fn presence_request(engine: &mut AppEngine) -> Option<Command> {
    engine.drain_pending_commands().into_iter().find(|command| {
        matches!(
            command,
            Command::CreateUserPresenceSecret { .. } | Command::RevealUserPresenceSecret { .. }
        )
    })
}

fn enrolled(engine: &mut AppEngine) {
    let _ = press(engine, "export");
    let _ = presence_request(engine);
    let _ = engine.handle_hardware_event(Event::UserPresenceSecretCreated {
        secret: SECRET.to_vec().into(),
    });
    let _ = press(engine, "cancel");
}

// @internal
#[test]
fn the_first_export_has_the_shell_seal_a_secret_and_continues_once_it_does() {
    let mut engine = privacy_screen(true);

    let _ = press(&mut engine, "export");

    assert_eq!(screen_id(&engine), "export_await_presence");
    let Some(Command::CreateUserPresenceSecret { prompt, .. }) = presence_request(&mut engine)
    else {
        panic!("the first export asks the shell to seal a secret");
    };
    assert!(!prompt.is_empty(), "the OS prompt carries Core's copy");
    let _ = engine.handle_hardware_event(Event::UserPresenceSecretCreated {
        secret: SECRET.to_vec().into(),
    });
    assert_eq!(screen_id(&engine), "export_choose_format");
    assert!(engine.vauchi().has_user_presence_secret().unwrap());
}

// @internal
#[test]
fn a_later_export_has_the_shell_reveal_the_secret_and_checks_it() {
    let mut engine = privacy_screen(true);
    enrolled(&mut engine);

    let _ = press(&mut engine, "export");

    assert!(matches!(
        presence_request(&mut engine),
        Some(Command::RevealUserPresenceSecret { .. })
    ));
    let _ = engine.handle_hardware_event(Event::UserPresenceSecretRevealed {
        secret: SECRET.to_vec().into(),
    });
    assert_eq!(screen_id(&engine), "export_choose_format");
}

// A hooked shell can claim success, but without the OS check it has no
// secret to show.
// @internal
#[test]
fn a_wrong_secret_or_a_second_enrolment_exports_nothing() {
    let mut engine = privacy_screen(true);
    enrolled(&mut engine);
    let _ = press(&mut engine, "export");
    let _ = presence_request(&mut engine);

    let _ = engine.handle_hardware_event(Event::UserPresenceSecretRevealed {
        secret: vec![1; 32].into(),
    });
    let _ = engine.handle_hardware_event(Event::UserPresenceSecretCreated {
        secret: vec![1; 32].into(),
    });

    assert_ne!(screen_id(&engine), "export_choose_format");
    assert_eq!(screen_id(&engine), "export_await_presence");
}

// @internal
#[test]
fn a_secret_nobody_asked_for_is_ignored() {
    let mut engine = privacy_screen(true);
    enrolled(&mut engine);

    let _ = engine.handle_hardware_event(Event::UserPresenceSecretRevealed {
        secret: SECRET.to_vec().into(),
    });

    assert_eq!(screen_id(&engine), "privacy_settings");
}

// @internal
#[test]
fn cancelling_the_os_check_returns_to_privacy() {
    let mut engine = privacy_screen(true);
    let _ = press(&mut engine, "export");

    let _ = engine.handle_hardware_event(Event::UserPresenceCheckCancelled);

    assert_eq!(screen_id(&engine), "privacy_settings");
}

// @internal
#[test]
fn a_shell_without_the_hardware_falls_back_to_setting_an_app_password() {
    let mut engine = privacy_screen(true);
    let _ = press(&mut engine, "export");

    let _ = engine.handle_hardware_event(Event::HardwareUnavailable {
        transport: "user_presence".into(),
    });

    assert_eq!(screen_id(&engine), "export_needs_app_password");
}

const APP_PIN: &str = "app-password-123";

fn with_app_password(engine: &mut AppEngine) {
    engine.vauchi_mut().setup_app_password(APP_PIN).unwrap();
    engine.vauchi_mut().authenticate(APP_PIN).unwrap();
    engine.navigate_to(AppScreen::Privacy);
}

fn export_with_pin(engine: &mut AppEngine, pin: &str) {
    let _ = press(engine, "export");
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: pin.into(),
    });
    let _ = press(engine, "confirm_pin");
}

// A device whose sealed secret is gone (lock screen removed, app data
// cleared, restored phone) answers unavailable on every reveal. Once the
// app password proves the user, the stale enrolment goes, so the OS check
// works again if the password is later removed (#739, owner decision
// 2026-10-21).
// @internal
#[test]
fn the_app_password_clears_an_enrolment_whose_secret_was_lost() {
    let mut engine = privacy_screen(true);
    enrolled(&mut engine);
    let _ = press(&mut engine, "export");
    let _ = presence_request(&mut engine);
    let _ = engine.handle_hardware_event(Event::HardwareUnavailable {
        transport: "user_presence".into(),
    });
    assert_eq!(screen_id(&engine), "export_needs_app_password");
    with_app_password(&mut engine);

    export_with_pin(&mut engine, APP_PIN);

    assert_eq!(screen_id(&engine), "export_choose_format");
    assert!(!engine.vauchi().has_user_presence_secret().unwrap());
}

// @internal
#[test]
fn a_wrong_app_password_keeps_the_enrolment() {
    let mut engine = privacy_screen(true);
    enrolled(&mut engine);
    with_app_password(&mut engine);

    export_with_pin(&mut engine, "not-the-password");

    assert_ne!(screen_id(&engine), "export_choose_format");
    assert!(engine.vauchi().has_user_presence_secret().unwrap());
}

fn verified_by_presence(engine: &mut AppEngine) {
    enrolled(engine);
    let _ = press(engine, "export");
    let _ = presence_request(engine);
    let _ = engine.handle_hardware_event(Event::UserPresenceSecretRevealed {
        secret: SECRET.to_vec().into(),
    });
    assert_eq!(screen_id(engine), "export_choose_format");
}

fn leave_and_return(engine: &mut AppEngine) {
    engine.navigate_to(AppScreen::Settings);
    engine.navigate_to(AppScreen::Privacy);
    let _ = engine.drain_pending_commands();
}

// Whoever picks the phone up after the user walked away from a verified
// export is asked again (#750, owner decision 2026-10-22).
// @internal
#[test]
fn leaving_a_verified_export_asks_again_on_return() {
    let mut engine = privacy_screen(true);
    verified_by_presence(&mut engine);

    leave_and_return(&mut engine);

    assert_eq!(screen_id(&engine), "privacy_settings");
    let _ = press(&mut engine, "export");
    assert!(matches!(
        presence_request(&mut engine),
        Some(Command::RevealUserPresenceSecret { .. })
    ));
}

// @internal
#[test]
fn leaving_after_the_app_password_asks_for_it_again() {
    let mut engine = privacy_screen(false);
    with_app_password(&mut engine);
    export_with_pin(&mut engine, APP_PIN);
    assert_eq!(screen_id(&engine), "export_choose_format");

    leave_and_return(&mut engine);

    assert_eq!(screen_id(&engine), "privacy_settings");
    let _ = press(&mut engine, "export");
    assert_eq!(screen_id(&engine), "export_confirm_pin");
}

// @internal
#[test]
fn back_from_the_format_choice_returns_to_privacy_and_ends_the_export() {
    let mut engine = privacy_screen(true);
    verified_by_presence(&mut engine);

    let _ = engine.navigate_back();

    assert!(matches!(engine.current_app_screen(), AppScreen::Privacy));
    assert_eq!(screen_id(&engine), "privacy_settings");
    let _ = press(&mut engine, "export");
    assert_eq!(screen_id(&engine), "export_await_presence");
}

const CHOSEN_PIN: &str = "chosen-pin-4821";

fn submit_first_app_password(engine: &mut AppEngine, password: &str) {
    engine.navigate_to(AppScreen::ChangePassword);
    for field in ["new_password", "confirm_password"] {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: field.into(),
            value: password.into(),
        });
    }
    let _ = press(engine, "submit");
}

// Whoever holds the unlocked phone must not trade the OS check for a
// password they just chose (#766). The session that set it stays
// unauthenticated, so the attack locks the app and reopens it with that
// password before exporting.
// @internal
#[test]
fn a_first_app_password_set_without_the_os_check_never_verifies_an_export() {
    let mut engine = privacy_screen(true);
    enrolled(&mut engine);

    submit_first_app_password(&mut engine, CHOSEN_PIN);
    let _ = engine.drain_pending_commands();
    let _ = engine.vauchi_mut().authenticate(CHOSEN_PIN);
    engine.navigate_to(AppScreen::Privacy);
    export_with_pin(&mut engine, CHOSEN_PIN);

    assert_ne!(screen_id(&engine), "export_choose_format");
    assert!(!engine.vauchi().is_password_enabled().unwrap());
}

// @internal
#[test]
fn the_first_app_password_is_set_once_the_os_check_passes() {
    let mut engine = privacy_screen(true);
    enrolled(&mut engine);

    submit_first_app_password(&mut engine, CHOSEN_PIN);

    assert!(matches!(
        presence_request(&mut engine),
        Some(Command::RevealUserPresenceSecret { .. })
    ));
    assert!(!engine.vauchi().is_password_enabled().unwrap());
    let _ = engine.handle_hardware_event(Event::UserPresenceSecretRevealed {
        secret: SECRET.to_vec().into(),
    });
    assert!(engine.vauchi().is_password_enabled().unwrap());
    assert_eq!(
        engine.vauchi_mut().authenticate(CHOSEN_PIN).unwrap(),
        AuthMode::Normal
    );
}

// @internal
#[test]
fn before_any_enrolment_the_first_app_password_has_the_shell_seal_a_secret() {
    let mut engine = privacy_screen(true);

    submit_first_app_password(&mut engine, CHOSEN_PIN);

    assert!(matches!(
        presence_request(&mut engine),
        Some(Command::CreateUserPresenceSecret { .. })
    ));
    let _ = engine.handle_hardware_event(Event::UserPresenceSecretCreated {
        secret: SECRET.to_vec().into(),
    });
    assert!(engine.vauchi().is_password_enabled().unwrap());
}

// @internal
#[test]
fn a_wrong_secret_or_a_cancelled_check_sets_no_app_password() {
    for answer in [
        Event::UserPresenceSecretRevealed {
            secret: vec![1; 32].into(),
        },
        Event::UserPresenceCheckCancelled,
    ] {
        let mut engine = privacy_screen(true);
        enrolled(&mut engine);
        submit_first_app_password(&mut engine, CHOSEN_PIN);
        let _ = presence_request(&mut engine);

        let _ = engine.handle_hardware_event(answer.clone());

        assert!(
            !engine.vauchi().is_password_enabled().unwrap(),
            "{answer:?} must not set the password"
        );
    }
}

// A lost sealed secret leaves the app password as the only way back to
// exporting (#739), so an unavailable check does not block it.
// @internal
#[test]
fn when_the_os_check_is_unavailable_the_first_app_password_is_set() {
    let mut engine = privacy_screen(true);
    enrolled(&mut engine);
    submit_first_app_password(&mut engine, CHOSEN_PIN);
    let _ = presence_request(&mut engine);

    let _ = engine.handle_hardware_event(Event::HardwareUnavailable {
        transport: "user_presence".into(),
    });

    assert!(engine.vauchi().is_password_enabled().unwrap());
}

// @internal
#[test]
fn without_the_capability_the_first_app_password_is_set_at_once() {
    let mut engine = privacy_screen(false);

    submit_first_app_password(&mut engine, CHOSEN_PIN);

    assert_eq!(presence_request(&mut engine), None);
    assert!(engine.vauchi().is_password_enabled().unwrap());
}

// @scenario: privacy_compliance :: Export asks me to set an app password when nothing else can check it is me
#[test]
fn without_the_capability_export_still_asks_for_an_app_password() {
    let mut engine = privacy_screen(false);

    let _ = press(&mut engine, "export");

    assert_eq!(screen_id(&engine), "export_needs_app_password");
    assert_eq!(presence_request(&mut engine), None);
}

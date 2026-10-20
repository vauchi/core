// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! No data leaves the app without the user verifying first: every export
//! asks for the PIN that opened the session, immediately before it, and
//! one verification covers one export (#712; owner decision 2026-10-20).

use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::AuthMode;
use vauchi_core::api::Vauchi;

const APP_PIN: &str = "app-password-123";
const DURESS_PIN: &str = "135790";

fn privacy_screen(with_password: bool) -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    if with_password {
        engine.vauchi_mut().setup_app_password(APP_PIN).unwrap();
        engine
            .vauchi_mut()
            .setup_duress_password(DURESS_PIN)
            .unwrap();
        assert_eq!(
            engine.vauchi_mut().authenticate(APP_PIN).unwrap(),
            AuthMode::Normal
        );
    }
    engine.navigate_to(AppScreen::Privacy);
    engine
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn enter_pin(engine: &mut AppEngine, pin: &str) -> ActionResult {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: pin.into(),
    });
    press(engine, "confirm_pin")
}

fn screen_id(engine: &AppEngine) -> String {
    engine.current_screen().screen_id
}

// @scenario: privacy_compliance :: Export asks for my app password first
#[test]
fn export_asks_for_the_pin_before_any_file() {
    let mut engine = privacy_screen(true);

    let result = press(&mut engine, "export");

    assert!(
        !matches!(result, ActionResult::GdprExportComplete { .. }),
        "export must not run before the PIN, got {result:?}"
    );
    assert_eq!(screen_id(&engine), "export_confirm_pin");
}

// @scenario: privacy_compliance :: One confirmation covers one export
#[test]
fn the_pin_that_unlocked_the_app_exports_once() {
    let mut engine = privacy_screen(true);
    let _ = press(&mut engine, "export");

    let _ = enter_pin(&mut engine, APP_PIN);
    let _ = press(&mut engine, "export_json");

    let result = press(&mut engine, "confirm_readable_export");

    let ActionResult::GdprExportComplete { json } = result else {
        panic!("the right PIN lets one export through, got {result:?}");
    };
    assert!(
        json.contains("\"Alice\""),
        "the export carries the own card"
    );
    let _ = press(&mut engine, "export");
    assert_eq!(
        screen_id(&engine),
        "export_confirm_pin",
        "a second export asks again"
    );
}

// @scenario: privacy_compliance :: Export asks for my app password first
#[test]
fn a_wrong_pin_exports_nothing_and_never_changes_the_session() {
    let mut engine = privacy_screen(true);
    let _ = press(&mut engine, "export");

    for pin in ["wrong", DURESS_PIN] {
        let result = enter_pin(&mut engine, pin);
        assert!(
            !matches!(result, ActionResult::GdprExportComplete { .. }),
            "{pin} must not export, got {result:?}"
        );
        assert_eq!(screen_id(&engine), "export_confirm_pin");
    }
    let wrong = get_string(Locale::English, "lock_screen.wrong_password");
    assert!(
        format!("{:?}", engine.current_screen()).contains(&wrong),
        "the screen says the PIN was wrong"
    );
    assert_eq!(engine.vauchi().auth_mode(), AuthMode::Normal);
}

// Someone holding the unlocked phone could otherwise try PINs here for
// ever; after as many misses as the lock screen allows, lock the app.
// @scenario: privacy_compliance :: Export asks for my app password first
#[test]
fn five_wrong_pins_lock_the_app() {
    let mut engine = privacy_screen(true);
    let _ = press(&mut engine, "export");

    for _ in 0..5 {
        let _ = enter_pin(&mut engine, "wrong");
    }

    assert!(matches!(engine.current_app_screen(), AppScreen::Lock));
}

// @internal
#[test]
fn the_heirloom_asks_for_the_pin_after_its_warning() {
    let mut engine = privacy_screen(true);
    let _ = press(&mut engine, "heirloom");

    let result = press(&mut engine, "confirm_heirloom");

    assert!(
        !matches!(result, ActionResult::HeirloomExportComplete { .. }),
        "the heirloom must not export before the PIN, got {result:?}"
    );
    assert_eq!(screen_id(&engine), "export_confirm_pin");
    assert!(matches!(
        enter_pin(&mut engine, APP_PIN),
        ActionResult::HeirloomExportComplete { .. }
    ));
}

// @internal
#[test]
fn cancelling_the_pin_returns_to_privacy_without_exporting() {
    let mut engine = privacy_screen(true);
    let _ = press(&mut engine, "export");

    let result = press(&mut engine, "cancel");

    assert!(!matches!(result, ActionResult::GdprExportComplete { .. }));
    assert_eq!(screen_id(&engine), "privacy_settings");
}

// Until the device-credential check exists (#712 step 5), a user with no
// app password sets one before exporting.
// @scenario: privacy_compliance :: Export asks me to set an app password when nothing else can check it is me
#[test]
fn without_an_app_password_export_asks_to_set_one_first() {
    let mut engine = privacy_screen(false);

    let result = press(&mut engine, "export");

    assert!(!matches!(result, ActionResult::GdprExportComplete { .. }));
    assert_eq!(screen_id(&engine), "export_needs_app_password");
    let _ = press(&mut engine, "set_app_password");
    assert!(matches!(
        engine.current_app_screen(),
        AppScreen::ChangePassword
    ));
}

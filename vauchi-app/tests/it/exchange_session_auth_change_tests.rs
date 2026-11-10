// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! An exchange session holds a copy of the identity it exchanges as, so no
//! session built under one auth mode may live on under another (ADR-080,
//! vauchi/private#1117). The Exchange screen stays cached across tab
//! switches and a lock; left in the cache, a session made before a duress
//! unlock carries the real identity into the duress foreground, and one
//! made under duress would hand the mask's key to a real exchange.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;

const PASSWORD: &str = "pass1234";
const DURESS_PIN: &str = "654321";

fn engine_with_duress_pin() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.vauchi_mut().setup_app_password(PASSWORD).unwrap();
    engine
        .vauchi_mut()
        .setup_duress_pin_with_biometric_unlock(DURESS_PIN)
        .unwrap();
    engine
}

/// Opens Exchange, so its session is built, and leaves it cached behind
/// another tab.
fn open_exchange_then_leave(engine: &mut AppEngine) {
    engine.navigate_to(AppScreen::Exchange);
    assert!(
        engine.exchange_session_signing_key_for_testing().is_some(),
        "precondition: the exchange screen holds a session"
    );
    engine.navigate_to(AppScreen::MyInfo);
}

fn lock_and_unlock_with(engine: &mut AppEngine, pin: &str) {
    let _ = engine.dispatch(Event::AppBackgrounded).unwrap();
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: pin.into(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "unlock".into(),
    });
}

fn front_signing_key(engine: &AppEngine) -> [u8; 32] {
    *engine
        .vauchi()
        .identity()
        .expect("an identity in front")
        .signing_public_key()
}

fn session_signing_key_on_exchange(engine: &mut AppEngine) -> [u8; 32] {
    engine.navigate_to(AppScreen::Exchange);
    engine
        .exchange_session_signing_key_for_testing()
        .expect("the exchange screen holds a session")
}

// @scenario: duress_mode :: Real keys never loaded in duress mode
#[test]
fn a_session_made_before_a_duress_unlock_does_not_reach_the_duress_foreground() {
    let mut engine = engine_with_duress_pin();
    engine.vauchi_mut().authenticate(PASSWORD).unwrap();
    let real_key = front_signing_key(&engine);
    open_exchange_then_leave(&mut engine);

    // The mobile bindings unlock through `Vauchi::authenticate`, not the
    // lock screen's PIN field.
    engine.vauchi_mut().authenticate(DURESS_PIN).unwrap();

    let session_key = session_signing_key_on_exchange(&mut engine);
    assert_ne!(session_key, real_key);
    assert_eq!(session_key, front_signing_key(&engine));
}

// @scenario: duress_mode :: No trace of duress mode after logout
#[test]
fn a_session_made_under_duress_is_gone_after_a_normal_unlock() {
    let mut engine = engine_with_duress_pin();
    lock_and_unlock_with(&mut engine, DURESS_PIN);
    let mask_key = front_signing_key(&engine);
    open_exchange_then_leave(&mut engine);

    lock_and_unlock_with(&mut engine, PASSWORD);

    let session_key = session_signing_key_on_exchange(&mut engine);
    assert_ne!(session_key, mask_key);
    assert_eq!(session_key, front_signing_key(&engine));
}

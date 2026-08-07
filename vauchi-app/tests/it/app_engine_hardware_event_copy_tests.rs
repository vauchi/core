// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Hardware-event toasts and alerts are user-facing copy: they must come
//! from the locale catalog (ADR-038) and must never leak the raw
//! capability token a shell reports (verification finding GTK-5).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;

fn engine_on_exchange() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Ana").expect("identity created");
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::Exchange);
    engine
}

fn toast_message(result: Option<ActionResult>) -> String {
    let Some(ActionResult::ShowToast { message, .. }) = result else {
        panic!("expected a toast, got {result:?}");
    };
    message
}

// @internal — toast wording for shell-reported capability tokens; no
// Gherkin scenario covers this copy.
#[test]
fn unavailable_toast_names_the_capability_not_the_raw_token() {
    let mut engine = engine_on_exchange();
    let message = toast_message(engine.handle_hardware_event(Event::HardwareUnavailable {
        transport: "camera_switch".into(),
    }));
    assert_eq!(message, "Camera switch is not available on this device");
}

// @internal — an unmapped token must fall back to generic localized copy
// instead of leaking the raw token into the UI.
#[test]
fn unavailable_toast_for_an_unknown_token_stays_generic() {
    let mut engine = engine_on_exchange();
    let message = toast_message(engine.handle_hardware_event(Event::HardwareUnavailable {
        transport: "flux_capacitor".into(),
    }));
    assert_eq!(message, "This feature is not available on this device");
    assert!(!message.contains("flux_capacitor"));
}

// @internal — permission-denied toast wording; same ADR-038 seam.
#[test]
fn permission_denied_toast_is_localized() {
    let mut engine = engine_on_exchange();
    let message = toast_message(engine.handle_hardware_event(Event::PermissionDenied {
        transport: "camera".into(),
    }));
    assert_eq!(message, "Camera access was denied");
}

// @internal — hardware-error alert title; same ADR-038 seam.
#[test]
fn hardware_error_alert_title_is_localized() {
    let mut engine = engine_on_exchange();
    let result = engine.handle_hardware_event(Event::HardwareError {
        transport: "camera".into(),
        error: "AVFoundation error -11800".into(),
    });
    let Some(ActionResult::ShowAlert { title, message }) = result else {
        panic!("expected an alert, got {result:?}");
    };
    assert_eq!(title, "Camera error");
    assert_eq!(message, "AVFoundation error -11800");
}

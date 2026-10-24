// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reachability test for `SessionPinEngine`, the step in front of a duress
//! PIN change, a device revoke or a recovery voucher (private#777).
//!
//! `confirm_pin` and `cancel` are handled by `AppEngine`
//! (`app_engine/credential_gate.rs`), which checks the PIN against the
//! session and puts the asking screen back; the engine itself only holds
//! the typed PIN.

use vauchi_app::i18n::Locale;
use vauchi_app::ui::testing::assert_reachability;
use vauchi_app::ui::{SESSION_PIN_SCREEN_ID, SessionPinEngine, WorkflowEngine};

const HANDLED: &[&str] = &["confirm_pin", "cancel"];

// @internal
#[test]
fn session_pin_screen_is_reachable() {
    let engine = SessionPinEngine::new(
        Locale::ENGLISH,
        "devices.revoke_button",
        "devices.revoke_confirm_password_hint",
    );
    assert_eq!(engine.current_screen().screen_id, SESSION_PIN_SCREEN_ID);
    assert_reachability(&engine, HANDLED);
}

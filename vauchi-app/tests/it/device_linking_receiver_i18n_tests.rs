// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S6b-6b (`2026-07-03-core-screens-bypass-i18n`): the device-
//! linking receiver-side screens (waiting-for-request, qr-expired,
//! confirming-device, verifying-proximity, completing, link-failed)
//! render in the user's locale. Keys in `devices.link.*` (locales!99).
//! Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{DeviceLinkingEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: device-linking :: qr-expired + link-failed render in the active locale
// @internal
#[test]
fn device_linking_qr_expired_and_link_failed_render_german() {
    load_german();
    let mut engine = DeviceLinkingEngine::new("qr-data".into()).with_locale(Locale::German);
    engine.transition_to_qr_expired();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "QR-Code abgelaufen");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "retry")
            .unwrap()
            .label,
        "Neuen QR-Code generieren"
    );

    let mut failed_engine = DeviceLinkingEngine::new("qr-data".into()).with_locale(Locale::German);
    failed_engine.transition_to_link_failed("boom".into());
    let failed_screen = failed_engine.current_screen();
    assert_eq!(failed_screen.title, "Verknüpfung fehlgeschlagen");
    assert_eq!(
        failed_screen
            .actions
            .iter()
            .find(|a| a.id == "retry")
            .unwrap()
            .label,
        "Erneut versuchen"
    );
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn device_linking_qr_expired_and_link_failed_english_copy_unchanged() {
    let mut engine = DeviceLinkingEngine::new("qr-data".into());
    engine.transition_to_qr_expired();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "QR Code Expired");
    assert_eq!(
        screen
            .actions
            .iter()
            .find(|a| a.id == "retry")
            .unwrap()
            .label,
        "Generate New QR"
    );

    let mut failed_engine = DeviceLinkingEngine::new("qr-data".into());
    failed_engine.transition_to_link_failed("boom".into());
    let failed_screen = failed_engine.current_screen();
    assert_eq!(failed_screen.title, "Linking Failed");
    assert_eq!(
        failed_screen
            .actions
            .iter()
            .find(|a| a.id == "retry")
            .unwrap()
            .label,
        "Try Again"
    );
}

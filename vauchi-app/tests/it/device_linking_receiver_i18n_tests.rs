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
            .contextual_actions
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
            .contextual_actions
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
            .contextual_actions
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
            .contextual_actions
            .iter()
            .find(|a| a.id == "retry")
            .unwrap()
            .label,
        "Try Again"
    );
}

fn failure_detail_of(screen: &vauchi_app::ui::ScreenModel) -> String {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            vauchi_app::ui::Component::StatusIndicator { detail, .. } => detail.clone(),
            _ => None,
        })
        .expect("link_failed status detail present")
}

// M5 B2 (2026-07-03-second-device-join-dead-end item 4): the machine's
// stable failure id maps to a localized sentence; a raw machine id
// never reaches the screen. CC-03 exactness in both locales.
// @internal
#[test]
fn device_link_failure_id_maps_to_localized_copy() {
    load_german();

    let mut en = DeviceLinkingEngine::new("qr-data".into());
    en.transition_to_link_failed("user_denied".into());
    assert_eq!(
        failure_detail_of(&en.current_screen()),
        "The other device declined the link."
    );

    let mut de = DeviceLinkingEngine::new("qr-data".into()).with_locale(Locale::German);
    de.transition_to_link_failed("user_denied".into());
    assert_eq!(
        failure_detail_of(&de.current_screen()),
        "Das andere Gerät hat die Verknüpfung abgelehnt."
    );

    // An unknown / relay reason falls back to the generic sentence, never raw.
    let mut generic = DeviceLinkingEngine::new("qr-data".into());
    generic.transition_to_link_failed("relay timeout".into());
    let generic_detail = failure_detail_of(&generic.current_screen());
    assert!(
        !generic_detail.contains("relay timeout"),
        "raw reason must never render: {generic_detail}"
    );
    assert_eq!(generic_detail, "Device linking failed. Please try again.");
}

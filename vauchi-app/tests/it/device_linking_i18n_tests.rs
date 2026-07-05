// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S6b-6a (`2026-07-03-core-screens-bypass-i18n`): the device-
//! linking sender-side screens (transport selection, offline stub,
//! show-QR, verify-code, syncing, complete, qr-pending) render in the
//! user's locale. Keys in `devices.link.*` (locales!98). Exact German
//! assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{DeviceLinkingEngine, WorkflowEngine};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

// @scenario: device-linking :: transport-selection + show-QR render in the active locale
// @internal
#[test]
fn device_linking_transport_and_qr_render_german() {
    load_german();
    let engine =
        DeviceLinkingEngine::with_transport_selection("qr-data".into()).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Neues Gerät verknüpfen");
    assert_eq!(
        screen.subtitle.as_deref(),
        Some("Wie möchten Sie verknüpfen?")
    );

    let qr_engine = DeviceLinkingEngine::new("qr-data".into()).with_locale(Locale::German);
    let qr_screen = qr_engine.current_screen();
    assert_eq!(qr_screen.title, "Gerät verknüpfen");
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn device_linking_transport_and_qr_english_copy_unchanged() {
    let engine = DeviceLinkingEngine::with_transport_selection("qr-data".into());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Link New Device");
    assert_eq!(
        screen.subtitle.as_deref(),
        Some("How would you like to link?")
    );

    let qr_engine = DeviceLinkingEngine::new("qr-data".into());
    let qr_screen = qr_engine.current_screen();
    assert_eq!(qr_screen.title, "Link Device");
}

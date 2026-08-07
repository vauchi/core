// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the device-linking
//! transport-selection and show-QR screens render in the user's locale.
//!
//! Asserts that the screens resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{DeviceLinkingEngine, WorkflowEngine};

/// `(transport title, transport subtitle, QR title)`.
fn linking_copy(locale: Locale) -> (String, String, String) {
    let transport =
        DeviceLinkingEngine::with_transport_selection("qr-data".into()).with_locale(locale);
    let transport_screen = transport.current_screen();
    let qr = DeviceLinkingEngine::new("qr-data".into()).with_locale(locale);
    (
        transport_screen.title.clone(),
        transport_screen
            .subtitle
            .clone()
            .expect("transport subtitle present"),
        qr.current_screen().title.clone(),
    )
}

// @scenario: device-linking :: transport-selection + show-QR render in the active locale
// @internal
#[test]
fn device_linking_transport_and_qr_render_the_active_locale() {
    load_german();
    let (de_title, de_subtitle, de_qr) = linking_copy(Locale::German);
    let (en_title, en_subtitle, en_qr) = linking_copy(Locale::English);

    assert_translated("transport-selection title", &de_title, &en_title);
    assert_translated("transport-selection subtitle", &de_subtitle, &en_subtitle);
    assert_translated("show-QR title", &de_qr, &en_qr);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
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

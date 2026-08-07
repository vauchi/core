// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the device-linking
//! qr-expired and link-failed screens render in the user's locale.
//!
//! Asserts that the screens resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`. The
//! failure-id mapping keeps its own assertions, since "a raw machine id
//! never reaches the screen" is core's contract, not a copy question.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{DeviceLinkingEngine, WorkflowEngine};

/// `(qr-expired title, retry label, link-failed title, retry label)`.
fn terminal_copy(locale: Locale) -> (String, String, String, String) {
    let mut expired = DeviceLinkingEngine::new("qr-data".into()).with_locale(locale);
    expired.transition_to_qr_expired();
    let expired_screen = expired.current_screen();

    let mut failed = DeviceLinkingEngine::new("qr-data".into()).with_locale(locale);
    failed.transition_to_link_failed("boom".into());
    let failed_screen = failed.current_screen();

    (
        expired_screen.title.clone(),
        action_label(&expired_screen, "retry"),
        failed_screen.title.clone(),
        action_label(&failed_screen, "retry"),
    )
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

fn failure_detail_for(locale: Locale, reason: &str) -> String {
    let mut engine = DeviceLinkingEngine::new("qr-data".into()).with_locale(locale);
    engine.transition_to_link_failed(reason.into());
    failure_detail_of(&engine.current_screen())
}

// @scenario: device-linking :: qr-expired + link-failed render in the active locale
// @internal
#[test]
fn device_linking_qr_expired_and_link_failed_render_the_active_locale() {
    load_german();
    let (de_expired, de_expired_retry, de_failed, de_failed_retry) = terminal_copy(Locale::German);
    let (en_expired, en_expired_retry, en_failed, en_failed_retry) = terminal_copy(Locale::English);

    assert_translated("qr-expired title", &de_expired, &en_expired);
    assert_translated(
        "qr-expired retry action",
        &de_expired_retry,
        &en_expired_retry,
    );
    assert_translated("link-failed title", &de_failed, &en_failed);
    assert_translated(
        "link-failed retry action",
        &de_failed_retry,
        &en_failed_retry,
    );
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn device_linking_qr_expired_and_link_failed_english_copy_unchanged() {
    let (expired, expired_retry, failed, failed_retry) = terminal_copy(Locale::English);
    assert_eq!(expired, "QR Code Expired");
    assert_eq!(expired_retry, "Generate New QR");
    assert_eq!(failed, "Linking Failed");
    assert_eq!(failed_retry, "Try Again");
}

// M5 B2 (2026-07-03-second-device-join-dead-end item 4): the machine's
// stable failure id maps to a localized sentence; a raw machine id never
// reaches the screen. That mapping — and the never-raw guarantee — is
// core's contract, so it is asserted directly rather than via copy.
// @internal
#[test]
fn device_link_failure_id_maps_to_localized_copy() {
    load_german();

    let en = failure_detail_for(Locale::English, "user_denied");
    let de = failure_detail_for(Locale::German, "user_denied");
    assert_eq!(en, "The other device declined the link.");
    assert_translated("user-denied failure detail", &de, &en);

    // An unknown / relay reason falls back to the generic sentence, never
    // raw — in every locale.
    for locale in [Locale::English, Locale::German] {
        let generic = failure_detail_for(locale, "relay timeout");
        assert!(
            !generic.contains("relay timeout"),
            "raw reason must never render in {locale:?}: {generic}"
        );
        assert!(
            !generic.is_empty(),
            "generic fallback must still say something in {locale:?}"
        );
    }
    assert_eq!(
        failure_detail_for(Locale::English, "relay timeout"),
        "Device linking failed. Please try again."
    );
}

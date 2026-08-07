// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the recovery claim-review
//! screen renders in the user's locale.
//!
//! Asserts that the screen resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`. Interpolation
//! of the contact name is asserted separately, since that IS core's.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::WorkflowEngine;
use vauchi_app::ui::recovery_claim_review::{
    ClaimContext, Confidence, RecoveryClaimReviewEngine, ReviewMode,
};

fn context() -> ClaimContext {
    ClaimContext {
        contact_name: "Alice".into(),
        old_pk_fingerprint: "ABCD-1234".into(),
        mutual_voucher_count: 2,
        threshold: 3,
        confidence: Confidence::High,
    }
}

/// `(title, vouch action label)` for the claim-review screen.
fn review_copy(locale: Locale) -> (String, String) {
    let engine =
        RecoveryClaimReviewEngine::new(ReviewMode::Vouching, context()).with_locale(locale);
    let screen = engine.current_screen();
    (screen.title.clone(), action_label(&screen, "vouch"))
}

// @scenario: recovery :: claim review screen renders in the active locale
// @internal
#[test]
fn claim_review_screen_renders_the_active_locale() {
    load_german();
    let (de_title, de_vouch) = review_copy(Locale::German);
    let (en_title, en_vouch) = review_copy(Locale::English);

    assert_translated("claim-review title", &de_title, &en_title);
    assert_translated("vouch action", &de_vouch, &en_vouch);

    // Interpolation IS core's to hold: the contact name must survive into
    // the title in every locale, whatever the surrounding wording says.
    assert!(
        de_title.contains("Alice"),
        "German title dropped the interpolated contact name, got {de_title:?}"
    );
    assert!(
        en_title.contains("Alice"),
        "English title dropped the interpolated contact name, got {en_title:?}"
    );
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn claim_review_screen_english_copy_unchanged() {
    let engine = RecoveryClaimReviewEngine::new(ReviewMode::Vouching, context());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Recovery: Alice");
    assert_eq!(action_label(&screen, "vouch"), "Vouch");
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S5-3 (`2026-07-03-core-screens-bypass-i18n`): the incoming
//! recovery-claim-review screens render in the user's locale. Keys
//! in `recovery.*` (locales!102). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::WorkflowEngine;
use vauchi_app::ui::recovery_claim_review::{
    ClaimContext, Confidence, RecoveryClaimReviewEngine, ReviewMode,
};

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

fn context() -> ClaimContext {
    ClaimContext {
        contact_name: "Alice".into(),
        old_pk_fingerprint: "ABCD-1234".into(),
        mutual_voucher_count: 2,
        threshold: 3,
        confidence: Confidence::High,
    }
}

// @scenario: recovery :: claim review screen renders in the active locale
// @internal
#[test]
fn claim_review_screen_renders_german() {
    load_german();
    let engine =
        RecoveryClaimReviewEngine::new(ReviewMode::Vouching, context()).with_locale(Locale::German);
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Wiederherstellung: Alice");
    assert_eq!(
        screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "vouch")
            .unwrap()
            .label,
        "Bürgen"
    );
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn claim_review_screen_english_copy_unchanged() {
    let engine = RecoveryClaimReviewEngine::new(ReviewMode::Vouching, context());
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Recovery: Alice");
    assert_eq!(
        screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "vouch")
            .unwrap()
            .label,
        "Vouch"
    );
}

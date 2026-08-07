// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The build records which `locales` revision it embedded.
//!
//! `.clone-locales` resolves the content repo to `origin/main` at job
//! time, so without a record a release cannot say which copy it shipped —
//! and a merge there changes every consumer branch retroactively, which is
//! how five German assertions went red across every core branch on
//! 2026-08-07.
//!
//! Recorded, never enforced: nothing fails when the revision changes, so
//! translations keep flowing and no manual bump gates them. Pinning was
//! considered and rejected — see
//! `problems/2026-08-07-locale-content-consumed-from-unpinned-head/`.

use vauchi_app::i18n::BUNDLED_LOCALES_REV;

// @scenario: build-provenance :: the embedded locale revision is recorded
// @internal
#[test]
fn build_records_the_embedded_locale_revision() {
    assert!(
        !BUNDLED_LOCALES_REV.is_empty(),
        "the build must always record something, even when it cannot resolve a revision"
    );

    // `none` (no locale file found) and `unknown` (not a git checkout) are
    // the two honest non-answers. Anything else must look like a git short
    // rev — the point is that a release can be traced back to its copy.
    match BUNDLED_LOCALES_REV {
        "none" => panic!(
            "no locale file was found at build time — the two-key stub was embedded, \
             which is never what a consumer wants"
        ),
        "unknown" => {
            // A tarball or vendored checkout. Acceptable, not desirable.
        }
        rev => {
            assert!(
                rev.len() >= 7 && rev.chars().all(|c| c.is_ascii_hexdigit()),
                "recorded revision should be a git short rev, got {rev:?}"
            );
        }
    }
}

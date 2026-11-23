// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::AppScreen;
use vauchi_core::api::Vauchi;

// Boot decision (audit
// `2026-04-28-app-launch-and-identity-orchestration-in-core`
// §2.1). Frontends trust Core's selected screen rather than duplicating
// this decision tree.
//
// The "identity exists but onboarding never marked complete"
// resume-path called out by §2.5 is gated behind a future
// migration: existing installs ship `OnboardingProgress` at
// default (`completed_at == None`) so honouring the flag
// today would route every legacy user back through
// onboarding. Until the legacy heal lands, "identity exists"
// implies "past onboarding"; the atomic
// `create_identity_with_onboarding` helper still closes the
// *new* crash window for installs after this commit.
/// Whether the app password guards this app. A password state that cannot
/// be read counts as set: the lock fails closed, because treating an
/// unreadable record as "no password" opened the app without it
/// (vauchi/private#1370).
pub(super) fn password_guards_the_app(vauchi: &Vauchi) -> bool {
    vauchi.is_password_enabled().unwrap_or(true)
}

pub(super) fn initial_screen(vauchi: &Vauchi) -> AppScreen {
    if !vauchi.has_identity() {
        match vauchi.pending_recovery_claim() {
            Ok(None) => AppScreen::Onboarding,
            Ok(Some(_)) | Err(_) => AppScreen::Recovery,
        }
    } else if password_guards_the_app(vauchi)
        && vauchi.auth_mode() == vauchi_core::api::AuthMode::Unauthenticated
    {
        // A Vauchi already opened with the password (the CLI's --pin)
        // is unlocked; any other starts on the lock screen (#639).
        AppScreen::Lock
    } else {
        AppScreen::MyInfo
    }
}

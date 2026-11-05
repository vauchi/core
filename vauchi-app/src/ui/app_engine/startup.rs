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
pub(super) fn initial_screen(vauchi: &Vauchi) -> AppScreen {
    if !vauchi.has_identity() {
        if vauchi.pending_recovery_claim().ok().flatten().is_some() {
            AppScreen::Recovery
        } else {
            AppScreen::Onboarding
        }
    } else if vauchi.is_password_enabled().unwrap_or(false)
        && vauchi.auth_mode() == vauchi_core::api::AuthMode::Unauthenticated
    {
        // A Vauchi already opened with the password (the CLI's --pin)
        // is unlocked; any other starts on the lock screen (#639).
        AppScreen::Lock
    } else {
        AppScreen::MyInfo
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The duress setup's password branch at the `AppEngine` level (ADR-032,
//! amended 2026-11-10): Core's refusal goes back to the step it concerns,
//! and a weak app password offers the Change Password screen.

use vauchi_core::VauchiError;
use vauchi_core::api::{VauchiResult, WeakCredential};

use super::{AppEngine, AppScreen};
use crate::ui::EngineUpdate;
use crate::ui::action::{ActionResult, UserAction};

impl AppEngine {
    /// `None` when duress was set up; otherwise the result that shows
    /// Core's refusal on its field.
    pub(super) fn duress_app_password_refused(
        &mut self,
        outcome: VauchiResult<()>,
    ) -> Option<ActionResult> {
        if outcome.is_err() && !self.vauchi.is_password_enabled().unwrap_or(false) {
            return Some(ActionResult::ShowAlert {
                title: self.t("error.title"),
                message: format!("Failed to set duress PIN: {}", outcome.err()?),
            });
        }
        let (component_id, key) = match outcome {
            Ok(()) => return None,
            Err(VauchiError::WeakCredential(WeakCredential::DuressPassword)) => {
                ("app_password", "resistance.duress.error_weak_password")
            }
            Err(VauchiError::WeakCredential(_)) => {
                ("app_password", "resistance.duress.error_weak_app_password")
            }
            Err(_) => ("app_password", "resistance.duress.error_wrong_app_password"),
        };
        self.engine
            .apply_update(EngineUpdate::DuressAppPasswordRefused);
        Some(ActionResult::ValidationError {
            component_id: component_id.into(),
            message: self.t(key),
        })
    }

    pub(super) fn intercept_duress_change_app_password(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        if self.screen != AppScreen::DuressPin {
            return None;
        }
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };
        if action_id != "change_app_password" {
            return None;
        }
        let screen = self.navigate_to(AppScreen::ChangePassword);
        Some(ActionResult::NavigateTo(screen))
    }
}

impl AppEngine {
    /// The password branch's first step takes the app password, and Core
    /// checks it as the session PIN gate would, so the gate is skipped.
    pub(super) fn duress_setup_carries_the_app_password(&self) -> bool {
        matches!(
            self.engine.engine_output(),
            Some(crate::ui::EngineOutput::DuressPin(setup))
                if setup.enabled && setup.app_password.is_some()
        )
    }
}

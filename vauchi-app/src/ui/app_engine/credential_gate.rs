// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The session PIN step in front of a credential change (#777): the duress
//! PIN, revoking a device and vouching for someone's recovery wait until
//! the PIN that opened this session is entered again, so someone holding
//! the unlocked phone cannot make them (owner decision 2026-10-23).

use super::{AppEngine, WorkflowEngine};
use crate::ui::device_link_pin::CONFIRM_PIN_ACTION_ID;
use crate::ui::session_pin::SessionPinEngine;
use crate::ui::{ActionResult, EngineOutput, EngineUpdate, UserAction};
use zeroize::Zeroize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CredentialChange {
    DuressPin,
    DeviceRevoke,
    RecoveryVoucher,
}

/// The change waiting on the PIN, and the engine of the screen that asked
/// for it, set aside while the PIN step stands in for it.
pub(super) struct PendingCredentialChange {
    change: CredentialChange,
    requesting_engine: Box<dyn WorkflowEngine>,
}

impl CredentialChange {
    fn copy_keys(self) -> (&'static str, &'static str) {
        match self {
            Self::DuressPin => (
                "resistance.duress.pin_label",
                "resistance.duress.confirm_password_hint",
            ),
            Self::DeviceRevoke => (
                "devices.revoke_button",
                "devices.revoke_confirm_password_hint",
            ),
            Self::RecoveryVoucher => (
                "recovery.how_to_vouch",
                "recovery_help.confirm_password_hint",
            ),
        }
    }
}

impl AppEngine {
    /// Asks for the session PIN before `change`, or makes it at once when
    /// there is no app password and so no PIN to ask for. A password state
    /// that cannot be read asks.
    pub(super) fn confirm_session_pin_then(&mut self, change: CredentialChange) -> ActionResult {
        if !self.vauchi.is_password_enabled().unwrap_or(true) {
            return self.make_credential_change(change);
        }
        let (title_key, hint_key) = change.copy_keys();
        let pin_step =
            SessionPinEngine::new(self.render_context.resolved_locale(), title_key, hint_key);
        let requesting_engine = std::mem::replace(&mut self.engine, Box::new(pin_step));
        self.pending_credential_change = Some(PendingCredentialChange {
            change,
            requesting_engine,
        });
        ActionResult::NavigateTo(self.engine.current_screen())
    }

    /// Every action while the PIN step is up: the PIN step's own, or none.
    pub(super) fn intercept_session_pin(&mut self, action: &UserAction) -> Option<ActionResult> {
        self.pending_credential_change.as_ref()?;
        let UserAction::ActionPressed { action_id } = action else {
            return Some(self.engine.handle_action(action.clone()));
        };
        if action_id == "cancel" {
            self.drop_pending_credential_change();
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        }
        if action_id != CONFIRM_PIN_ACTION_ID {
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        }
        let Some(EngineOutput::SessionPin { mut pin }) = self.engine.engine_output() else {
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        };
        let confirmed = self.vauchi.confirm_session_password(&pin).unwrap_or(false);
        pin.zeroize();
        if confirmed {
            self.confirm_pin_failures = 0;
            let pending = self.pending_credential_change.take()?;
            self.engine = pending.requesting_engine;
            return Some(self.make_credential_change(pending.change));
        }
        if let Some(locked) = self.record_confirm_pin_miss() {
            return Some(locked);
        }
        let _ = self.engine.apply_update(EngineUpdate::SessionPinRejected);
        Some(ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Forgets a waiting change and rebuilds the screen that asked for it
    /// from storage, so nothing it showed as changed outlives the step.
    pub(super) fn drop_pending_credential_change(&mut self) {
        if let Some(pending) = self.pending_credential_change.take() {
            self.engine = pending.requesting_engine;
            let screen = self.screen.clone();
            self.invalidate_screen(&screen);
        }
    }

    /// Whether the device-management screen completed on a confirmed
    /// revoke rather than on leaving.
    pub(super) fn revoke_is_confirmed(&self) -> bool {
        matches!(
            self.engine.engine_output(),
            Some(EngineOutput::DeviceManagement {
                confirmed_revoke_index: Some(_)
            })
        )
    }

    fn make_credential_change(&mut self, change: CredentialChange) -> ActionResult {
        match change {
            CredentialChange::DuressPin => self.complete_duress_pin(),
            CredentialChange::DeviceRevoke => self.complete_device_management(),
            CredentialChange::RecoveryVoucher => self
                .intercept_create_voucher_action()
                .unwrap_or_else(|| ActionResult::UpdateScreen(self.engine.current_screen())),
        }
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! ContactDetail toggle intercepts: proposal-trust, recovery-trust, and
//! hide/unhide, each persisted to storage before the engine flips its
//! optimistic in-memory state. Split out of `intercept.rs` (cohesion).
//! `impl AppEngine` methods, dispatched from `dispatch.rs`.

use super::super::AppEngine;
use crate::ui::action::{ActionResult, UserAction};

impl AppEngine {
    /// Intercept proposal_trusted toggle on ContactDetail and persist to storage.
    ///
    /// When the user toggles the "proposal_trusted" item in the "trust_permissions"
    /// SettingsGroup, we load the contact, flip the flag, save it, then let the
    /// engine update its own local state.
    pub(in crate::ui::app_engine) fn intercept_proposal_trust_toggle(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let UserAction::SettingsToggled {
            component_id,
            item_id,
        } = action
        else {
            return None;
        };
        if component_id != "trust_permissions" || item_id != "proposal_trusted" {
            return None;
        }

        // Load the contact, flip the flag, save it back.
        // best-effort: the engine's optimistic flip below will revert
        // on the next ContactDetail engine read if storage didn't change.
        if let Ok(Some(mut contact)) = self.vauchi.get_contact(contact_id) {
            #[allow(clippy::let_underscore_must_use)]
            let _ = contact.set_proposal_trusted(!contact.is_proposal_trusted());
            if let Err(e) = self.vauchi.update_contact(&contact) {
                // Drop the error explicitly — same best-effort rationale.
                #[allow(clippy::let_underscore_must_use)]
                let _ = e;
            }
        }

        // Let the engine update its in-memory state and return the new screen.
        self.engine
            .apply_update(crate::ui::EngineUpdate::ContactDetail(
                crate::ui::ContactDetailUpdate::ToggleProposalTrusted,
            ))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept the recovery-trust SettingsToggled on ContactDetail and
    /// persist to storage. Mirror of `intercept_proposal_trust_toggle`,
    /// added 2026-04-28 for the Pair 3 (ContactDetail) Pure Humble UI
    /// retirement work.
    pub(in crate::ui::app_engine) fn intercept_recovery_trust_toggle(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let UserAction::SettingsToggled {
            component_id,
            item_id,
        } = action
        else {
            return None;
        };
        if component_id != "recovery_permissions" || item_id != "recovery_trusted" {
            return None;
        }

        // Toggle recovery trust via the canonical Vauchi API. Errors are
        // swallowed here — the engine's optimistic flip will be reverted
        // on the next AppScreen::ContactDetail engine read if storage
        // didn't actually change.
        #[allow(clippy::let_underscore_must_use)]
        let _ = self.vauchi.toggle_recovery_trust(contact_id);

        self.engine
            .apply_update(crate::ui::EngineUpdate::ContactDetail(
                crate::ui::ContactDetailUpdate::ToggleRecoveryTrusted,
            ))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// Intercept hide/unhide toggle on ContactDetail and persist to storage.
    pub(in crate::ui::app_engine) fn intercept_hide_toggle(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };
        if action_id != "toggle_hidden" {
            return None;
        }

        let is_hidden = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::ContactDetail { is_hidden }) => is_hidden,
            other => {
                tracing::warn!(?other, "hide toggle without ContactDetail output");
                false
            }
        };

        // best-effort: screen rebuild below reflects actual storage
        // state; an optimistic flip that diverges is recovered on next read
        if is_hidden {
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.unhide_contact(contact_id);
        } else {
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.hide_contact(contact_id);
        }

        self.engine
            .apply_update(crate::ui::EngineUpdate::ContactDetail(
                crate::ui::ContactDetailUpdate::ToggleHidden,
            ))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }
}

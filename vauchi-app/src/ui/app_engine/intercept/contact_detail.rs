// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! ContactDetail toggle intercepts: proposal-trust, recovery-trust, and
//! hide/unhide, each persisted to storage before the engine flips its
//! optimistic in-memory state. Split out of `intercept.rs` (cohesion).
//! `impl AppEngine` methods, dispatched from `dispatch.rs`.

use super::super::AppEngine;
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::contact_detail_guardian_group::{
    GROUP_ROW_PREFIX, GUARDIAN_GROUP_LIST, NEW_GROUP_ROW,
};

/// The group a picked row stands for: `Some(Some(n))` for group `n`,
/// `Some(None)` for a new group, `None` for a row the picker never made.
fn picked_group(row_id: &str) -> Option<Option<u8>> {
    if row_id == NEW_GROUP_ROW {
        return Some(None);
    }
    row_id
        .strip_prefix(GROUP_ROW_PREFIX)?
        .parse()
        .ok()
        .map(Some)
}

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

        if let Some(groups) = self.guardian_groups_to_ask(contact_id) {
            return self
                .engine
                .apply_update(crate::ui::EngineUpdate::ContactDetail(
                    crate::ui::ContactDetailUpdate::AskGuardianGroup(groups),
                ))
                .then(|| ActionResult::UpdateScreen(self.engine.current_screen()));
        }

        // A refused toggle (a tenth guardian, private#929) must not flip the
        // switch on screen only to revert on the next read: the person
        // reads why instead.
        if let Err(refused) = self.vauchi.toggle_recovery_trust(contact_id) {
            return Some(ActionResult::ValidationError {
                component_id: component_id.clone(),
                message: refused.to_string(),
            });
        }

        self.engine
            .apply_update(crate::ui::EngineUpdate::ContactDetail(
                crate::ui::ContactDetailUpdate::ToggleRecoveryTrusted,
            ))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }

    /// The groups to ask about as recovery trust turns on for `contact_id`;
    /// `None` when it is turning off or no group exists yet, since the
    /// first guardian can only start one (vauchi/private#796, D17).
    fn guardian_groups_to_ask(&self, contact_id: &str) -> Option<Vec<crate::ui::GuardianGroupRow>> {
        let contact = self.vauchi.get_contact(contact_id).ok().flatten()?;
        if contact.is_recovery_trusted() {
            return None;
        }
        let groups = self.vauchi.guardian_groups().ok()?;
        if groups.is_empty() {
            return None;
        }
        let name_of = |id: &String| {
            self.vauchi
                .get_contact(id)
                .ok()
                .flatten()
                .map(|member| member.display_name().to_string())
        };
        Some(
            groups
                .into_iter()
                .map(|(group_id, members)| crate::ui::GuardianGroupRow {
                    group_id,
                    member_names: members.iter().filter_map(name_of).collect(),
                })
                .collect(),
        )
    }

    /// Intercept the group picked in the guardian group question: trust
    /// the contact in it, or say why not and keep the question open.
    pub(in crate::ui::app_engine) fn intercept_guardian_group_pick(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let UserAction::ListItemSelected {
            component_id,
            item_id,
        } = action
        else {
            return None;
        };
        if component_id != GUARDIAN_GROUP_LIST {
            return None;
        }
        let refused = |message: String| {
            Some(ActionResult::ValidationError {
                component_id: component_id.clone(),
                message,
            })
        };
        let Some(group) = picked_group(item_id) else {
            return refused(crate::i18n::get_string(
                self.render_context.resolved_locale(),
                "contact_detail.guardian_group_unknown",
            ));
        };
        if let Err(error) = self.vauchi.trust_for_recovery_in_group(contact_id, group) {
            return refused(error.to_string());
        }
        self.engine
            .apply_update(crate::ui::EngineUpdate::ContactDetail(
                crate::ui::ContactDetailUpdate::CloseGuardianGroup,
            ));
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
            Some(crate::ui::EngineOutput::ContactDetail { is_hidden, .. }) => is_hidden,
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

    /// Intercept `ignore_contact` / `unignore_contact` on ContactDetail
    /// (ADR-072) and persist before the engine flips its in-memory flag.
    pub(in crate::ui::app_engine) fn intercept_ignore_toggle(
        &mut self,
        contact_id: &str,
        action: &UserAction,
    ) -> Option<ActionResult> {
        let UserAction::ActionPressed { action_id } = action else {
            return None;
        };
        let persisted = match action_id.as_str() {
            "ignore_contact" => self.vauchi.ignore_contact(contact_id),
            "unignore_contact" => self.vauchi.unignore_contact(contact_id),
            _ => return None,
        };
        if let Err(error) = persisted {
            tracing::warn!(%error, action_id, "ignore toggle not persisted");
            return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
        }

        self.engine
            .apply_update(crate::ui::EngineUpdate::ContactDetail(
                crate::ui::ContactDetailUpdate::ToggleIgnored,
            ))
            .then(|| ActionResult::UpdateScreen(self.engine.current_screen()))
    }
}

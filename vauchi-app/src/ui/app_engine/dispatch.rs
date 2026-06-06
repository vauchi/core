// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `AppEngine::handle_action` dispatch helpers.
//!
//! `handle_action` (in the parent module) is a sequence of early-return
//! guards. To keep that dispatch flat and under the cognitive-complexity
//! budget, the inline guard groups live here as `Option<ActionResult>`
//! returning helpers, mirroring the `intercept_*` methods in `intercept.rs`.
//! The rationale comments that used to sit inline in `handle_action` move
//! with their guards.

#[cfg(feature = "network-http")]
use super::SyncChromeStatus;
use super::{
    ACTION_DISMISS_DEMO_CONTACT, ACTION_OPEN_SETTINGS, ACTION_OPEN_UPDATE_LINK, ACTION_SYNC_NOW,
    AppEngine, AppScreen,
};
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::engine::WorkflowEngine;
use vauchi_core::version::AppUpdateStatus;

impl AppEngine {
    /// Global-chrome and top-level navigation guards that resolve *before*
    /// per-screen dispatch: sync-now indicator, backup reminder toast,
    /// update link, tab navigation, system back, the settings gear, and the
    /// demo-contact dismiss banner. Returns `None` when the action is not
    /// one of these reserved chrome affordances.
    pub(super) fn intercept_global_chrome(&mut self, action: &UserAction) -> Option<ActionResult> {
        // Handle sync_now from the chrome Indicator emitted by
        // apply_sync_chrome_overlay. Updates sync_chrome_status with
        // the outcome so the chip reflects the new state on next
        // render. No-op in builds without network-http feature.
        if matches!(action, UserAction::ActionPressed { action_id } if action_id == ACTION_SYNC_NOW)
        {
            #[cfg(feature = "network-http")]
            {
                use std::time::{SystemTime, UNIX_EPOCH};
                use vauchi_core::api::VauchiSyncOutcome;
                self.sync_chrome_status = match self.vauchi.sync() {
                    Ok(VauchiSyncOutcome::Ok { .. }) => SyncChromeStatus::Synced {
                        unix_ts: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0),
                    },
                    Ok(_) => self.sync_chrome_status,
                    Err(_) => SyncChromeStatus::Failed,
                };
            }
            return Some(ActionResult::UpdateScreen(self.current_screen()));
        }

        // Handle backup reminder toast action
        if matches!(action, UserAction::ActionPressed { action_id } if action_id == "backup_now") {
            return Some(ActionResult::NavigateTo(
                self.navigate_to(AppScreen::Backup),
            ));
        }

        // Handle update link action from banner/button presses
        if matches!(action, UserAction::ActionPressed { action_id } if action_id == ACTION_OPEN_UPDATE_LINK)
        {
            if matches!(self.update_status, AppUpdateStatus::UpdateAvailable) {
                self.update_dismissed = true;
            }
            return Some(ActionResult::OpenUrl {
                url: "vauchi://update".into(),
            });
        }

        // Top-level / tab navigation (ADR-043 Amendment 4): a NavigateToTab
        // action carries the opaque token core minted on `TabInfo.action_id`.
        // Resolve it to a target screen and return `NavigateTo` *before*
        // per-screen dispatch, so the frontend never constructs a navigation
        // target. An unknown token (adversarial / stale) leaves the engine
        // where it is rather than navigating somewhere wrong — same
        // unknown-id stance as `route_result` (`from_screen_id` → `None`).
        if let UserAction::NavigateToTab { action_id } = action {
            return Some(match AppScreen::from_screen_id(action_id) {
                Some(target) => ActionResult::NavigateTo(self.navigate_to(target)),
                None => ActionResult::UpdateScreen(self.engine.current_screen()),
            });
        }

        // System back gesture (ADR-043 Am4): typed twin of `navigate_back_json`
        // — pops via `navigate_back()`, gated on the frontend by `can_go_back`.
        if matches!(action, UserAction::NavigateBack) {
            return Some(ActionResult::NavigateTo(self.navigate_back()));
        }

        // Global-chrome navigation (ADR-043 Amendment 4): the native
        // top-bar gear forwards a reserved `ActionPressed` token rather
        // than constructing the Settings screen name. Resolve it to
        // `NavigateTo` before per-screen dispatch, on the same closed-set
        // basis as `open_update_link` above. Settings lives on its own
        // reserved id, so it cannot collide with the More-menu list item
        // (`"settings"`) or any per-screen `ActionPressed`.
        if matches!(action, UserAction::ActionPressed { action_id } if action_id == ACTION_OPEN_SETTINGS)
        {
            return Some(ActionResult::NavigateTo(
                self.navigate_to(AppScreen::Settings),
            ));
        }

        // Demo-contact dismiss (Tier-0 for the shell-purity spike): the
        // demo banner emitted by `apply_demo_contact_overlay` carries
        // this reserved id; intercept it here and call Vauchi to clear
        // the demo state, then re-render. Replaces iOS's
        // `viewModel.dismissDemoContact()` direct dispatch from
        // `DemoContactCard`.
        if matches!(action, UserAction::ActionPressed { action_id } if action_id == ACTION_DISMISS_DEMO_CONTACT)
        {
            // Storage failure here is non-actionable from the action
            // loop; the next render still reflects the previous state, and
            // the user can retry. Log instead of panicking.
            if let Err(err) = self.vauchi.dismiss_demo_contact() {
                tracing::warn!(?err, "dismiss_demo_contact storage write failed");
            }
            return Some(ActionResult::UpdateScreen(self.current_screen()));
        }

        None
    }

    /// Capture pending text/toggle input that later steps consume
    /// (onboarding display name, backup password + level). These guards
    /// *fall through* — they mutate pending state, they never resolve the
    /// action — so this returns `()` rather than an `ActionResult`.
    pub(super) fn capture_pending_input(&mut self, action: &UserAction) {
        // Capture display name during onboarding for identity persistence
        if self.screen == AppScreen::Onboarding
            && let UserAction::TextChanged {
                component_id,
                value,
            } = action
            && component_id == "display_name"
        {
            self.pending_display_name = Some(value.clone());
        }

        // Capture backup password and level toggle during backup flow
        if self.screen == AppScreen::Backup {
            match action {
                UserAction::TextChanged {
                    component_id,
                    value,
                } if component_id == "password" => {
                    self.pending_backup_password = Some(value.clone());
                }
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                } if component_id == "backup_level" && item_id == "level_toggle" => {
                    self.pending_backup_full = !self.pending_backup_full;
                }
                _ => {}
            }
        }
    }

    /// Per-screen interception for the detail screens that first need a
    /// cloned id out of the current `AppScreen` (MyInfo entry detail and
    /// contact detail). Delegates to the fine-grained `intercept_*` methods.
    pub(super) fn intercept_contact_screen(&mut self, action: &UserAction) -> Option<ActionResult> {
        if let AppScreen::MyInfoEntryDetail { field_id } = &self.screen {
            let field_id = field_id.clone();
            if let Some(result) = self.intercept_entry_detail_action(&field_id, action) {
                return Some(result);
            }
        }

        if let AppScreen::ContactDetail { contact_id } = &self.screen {
            let contact_id = contact_id.clone();
            if let Some(result) = self.intercept_personal_note_change(&contact_id, action) {
                return Some(result);
            }
            if let Some(result) = self.intercept_field_note_change(&contact_id, action) {
                return Some(result);
            }
            if let Some(result) = self.intercept_proposal_trust_toggle(&contact_id, action) {
                return Some(result);
            }
            if let Some(result) = self.intercept_recovery_trust_toggle(&contact_id, action) {
                return Some(result);
            }
            if let Some(result) = self.intercept_hide_toggle(&contact_id, action) {
                return Some(result);
            }
            if let Some(result) = self.intercept_contact_delete_archive(&contact_id, action) {
                return Some(result);
            }
        }

        None
    }

    /// List-screen CTAs, duplicate merge/dismiss, the recovery actions that
    /// need Vauchi (identity-key) access, and unarchive. Each guard is
    /// screen-scoped, so an action only resolves on the screen that owns it.
    pub(super) fn intercept_list_and_recovery(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        // "Go exchange" (empty-state CTA) and "Add contact" (always-visible
        // Primary button on the contact list) share the same target — on
        // MVP we only acquire contacts via in-person exchange, so both
        // affordances carry the same user intent. `add_contact` is only
        // emitted on Contacts; `go_exchange` also fires from MyInfo when
        // the list is empty. A dedicated VCF-import path can later redirect
        // `add_contact` via a frontend capability hint.
        if matches!(action, UserAction::ActionPressed { action_id } if action_id == "go_exchange" || action_id == "add_contact")
            && matches!(self.screen, AppScreen::Contacts | AppScreen::MyInfo)
        {
            let screen = self.navigate_to(AppScreen::Exchange);
            return Some(ActionResult::NavigateTo(screen));
        }

        // "View archived" from contacts → navigate to ArchivedContacts screen
        if matches!(action, UserAction::ActionPressed { action_id } if action_id == "view_archived")
            && matches!(self.screen, AppScreen::Contacts)
        {
            let screen = self.navigate_to(AppScreen::ArchivedContacts);
            return Some(ActionResult::NavigateTo(screen));
        }

        // "Find duplicates" from contacts → navigate to ContactDuplicates screen
        if matches!(action, UserAction::ActionPressed { action_id } if action_id == "find_duplicates")
            && matches!(self.screen, AppScreen::Contacts)
        {
            let screen = self.navigate_to(AppScreen::ContactDuplicates);
            return Some(ActionResult::NavigateTo(screen));
        }

        // "merge" from ContactDuplicates → store pending pair and navigate to ContactMerge
        if self.screen == AppScreen::ContactDuplicates
            && matches!(action, UserAction::ActionPressed { action_id } if action_id == "merge")
            && let Some(result) = self.intercept_merge_action()
        {
            return Some(result);
        }

        // "dismiss" from ContactDuplicates → drop the selected pair from
        // the duplicate set (reversible — re-detects on next find_duplicates).
        if self.screen == AppScreen::ContactDuplicates
            && matches!(action, UserAction::ActionPressed { action_id } if action_id == "dismiss")
            && let Some(result) = self.intercept_dismiss_duplicate_action()
        {
            return Some(result);
        }

        // RecoveryHelp screen: parse claim + create voucher need Vauchi
        // access (identity keypair for signing) so they're handled at the
        // AppEngine layer rather than inside the engine.
        if self.screen == AppScreen::RecoveryHelp
            && matches!(action, UserAction::ActionPressed { action_id } if action_id == "verify_claim")
            && let Some(result) = self.intercept_verify_claim_action()
        {
            return Some(result);
        }
        if self.screen == AppScreen::RecoveryHelp
            && matches!(action, UserAction::ActionPressed { action_id } if action_id == "create_voucher")
            && let Some(result) = self.intercept_create_voucher_action()
        {
            return Some(result);
        }

        // Recovery screen (EnterOldKey step): hex-decode + sign claim
        // need Vauchi/Identity access; engine signals Complete and the
        // intercept does the actual work via Vauchi::create_recovery_claim_hex_b64.
        if self.screen == AppScreen::Recovery
            && matches!(action, UserAction::ActionPressed { action_id } if action_id == "create_claim")
            && let Some(result) = self.intercept_create_claim_action()
        {
            return Some(result);
        }

        // Unarchive from ArchivedContacts screen
        if self.screen == AppScreen::ArchivedContacts
            && let UserAction::ActionPressed { action_id } = action
            && let Some(contact_id) = action_id.strip_prefix("unarchive_")
        {
            // best-effort: engine recreate below reflects storage truth;
            // the canonical archive/unarchive path is `apply_contact_action`
            // which surfaces ShowAlert on failure
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.unarchive_contact(contact_id);
            self.engine_cache.remove(&AppScreen::Contacts);
            self.engine_cache.remove(&AppScreen::ArchivedContacts);
            let screen = self.screen.clone();
            self.engine = Self::create_engine(
                &self.vauchi,
                &screen,
                self.preview_as_contact.as_deref(),
                &self.device_capabilities,
                &self.render_context,
            );
            return Some(ActionResult::ShowToast {
                message: "Contact unarchived".into(),
                undo_action_id: None,
            });
        }

        None
    }
}

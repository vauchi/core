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
    ACTION_DISMISS_DEMO_CONTACT, ACTION_GO_BACK, ACTION_OPEN_SETTINGS, ACTION_OPEN_UPDATE_LINK,
    ACTION_SYNC_NOW, AppEngine, AppScreen,
};
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::engine::WorkflowEngine;
use vauchi_core::version::AppUpdateStatus;

impl AppEngine {
    /// The single back decision shared by the OS back gesture
    /// (`UserAction::NavigateBack`) and the visible `go_back` chrome
    /// affordance: pop when there is a back step, else hand the frontend its
    /// native default (ADR-044 Am2a — core owns the empty-history decision).
    fn back_result(&mut self) -> ActionResult {
        if self.can_go_back() {
            ActionResult::NavigateTo(self.navigate_back())
        } else {
            ActionResult::PerformNativeBack
        }
    }

    /// Global-chrome and top-level navigation guards that resolve *before*
    /// per-screen dispatch: sync-now indicator, backup reminder toast,
    /// update link, tab navigation, system back, the settings gear, and the
    /// demo-contact dismiss banner. Returns `None` when the action is not
    /// one of these reserved chrome affordances.
    /// Relay catch-up sync (when the `network-http` transport is compiled in)
    /// followed by a re-render of the current screen. Shared by the `sync_now`
    /// chrome action and the `AppForegrounded` lifecycle event so core owns the
    /// "sync on resume" decision exactly once (ADR-021).
    pub(super) fn sync_and_rerender(&mut self) -> ActionResult {
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
        ActionResult::UpdateScreen(self.current_screen())
    }

    pub(super) fn intercept_global_chrome(&mut self, action: &UserAction) -> Option<ActionResult> {
        // Handle sync_now from the chrome Indicator emitted by
        // apply_sync_chrome_overlay. Updates sync_chrome_status with
        // the outcome so the chip reflects the new state on next
        // render. No-op in builds without network-http feature.
        // The explicit sync-now chrome action and the OS foreground event
        // (ADR-044 Am2a Family-A) share one consequence: relay catch-up +
        // re-render. Core owns the "sync on resume" decision (ADR-021),
        // retiring the frontend's ON_RESUME/becameActive branch.
        if matches!(action, UserAction::ActionPressed { action_id } if action_id == ACTION_SYNC_NOW)
            || matches!(action, UserAction::AppForegrounded)
        {
            return Some(self.sync_and_rerender());
        }

        // Handle backup reminder toast action. Toast buttons report
        // `UndoPressed` for wire compatibility even when their core-owned
        // label describes a different immediate action.
        if matches!(action, UserAction::ActionPressed { action_id } | UserAction::UndoPressed { action_id } if action_id == "backup_now")
        {
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
            // A locked app goes nowhere but through the password. Surface
            // composition already withholds these destinations, so reaching
            // here means a stale id from an earlier revision or a composition
            // path that regressed — either way the route refuses, and the two
            // guards fail independently
            // (2026-08-12-android-app-password-bypass).
            if self.is_locked() {
                return Some(ActionResult::UpdateScreen(self.engine.current_screen()));
            }
            return Some(match AppScreen::from_screen_id(action_id) {
                Some(target) => ActionResult::NavigateTo(self.navigate_to(target)),
                None => ActionResult::UpdateScreen(self.engine.current_screen()),
            });
        }

        // System back gesture (ADR-043 Am4 + ADR-044 Am2a): the frontend
        // forwards it *unconditionally* and core owns the decision. A back
        // step (engine-internal sub-flow or `nav_history`) pops via
        // `navigate_back()`; a back-stopping root has nothing to pop, so core
        // returns `PerformNativeBack` and the frontend performs its native
        // default — never a phantom re-nav, never a frontend `can_go_back`
        // gate on the handler.
        if matches!(action, UserAction::NavigateBack) {
            return Some(self.back_result());
        }

        // Visible Back affordance (ADR-044 Am2a): the reserved `go_back`
        // chrome action stamped on `nav_actions` shares the exact back logic
        // as the OS `NavigateBack` gesture — one code path, so a rendered
        // Back button and a swipe can never diverge.
        if matches!(action, UserAction::ActionPressed { action_id } if action_id == ACTION_GO_BACK)
        {
            return Some(self.back_result());
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

    /// A `vauchi://...` link was opened. Core parses the URI and routes to
    /// the correct flow: exchange consent for `vauchi://exchange`, device-link
    /// join for `vauchi://device-link`, or an error for anything else. The
    /// frontend only forwards the raw URI string; core owns all interpretation.
    pub(super) fn intercept_link_opened(&mut self, action: &UserAction) -> Option<ActionResult> {
        let UserAction::LinkOpened { uri } = action else {
            return None;
        };

        // 1. Exchange deep link → consent screen.
        if let Ok(payload) = vauchi_core::exchange::link_mode::parse_exchange_deep_link(uri) {
            return Some(ActionResult::NavigateTo(
                self.navigate_to(AppScreen::DeepLinkConsent { payload }),
            ));
        }

        // 2. Contact deep link → contact detail screen.
        if let Some(contact_id) = parse_contact_deep_link(uri) {
            return Some(match self.vauchi.get_contact(&contact_id) {
                Ok(Some(_)) => ActionResult::NavigateTo(
                    self.navigate_to(AppScreen::ContactDetail { contact_id }),
                ),
                Ok(None) => ActionResult::ShowAlert {
                    title: self.t("contact_detail.not_found_title"),
                    message: crate::i18n::get_string_with_args(
                        self.render_context.resolved_locale(),
                        "contact_detail.not_found_detail",
                        &[("id", &contact_id)],
                    ),
                },
                Err(_err) => ActionResult::ShowAlert {
                    title: self.t("error.title"),
                    message: self.t("error.generic"),
                },
            });
        }

        // 3. Device-link join invitation → join screen (fresh device only).
        if uri.starts_with("vauchi://device-link") {
            return Some(match self.open_device_link_invitation(uri) {
                Ok(screen) => ActionResult::NavigateTo(screen),
                Err(message) => ActionResult::ShowAlert {
                    title: self.t("device_link.invalid_title"),
                    message,
                },
            });
        }

        // 4. Unknown vauchi link (or non-vauchi scheme that the OS somehow
        //    delivered). Surface a single, core-owned error.
        Some(ActionResult::ShowAlert {
            title: self.t("deep_link.invalid_title"),
            message: "This link cannot be opened in Vauchi.".into(),
        })
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
            self.persist_personal_note_save(&contact_id, action);
            self.persist_field_note_save(&contact_id, action);
            if let Some(result) = self.intercept_tag_action(&contact_id, action) {
                return Some(result);
            }
            if let Some(result) = self.intercept_place_action(&contact_id, action) {
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
                &self.transport_readiness,
                &self.render_context,
                &self.pending_exchange_groups,
                self.glance_display_qr.as_deref(),
            );
            return Some(ActionResult::ShowToast {
                message: "Contact unarchived".into(),
                undo_action_id: None,
                undo_label: None,
            });
        }

        None
    }
}

/// Parse a `vauchi://contact/<id>` URI and return the contact id if valid.
///
/// Rejects empty ids, ids containing path separators, and any query/fragment
/// noise so the frontend cannot accidentally route to a malformed contact.
fn parse_contact_deep_link(uri: &str) -> Option<String> {
    let prefix = "vauchi://contact/";
    let rest = uri.strip_prefix(prefix)?;
    if rest.is_empty() || rest.contains('/') || rest.contains('?') || rest.contains('#') {
        return None;
    }
    Some(rest.to_string())
}

/// Translate renderer-convention InlineConfirm presses into the
/// canonical engine form.
///
/// iOS (`InlineConfirmView.swift`) and Android
/// (`InlineConfirmComponent.kt`) emit `<component_id>:confirm` /
/// `<component_id>:cancel`; engines match `confirm_<id>` /
/// `cancel_<id>`. Without this chokepoint normalization every inline
/// confirmation was a silent no-op — a dirty form became a screen the
/// user could not leave (device-verified Samsung S7,
/// `2026-06-11-add-entry-form-cannot-be-exited`). Colons appear in no
/// other action-id vocabulary, so the rewrite cannot collide.
pub(super) fn normalize_inline_confirm_action(action: UserAction) -> UserAction {
    if let UserAction::ActionPressed { action_id } = &action
        && let Some((id, verb)) = action_id.rsplit_once(':')
        && matches!(verb, "confirm" | "cancel")
        && !id.is_empty()
    {
        return UserAction::ActionPressed {
            action_id: format!("{verb}_{id}"),
        };
    }
    action
}

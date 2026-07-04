// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine-result routing for `AppEngine`.
//!
//! `route_result` maps an `ActionResult` emitted by the active engine onto a
//! navigation target or side effect. It is a flat dispatcher; the arms that
//! carry real work delegate to the `route_*` / `start_exchange_to` helpers
//! here so the match stays a readable table.

use super::{AppEngine, AppScreen};
use crate::ui::action::{ActionResult, PostOnboardingDestination};
use vauchi_core::exchange::mode::ExchangeMode;

impl AppEngine {
    /// Route engine results to appropriate navigation targets.
    pub(super) fn route_result(&mut self, result: ActionResult) -> ActionResult {
        match result {
            ActionResult::ContactAction { contact_id, kind } => {
                self.apply_contact_action(&contact_id, kind)
            }
            ActionResult::Complete => self.handle_completion(),
            ActionResult::CompleteWith { destination } => self.route_complete_with(destination),
            ActionResult::EditContact { contact_id } => {
                ActionResult::NavigateTo(self.navigate_to(AppScreen::ContactEdit { contact_id }))
            }
            // DeviceManagementEngine emits StartDeviceLink when the user taps
            // "Link New Device" — the link flow is fully core-driven, so route
            // straight there. Onboarding / DeviceReplacement also emit it but
            // on screens with their own native flows, so leave those untouched.
            ActionResult::StartDeviceLink if self.screen == AppScreen::DeviceManagement => {
                ActionResult::NavigateTo(self.navigate_to(AppScreen::DeviceLinking))
            }
            // ExchangeEngine emits these when the user picks an exchange mode.
            // All four carry the group-selection preamble across the engine
            // handoff (see start_exchange_to). The `mode` payload threads down
            // to the screen factory so it can pick the right session variant.
            ActionResult::StartMultiStageExchange { mode } => {
                self.start_exchange_to(AppScreen::MultiStageExchange { mode }, mode)
            }
            // StartLinkExchange carries no group preamble — the engine-owned
            // initiator on the LinkExchange screen drives the relay handshake.
            // The commit still becomes the last-used default (M2 S1).
            ActionResult::StartLinkExchange => {
                self.persist_exchange_defaults(ExchangeMode::Link, self.current_exchange_groups());
                ActionResult::NavigateTo(self.navigate_to(AppScreen::LinkExchange))
            }
            ActionResult::StartBleExchange { mode } => {
                self.start_exchange_to(AppScreen::BleExchange { mode }, mode)
            }
            ActionResult::StartNfcExchange => {
                self.start_exchange_to(AppScreen::NfcExchange, ExchangeMode::TapTap)
            }
            ActionResult::StartDirectTransport => {
                self.start_exchange_to(AppScreen::DirectTransport, ExchangeMode::Cable)
            }
            ActionResult::OpenEntryDetail { field_id } => ActionResult::NavigateTo(
                self.navigate_to(AppScreen::MyInfoEntryDetail { field_id }),
            ),
            ActionResult::VerifyFingerprint { contact_id } => ActionResult::NavigateTo(
                self.navigate_to(AppScreen::VerifyFingerprint { contact_id }),
            ),
            // MoreEngine reuses OpenContact to signal menu selection.
            ActionResult::OpenContact { contact_id } if self.screen == AppScreen::More => {
                match AppScreen::from_screen_id(&contact_id) {
                    Some(target) => ActionResult::NavigateTo(self.navigate_to(target)),
                    None => ActionResult::UpdateScreen(self.engine.current_screen()),
                }
            }
            // GroupsEngine reuses OpenContact to signal group selection.
            ActionResult::OpenContact { contact_id } if self.screen == AppScreen::Groups => {
                ActionResult::NavigateTo(self.navigate_to(AppScreen::GroupDetail {
                    group_id: contact_id,
                }))
            }
            // General contact open — the More/Groups guards above intercept
            // their reuse of OpenContact first, so this only sees a genuine
            // "open this contact". Core resolves the navigation so the frontend
            // never has to domain-map OpenContact (ADR-043/044; problem
            // 2026-05-25-contact-tap-opens-own-card).
            ActionResult::OpenContact { contact_id } => {
                ActionResult::NavigateTo(self.navigate_to(AppScreen::ContactDetail { contact_id }))
            }
            ActionResult::PreviewAs { contact_id } => {
                ActionResult::NavigateTo(self.preview_as(contact_id))
            }
            ActionResult::ShowContactPicker => {
                ActionResult::NavigateTo(self.navigate_to(AppScreen::Contacts))
            }
            ActionResult::ShowFormDialog {
                dialog_type,
                context_id,
            } => self.route_show_form_dialog(&dialog_type, context_id),
            // Intercept backup Processing: execute backup in core, return result screen
            ActionResult::NavigateTo(ref screen)
                if self.screen == AppScreen::Backup && screen.screen_id == "backup_processing" =>
            {
                self.execute_backup()
            }
            ActionResult::SetGroupFieldVisibility {
                group_id,
                field_id,
                visible,
            } => self.route_set_group_field_visibility(&group_id, &field_id, visible),
            ActionResult::RetryFailedDeliveries { message_ids } => {
                self.route_retry_failed_deliveries(&message_ids)
            }
            other => other,
        }
    }

    /// Capture the exchange-mode group selection across the engine handoff
    /// (navigate_to replaces the ExchangeEngine) so persist_exchanged_contact
    /// can assign the new contact + show the group on the success screen
    /// (2026-06-04-exchange-terminal-screens), then navigate to `target`.
    fn start_exchange_to(&mut self, target: AppScreen, mode: ExchangeMode) -> ActionResult {
        self.pending_exchange_groups = self.current_exchange_groups();
        // M2 S1: the committed (groups, mode) pair becomes the last-used
        // default, so the next exchange skips the group gate.
        self.persist_exchange_defaults(mode, self.pending_exchange_groups.clone());
        ActionResult::NavigateTo(self.navigate_to(target))
    }

    /// The active ExchangeEngine's selected groups (empty when the current
    /// engine is not the exchange flow).
    fn current_exchange_groups(&self) -> Vec<String> {
        self.engine
            .as_any()
            .and_then(|a| a.downcast_ref::<crate::ui::exchange::ExchangeEngine>())
            .map(|ex| ex.selected_groups().to_vec())
            .unwrap_or_default()
    }

    /// Persist the last-used exchange defaults (M2 S1). Best-effort: a
    /// storage failure must not block starting the exchange.
    fn persist_exchange_defaults(&self, mode: ExchangeMode, group_ids: Vec<String>) {
        let defaults = vauchi_core::types::ExchangeDefaults { group_ids, mode };
        if let Err(e) = self.vauchi.storage().ux().save_exchange_defaults(&defaults) {
            log::warn!("exchange defaults not persisted: {e}");
        }
    }

    /// `CompleteWith` — run completion, then route to the post-onboarding
    /// destination unless completion surfaced a validation error / alert.
    fn route_complete_with(&mut self, destination: PostOnboardingDestination) -> ActionResult {
        let base_result = self.handle_completion();
        if matches!(
            base_result,
            ActionResult::ValidationError { .. } | ActionResult::ShowAlert { .. }
        ) {
            return base_result;
        }
        let target = match destination {
            PostOnboardingDestination::MainScreen => AppScreen::MyInfo,
            PostOnboardingDestination::Exchange => AppScreen::Exchange,
            PostOnboardingDestination::ImportContacts => AppScreen::Contacts,
            PostOnboardingDestination::SecurityInfo => AppScreen::Help,
            PostOnboardingDestination::BackupSetup => AppScreen::Backup,
        };
        ActionResult::NavigateTo(self.navigate_to(target))
    }

    /// `ShowFormDialog` — resolve the group create/rename dialog and navigate
    /// to the FormDialog screen (or re-render if the type is unknown).
    fn route_show_form_dialog(
        &mut self,
        dialog_type: &str,
        context_id: Option<String>,
    ) -> ActionResult {
        let form_type = match dialog_type {
            "create_group" => Some(crate::ui::form_dialog::FormDialogType::CreateGroup),
            "rename_group" => {
                let group_id = context_id.unwrap_or_default();
                let current_name = self
                    .vauchi
                    .get_group(&group_id)
                    .ok()
                    .map(|g| g.name().to_string())
                    .unwrap_or_default();
                Some(crate::ui::form_dialog::FormDialogType::RenameGroup {
                    group_id,
                    current_name,
                })
            }
            _ => None,
        };
        if let Some(ft) = form_type {
            ActionResult::NavigateTo(self.navigate_to(AppScreen::FormDialog { dialog_type: ft }))
        } else {
            ActionResult::UpdateScreen(self.engine.current_screen())
        }
    }

    /// `SetGroupFieldVisibility` — persist field-visibility toggles emitted by
    /// GroupDetailEngine via the repropagating variant so downstream contacts
    /// re-fetch the visible field set on the next sync (Pure Humble UI Pair 2).
    fn route_set_group_field_visibility(
        &mut self,
        group_id: &str,
        field_id: &str,
        visible: bool,
    ) -> ActionResult {
        if let Err(e) = self
            .vauchi
            .set_group_field_visibility_and_repropagate(group_id, field_id, visible)
        {
            return ActionResult::ShowAlert {
                title: "Visibility Update Failed".into(),
                message: format!("{e}"),
            };
        }
        self.engine_cache.remove(&self.screen);
        ActionResult::UpdateScreen(self.engine.current_screen())
    }

    /// `RetryFailedDeliveries` — reschedule every failed delivery for immediate
    /// retry (mirror of `mobile_delivery::manual_retry`, applied per id).
    fn route_retry_failed_deliveries(&mut self, message_ids: &[String]) -> ActionResult {
        let now = self.vauchi.clock().unix_seconds();
        let mut rescheduled = 0u32;
        for id in message_ids {
            if let Ok(Some(_)) = self.vauchi.storage().retries().get_retry_entry(id)
                && self
                    .vauchi
                    .storage()
                    .retries()
                    .update_retry_next_time(id, now)
                    .is_ok()
            {
                rescheduled += 1;
            }
        }
        self.engine_cache.remove(&self.screen);
        ActionResult::ShowToast {
            message: if rescheduled == 1 {
                "Retry scheduled for 1 message".to_string()
            } else {
                format!("Retry scheduled for {rescheduled} messages")
            },
            undo_action_id: None,
        }
    }
}

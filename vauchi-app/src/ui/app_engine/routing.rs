// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Result routing for `AppEngine` — completion handling, hardware events,
//! and action result dispatch.

use super::AppEngine;
use super::AppScreen;
use crate::ui::ScreenModel;
use crate::ui::action::{ActionResult, ContactActionKind};
use crate::ui::form_dialog::FormDialogType;
use vauchi_core::Event;

impl AppEngine {
    /// Returns `true` if the current engine has user-entered data that differs
    /// from the original. Used by frontends to show a "discard changes?" prompt.
    pub fn form_has_data(&self) -> bool {
        let dialog_type = match &self.screen {
            AppScreen::FormDialog { dialog_type } => dialog_type,
            _ => return false,
        };
        use crate::ui::FormInput;
        let input = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Form(input)) => input,
            _ => return false,
        };
        match (dialog_type, input) {
            (FormDialogType::AddField { .. }, FormInput::AddField { label, value, .. }) => {
                !label.trim().is_empty() || !value.trim().is_empty()
            }
            (
                FormDialogType::EditField {
                    current_value,
                    current_note,
                    ..
                },
                FormInput::EditField { value, note },
            ) => value != *current_value || note != current_note.as_deref().unwrap_or(""),
            (FormDialogType::EditName { current_name }, FormInput::EditName { name }) => {
                name != *current_name
            }
            (FormDialogType::EditRelayUrl { current_url }, FormInput::EditRelayUrl { url }) => {
                url != *current_url
            }
            (FormDialogType::CreateGroup, FormInput::CreateGroup { name }) => !name.is_empty(),
            (FormDialogType::RenameGroup { current_name, .. }, FormInput::RenameGroup { name }) => {
                name != *current_name
            }
            _ => false,
        }
    }

    /// Returns all groups as (id, name) pairs for UI forms.
    pub fn available_groups(&self) -> Vec<(String, String)> {
        self.vauchi
            .list_groups()
            .unwrap_or_default()
            .into_iter()
            .map(|g| (g.id().to_string(), g.name().to_string()))
            .collect()
    }

    /// Returns the field type catalog for the Add Field picker.
    pub fn field_type_catalog(&self) -> &vauchi_core::contact_card::FieldTypeCatalog {
        &self.field_catalog
    }

    /// Handle a hardware event from the frontend (ADR-031).
    ///
    /// Frontends call this when hardware reports results (QR scanned, BLE data
    /// received, image picked, etc.). Returns `Commands` with response
    /// commands, or a screen update if the engine state changed.
    ///
    /// Returns `None` if the current screen doesn't handle hardware events.
    #[tracing::instrument(level = "debug", skip_all, name = "app.handle_hardware_event")]
    /// Queue a `Command::LocationRequest` and remember the contact it should
    /// annotate (ADR-051 capture-at-exchange). The frontend replies with
    /// `Event::LocationResult` (or a location `PermissionDenied` /
    /// `HardwareUnavailable`), consumed in `handle_hardware_event`.
    pub fn request_exchange_location(&mut self, contact_id: String) {
        self.pending_location_contact = Some(contact_id);
        self.pending_commands
            .push_back(vauchi_core::Command::LocationRequest { timeout_ms: 10_000 });
    }

    pub fn handle_hardware_event(&mut self, event: Event) -> Option<ActionResult> {
        // Phase 2 (T2.1b): record a transport permission denial in the
        // device-wide readiness ledger up front, regardless of screen, so the
        // mode picker reflects it on next visit (T2.2 consult). The ledger
        // ignores non-transport labels — notably "location" (the ADR-051
        // capture-geolocation permission, handled separately below).
        if let Event::PermissionDenied { transport } = &event {
            self.transport_readiness.note_permission_denied(transport);
            // Rebuild the picker on next visit. TODO(T2.2): a cache remove does
            // not rebuild the LIVE engine — the consult slice must re-render it.
            self.engine_cache.remove(&AppScreen::Exchange);
        }

        // ADR-031 file-picker: dispatched by current screen, not by the
        // narrow screen guard below. The picker is reachable from More
        // (contacts import) and — once Phase 2B lands — Onboarding
        // (backup restore). Phase 2A handles contacts only; backup
        // restore is deferred (multi-step password flow).
        match &event {
            Event::FilePickedFromUser { bytes, filename } => {
                return self.handle_file_picked(bytes.clone(), filename.clone());
            }
            Event::FilePickCancelledByUser => {
                // User dismissed the picker — no-op. Frontend stays on
                // the originating screen with no toast / alert.
                return None;
            }
            // ADR-051 capture-at-exchange: record where a just-exchanged
            // contact was met. Handled before the exchange-screen guard
            // because the reply can arrive after navigating to the success
            // / contact screen.
            Event::LocationResult {
                latitude,
                longitude,
                ..
            } => {
                if let Some(contact_id) = self.pending_location_contact.take() {
                    #[allow(clippy::let_underscore_must_use)]
                    let _ = self
                        .vauchi
                        .set_exchange_location(&contact_id, *latitude, *longitude);
                }
                return None;
            }
            Event::PermissionDenied { transport } | Event::HardwareUnavailable { transport }
                if transport == "location" =>
            {
                // Declined / no provider — drop the pending capture silently
                // (no toast on the post-exchange screen).
                self.pending_location_contact = None;
                return None;
            }
            _ => {}
        }

        // Slice 32l Phase 2: the engine-owned responder consumes RelayEscrow*
        // on its screen (ADR-031 escrow); the guard lives in the helper.
        if let Some(result) = self.route_link_responder_hardware_event(&event) {
            return Some(result);
        }

        // Slice 32l Phase 3: the engine-owned link initiator consumes
        // LinkShared / LinkOpened / RelayEscrow* on its screen (ADR-031).
        if let Some(result) = self.route_link_initiator_hardware_event(&event) {
            return Some(result);
        }

        if !matches!(
            self.screen,
            AppScreen::Exchange
                | AppScreen::AvatarEditor
                | AppScreen::Recovery
                | AppScreen::MultiStageExchange { .. }
                | AppScreen::BleExchange { .. }
                | AppScreen::NfcExchange
                | AppScreen::DirectTransport
        ) {
            return None;
        }

        // ADR-031: For error events, build a user-friendly UI response
        // before delegating to the engine (which may transition to Failed).
        let ui_override = match &event {
            Event::HardwareUnavailable { transport } => Some(ActionResult::ShowToast {
                message: format!("{} is not available on this device", transport),
                undo_action_id: None,
            }),
            Event::PermissionDenied { transport } => Some(ActionResult::ShowToast {
                message: format!("{} access was denied", transport),
                undo_action_id: None,
            }),
            Event::HardwareError { transport, error } => Some(ActionResult::ShowAlert {
                title: format!("{} error", transport),
                message: error.clone(),
            }),
            _ => None,
        };

        // Delegate to the engine via the WorkflowEngine trait (ADR-031).
        // ExchangeEngine handles session-aware events; other engines return None.
        if let Some(result) = self.engine.handle_hardware_event(event) {
            // Navigation and command results take priority over informational
            // toasts — the engine handled the event with a state transition
            // (e.g., camera denied → ManualEntry). Toasts are only used when
            // the engine returns a simple screen update.
            if matches!(
                result,
                ActionResult::NavigateTo(_) | ActionResult::Commands { .. }
            ) {
                return Some(result);
            }
            return Some(ui_override.unwrap_or(result));
        }

        // Engine didn't handle it — return error UI if applicable
        if let Some(ui) = ui_override {
            return Some(ui);
        }

        None
    }

    /// Advance the animated QR to its next frame (~10fps timer from the frontend).
    ///
    /// Delegates to the active engine's `WorkflowEngine::advance_qr_frame`. Only
    /// `ExchangeEngine` on the ShowQr step responds — everything else returns
    /// `None`, so frontends can safely tick the timer without guarding on screen.
    pub fn advance_qr_frame(&mut self) -> Option<ScreenModel> {
        if !matches!(self.screen, AppScreen::Exchange) {
            return None;
        }
        self.engine.advance_qr_frame()
    }

    /// ADR-031 file-picker handler: route picked bytes by the screen
    /// that requested the pick. Phase 2A wires only `AppScreen::More`
    /// (contacts import via vCard); Phase 2B adds `AppScreen::Onboarding`
    /// (backup restore — needs multi-step password flow).
    ///
    /// Returns `Some(ActionResult)` describing the outcome (toast on
    /// success, alert on failure, or further commands if a multi-step
    /// flow continues), or `None` when the current screen does not
    /// participate in the file-picker protocol.
    fn handle_file_picked(&mut self, bytes: Vec<u8>, _filename: String) -> Option<ActionResult> {
        match self.screen {
            AppScreen::More => {
                // Contacts import (vCard / VCF). Core handles bytes →
                // import_contacts_from_vcf; result rendered as a toast
                // with imported / skipped counts. Warnings beyond the
                // counts surface in the next visit to ImportSummary
                // (out of scope for Phase 2A).
                match self.vauchi.import_contacts_from_vcf(&bytes) {
                    Ok(result) => {
                        // Refresh the Contacts screen cache so the
                        // newly imported rows appear on next navigation.
                        self.engine_cache.remove(&AppScreen::Contacts);
                        Some(ActionResult::ShowToast {
                            message: format_import_toast(result.imported, result.skipped),
                            undo_action_id: None,
                        })
                    }
                    Err(e) => Some(ActionResult::ShowAlert {
                        title: "Import failed".into(),
                        message: e.to_string(),
                    }),
                }
            }
            AppScreen::Onboarding => {
                // ADR-031 Phase 2B: backup-restore. The picked bytes
                // are the encrypted backup file (hex-encoded ASCII —
                // matches `Vauchi::export_full_backup` output). Stash
                // on the OnboardingEngine and transition to its new
                // `BackupPasswordEntry` step so the user can enter
                // the password core-side.
                if self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::Onboarding(
                        crate::ui::OnboardingUpdate::PendingBackupBytes(bytes),
                    ))
                {
                    return Some(ActionResult::NavigateTo(self.engine.current_screen()));
                }
                None
            }
            AppScreen::DeviceReplacement => {
                // ADR-031 Phase 2B: lost-device backup restore. The
                // user is on the DeviceReplacement SelectMode screen
                // and just picked their encrypted backup. Swap to the
                // OnboardingEngine seeded directly at
                // BackupPasswordEntry (skipping IdentityCheck +
                // LinkChoice — they already consented when they
                // entered DeviceReplacement). Result: smooth password
                // → import flow shared with the onboarding entry point.
                let _ = self.navigate_to_internal(AppScreen::Onboarding);
                if self
                    .engine
                    .apply_update(crate::ui::EngineUpdate::Onboarding(
                        crate::ui::OnboardingUpdate::PendingBackupBytes(bytes),
                    ))
                {
                    return Some(ActionResult::NavigateTo(self.engine.current_screen()));
                }
                None
            }
            // Other screens don't participate in the file-picker
            // protocol. Phase 2B+ extends as new flows wire through.
            _ => None,
        }
    }

    pub(super) fn handle_completion(&mut self) -> ActionResult {
        // ADR-031 Phase 2B (`2026-05-03-core-file-picker-command`):
        // detect a `BackupPasswordEntry` submit on the OnboardingEngine
        // BEFORE the default identity-creation path runs. The submit
        // returns `ActionResult::Complete` from the engine; here we
        // pull the pending bytes + password and call
        // `Vauchi::import_full_backup`. Bytes are the file content
        // verbatim — hex-encoded ASCII matching `export_full_backup`.
        if matches!(self.screen, AppScreen::Onboarding)
            && let Some(crate::ui::EngineOutput::Onboarding(snap)) = self.engine.engine_output()
            && snap.step == vauchi_core::types::OnboardingStep::BackupPasswordEntry
            && let Some(pending) = snap.pending_backup
        {
            // Consume the staged backup so re-submitting without
            // re-picking the file stays impossible (take semantics).
            let _ = self
                .engine
                .apply_update(crate::ui::EngineUpdate::Onboarding(
                    crate::ui::OnboardingUpdate::ClearPendingBackup,
                ));
            return self.execute_backup_restore(pending.bytes, pending.password);
        }

        // Dispatch on a cloned screen so the per-handler `&mut self` calls
        // are not blocked by a borrow of `self.screen`. Each arm delegates
        // to a `complete_<screen>` handler in `completion.rs`.
        let screen = self.screen.clone();
        match &screen {
            AppScreen::Onboarding => self.complete_onboarding(),
            AppScreen::Lock => self.complete_lock(),
            AppScreen::Exchange => self.complete_exchange(),
            AppScreen::MultiStageExchange { .. } => self.complete_multi_stage_exchange(),
            AppScreen::ContactVisibility { contact_id } => {
                self.complete_contact_visibility(contact_id)
            }
            AppScreen::VerifyFingerprint { contact_id } => {
                self.complete_verify_fingerprint(contact_id)
            }
            AppScreen::EmergencyShred => self.complete_emergency_shred(),
            AppScreen::EmergencyBroadcast => self.complete_emergency_broadcast(),
            AppScreen::Privacy => self.complete_privacy(),
            AppScreen::FormDialog { dialog_type } => self.complete_form_dialog(dialog_type),
            AppScreen::Sync => self.complete_sync(),
            AppScreen::ChangePassword => self.complete_change_password(),
            AppScreen::DuressPin => self.complete_duress_pin(),
            AppScreen::DeviceManagement => self.complete_device_management(),
            AppScreen::ContactDetail { contact_id } => self.complete_contact_detail(contact_id),
            AppScreen::ContactEdit { contact_id } => self.complete_contact_edit(contact_id),
            AppScreen::GroupDetail { group_id } => self.complete_group_detail(group_id),
            AppScreen::Groups => self.complete_groups(),
            AppScreen::ContactMerge { .. } => self.complete_contact_merge(),
            AppScreen::AvatarEditor => self.complete_avatar_editor(),
            AppScreen::DeviceReplacement => self.complete_device_replacement(),
            AppScreen::DeepLinkConsent { payload } => self.complete_deep_link_consent(payload),
            _ => ActionResult::NavigateTo(self.navigate_back()),
        }
    }

    /// Execute backup export using captured password and level.
    ///
    /// Called when the BackupRecoveryEngine transitions to Processing.
    /// Runs the backup operation synchronously (Argon2id KDF is slow but
    /// the platform already calls handle_action on a background thread).
    /// Execute a backup-restore using bytes picked through the
    /// ADR-031 file-picker (`Command::FilePickFromUser` with
    /// `purpose = ImportBackup`) and the password the user typed on
    /// the `OnboardingStep::BackupPasswordEntry` screen.
    ///
    /// The picked file content is the exact output of
    /// `Vauchi::export_full_backup` (hex-encoded ASCII). After UTF-8
    /// decoding, it's passed straight to `Vauchi::import_full_backup`
    /// which hex-decodes internally and rehydrates the identity, own
    /// card, contacts, and labels.
    ///
    /// On success, navigate to MainScreen (MyInfo). On failure
    /// (corrupt bytes, wrong password, IO), return to LinkChoice with
    /// a `ShowAlert` so the user can retry.
    fn execute_backup_restore(&mut self, bytes: Vec<u8>, password: String) -> ActionResult {
        let backup_hex = match std::str::from_utf8(&bytes) {
            Ok(s) => s.trim().to_string(),
            Err(_) => {
                self.reset_onboarding_to_link_choice();
                return ActionResult::ShowAlert {
                    title: "Restore failed".into(),
                    message: "The selected file does not look like a Vauchi backup.".into(),
                };
            }
        };

        match self.vauchi.import_full_backup(&backup_hex, &password) {
            Ok(()) => {
                let target = AppScreen::MyInfo;
                let screen = self.navigate_to_internal(target);
                ActionResult::NavigateTo(screen)
            }
            Err(e) => {
                self.reset_onboarding_to_link_choice();
                ActionResult::ShowAlert {
                    title: "Restore failed".into(),
                    message: format!("{e}"),
                }
            }
        }
    }

    /// Helper: rewind the OnboardingEngine to LinkChoice so the user
    /// can retry restore after a failure. Called only from
    /// `execute_backup_restore`'s error paths.
    fn reset_onboarding_to_link_choice(&mut self) {
        // best-effort: navigation reset is advisory; if the engine
        // can't transition the screen will rebuild fresh
        let _ = self
            .engine
            .apply_update(crate::ui::EngineUpdate::Onboarding(
                crate::ui::OnboardingUpdate::ResetToLinkChoice,
            ));
    }

    ///
    /// Import (restore) flows through `execute_backup_restore` (Phase 2B
    /// of `2026-05-03-core-file-picker-command`) which wires the file
    /// picker + password entry through core. This helper handles the
    /// export side only.
    pub(super) fn execute_backup(&mut self) -> ActionResult {
        // The backup password, level toggle, and restore blob all live on the
        // engine (it captures them as the user advances and zeroizes the
        // password on drop; the snapshot redacts the password in Debug).
        let (is_restore, backup_hex, password, is_full) = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Backup(snap)) => (
                snap.restore_mode,
                snap.restore_data,
                snap.password,
                snap.full_level,
            ),
            other => {
                tracing::warn!(?other, "execute_backup without Backup output");
                (false, String::new(), String::new(), true)
            }
        };

        if is_restore {
            // Restore from a pasted/typed backup blob (keyboard frontends).
            // Mobile's file-picker restore runs through `execute_backup_restore`
            // instead and never reaches here.
            if backup_hex.is_empty() || password.is_empty() {
                self.engine.processing_failed();
                return ActionResult::NavigateTo(self.engine.current_screen());
            }
            return match self.vauchi.import_full_backup(&backup_hex, &password) {
                Ok(()) => {
                    self.engine.processing_complete();
                    ActionResult::NavigateTo(self.engine.current_screen())
                }
                Err(_) => {
                    self.engine.processing_failed();
                    ActionResult::NavigateTo(self.engine.current_screen())
                }
            };
        }

        // `is_full` now tracks the engine's live level toggle directly, so a
        // retry after a failed export uses the level the user still sees on
        // screen (the old AppEngine copy was reset to Full on failure, which
        // could silently export Full after an identity-only selection).
        if password.is_empty() {
            self.engine.processing_failed();
            return ActionResult::NavigateTo(self.engine.current_screen());
        }

        let result = if is_full {
            self.vauchi.export_full_backup(&password)
        } else {
            self.vauchi.export_backup(&password)
        };

        match result {
            Ok(data) => {
                self.engine.processing_complete();
                ActionResult::BackupExportComplete { data }
            }
            Err(_) => {
                self.engine.processing_failed();
                ActionResult::NavigateTo(self.engine.current_screen())
            }
        }
    }

    /// Apply a per-row contact mutation triggered by `ContactListEngine`.
    ///
    /// Calls the matching `Vauchi` op, invalidates the contact-list cache so
    /// the next screen read reflects the new state, then emits a
    /// `ShowToast` carrying an `undo_action_id` for reversible mutations.
    /// The undo id is a `undo_<verb>_contact:<contact_id>` string that
    /// `AppEngine::handle_undo` already matches on — see
    /// `intercept.rs::handle_undo`.
    pub(super) fn apply_contact_action(
        &mut self,
        contact_id: &str,
        kind: ContactActionKind,
    ) -> ActionResult {
        // Invalidate the list cache regardless of outcome so a failed op
        // still yields a fresh screen read.
        self.engine_cache.remove(&AppScreen::Contacts);
        self.engine_cache.remove(&AppScreen::ArchivedContacts);
        // 2026-05-21 silent-failures sweep: each arm matches the
        // mutation Result. On Ok we emit ShowToast with the documented
        // success copy + optional undo id; on Err we surface ShowAlert
        // so the user sees the DB/CEK failure instead of a fake success.
        match kind {
            ContactActionKind::Archive => match self.vauchi.archive_contact(contact_id) {
                Ok(()) => {
                    self.pending_contact_undo = Some(super::PendingContactUndo::Archive);
                    ActionResult::ShowToast {
                        message: "Contact archived".into(),
                        undo_action_id: Some(format!("undo_archive_contact:{contact_id}")),
                    }
                }
                Err(e) => ActionResult::ShowAlert {
                    title: "Archive Failed".into(),
                    message: format!("{e}"),
                },
            },
            ContactActionKind::Unarchive => match self.vauchi.unarchive_contact(contact_id) {
                Ok(()) => ActionResult::ShowToast {
                    message: "Contact unarchived".into(),
                    undo_action_id: None,
                },
                Err(e) => ActionResult::ShowAlert {
                    title: "Unarchive Failed".into(),
                    message: format!("{e}"),
                },
            },
            ContactActionKind::Hide => match self.vauchi.hide_contact(contact_id) {
                Ok(()) => ActionResult::ShowToast {
                    message: "Contact hidden".into(),
                    undo_action_id: Some(format!("undo_hide_contact:{contact_id}")),
                },
                Err(e) => ActionResult::ShowAlert {
                    title: "Hide Failed".into(),
                    message: format!("{e}"),
                },
            },
            ContactActionKind::Unhide => match self.vauchi.unhide_contact(contact_id) {
                Ok(()) => ActionResult::ShowToast {
                    message: "Contact unhidden".into(),
                    undo_action_id: None,
                },
                Err(e) => ActionResult::ShowAlert {
                    title: "Unhide Failed".into(),
                    message: format!("{e}"),
                },
            },
            ContactActionKind::Delete => match self.vauchi.soft_delete_imported_contact(contact_id)
            {
                Ok(()) => ActionResult::ShowToast {
                    message: "Contact deleted".into(),
                    undo_action_id: Some(format!("undo_delete_contact:{contact_id}")),
                },
                Err(e) => ActionResult::ShowAlert {
                    title: "Delete Failed".into(),
                    message: format!("{e}"),
                },
            },
            ContactActionKind::Undelete => {
                match self.vauchi.undo_delete_imported_contact(contact_id) {
                    Ok(()) => ActionResult::ShowToast {
                        message: "Contact restored".into(),
                        undo_action_id: None,
                    },
                    Err(e) => ActionResult::ShowAlert {
                        title: "Restore Failed".into(),
                        message: format!("{e}"),
                    },
                }
            }
        }
    }
}

/// Format the toast message shown after a vCard import completes.
///
/// Mirrors the existing macOS / iOS sheet copy so frontends that adopt
/// the new core-driven flow render the same wording. Pluralization
/// uses simple English rules — proper i18n is part of D3 (post-MVP).
fn format_import_toast(imported: usize, skipped: usize) -> String {
    let imported_word = if imported == 1 { "contact" } else { "contacts" };
    if skipped == 0 {
        format!("Imported {} {}", imported, imported_word)
    } else {
        let skipped_word = if skipped == 1 {
            "duplicate"
        } else {
            "duplicates"
        };
        format!(
            "Imported {} {} ({} {} skipped)",
            imported, imported_word, skipped, skipped_word
        )
    }
}

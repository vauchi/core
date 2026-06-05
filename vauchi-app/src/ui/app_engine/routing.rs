// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Result routing for `AppEngine` — completion handling, hardware events,
//! and action result dispatch.

use super::AppEngine;
use super::AppScreen;
use crate::ui::ScreenModel;
use crate::ui::action::{ActionResult, ContactActionKind, PostOnboardingDestination, UserAction};
use crate::ui::engine::WorkflowEngine;
use crate::ui::form_dialog::FormDialogType;
use vauchi_core::Event;
use vauchi_core::contact_card::FieldType;

impl AppEngine {
    /// Returns `true` if the current engine has user-entered data that differs
    /// from the original. Used by frontends to show a "discard changes?" prompt.
    pub fn form_has_data(&self) -> bool {
        let dialog_type = match &self.screen {
            AppScreen::FormDialog { dialog_type } => dialog_type,
            _ => return false,
        };
        let input = match self.engine.collected_input() {
            Some(v) => v,
            None => return false,
        };
        match dialog_type {
            FormDialogType::AddField { .. } => {
                // Format: "type\nlabel\nvalue\nnote\ngroups"
                let parts: Vec<&str> = input.splitn(5, '\n').collect();
                if parts.len() >= 3 {
                    let label = parts.get(1).unwrap_or(&"").trim();
                    let value = parts.get(2).unwrap_or(&"").trim();
                    !label.is_empty() || !value.is_empty()
                } else {
                    false
                }
            }
            FormDialogType::EditField {
                current_value,
                current_note,
                ..
            } => {
                // Format: "value\nnote"
                let mut parts = input.splitn(2, '\n');
                let value = parts.next().unwrap_or("");
                let note = parts.next().unwrap_or("");
                value != current_value.as_str() || note != current_note.as_deref().unwrap_or("")
            }
            FormDialogType::EditName { current_name } => input != *current_name,
            FormDialogType::EditRelayUrl { current_url } => input != *current_url,
            FormDialogType::CreateGroup => !input.is_empty(),
            FormDialogType::RenameGroup { current_name, .. } => input != *current_name,
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
    pub fn handle_hardware_event(&mut self, event: Event) -> Option<ActionResult> {
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
                if let Some(eng) = self
                    .engine
                    .as_any_mut()
                    .and_then(|a| a.downcast_mut::<crate::ui::onboarding::OnboardingEngine>())
                {
                    eng.set_pending_backup_bytes(bytes);
                    return Some(ActionResult::NavigateTo(eng.current_screen()));
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
                if let Some(eng) = self
                    .engine
                    .as_any_mut()
                    .and_then(|a| a.downcast_mut::<crate::ui::onboarding::OnboardingEngine>())
                {
                    eng.set_pending_backup_bytes(bytes);
                    return Some(ActionResult::NavigateTo(eng.current_screen()));
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
            && let Some(eng) = self
                .engine
                .as_any_mut()
                .and_then(|a| a.downcast_mut::<crate::ui::onboarding::OnboardingEngine>())
            && eng.current_step() == vauchi_core::types::OnboardingStep::BackupPasswordEntry
            && let Some((bytes, password)) = eng.take_pending_backup()
        {
            return self.execute_backup_restore(bytes, password);
        }

        match &self.screen {
            AppScreen::Onboarding => {
                let name = match self.pending_display_name.take() {
                    Some(n) if !n.trim().is_empty() => n,
                    _ => {
                        return ActionResult::ValidationError {
                            component_id: "display_name".into(),
                            message: "Please enter a display name".into(),
                        };
                    }
                };
                // Extract data from the active onboarding engine before identity
                // creation (engine will be discarded after navigating away).
                let onboarding_engine = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::onboarding::OnboardingEngine>());
                let onboarding_groups: Vec<String> = onboarding_engine
                    .map(|ob| {
                        ob.onboarding_data()
                            .selected_groups
                            .iter()
                            .filter(|g| g.selected)
                            .map(|g| g.name.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                // Slice 32c S2: also pull the fields collected during the
                // ContactInfo step. `OnboardingEngine::sync_quick_add_fields`
                // (onboarding.rs:855) pushes phone/email values into
                // `OnboardingData.fields[]` on the "continue" press;
                // these were silently dropped before this slice. Only
                // `shown == true` entries persist — `shown == false`
                // marks user-skipped inputs.
                let onboarding_fields: Vec<crate::ui::onboarding::FieldSetup> = onboarding_engine
                    .map(|ob| {
                        ob.onboarding_data()
                            .fields
                            .iter()
                            .filter(|f| f.shown)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                // Use the atomic helper so identity creation +
                // onboarding-complete flag land in one call. Closes
                // the crash window the audit
                // `2026-04-28-app-launch-and-identity-orchestration-in-core`
                // §2.5 calls out — a kill between the two writes
                // used to leave the next launch in a state where
                // identity exists but onboarding is "incomplete".
                match self.vauchi.create_identity_with_onboarding(&name) {
                    Ok(()) => {
                        // Persist onboarding groups — best-effort:
                        // partial-groups failure is recoverable; user can
                        // create missing groups from Settings (slice 32c S2)
                        for group_name in &onboarding_groups {
                            #[allow(clippy::let_underscore_must_use)]
                            let _ = self.vauchi.create_group(group_name);
                        }
                        // Slice 32c S2: persist onboarding fields
                        // (phone, email collected during ContactInfo).
                        // `FieldType::from_alias` resolves "phone" /
                        // "email" / "twitter" etc. to the typed enum
                        // with an optional label override; unknown
                        // aliases fall back to `FieldType::Custom`
                        // (mirrors the entry-detail pattern at
                        // routing.rs:511-520). Errors are swallowed
                        // per the existing groups-loop convention —
                        // partial-fields failure is recoverable: the
                        // user can add missing fields manually from
                        // MyInfo. Crash-resume semantics covered in
                        // slice 32c S3.
                        let now = self.vauchi.clock().unix_seconds();
                        for setup in &onboarding_fields {
                            let (field_type, alias_label) =
                                FieldType::from_alias(&setup.field_type)
                                    .unwrap_or((FieldType::Custom, None));
                            let label = alias_label.unwrap_or_else(|| setup.label.clone());
                            let field = vauchi_core::contact_card::ContactField::new(
                                field_type,
                                &label,
                                &setup.value,
                                now,
                            );
                            // best-effort: partial-fields failure is
                            // recoverable; user can add missing fields
                            // manually from MyInfo
                            #[allow(clippy::let_underscore_must_use)]
                            let _ = self.vauchi.add_own_field(field);
                        }
                        let target = AppScreen::MyInfo;
                        let screen = self.navigate_to_internal(target);
                        ActionResult::NavigateTo(screen)
                    }
                    Err(e) => ActionResult::ShowAlert {
                        title: "Error".into(),
                        message: format!("Failed to create identity: {e}"),
                    },
                }
            }
            AppScreen::Lock => {
                let pin = match self.engine.collected_input() {
                    Some(p) => p,
                    None => {
                        return ActionResult::ValidationError {
                            component_id: "pin".into(),
                            message: "Please enter your password".into(),
                        };
                    }
                };
                match self.vauchi.authenticate(&pin) {
                    Ok(_auth_mode) => {
                        let screen = self.navigate_to_internal(AppScreen::MyInfo);
                        ActionResult::NavigateTo(screen)
                    }
                    Err(_) => {
                        // Notify lock engine of failed auth so it tracks attempts
                        // and clears the entered PIN.
                        self.engine.handle_action(UserAction::ActionPressed {
                            action_id: "auth_failed".into(),
                        })
                    }
                }
            }
            AppScreen::Exchange => {
                // ADR-031: Extract exchange result BEFORE navigate_to_internal
                // replaces the engine (navigation.rs:34 does std::mem::replace).
                let exchange_data = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::exchange::ExchangeEngine>())
                    .and_then(|ex| {
                        let groups = ex.selected_groups().to_vec();
                        // QR path: contact is in session.state() → Complete { contact }.
                        // Build the ratchet here while `session` is in scope; it owns
                        // the correct role + exchange-key selection (see
                        // ExchangeSession::build_exchange_ratchet).
                        let session = ex.session()?;
                        if let vauchi_core::exchange::ExchangeState::Complete { contact } =
                            session.state()
                        {
                            let contact = (**contact).clone();
                            let ratchet = session.build_exchange_ratchet(&contact);
                            return Some((contact, ratchet, groups));
                        }
                        None
                    });

                let screen = self.navigate_to_internal(AppScreen::Contacts);

                // Persist exchange result: upsert contact + init ratchet + assign groups
                if let Some((contact, ratchet, groups)) = exchange_data {
                    let contact_id = contact.id().to_string();
                    if let Err(e) = self.vauchi.update_contact(&contact) {
                        return ActionResult::ShowAlert {
                            title: "Exchange Error".into(),
                            message: format!("Failed to save contact: {e}"),
                        };
                    }
                    match ratchet {
                        Ok((ratchet, is_initiator)) => {
                            if let Err(e) = self.vauchi.save_exchange_ratchet(
                                &contact_id,
                                &ratchet,
                                is_initiator,
                            ) {
                                return ActionResult::ShowAlert {
                                    title: "Exchange Error".into(),
                                    message: format!("Failed to initialize encryption: {e}"),
                                };
                            }
                        }
                        Err(e) => {
                            return ActionResult::ShowAlert {
                                title: "Exchange Error".into(),
                                message: format!("Failed to initialize encryption: {e}"),
                            };
                        }
                    }
                    for group_id in &groups {
                        // best-effort: group assignment after exchange;
                        // failures don't block the exchange itself
                        #[allow(clippy::let_underscore_must_use)]
                        let _ = self.vauchi.add_contact_to_group(group_id, &contact_id);
                    }
                }

                ActionResult::NavigateTo(screen)
            }
            // Fix A of `2026-06-02-exchange-back-cancel-broken`: the
            // core-driven multi-stage screen returns `Complete` on both
            // Cancel and Done. Without an explicit arm this fell to the
            // catch-all `navigate_back`, which popped an unstamped target
            // and produced an empty `screen_id` → a white screen on
            // device. Route to a deterministic, stamped destination:
            // Done (success — the contact was already persisted on the
            // `Finalized` event, see app_engine/multi_stage_exchange.rs)
            // lands on Contacts; Cancel returns to the mode picker.
            AppScreen::MultiStageExchange { .. } => {
                let cancelled = self
                    .engine
                    .as_any()
                    .and_then(|a| {
                        a.downcast_ref::<crate::ui::multi_stage_exchange::MultiStageExchangeEngine>(
                        )
                    })
                    .map(|ms| ms.was_cancelled())
                    .unwrap_or(false);
                let target = if cancelled {
                    AppScreen::Exchange
                } else {
                    AppScreen::Contacts
                };
                let screen = self.navigate_to_internal(target);
                ActionResult::NavigateTo(screen)
            }
            AppScreen::ContactVisibility { contact_id } => {
                if let Some(input) = self.engine.collected_input() {
                    // Parse "field_id:visible,field_id:hidden,..." and persist
                    let contact_id = contact_id.clone();
                    for pair in input.split(',') {
                        let mut parts = pair.splitn(2, ':');
                        if let (Some(field_id), Some(state)) = (parts.next(), parts.next()) {
                            let should_show = state == "visible";
                            let is_visible = self
                                .vauchi
                                .get_effective_field_visibility(&contact_id, field_id)
                                .unwrap_or(true);
                            if should_show != is_visible {
                                // best-effort: visibility toggle is idempotent;
                                // a failure leaves the row in its prior state
                                // and the user can retry from the same screen
                                #[allow(clippy::let_underscore_must_use)]
                                let _ = self.vauchi.toggle_field_visibility(&contact_id, field_id);
                            }
                        }
                    }
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::VerifyFingerprint { contact_id } => {
                use crate::ui::fingerprint_verify::VerifyAction;
                let fp_engine = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::FingerprintVerifyEngine>());
                if let Some(fp_engine) = fp_engine {
                    match fp_engine.completion_action() {
                        VerifyAction::Verified => {
                            if let Err(e) = self.vauchi.verify_contact_fingerprint(contact_id) {
                                return ActionResult::ShowAlert {
                                    title: "Verification Failed".into(),
                                    message: format!("{e}"),
                                };
                            }
                        }
                        VerifyAction::Unverified => {
                            if let Err(e) = self.vauchi.unverify_contact_fingerprint(contact_id) {
                                return ActionResult::ShowAlert {
                                    title: "Verification Failed".into(),
                                    message: format!("{e}"),
                                };
                            }
                        }
                        VerifyAction::None => {}
                    }
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::EmergencyShred => {
                let screen = self.navigate_to_internal(AppScreen::Onboarding);
                ActionResult::NavigateTo(screen)
            }
            AppScreen::EmergencyBroadcast => {
                use crate::ui::{EmergencyBroadcastEngine, EmergencyOutcome};
                // Read everything off the engine into owned values first so the
                // immutable engine borrow is released before the mutating
                // `vauchi` calls below.
                let eng = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<EmergencyBroadcastEngine>());
                let plan = eng.map(|e| {
                    (
                        e.outcome().cloned(),
                        e.contact_ids(),
                        e.message().to_string(),
                        e.include_location(),
                    )
                });
                let Some((outcome, ids, message, include_location)) = plan else {
                    let screen = self.navigate_back();
                    return ActionResult::NavigateTo(screen);
                };
                match outcome {
                    Some(EmergencyOutcome::Save) => {
                        if let Err(e) = self.vauchi.configure_emergency_broadcast(
                            ids,
                            message,
                            include_location,
                        ) {
                            return ActionResult::ShowAlert {
                                title: "Error".into(),
                                message: format!("Failed to save emergency broadcast: {e}"),
                            };
                        }
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                    Some(EmergencyOutcome::Send) => match self.vauchi.send_emergency_broadcast() {
                        Ok(result) => {
                            let _ = self.navigate_back();
                            ActionResult::ShowToast {
                                message: format!(
                                    "Emergency alert sent to {}/{} contacts",
                                    result.sent, result.total
                                ),
                                undo_action_id: None,
                            }
                        }
                        Err(e) => ActionResult::ShowAlert {
                            title: "Send failed".into(),
                            message: format!("{e}"),
                        },
                    },
                    Some(EmergencyOutcome::Disable) => {
                        if let Err(e) = self.vauchi.delete_emergency_config() {
                            return ActionResult::ShowAlert {
                                title: "Error".into(),
                                message: format!("Failed to disable emergency broadcast: {e}"),
                            };
                        }
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                    None => {
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                }
            }
            AppScreen::Privacy => {
                // GdprEngine returns "export" or "delete" via collected_input().
                // The actual API calls happen in the platform layer (UniFFI/CABI);
                // here we just navigate back and show feedback.
                let action = self.engine.collected_input().unwrap_or_default();
                match action.as_str() {
                    "export" => match vauchi_core::api::export_all_data(self.vauchi.storage()) {
                        Ok(export) => match serde_json::to_string_pretty(&export) {
                            Ok(json) => ActionResult::GdprExportComplete { json },
                            Err(_) => ActionResult::ShowToast {
                                message: "Export failed: could not serialize data.".into(),
                                undo_action_id: None,
                            },
                        },
                        Err(_) => ActionResult::ShowToast {
                            message: "Export failed: could not read data.".into(),
                            undo_action_id: None,
                        },
                    },
                    "delete" => match vauchi_core::api::DeletionManager::new(self.vauchi.storage())
                        .schedule_deletion()
                    {
                        Ok(_) => {
                            let _ = self.navigate_back();
                            // Rebuild Privacy fresh on revisit so it shows the
                            // now-scheduled state (cancel action) instead of the
                            // cached ConfirmDelete sub-step.
                            self.engine_cache.remove(&AppScreen::Privacy);
                            ActionResult::ShowToast {
                                message: "Identity deletion scheduled. You have 7 days to cancel."
                                    .into(),
                                undo_action_id: None,
                            }
                        }
                        Err(_) => ActionResult::ShowToast {
                            message: "Could not schedule deletion.".into(),
                            undo_action_id: None,
                        },
                    },
                    "cancel_deletion" => {
                        match vauchi_core::api::DeletionManager::new(self.vauchi.storage())
                            .cancel_deletion()
                        {
                            Ok(_) => {
                                let _ = self.navigate_back();
                                self.engine_cache.remove(&AppScreen::Privacy);
                                ActionResult::ShowToast {
                                    message: "Identity deletion cancelled.".into(),
                                    undo_action_id: None,
                                }
                            }
                            Err(_) => ActionResult::ShowToast {
                                message: "Could not cancel deletion.".into(),
                                undo_action_id: None,
                            },
                        }
                    }
                    "execute" => {
                        // Borrow `identity` only long enough to run the delete;
                        // then the cache can be cleared (all data is gone).
                        let executed = match self.vauchi.identity() {
                            Some(identity) => {
                                vauchi_core::api::DeletionManager::new(self.vauchi.storage())
                                    .execute_deletion(identity)
                                    .is_ok()
                            }
                            None => false,
                        };
                        if executed {
                            self.engine_cache.clear();
                            ActionResult::WipeComplete
                        } else {
                            ActionResult::ShowToast {
                                message: "Could not execute deletion.".into(),
                                undo_action_id: None,
                            }
                        }
                    }
                    "shred" => match self.vauchi.perform_emergency_wipe(true) {
                        Ok(_) => {
                            self.engine_cache.clear();
                            ActionResult::WipeComplete
                        }
                        Err(_) => ActionResult::ShowToast {
                            message: "Emergency wipe failed.".into(),
                            undo_action_id: None,
                        },
                    },
                    _ => {
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                }
            }
            AppScreen::FormDialog { dialog_type } => {
                // Cancel navigates back without saving
                if self.engine.was_cancelled() {
                    let screen = self.navigate_back();
                    return ActionResult::NavigateTo(screen);
                }
                let input = self.engine.collected_input();
                let result = match dialog_type {
                    FormDialogType::EditName { .. } => {
                        let name = input.unwrap_or_default();
                        if name.trim().is_empty() {
                            return ActionResult::ValidationError {
                                component_id: "display_name".into(),
                                message: "Display name cannot be empty".into(),
                            };
                        }
                        self.vauchi.update_display_name(&name)
                    }
                    FormDialogType::EditField { field_id, .. } => {
                        let raw = input.unwrap_or_default();
                        // Format: value\nnote
                        let mut parts = raw.splitn(2, '\n');
                        let value = parts.next().unwrap_or("").to_string();
                        let note = parts.next().unwrap_or("").trim().to_string();
                        let now = self.vauchi.clock().unix_seconds();
                        match self.vauchi.own_card() {
                            Ok(Some(mut card)) => {
                                if let Err(e) = card.update_field_value(field_id, &value, now) {
                                    return ActionResult::ShowAlert {
                                        title: "Error".into(),
                                        message: format!("Failed to update field: {e}"),
                                    };
                                }
                                let note_opt = if note.is_empty() { None } else { Some(note) };
                                if let Err(e) = card.update_field_note(field_id, note_opt) {
                                    return ActionResult::ShowAlert {
                                        title: "Error".into(),
                                        message: format!("Failed to update field note: {e}"),
                                    };
                                }
                                self.vauchi.update_own_card(&card).map(|_| ())
                            }
                            Ok(None) => {
                                return ActionResult::ShowAlert {
                                    title: "Error".into(),
                                    message: "No contact card found".into(),
                                };
                            }
                            Err(e) => Err(e),
                        }
                    }
                    FormDialogType::AddField { .. } => {
                        let raw = input.unwrap_or_default();
                        // Format: type\nlabel\nvalue\nnote\ngroups
                        let mut lines = raw.splitn(5, '\n');
                        let entry_type = lines.next().unwrap_or("custom").trim();
                        let label_input = lines.next().unwrap_or("").trim();
                        let value = lines.next().unwrap_or("").trim();
                        let note = lines.next().unwrap_or("").trim();
                        let _groups = lines.next().unwrap_or("").trim();
                        if value.is_empty() {
                            return ActionResult::ValidationError {
                                component_id: "field_value".into(),
                                message: "Value cannot be empty".into(),
                            };
                        }
                        let field_type = match entry_type {
                            "phone" => FieldType::Phone,
                            "email" => FieldType::Email,
                            "social" => FieldType::Social,
                            s if s.starts_with("social:") => FieldType::Social,
                            "address" => FieldType::Address,
                            "website" => FieldType::Website,
                            "birthday" => FieldType::Birthday,
                            _ => FieldType::Custom,
                        };
                        // Use label_input if provided, otherwise derive from catalog
                        let label = if !label_input.is_empty() {
                            label_input.to_string()
                        } else if let Some(entry) = self.field_catalog.get(entry_type) {
                            entry.display_name.clone()
                        } else {
                            entry_type
                                .chars()
                                .next()
                                .map(|c| c.to_uppercase().to_string() + &entry_type[1..])
                                .unwrap_or_else(|| "Custom".into())
                        };
                        let mut field = vauchi_core::contact_card::ContactField::new(
                            field_type,
                            &label,
                            value,
                            self.vauchi.clock().unix_seconds(),
                        );
                        if !note.is_empty() {
                            field = field.with_note(note.to_string());
                        }
                        let field_id = field.id().to_string();
                        let group_list: Vec<String> = _groups
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        let result = self.vauchi.add_own_field(field);
                        // Apply group visibility from selected groups
                        if result.is_ok() && !group_list.is_empty() {
                            for group_id in &group_list {
                                // best-effort: per-group visibility after
                                // field was added successfully; failures
                                // here are recoverable from group settings
                                #[allow(clippy::let_underscore_must_use)]
                                let _ = self
                                    .vauchi
                                    .set_group_field_visibility(group_id, &field_id, true);
                            }
                        }
                        // During onboarding, also buffer the field in the cached
                        // OnboardingEngine so build_contact_info shows it.
                        if result.is_ok()
                            && let Some(parent) = self.nav_history.last()
                            && matches!(parent, AppScreen::Onboarding)
                            && let Some(engine) = self.engine_cache.get_mut(parent)
                            && let Some(ob) = engine.as_any_mut().and_then(|a| {
                                a.downcast_mut::<crate::ui::onboarding::OnboardingEngine>()
                            })
                        {
                            ob.push_field(crate::ui::onboarding::FieldSetup {
                                field_type: entry_type.to_string(),
                                label: label.clone(),
                                value: value.to_string(),
                                visible_to_groups: group_list,
                                shown: true,
                            });
                        }
                        result
                    }
                    FormDialogType::CreateGroup => {
                        let name = input.unwrap_or_default();
                        if name.trim().is_empty() {
                            return ActionResult::ValidationError {
                                component_id: "group_name".into(),
                                message: "Group name cannot be empty".into(),
                            };
                        }
                        self.vauchi.create_group(name.trim()).map(|_| ())
                    }
                    FormDialogType::RenameGroup { group_id, .. } => {
                        let name = input.unwrap_or_default();
                        if name.trim().is_empty() {
                            return ActionResult::ValidationError {
                                component_id: "group_name".into(),
                                message: "Group name cannot be empty".into(),
                            };
                        }
                        self.vauchi.rename_group(group_id, name.trim())
                    }
                    FormDialogType::EditRelayUrl { .. } => {
                        // Relay URL is TUI-specific config (Backend), not in Vauchi.
                        // Navigate back; TUI handles save via Backend::set_relay_url.
                        Ok(())
                    }
                };
                match result {
                    Ok(()) => {
                        // Invalidate parent screen cache so it refreshes with updated data.
                        // Exception: don't invalidate Onboarding — its state machine
                        // (step, groups, name) must survive FormDialog round-trips.
                        // The onboarding ContactInfo screen syncs fields from storage.
                        if let Some(parent) = self.nav_history.last()
                            && !matches!(parent, AppScreen::Onboarding)
                        {
                            self.engine_cache.remove(parent);
                        }
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                    Err(e) => ActionResult::ShowAlert {
                        title: "Error".into(),
                        message: format!("{e}"),
                    },
                }
            }
            AppScreen::Sync => {
                let action = self.engine.collected_input().unwrap_or_default();
                match action.as_str() {
                    "sync_now" => {
                        let pending = self.vauchi.pending_update_count().unwrap_or(0);
                        if pending == 0 {
                            ActionResult::ShowToast {
                                message: "Already up to date".into(),
                                undo_action_id: None,
                            }
                        } else {
                            ActionResult::ShowToast {
                                message: format!("{pending} update(s) queued for sync"),
                                undo_action_id: None,
                            }
                        }
                    }
                    "test_connection" => ActionResult::ShowToast {
                        message: "Connection check initiated".into(),
                        undo_action_id: None,
                    },
                    _ => {
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                }
            }
            AppScreen::ChangePassword => {
                let cp_engine = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::ChangePasswordEngine>());
                if let Some(engine) = cp_engine {
                    let current = engine.current_password().to_string();
                    let new = engine.new_password().to_string();
                    // An empty current_password reaches here only on Cancel
                    // (the Save button stays disabled until both fields are
                    // populated and matching).  Treat empty current as a
                    // cancel — navigate back without touching storage.
                    if !current.is_empty()
                        && !new.is_empty()
                        && let Err(e) = self.vauchi.change_app_password(&current, &new)
                    {
                        return ActionResult::ShowAlert {
                            title: "Error".into(),
                            message: format!("Could not change password: {e}"),
                        };
                    }
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::DuressPin => {
                let dp_engine = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::DuressPinEngine>());
                if let Some(dp_engine) = dp_engine {
                    let config = dp_engine.config();
                    if config.enabled {
                        let pin = dp_engine.pin();
                        if let Err(e) = self.vauchi.setup_duress_password(pin) {
                            return ActionResult::ShowAlert {
                                title: "Error".into(),
                                message: format!("Failed to set duress PIN: {e}"),
                            };
                        }
                        let settings = vauchi_core::types::DuressSettings {
                            alert_contact_ids: config
                                .alert_contacts
                                .iter()
                                .map(|c| c.id.clone())
                                .collect(),
                            alert_message: config.alert_message.clone(),
                            include_location: config.include_location,
                        };
                        if let Err(e) = self.vauchi.save_duress_settings(&settings) {
                            return ActionResult::ShowAlert {
                                title: "Error".into(),
                                message: format!("Failed to save duress settings: {e}"),
                            };
                        }
                    } else if let Err(e) = self.vauchi.disable_duress() {
                        return ActionResult::ShowAlert {
                            title: "Error".into(),
                            message: format!("Failed to disable duress: {e}"),
                        };
                    }
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::DeviceManagement => {
                // Read the confirmed index from the engine before navigating away
                let revoke_index = self
                    .engine
                    .as_any()
                    .and_then(|a| {
                        a.downcast_ref::<crate::ui::device_management::DeviceManagementEngine>()
                    })
                    .and_then(|e| e.confirmed_revoke_index());

                if let Some(idx) = revoke_index {
                    match self.vauchi.revoke_device(idx as usize) {
                        Ok(_name) => {
                            // Refresh the device list after revocation
                            let screen = self.navigate_to_internal(AppScreen::DeviceManagement);
                            return ActionResult::NavigateTo(screen);
                        }
                        Err(e) => {
                            return ActionResult::ShowAlert {
                                title: "Revoke Failed".into(),
                                message: format!("{e}"),
                            };
                        }
                    }
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::ContactDetail { contact_id } => {
                // InlineConfirm → hard delete the imported contact and navigate back.
                // best-effort: plain "back" also routes through this
                // completion handler (no pending-confirm flag yet), so a
                // "not found" / non-imported contact is expected and we
                // navigate-back regardless. Propagating would force every
                // plain back press to surface ShowAlert — the user-intent
                // gate belongs in the InlineConfirm engine, not here.
                let contact_id = contact_id.clone();
                #[allow(clippy::let_underscore_must_use)]
                let _ = self.vauchi.hard_delete_imported_contact(&contact_id);
                self.engine_cache.remove(&AppScreen::Contacts);
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::GroupDetail { group_id } => {
                let group_id = group_id.clone();
                if let Err(e) = self.vauchi.delete_group(&group_id) {
                    return ActionResult::ShowAlert {
                        title: "Delete Group Failed".into(),
                        message: format!("{e}"),
                    };
                }
                self.engine_cache.remove(&AppScreen::Groups);
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::Groups => {
                // Group deletion is confirmed on GroupDetail (the arm above),
                // where the target group is unambiguous. The Groups list no
                // longer offers a list-level delete, so it never issues a
                // Complete here; kept as a defensive navigate-back.
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::ContactMerge { .. } => {
                match self.pending_merge.take() {
                    Some((primary_id, secondary_id)) => {
                        match self.vauchi.merge_contacts(&primary_id, &secondary_id) {
                            Ok(_merged) => {
                                self.engine_cache.remove(&AppScreen::Contacts);
                                self.engine_cache.remove(&AppScreen::ContactDuplicates);
                                // Navigate back to ContactDuplicates (or wherever we came from).
                                // navigate_back() mutates nav state; ShowToast causes the
                                // frontend to re-query current_screen() for the updated view.
                                self.navigate_back();
                                ActionResult::ShowToast {
                                    message: "Contacts merged".into(),
                                    undo_action_id: None,
                                }
                            }
                            Err(e) => ActionResult::ShowAlert {
                                title: "Merge Failed".into(),
                                message: format!("{e}"),
                            },
                        }
                    }
                    None => {
                        // No pending merge state — just navigate back
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                }
            }
            AppScreen::AvatarEditor => {
                if self.engine.was_cancelled() {
                    let screen = self.navigate_back();
                    return ActionResult::NavigateTo(screen);
                }
                let editor = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::avatar_editor::AvatarEditorEngine>());
                if let Some(editor) = editor {
                    if editor.avatar_removed() {
                        // Clear avatar from own card
                        if let Ok(Some(mut card)) = self.vauchi.own_card() {
                            card.clear_avatar();
                            if let Err(e) = self.vauchi.update_own_card(&card) {
                                return ActionResult::ShowAlert {
                                    title: "Avatar Update Failed".into(),
                                    message: format!("{e}"),
                                };
                            }
                        }
                    } else if let Some(avatar) = editor.result_avatar() {
                        // Persist the new avatar
                        if let Ok(Some(mut card)) = self.vauchi.own_card() {
                            if let Err(e) = card.set_avatar(avatar.to_vec()) {
                                return ActionResult::ShowAlert {
                                    title: "Avatar Update Failed".into(),
                                    message: format!("{e}"),
                                };
                            }
                            if let Err(e) = self.vauchi.update_own_card(&card) {
                                return ActionResult::ShowAlert {
                                    title: "Avatar Update Failed".into(),
                                    message: format!("{e}"),
                                };
                            }
                        }
                    }
                }
                self.invalidate_screen(&AppScreen::MyInfo);
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::DeviceReplacement => {
                if self.engine.was_cancelled() {
                    let screen = self.navigate_back();
                    return ActionResult::NavigateTo(screen);
                }
                // Check if user chose to decommission old device
                let outcome = self
                    .engine
                    .as_any()
                    .and_then(|a| {
                        a.downcast_ref::<crate::ui::device_replacement::DeviceReplacementEngine>()
                    })
                    .map(|e| e.completion_outcome().clone());
                if let Some(crate::ui::device_replacement::CompletionOutcome::RemoveOldDevice) =
                    outcome
                {
                    // Delegate to existing device management unlink
                    // (current device index = 0, handled by the platform layer)
                    self.navigate_back();
                    return ActionResult::ShowToast {
                        message: "Device removal requested. Complete in Settings > Devices.".into(),
                        undo_action_id: None,
                    };
                }
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
            AppScreen::DeepLinkConsent { payload } => {
                // Phase 1 T7 of `2026-04-27-deep-link-responder-flow`:
                // grant → DeepLinkResponder, deny / cancel → back. Read
                // the consent decision off the engine before
                // navigate_back / navigate_to_internal replaces it.
                let granted = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::DeepLinkConsentEngine>())
                    .map(|e| matches!(e.decision(), crate::ui::ConsentDecision::Granted))
                    .unwrap_or(false);

                if granted {
                    let payload = payload.clone();
                    let screen =
                        self.navigate_to_internal(AppScreen::DeepLinkResponder { payload });
                    ActionResult::NavigateTo(screen)
                } else {
                    let screen = self.navigate_back();
                    ActionResult::NavigateTo(screen)
                }
            }
            _ => {
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
        }
    }

    /// Route engine results to appropriate navigation targets.
    pub(super) fn route_result(&mut self, result: ActionResult) -> ActionResult {
        match result {
            ActionResult::ContactAction { contact_id, kind } => {
                self.apply_contact_action(&contact_id, kind)
            }
            ActionResult::Complete => self.handle_completion(),
            ActionResult::CompleteWith { destination } => {
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
                let screen = self.navigate_to(target);
                ActionResult::NavigateTo(screen)
            }
            ActionResult::EditContact { contact_id } => {
                let screen = self.navigate_to(AppScreen::ContactEdit { contact_id });
                ActionResult::NavigateTo(screen)
            }
            // DeviceManagementEngine emits StartDeviceLink when the user
            // taps "Link New Device". The link flow is fully core-driven
            // (`DeviceLinkingEngine` shows the QR + handles verify code +
            // sync), so route the user straight there. Onboarding and
            // DeviceReplacement also emit StartDeviceLink, but those
            // happen on screens with their own existing native flows
            // — we leave their results untouched so frontends keep
            // calling `viewModel.startDeviceLinkInitiator()` /
            // equivalent during onboarding.
            ActionResult::StartDeviceLink if self.screen == AppScreen::DeviceManagement => {
                let screen = self.navigate_to(AppScreen::DeviceLinking);
                ActionResult::NavigateTo(screen)
            }
            // Pair 4 — `ExchangeEngine` emits StartMultiStageExchange when
            // the user picks `ExchangeMode::Glance` from the mode list.
            // Phase 1.E of `2026-05-11-hover-graduation-plan.md` extended
            // the handoff to `ExchangeMode::Hover`. Route to the
            // dedicated screen so `PlatformAppEngine` can auto-create
            // the `MobileMultiStageSession` on entry. The `mode`
            // payload threads down to the screen-factory at
            // `screens.rs:872` so it can pick `new_hover()` vs
            // `new_glance()`. The frontend never decides any of this
            // — picking a mode is a user-action, the rest is core's
            // responsibility.
            ActionResult::StartMultiStageExchange { mode } => {
                // Carry the group-selection preamble's choice across the
                // engine handoff (navigate_to replaces the ExchangeEngine)
                // so persist_exchanged_contact can assign the new contact
                // + show the group on the success screen
                // (2026-06-04-exchange-terminal-screens).
                self.pending_exchange_groups = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::exchange::ExchangeEngine>())
                    .map(|ex| ex.selected_groups().to_vec())
                    .unwrap_or_default();
                let screen = self.navigate_to(AppScreen::MultiStageExchange { mode });
                ActionResult::NavigateTo(screen)
            }
            // Slice 32l Phase 3: ExchangeEngine emits StartLinkExchange when
            // the user picks link mode; navigate to the dedicated
            // LinkExchange screen whose engine-owned initiator drives the
            // relay-escrow handshake. Replaces the legacy ExchangeStep::Link.
            ActionResult::StartLinkExchange => {
                let screen = self.navigate_to(AppScreen::LinkExchange);
                ActionResult::NavigateTo(screen)
            }
            // BLE graduation slice 2: ExchangeEngine emits StartBleExchange when
            // the user picks Magic/Bump/Shake; navigate to the dedicated
            // BleExchange screen whose BleExchangeEngine drives the flow.
            ActionResult::StartBleExchange { mode } => {
                self.pending_exchange_groups = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::exchange::ExchangeEngine>())
                    .map(|ex| ex.selected_groups().to_vec())
                    .unwrap_or_default();
                let screen = self.navigate_to(AppScreen::BleExchange { mode });
                ActionResult::NavigateTo(screen)
            }
            // NFC graduation: ExchangeEngine emits StartNfcExchange when the
            // user picks TapTap; NfcExchangeEngine also emits it on retry (a
            // fresh engine re-provisions the consumed identity). Navigate to
            // the dedicated NfcExchange screen.
            ActionResult::StartNfcExchange => {
                self.pending_exchange_groups = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::exchange::ExchangeEngine>())
                    .map(|ex| ex.selected_groups().to_vec())
                    .unwrap_or_default();
                let screen = self.navigate_to(AppScreen::NfcExchange);
                ActionResult::NavigateTo(screen)
            }
            // Cable graduation: ExchangeEngine emits StartDirectTransport when
            // the user picks Cable; DirectTransportEngine also emits it on retry
            // (a fresh engine re-provisions the consumed identity). Navigate to
            // the dedicated DirectTransport screen.
            ActionResult::StartDirectTransport => {
                self.pending_exchange_groups = self
                    .engine
                    .as_any()
                    .and_then(|a| a.downcast_ref::<crate::ui::exchange::ExchangeEngine>())
                    .map(|ex| ex.selected_groups().to_vec())
                    .unwrap_or_default();
                let screen = self.navigate_to(AppScreen::DirectTransport);
                ActionResult::NavigateTo(screen)
            }
            ActionResult::OpenEntryDetail { field_id } => {
                let screen = self.navigate_to(AppScreen::MyInfoEntryDetail { field_id });
                ActionResult::NavigateTo(screen)
            }
            ActionResult::VerifyFingerprint { contact_id } => {
                let screen = self.navigate_to(AppScreen::VerifyFingerprint { contact_id });
                ActionResult::NavigateTo(screen)
            }
            // MoreEngine reuses OpenContact to signal menu selection.
            // Route to the target screen when on the More screen.
            ActionResult::OpenContact { contact_id } if self.screen == AppScreen::More => {
                match AppScreen::from_screen_id(&contact_id) {
                    Some(target) => {
                        let screen = self.navigate_to(target);
                        ActionResult::NavigateTo(screen)
                    }
                    None => ActionResult::UpdateScreen(self.engine.current_screen()),
                }
            }
            // GroupsEngine reuses OpenContact to signal group selection.
            // Route to GroupDetail when on the Groups screen.
            ActionResult::OpenContact { contact_id } if self.screen == AppScreen::Groups => {
                let screen = self.navigate_to(AppScreen::GroupDetail {
                    group_id: contact_id,
                });
                ActionResult::NavigateTo(screen)
            }
            // General contact open (contacts list, social graph, delivery,
            // activity log) — the More/Groups guards above intercept their
            // reuse of OpenContact first, so this only sees a genuine
            // "open this contact". Core resolves the navigation and hands
            // back a generic NavigateTo(ScreenModel): the frontend renders
            // whatever screen it receives and never has to know that
            // `open_contact` maps to a `contact_detail` screen. Without
            // this arm the result shipped raw and each frontend had to
            // domain-map it — the mobile mapping was broken, navigating to
            // My Card (problem 2026-05-25-contact-tap-opens-own-card). This
            // keeps OpenContact off the wire entirely (Pure Humble UI,
            // ADR-043/044): no path returns it raw anymore.
            ActionResult::OpenContact { contact_id } => {
                let screen = self.navigate_to(AppScreen::ContactDetail { contact_id });
                ActionResult::NavigateTo(screen)
            }
            // Navigate to MyInfo in preview mode for the given contact.
            ActionResult::PreviewAs { contact_id } => {
                let screen = self.preview_as(contact_id);
                ActionResult::NavigateTo(screen)
            }
            // Navigate to Contacts screen so the user can pick a contact to preview as.
            ActionResult::ShowContactPicker => {
                let screen = self.navigate_to(AppScreen::Contacts);
                ActionResult::NavigateTo(screen)
            }
            // Group management: route ShowFormDialog to FormDialog screen
            ActionResult::ShowFormDialog {
                dialog_type,
                context_id,
            } => {
                let form_type = match dialog_type.as_str() {
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
                    let screen = self.navigate_to(AppScreen::FormDialog { dialog_type: ft });
                    ActionResult::NavigateTo(screen)
                } else {
                    ActionResult::UpdateScreen(self.engine.current_screen())
                }
            }
            // Intercept backup Processing: execute backup in core, return result screen
            ActionResult::NavigateTo(ref screen)
                if self.screen == AppScreen::Backup && screen.screen_id == "backup_processing" =>
            {
                self.execute_backup()
            }
            // Persist field-visibility toggles emitted by GroupDetailEngine
            // (Pair 2 of Pure Humble UI retirement). Calls the
            // repropagating variant so downstream contacts re-fetch the
            // visible field set on the next sync. Engine cache is
            // invalidated so the Visible Fields count refreshes.
            ActionResult::SetGroupFieldVisibility {
                group_id,
                field_id,
                visible,
            } => {
                if let Err(e) = self
                    .vauchi
                    .set_group_field_visibility_and_repropagate(&group_id, &field_id, visible)
                {
                    return ActionResult::ShowAlert {
                        title: "Visibility Update Failed".into(),
                        message: format!("{e}"),
                    };
                }
                self.engine_cache.remove(&self.screen);
                ActionResult::UpdateScreen(self.engine.current_screen())
            }
            // Reschedule every failed delivery for immediate retry —
            // mirror of `mobile_delivery::manual_retry`, applied per id.
            // Emitted by DeliveryStatusEngine on the "Retry Failed"
            // footer (Pair 1 of Pure Humble UI retirement).
            ActionResult::RetryFailedDeliveries { message_ids } => {
                let now = self.vauchi.clock().unix_seconds();
                let mut rescheduled = 0u32;
                for id in &message_ids {
                    if let Ok(Some(_)) = self.vauchi.storage().get_retry_entry(id)
                        && self
                            .vauchi
                            .storage()
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
            other => other,
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
        if let Some(eng) = self
            .engine
            .as_any_mut()
            .and_then(|a| a.downcast_mut::<crate::ui::onboarding::OnboardingEngine>())
        {
            // Re-emitting "back" from BackupPasswordEntry clears
            // pending bytes + password and routes to LinkChoice.
            // best-effort: navigation reset is advisory; if the
            // engine can't transition the screen will rebuild fresh
            #[allow(clippy::let_underscore_must_use)]
            let _ = eng.handle_action(UserAction::ActionPressed {
                action_id: "back".into(),
            });
        }
    }

    ///
    /// Import (restore) flows through `execute_backup_restore` (Phase 2B
    /// of `2026-05-03-core-file-picker-command`) which wires the file
    /// picker + password entry through core. This helper handles the
    /// export side only.
    fn execute_backup(&mut self) -> ActionResult {
        use crate::ui::backup_recovery::BackupRecoveryEngine;

        // Read mode from the engine via downcast (avoids fragile string matching)
        let is_restore = self
            .engine
            .as_any()
            .and_then(|a| a.downcast_ref::<BackupRecoveryEngine>())
            .is_some_and(|e| *e.mode() == crate::ui::backup_recovery::BackupMode::Restore);

        if is_restore {
            // Restore from a pasted/typed backup blob (keyboard frontends).
            // The blob lives on the engine (`restore_data`); the password was
            // captured into `pending_backup_password` by the AppScreen::Backup
            // TextChanged intercept. Mobile's file-picker restore runs through
            // `execute_backup_restore` instead and never reaches here.
            let backup_hex = self
                .engine
                .as_any()
                .and_then(|a| a.downcast_ref::<BackupRecoveryEngine>())
                .map(|e| e.restore_data().trim().to_string())
                .unwrap_or_default();
            let password = self.pending_backup_password.take().unwrap_or_default();
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

        let password = match self.pending_backup_password.take() {
            Some(p) => p,
            None => {
                self.engine.processing_failed();
                self.pending_backup_full = true;
                return ActionResult::NavigateTo(self.engine.current_screen());
            }
        };

        let result = if self.pending_backup_full {
            self.vauchi.export_full_backup(&password)
        } else {
            self.vauchi.export_backup(&password)
        };

        // Reset captured state
        self.pending_backup_full = true;

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

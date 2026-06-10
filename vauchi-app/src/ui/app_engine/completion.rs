// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-screen completion handlers for `AppEngine`.
//!
//! `routing::handle_completion` is a dispatcher: it matches the current
//! `AppScreen` and delegates to one `complete_<screen>` method here. Each
//! method owns the side effects (Vauchi calls, navigation) for finishing
//! that screen's workflow. Splitting one method per screen keeps the
//! dispatcher flat and each handler independently testable.

use super::{AppEngine, AppScreen};
use crate::ui::action::{ActionResult, UserAction};
use crate::ui::form_dialog::FormDialogType;
use vauchi_core::contact_card::FieldType;

impl AppEngine {
    /// Onboarding complete: create identity + persist onboarding groups/fields.
    pub(super) fn complete_onboarding(&mut self) -> ActionResult {
        // All onboarding input lives on the engine — it captures the display
        // name, group selection, and ContactInfo fields as the user advances
        // (and is still alive at completion time). Read everything from it
        // here rather than from a duplicated AppEngine-level `pending_*` field
        // (#2026-06-07-app-engine-dispatch-tier-consolidation, Phase 1).
        let onboarding_engine = self
            .engine
            .as_any()
            .and_then(|a| a.downcast_ref::<crate::ui::onboarding::OnboardingEngine>());
        let name = onboarding_engine
            .map(|ob| ob.onboarding_data().display_name.clone())
            .unwrap_or_default();
        if name.trim().is_empty() {
            return ActionResult::ValidationError {
                component_id: "display_name".into(),
                message: "Please enter a display name".into(),
            };
        }
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
                    let (field_type, alias_label) = FieldType::from_alias(&setup.field_type)
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

    /// Lock screen complete: authenticate with the entered password.
    pub(super) fn complete_lock(&mut self) -> ActionResult {
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

    /// Exchange complete: persist contact + init ratchet + assign groups.
    pub(super) fn complete_exchange(&mut self) -> ActionResult {
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
                if let vauchi_core::exchange::ExchangeState::Complete { contact } = session.state()
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
                    if let Err(e) =
                        self.vauchi
                            .save_exchange_ratchet(&contact_id, &ratchet, is_initiator)
                    {
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
            // Snapshot the revocation baseline
            // (2026-06-08-card-revocation-not-propagated). Best-effort.
            #[allow(clippy::let_underscore_must_use)]
            let _ = self.vauchi.initialize_sent_baseline(&contact_id);
            // Capture-at-exchange (ADR-051): QR is an in-person mode, so
            // record where this contact was met — same seam as multi-stage
            // and BLE. The Event::LocationResult reply is consumed in
            // handle_hardware_event.
            self.request_exchange_location(contact_id.clone());
        }

        ActionResult::NavigateTo(screen)
    }

    /// Multi-stage exchange complete: route Done → Contacts, Cancel → Exchange.
    ///
    /// Fix A of `2026-06-02-exchange-back-cancel-broken`: the core-driven
    /// multi-stage screen returns `Complete` on both Cancel and Done.
    /// Without an explicit destination this fell to the catch-all
    /// `navigate_back`, which popped an unstamped target and produced an
    /// empty `screen_id` → a white screen on device. Route to a
    /// deterministic, stamped destination: Done (success — the contact was
    /// already persisted on the `Finalized` event, see
    /// app_engine/multi_stage_exchange.rs) lands on Contacts; Cancel returns
    /// to the mode picker.
    pub(super) fn complete_multi_stage_exchange(&mut self) -> ActionResult {
        let cancelled = self.engine.was_cancelled();
        let target = if cancelled {
            AppScreen::Exchange
        } else {
            AppScreen::Contacts
        };
        let screen = self.navigate_to_internal(target);
        ActionResult::NavigateTo(screen)
    }

    /// Contact visibility complete: persist per-field show/hide toggles.
    pub(super) fn complete_contact_visibility(&mut self, contact_id: &str) -> ActionResult {
        if let Some(input) = self.engine.collected_input() {
            // Parse "field_id:visible,field_id:hidden,..." and persist
            for pair in input.split(',') {
                let mut parts = pair.splitn(2, ':');
                if let (Some(field_id), Some(state)) = (parts.next(), parts.next()) {
                    let should_show = state == "visible";
                    let is_visible = self
                        .vauchi
                        .get_effective_field_visibility(contact_id, field_id)
                        .unwrap_or(true);
                    if should_show != is_visible {
                        // best-effort: visibility toggle is idempotent;
                        // a failure leaves the row in its prior state
                        // and the user can retry from the same screen
                        #[allow(clippy::let_underscore_must_use)]
                        let _ = self.vauchi.toggle_field_visibility(contact_id, field_id);
                    }
                }
            }
        }
        let screen = self.navigate_back();
        ActionResult::NavigateTo(screen)
    }

    /// Fingerprint verification complete: apply the verify/unverify decision.
    pub(super) fn complete_verify_fingerprint(&mut self, contact_id: &str) -> ActionResult {
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

    /// Emergency shred complete: route to onboarding (data already wiped).
    pub(super) fn complete_emergency_shred(&mut self) -> ActionResult {
        let screen = self.navigate_to_internal(AppScreen::Onboarding);
        ActionResult::NavigateTo(screen)
    }

    /// Emergency broadcast complete: save / send / disable per engine outcome.
    pub(super) fn complete_emergency_broadcast(&mut self) -> ActionResult {
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
                if let Err(e) =
                    self.vauchi
                        .configure_emergency_broadcast(ids, message, include_location)
                {
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

    /// Privacy / GDPR complete: export, delete, cancel, execute, or shred.
    pub(super) fn complete_privacy(&mut self) -> ActionResult {
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
                        message: "Identity deletion scheduled. You have 7 days to cancel.".into(),
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
                // Borrow `identity` only long enough to run the delete and
                // capture the relay deliveries (signed pre-shred); then the
                // cache can be cleared (all data is gone).
                let deliveries = match self.vauchi.identity() {
                    Some(identity) => vauchi_core::api::DeletionManager::new(self.vauchi.storage())
                        .execute_deletion(identity)
                        .ok()
                        .map(|result| result.deliveries),
                    None => None,
                };
                match deliveries {
                    Some(deliveries) => {
                        // Notify contacts over the relay so they crypto-shred
                        // this revoked identity. Best-effort: the blobs were
                        // signed before the keys were shredded. Network builds
                        // only — without a relay transport there is nowhere to
                        // send (the `recovery` module is `network-http`-gated).
                        #[cfg(feature = "network-http")]
                        let _ = self.vauchi.broadcast_identity_revocations(&deliveries);
                        #[cfg(not(feature = "network-http"))]
                        let _ = &deliveries;
                        self.engine_cache.clear();
                        ActionResult::WipeComplete
                    }
                    None => ActionResult::ShowToast {
                        message: "Could not execute deletion.".into(),
                        undo_action_id: None,
                    },
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

    /// Form dialog complete: dispatch by dialog type, then the common
    /// save-and-navigate-back tail (`form_saved`). Cancel navigates back.
    pub(super) fn complete_form_dialog(&mut self, dialog_type: &FormDialogType) -> ActionResult {
        // Cancel navigates back without saving
        if self.engine.was_cancelled() {
            let screen = self.navigate_back();
            return ActionResult::NavigateTo(screen);
        }
        let input = self.engine.collected_input();
        match dialog_type {
            FormDialogType::EditName { .. } => self.form_edit_name(input),
            FormDialogType::EditField { field_id, .. } => self.form_edit_field(field_id, input),
            FormDialogType::AddField { .. } => self.form_add_field(input),
            FormDialogType::CreateGroup => self.form_create_group(input),
            FormDialogType::RenameGroup { group_id, .. } => self.form_rename_group(group_id, input),
            FormDialogType::EditRelayUrl { .. } => match input {
                // Persist durably via core so the change survives a restart on
                // every frontend (mobile had no Backend, so this was a no-op).
                Some(url) => {
                    let result = self.vauchi.set_relay_url(&url);
                    self.form_saved(result)
                }
                None => self.form_saved(Ok::<(), std::convert::Infallible>(())),
            },
        }
    }

    /// Common tail for a saved form dialog: invalidate the parent cache
    /// (except Onboarding, whose state must survive round-trips) and
    /// navigate back, or surface the save error.
    fn form_saved<E: std::fmt::Display>(&mut self, result: Result<(), E>) -> ActionResult {
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

    /// `FormDialogType::EditName` — update the display name.
    fn form_edit_name(&mut self, input: Option<String>) -> ActionResult {
        let name = input.unwrap_or_default();
        if name.trim().is_empty() {
            return ActionResult::ValidationError {
                component_id: "display_name".into(),
                message: "Display name cannot be empty".into(),
            };
        }
        let result = self.vauchi.update_display_name(&name);
        self.form_saved(result)
    }

    /// `FormDialogType::EditField` — update a field's value + note.
    fn form_edit_field(&mut self, field_id: &str, input: Option<String>) -> ActionResult {
        let raw = input.unwrap_or_default();
        // Format: value\nnote
        let mut parts = raw.splitn(2, '\n');
        let value = parts.next().unwrap_or("").to_string();
        let note = parts.next().unwrap_or("").trim().to_string();
        let now = self.vauchi.clock().unix_seconds();
        let result = match self.vauchi.own_card() {
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
        };
        self.form_saved(result)
    }

    /// `FormDialogType::AddField` — parse + add a new own-card field, then
    /// apply group visibility and buffer it into a cached onboarding engine.
    fn form_add_field(&mut self, input: Option<String>) -> ActionResult {
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
            && let Some(ob) = engine
                .as_any_mut()
                .and_then(|a| a.downcast_mut::<crate::ui::onboarding::OnboardingEngine>())
        {
            ob.push_field(crate::ui::onboarding::FieldSetup {
                field_type: entry_type.to_string(),
                label: label.clone(),
                value: value.to_string(),
                visible_to_groups: group_list,
                shown: true,
            });
        }
        self.form_saved(result)
    }

    /// `FormDialogType::CreateGroup` — create a new group.
    fn form_create_group(&mut self, input: Option<String>) -> ActionResult {
        let name = input.unwrap_or_default();
        if name.trim().is_empty() {
            return ActionResult::ValidationError {
                component_id: "group_name".into(),
                message: "Group name cannot be empty".into(),
            };
        }
        let result = self.vauchi.create_group(name.trim()).map(|_| ());
        self.form_saved(result)
    }

    /// `FormDialogType::RenameGroup` — rename an existing group.
    fn form_rename_group(&mut self, group_id: &str, input: Option<String>) -> ActionResult {
        let name = input.unwrap_or_default();
        if name.trim().is_empty() {
            return ActionResult::ValidationError {
                component_id: "group_name".into(),
                message: "Group name cannot be empty".into(),
            };
        }
        let result = self.vauchi.rename_group(group_id, name.trim());
        self.form_saved(result)
    }

    /// Sync screen complete: surface pending-update / connection feedback.
    pub(super) fn complete_sync(&mut self) -> ActionResult {
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

    /// Change-password complete: apply if both fields populated (else cancel).
    pub(super) fn complete_change_password(&mut self) -> ActionResult {
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

    /// Duress-PIN complete: set up or disable the duress password/settings.
    pub(super) fn complete_duress_pin(&mut self) -> ActionResult {
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
                    alert_contact_ids: config.alert_contacts.iter().map(|c| c.id.clone()).collect(),
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

    /// Device-management complete: revoke the confirmed device if any.
    pub(super) fn complete_device_management(&mut self) -> ActionResult {
        // Read the confirmed index from the engine before navigating away
        let revoke_index = self
            .engine
            .as_any()
            .and_then(|a| a.downcast_ref::<crate::ui::device_management::DeviceManagementEngine>())
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

    /// Avatar-editor complete: clear or persist the edited avatar.
    pub(super) fn complete_avatar_editor(&mut self) -> ActionResult {
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

    /// Device-replacement complete: route the decommission-old-device outcome.
    pub(super) fn complete_device_replacement(&mut self) -> ActionResult {
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
        if let Some(crate::ui::device_replacement::CompletionOutcome::RemoveOldDevice) = outcome {
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

    /// Deep-link consent complete: grant → responder flow, else navigate back.
    pub(super) fn complete_deep_link_consent(
        &mut self,
        payload: &vauchi_core::exchange::link_mode::DeepLinkPayload,
    ) -> ActionResult {
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
            let screen = self.navigate_to_internal(AppScreen::DeepLinkResponder { payload });
            ActionResult::NavigateTo(screen)
        } else {
            let screen = self.navigate_back();
            ActionResult::NavigateTo(screen)
        }
    }
}

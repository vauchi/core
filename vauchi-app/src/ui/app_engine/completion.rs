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
use vauchi_core::contact_card::FieldType;
use zeroize::Zeroize;

impl AppEngine {
    /// Onboarding complete: create identity + persist onboarding groups/fields.
    pub(super) fn complete_onboarding(&mut self) -> ActionResult {
        // All onboarding input lives on the engine — it captures the display
        // name, group selection, and ContactInfo fields as the user advances
        // (and is still alive at completion time). Read everything from it
        // here rather than from a duplicated AppEngine-level `pending_*` field
        // (#2026-06-07-app-engine-dispatch-tier-consolidation, Phase 1).
        let onboarding_data = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Onboarding(snap)) => Some(snap.data),
            other => {
                tracing::warn!(?other, "onboarding completion without Onboarding output");
                None
            }
        };
        let name = onboarding_data
            .as_ref()
            .map(|ob| ob.display_name.clone())
            .unwrap_or_default();
        if name.trim().is_empty() {
            return ActionResult::ValidationError {
                component_id: "display_name".into(),
                message: "Please enter a display name".into(),
            };
        }
        let onboarding_groups: Vec<String> = onboarding_data
            .as_ref()
            .map(|ob| {
                ob.selected_groups
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
        let onboarding_fields: Vec<crate::ui::onboarding::FieldSetup> = onboarding_data
            .map(|ob| ob.fields.into_iter().filter(|f| f.shown).collect())
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
        let pin = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Lock { pin }) => pin,
            _ => {
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
                // QR path: the contact to persist carries its reciprocity outcome
                // (Confirmed/Pending) stamped by the engine, so an unconfirmed
                // exchange surfaces via the banner rather than looking mutual.
                // Build the ratchet from the session (owns the correct role +
                // exchange-key selection, see build_exchange_ratchet).
                let contact = ex.reciprocity_stamped_contact()?;
                let session = ex.session()?;
                let ratchet = session.build_exchange_ratchet(&contact);
                Some((contact, ratchet, groups))
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
        match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::ContactVisibility { toggles }) => {
                for (field_id, should_show) in toggles {
                    let is_visible = self
                        .vauchi
                        .get_effective_field_visibility(contact_id, &field_id)
                        .unwrap_or(true);
                    if should_show != is_visible {
                        // best-effort: a failure leaves the prior state and the
                        // user can retry from the same screen. Route through the
                        // Layer-C override + repropagate path (not the bare
                        // Layer-A toggle) so the change reaches the contact on
                        // the wire (2026-06-14-visibility-changes-not-fully-propagated).
                        #[allow(clippy::let_underscore_must_use)]
                        let _ = self.vauchi.set_contact_visibility_override_and_repropagate(
                            contact_id,
                            &field_id,
                            should_show,
                        );
                    }
                }
            }
            other => {
                tracing::warn!(
                    ?other,
                    "contact-visibility completion without ContactVisibility output"
                );
            }
        }
        let screen = self.navigate_back();
        ActionResult::NavigateTo(screen)
    }

    /// Fingerprint verification complete: apply the verify/unverify decision.
    pub(super) fn complete_verify_fingerprint(&mut self, contact_id: &str) -> ActionResult {
        use crate::ui::fingerprint_verify::VerifyAction;
        match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::FingerprintVerify(action)) => match action {
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
            },
            other => {
                tracing::warn!(
                    ?other,
                    "verify-fingerprint completion without FingerprintVerify output"
                );
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
        use crate::ui::EmergencyOutcome;
        let plan = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::EmergencyBroadcast(plan)) => Some(plan),
            other => {
                tracing::warn!(
                    ?other,
                    "emergency-broadcast completion without EmergencyBroadcast output"
                );
                None
            }
        };
        let Some(plan) = plan else {
            let screen = self.navigate_back();
            return ActionResult::NavigateTo(screen);
        };
        let (outcome, ids, message, include_location) = (
            plan.outcome,
            plan.contact_ids,
            plan.message,
            plan.include_location,
        );
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
        // GdprEngine exposes the confirmed operation via EngineOutput::Gdpr.
        use crate::ui::GdprChoice;
        let choice = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::Gdpr(choice)) => Some(choice),
            None => None,
            other => {
                tracing::warn!(?other, "privacy completion without Gdpr output");
                None
            }
        };
        match choice {
            Some(GdprChoice::Export) => {
                match vauchi_core::api::export_all_data(self.vauchi.storage()) {
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
                }
            }
            Some(GdprChoice::Delete) => {
                match vauchi_core::api::DeletionManager::new(self.vauchi.storage())
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
                }
            }
            Some(GdprChoice::CancelDeletion) => {
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
            Some(GdprChoice::Execute) => {
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
            Some(GdprChoice::Shred) => match self.vauchi.perform_emergency_wipe(true) {
                Ok(_) => {
                    self.engine_cache.clear();
                    ActionResult::WipeComplete
                }
                Err(_) => ActionResult::ShowToast {
                    message: "Emergency wipe failed.".into(),
                    undo_action_id: None,
                },
            },
            None => {
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
        }
    }

    /// Set/change-password complete: set the first password (setup mode) or
    /// rotate an existing one (change mode), else cancel.
    pub(super) fn complete_change_password(&mut self) -> ActionResult {
        let creds = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::ChangePassword { current, new }) => Some((current, new)),
            other => {
                tracing::warn!(
                    ?other,
                    "change-password completion without ChangePassword output"
                );
                None
            }
        };
        let mut password_was_set = false;
        if let Some((mut current, mut new)) = creds {
            // An empty `new` reaches here only on Cancel (Save stays disabled
            // until new is populated and matches confirm; Cancel zeroizes the
            // fields). Treat it as a cancel — navigate back, touch nothing.
            let alert = if new.is_empty() {
                None
            } else {
                // setup_app_password sets the FIRST password; change_app_password
                // rotates an existing one. Pick by current state so the
                // first-password path works on every Humble frontend
                // (problem 2026-06-13-ios-app-password-setup-missing). A storage
                // read error must NOT default to setup — that would route an
                // existing password to setup_app_password and clobber it.
                match self.vauchi.is_password_enabled() {
                    Err(e) => Some(format!("Could not read password state: {e}")),
                    Ok(had_password) => {
                        let result = if had_password {
                            self.vauchi.change_app_password(&current, &new)
                        } else {
                            self.vauchi.setup_app_password(&new)
                        };
                        password_was_set = result.is_ok();
                        result.err().map(|e| {
                            let verb = if had_password {
                                "change password"
                            } else {
                                "set password"
                            };
                            format!("Could not {verb}: {e}")
                        })
                    }
                }
            };
            // Zeroize the credential copies moved out of the engine output
            // before they drop (EngineOutput itself can't impl Drop — handlers
            // move fields out of it).
            current.zeroize();
            new.zeroize();
            if let Some(message) = alert {
                return ActionResult::ShowAlert {
                    title: "Error".into(),
                    message,
                };
            }
        }
        let screen = self.navigate_back();
        if password_was_set {
            // Setting the first password flips this screen setup→change mode;
            // `navigate_back` just re-cached the now-stale engine, so evict it
            // so the next open rebuilds from is_password_enabled() (same
            // pattern as completion_contact.rs after a contact mutation).
            self.engine_cache.remove(&AppScreen::ChangePassword);
        }
        ActionResult::NavigateTo(screen)
    }

    /// Duress-PIN complete: set up or disable the duress password/settings.
    pub(super) fn complete_duress_pin(&mut self) -> ActionResult {
        let setup = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::DuressPin(setup)) => Some(setup),
            other => {
                tracing::warn!(?other, "duress-pin completion without DuressPin output");
                None
            }
        };
        if let Some(setup) = setup {
            if setup.enabled {
                if let Err(e) = self.vauchi.setup_duress_password(&setup.pin) {
                    return ActionResult::ShowAlert {
                        title: "Error".into(),
                        message: format!("Failed to set duress PIN: {e}"),
                    };
                }
                let settings = vauchi_core::types::DuressSettings {
                    alert_contact_ids: setup.alert_contact_ids,
                    alert_message: setup.alert_message,
                    include_location: setup.include_location,
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
        let revoke_index = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::DeviceManagement {
                confirmed_revoke_index,
            }) => confirmed_revoke_index,
            other => {
                tracing::warn!(
                    ?other,
                    "device-management completion without DeviceManagement output"
                );
                None
            }
        };

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
        let editor = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::AvatarEditor { removed, avatar }) => {
                Some((removed, avatar))
            }
            other => {
                tracing::warn!(
                    ?other,
                    "avatar-editor completion without AvatarEditor output"
                );
                None
            }
        };
        if let Some((removed, avatar)) = editor {
            if removed {
                if let Ok(Some(mut card)) = self.vauchi.own_card() {
                    card.clear_avatar();
                    if let Err(e) = self.vauchi.update_own_card(&card) {
                        return ActionResult::ShowAlert {
                            title: "Avatar Update Failed".into(),
                            message: format!("{e}"),
                        };
                    }
                }
            } else if let Some(avatar) = avatar {
                // Persist the new avatar
                if let Ok(Some(mut card)) = self.vauchi.own_card() {
                    if let Err(e) = card.set_avatar(avatar) {
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
        let outcome = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::DeviceReplacement(outcome)) => Some(outcome),
            other => {
                tracing::warn!(
                    ?other,
                    "device-replacement completion without DeviceReplacement output"
                );
                None
            }
        };
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
        let granted = match self.engine.engine_output() {
            Some(crate::ui::EngineOutput::DeepLinkConsent { granted }) => granted,
            other => {
                tracing::warn!(
                    ?other,
                    "deep-link-consent completion without DeepLinkConsent output"
                );
                false
            }
        };

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

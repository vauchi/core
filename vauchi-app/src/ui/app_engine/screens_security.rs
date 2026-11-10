// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Engine factories for the security screens: Recovery, Duress PIN and
//! Backup (#459), split from `screens.rs` to keep it under the file-size
//! limit.

use super::AppEngine;
use super::AppScreen;
use crate::ui::backup_recovery::BackupRecoveryEngine;
use crate::ui::component::Item;
use crate::ui::duress_pin::{DuressConfig, DuressPinEngine};
use crate::ui::engine::WorkflowEngine;
use crate::ui::recovery_status::RecoveryEngine;
use vauchi_core::api::Vauchi;

impl AppEngine {
    pub(super) fn create_security_engine(
        vauchi: &Vauchi,
        screen: &AppScreen,
        render_context: &crate::ui::RenderContext,
        device_capabilities: &vauchi_core::exchange::capability::types::DeviceCapabilities,
    ) -> Box<dyn WorkflowEngine> {
        match screen {
            AppScreen::Recovery => {
                // Only contacts marked "Trust for recovery", against core's
                // configured threshold (#459); the screen used to count
                // every contact against a hard-coded 3.
                let readiness = vauchi.get_recovery_readiness().ok();
                let trusted_ids: std::collections::HashSet<String> = vauchi
                    .list_contacts()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|c| c.is_recovery_trusted())
                    .map(|c| c.id().to_string())
                    .collect();
                let contacts: Vec<Item> =
                    Self::load_contact_items(vauchi, render_context.resolved_locale())
                        .into_iter()
                        .map(|c| c.item)
                        .filter(|item| trusted_ids.contains(&item.id))
                        .collect();
                let threshold = readiness.map_or(3, |r| r.threshold as usize);
                let device_count = vauchi
                    .list_devices()
                    .map(|d| d.len().saturating_sub(1))
                    .unwrap_or(0);
                let mut engine = RecoveryEngine::new(contacts, threshold)
                    .with_locale(render_context.resolved_locale());
                engine.set_linked_device_count(device_count);
                if !vauchi.has_identity() {
                    engine.set_identity_free_recovery();
                }
                let pending_claim = match vauchi.pending_recovery_claim() {
                    Ok(claim) => claim,
                    Err(_) => {
                        engine.set_load_error();
                        return Box::new(engine);
                    }
                };
                if let Some(claim) = pending_claim {
                    let expired = vauchi
                        .parse_recovery_claim_b64(&claim)
                        .map_or(true, |parsed| {
                            parsed.is_expired(vauchi.clock().unix_seconds())
                        });
                    engine.set_generated_claim(claim);
                    if expired {
                        engine.mark_generated_claim_expired();
                    }
                    match vauchi.hand_back_restore_status() {
                        Ok(Some(status)) => {
                            if status.can_attempt_restore {
                                engine.resume_hand_back_restore(
                                    status.handed_back,
                                    status.needs_passphrase,
                                );
                            }
                        }
                        Ok(None) | Err(_) => engine.set_load_error(),
                    }
                    return Box::new(engine);
                } else if !vauchi.has_identity() {
                    return Box::new(engine);
                } else if let Ok(Some(progress)) = vauchi.get_recovery_progress()
                    && !progress.claim.is_expired(vauchi.clock().unix_seconds())
                {
                    use base64::Engine;
                    engine.resume_collecting(
                        base64::engine::general_purpose::STANDARD.encode(progress.claim.to_bytes()),
                    );
                    engine.apply_update(crate::ui::EngineUpdate::Recovery(
                        super::intercept::vouchers_collected(&progress),
                    ));
                }
                engine.apply_update(crate::ui::EngineUpdate::Recovery(
                    crate::ui::RecoveryUpdate::PassphraseSet(
                        vauchi.has_recovery_passphrase().unwrap_or(false),
                    ),
                ));
                engine.apply_update(crate::ui::EngineUpdate::Recovery(
                    crate::ui::RecoveryUpdate::RekeyWaiting(rekey_waiting_rows(vauchi)),
                ));
                if let Ok(handed_back @ 1..) = vauchi.guardians_handed_back() {
                    engine.apply_update(crate::ui::EngineUpdate::Recovery(
                        crate::ui::RecoveryUpdate::RestoreOffered {
                            handed_back,
                            needs_passphrase: vauchi
                                .returned_backup_needs_passphrase()
                                .unwrap_or(true),
                        },
                    ));
                }
                Box::new(engine)
            }
            AppScreen::DuressPin => {
                let enabled = vauchi.is_duress_enabled().unwrap_or(false);
                let decoy_count = vauchi.list_decoy_contacts().map_or(0, |d| d.len());
                Box::new(
                    DuressPinEngine::new(
                        DuressConfig { enabled },
                        render_context.resolved_locale(),
                    )
                    .with_decoy_count(decoy_count)
                    .with_password_branch(!device_capabilities.has_biometrics),
                )
            }
            AppScreen::Backup => Box::new(
                BackupRecoveryEngine::new(
                    None,
                    vauchi.has_identity(),
                    render_context.resolved_locale(),
                )
                .with_last_backup(
                    vauchi
                        .load_backup_reminder_state()
                        .ok()
                        .and_then(|s| s.last_backup_timestamp),
                    vauchi.clock().unix_seconds(),
                )
                // Under duress the mask's session reports a duress PIN too,
                // so the real profile is told apart by the auth mode.
                .with_decoy_offer(
                    vauchi.auth_mode() == vauchi_core::AuthMode::Normal
                        && vauchi.is_duress_enabled().unwrap_or(false),
                )
                .with_biometric_opt_in(
                    device_capabilities
                        .biometric_type
                        .is_some()
                        .then(|| vauchi.confirms_exports_with_biometrics().unwrap_or(false)),
                ),
            ),
            _ => unreachable!("create_security_engine called for {screen:?}"),
        }
    }
}

/// The recovered contacts still waiting for their rekey, by name, with
/// whether each did not answer in 14 days (vauchi/private#796).
pub(super) fn rekey_waiting_rows(vauchi: &vauchi_core::api::Vauchi) -> Vec<(String, bool)> {
    vauchi
        .rekey_waiting()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(id, wait)| {
            let name = vauchi
                .get_contact(&id)
                .ok()
                .flatten()?
                .display_name()
                .to_string();
            Some((name, wait == vauchi_core::api::RekeyWait::NotAnswered))
        })
        .collect()
}

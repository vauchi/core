// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `security` arm group of [`PlatformAppEngine::dispatch_domain_command`] —
//! split out of `platform_app_engine.rs` (pure code motion).

use vauchi_app::ui::{AppEngine, AppScreen};

use crate::domain_command::{DomainCommand, DomainCommandResult};
use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

impl PlatformAppEngine {
    pub(crate) fn dispatch_security(
        &self,
        engine: &mut AppEngine,
        command: DomainCommand,
    ) -> Result<DomainCommandResult, MobileError> {
        match command {
            DomainCommand::ExportGdprData => {
                let storage = engine.vauchi().storage();
                let export =
                    vauchi_core::api::export_all_data(storage).map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                let json_data =
                    serde_json::to_string_pretty(&export).map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::GdprExport {
                    export: crate::types::MobileGdprExport {
                        json_data,
                        exported_at: export.exported_at,
                        version: export.version,
                    },
                })
            }
            DomainCommand::ScheduleIdentityDeletion => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::DeletionManager::new(storage);
                manager
                    .schedule_deletion()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                let state = manager.deletion_state().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                let now = engine.vauchi().clock().unix_seconds();
                let mut info = crate::types::MobileDeletionInfo::from(&state);
                info.days_remaining = ((info.execute_at.saturating_sub(now)) / 86400) as u32;
                Ok(DomainCommandResult::DeletionInfo { info })
            }
            DomainCommand::CancelIdentityDeletion => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::DeletionManager::new(storage);
                manager.cancel_deletion().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ExecuteIdentityDeletion => {
                let revocation_count = {
                    let vauchi = engine.vauchi();
                    let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                    let manager = vauchi_core::api::DeletionManager::new(vauchi.storage());
                    let result =
                        manager
                            .execute_deletion(identity)
                            .map_err(|e| MobileError::Other {
                                detail: e.to_string(),
                            })?;
                    result.revocations.len() as u32
                };
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::Count {
                    value: revocation_count,
                })
            }
            DomainCommand::GetDeletionState => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::DeletionManager::new(storage);
                let state = manager.deletion_state().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                Ok(DomainCommandResult::DeletionInfo {
                    info: crate::types::MobileDeletionInfo::from(&state),
                })
            }
            DomainCommand::ShredStatus => {
                use crate::types::MobileShredStatus as MShred;
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::DeletionManager::new(storage);
                let state = manager.deletion_state().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                let status = match state {
                    vauchi_core::storage::DeletionState::None => MShred::None,
                    vauchi_core::storage::DeletionState::Scheduled { execute_at, .. } => {
                        // TODO(PFC): SystemTime::now() in PAE dispatch — see 2026-07-06-core-pfc-violations C8
                        let now = engine.vauchi().clock().unix_seconds();
                        MShred::Scheduled {
                            remaining_secs: execute_at.saturating_sub(now),
                        }
                    }
                    vauchi_core::storage::DeletionState::Executed { .. } => MShred::Executed,
                    _ => MShred::None,
                };
                Ok(DomainCommandResult::ShredStatus { status })
            }
            DomainCommand::SoftShred => {
                let bridge = self.shred_keychain_bridge()?;
                let data_dir = self.shred_data_dir();
                let token = {
                    let vauchi = engine.vauchi();
                    let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                    let manager = vauchi_core::api::ShredManager::new(
                        vauchi.storage(),
                        &bridge,
                        identity,
                        data_dir,
                    );
                    manager.soft_shred().map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?
                };
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::ShredScheduled {
                    token: crate::types::MobileShredToken::from(&token),
                })
            }
            DomainCommand::CancelShred { token } => {
                let bridge = self.shred_keychain_bridge()?;
                let data_dir = self.shred_data_dir();
                let core_token = vauchi_core::api::ShredToken::from_created_at(token.created_at);
                {
                    let vauchi = engine.vauchi();
                    let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                    let manager = vauchi_core::api::ShredManager::new(
                        vauchi.storage(),
                        &bridge,
                        identity,
                        data_dir,
                    );
                    manager
                        .cancel_shred(core_token)
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                }
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::HardShred { token } => {
                let bridge = self.shred_keychain_bridge()?;
                let data_dir = self.shred_data_dir();
                let core_token = vauchi_core::api::ShredToken::from_created_at(token.created_at);
                let report = {
                    let vauchi = engine.vauchi();
                    let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                    let sender_id = identity.public_id();
                    let (mut purge, mut rev) = self.shred_senders(vauchi, &sender_id);
                    let manager = vauchi_core::api::ShredManager::new(
                        vauchi.storage(),
                        &bridge,
                        identity,
                        data_dir,
                    );
                    manager
                        .hard_shred(core_token, Some(&mut purge), Some(&mut rev))
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?
                };
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::ShredCompleted {
                    report: crate::types::MobileShredReport::from(&report),
                })
            }
            DomainCommand::PanicShred => {
                let bridge = self.shred_keychain_bridge()?;
                let data_dir = self.shred_data_dir();
                let report = {
                    let vauchi = engine.vauchi();
                    let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                    let sender_id = identity.public_id();
                    let (mut purge, mut rev) = self.shred_senders(vauchi, &sender_id);
                    let manager = vauchi_core::api::ShredManager::new(
                        vauchi.storage(),
                        &bridge,
                        identity,
                        data_dir,
                    );
                    manager
                        .panic_shred(Some(&mut purge), Some(&mut rev))
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?
                };
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::ShredCompleted {
                    report: crate::types::MobileShredReport::from(&report),
                })
            }

            // ── Recovery leftovers (B7 batch 4) ──
            DomainCommand::SetupAppPassword { password } => {
                engine
                    .vauchi_mut()
                    .setup_app_password(&password)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Lock);
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetupDuressPassword { duress_password } => {
                engine
                    .vauchi_mut()
                    .setup_duress_password(&duress_password)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::DuressPin);
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::Authenticate { password } => {
                let mode = engine.vauchi_mut().authenticate(&password).map_err(|e| {
                    MobileError::Other {
                        detail: e.to_string(),
                    }
                })?;
                let mapped = match mode {
                    vauchi_core::AuthMode::Normal => crate::types::MobileAuthMode::Normal,
                    vauchi_core::AuthMode::Duress => crate::types::MobileAuthMode::Duress,
                    _ => crate::types::MobileAuthMode::Normal,
                };
                Ok(DomainCommandResult::AuthMode { mode: mapped })
            }
            DomainCommand::IsPasswordEnabled => {
                let value =
                    engine
                        .vauchi()
                        .is_password_enabled()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::Bool { value })
            }
            DomainCommand::IsDuressEnabled => {
                let value =
                    engine
                        .vauchi()
                        .is_duress_enabled()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::Bool { value })
            }
            DomainCommand::DisableDuress => {
                engine
                    .vauchi_mut()
                    .disable_duress()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::DuressPin);
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ConfigureDuressAlerts {
                contact_ids,
                message,
            } => {
                let settings = vauchi_core::DuressSettings {
                    alert_contact_ids: contact_ids,
                    alert_message: message,
                    include_location: false,
                };
                engine
                    .vauchi()
                    .save_duress_settings(&settings)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetDuressSettings => {
                let settings =
                    engine
                        .vauchi()
                        .load_duress_settings()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::DuressSettingsOpt {
                    settings: settings.map(|s| crate::types::MobileDuressSettings {
                        alert_contact_ids: s.alert_contact_ids,
                        alert_message: s.alert_message,
                        include_location: s.include_location,
                    }),
                })
            }
            DomainCommand::AddDecoyContact { name, card_json } => {
                let card: vauchi_core::ContactCard =
                    serde_json::from_str(&card_json).map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                let id = format!(
                    "decoy-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                );
                engine
                    .vauchi()
                    .add_decoy_contact(&id, &name, &card)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Text { value: id })
            }
            DomainCommand::ListDecoyContacts => {
                let decoys =
                    engine
                        .vauchi()
                        .list_decoy_contacts()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::DecoyContacts {
                    contacts: decoys
                        .into_iter()
                        .map(
                            |(id, display_name, _card)| crate::types::MobileDecoyContact {
                                id,
                                display_name,
                            },
                        )
                        .collect(),
                })
            }
            DomainCommand::DeleteDecoyContact { id } => {
                engine
                    .vauchi()
                    .remove_decoy_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }

            // ── Sync / Delivery / Retry — read paths + simple writes (B7 batch 8) ──
            //
            // Cache invalidation: ManualRetry / DeleteRetry /
            // ClearPendingUpdatesForContact invalidate `DeliveryStatus`
            // (the user-visible delivery feed). Pure reads invalidate
            // nothing.
            DomainCommand::SetPinnedCertificate { cert_pem } => {
                let path = self.cert_pin_path_engine();
                if cert_pem.is_empty() {
                    // Empty string clears the pin: remove the sidecar file.
                    // Ignore NotFound — already-cleared is idempotent.
                    if let Err(e) = std::fs::remove_file(&path)
                        && e.kind() != std::io::ErrorKind::NotFound
                    {
                        return Err(MobileError::StorageError {
                            detail: e.to_string(),
                        });
                    }
                } else {
                    std::fs::write(&path, cert_pem).map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                }
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::IsCertificatePinningEnabled => Ok(DomainCommandResult::Bool {
                value: self.cert_pin_path_engine().exists(),
            }),

            // ── Device linking — Track B Tier 2 (B7 batch 22) ──
            DomainCommand::ConfigureEmergencyBroadcast {
                contact_ids,
                message,
                include_location,
            } => {
                engine
                    .vauchi_mut()
                    .configure_emergency_broadcast(contact_ids, message, include_location)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::Bool { value: true })
            }
            DomainCommand::SendEmergencyBroadcast => {
                let result = engine
                    .vauchi_mut()
                    .send_emergency_broadcast()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::BroadcastResult {
                    result: crate::types::MobileBroadcastResult {
                        sent: result.sent as u32,
                        total: result.total as u32,
                    },
                })
            }
            DomainCommand::GetEmergencyConfig => {
                let config =
                    engine
                        .vauchi()
                        .load_emergency_config()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::OptionalEmergencyConfig {
                    config: config.map(|c| crate::types::MobileEmergencyConfig {
                        trusted_contact_ids: c.trusted_contact_ids,
                        message: c.message,
                        include_location: c.include_location,
                    }),
                })
            }
            DomainCommand::DisableEmergencyBroadcast => {
                engine
                    .vauchi_mut()
                    .delete_emergency_config()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::Bool { value: true })
            }
            other => unreachable!("non-security command {other:?} routed to security dispatcher"),
        }
    }
}

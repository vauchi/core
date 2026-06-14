// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `own_card_identity` arm group of [`PlatformAppEngine::dispatch_domain_command`] —
//! split out of `platform_app_engine.rs` (pure code motion).

use vauchi_app::ui::{AppEngine, AppScreen};

use crate::domain_command::{DomainCommand, DomainCommandResult};
use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

impl PlatformAppEngine {
    pub(crate) fn dispatch_own_card_identity(
        &self,
        engine: &mut AppEngine,
        command: DomainCommand,
    ) -> Result<DomainCommandResult, MobileError> {
        match command {
            DomainCommand::GetOwnCard => {
                let card = engine
                    .vauchi()
                    .storage()
                    .contacts()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                Ok(DomainCommandResult::ContactCardPayload {
                    card: crate::types::MobileContactCard::from(&card),
                })
            }
            DomainCommand::AddField {
                field_type,
                label,
                value,
            } => {
                let storage = engine.vauchi().storage();
                let old_card = storage
                    .contacts()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let mut card = old_card.clone();
                let field = vauchi_core::ContactField::new(
                    field_type.into(),
                    &label,
                    &value,
                    engine.vauchi().clock().unix_seconds(),
                );
                card.add_field(field)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .contacts()
                    .save_own_card(&card)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                // Repropagation retry queue: arm the durable marker, then
                // attempt an immediate flush. The edit never fails on a
                // propagation error; the next sync tick retries
                // (2026-06-14-own-card-propagation-retry-queue).
                engine.vauchi().mark_own_card_repropagate().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                #[allow(clippy::let_underscore_must_use)]
                let _ = engine.vauchi().run_owed_repropagation();
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::UpdateField { label, new_value } => {
                let storage = engine.vauchi().storage();
                let old_card = storage
                    .contacts()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let mut card = old_card.clone();
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field '{label}' not found"),
                    })?
                    .id()
                    .to_string();
                card.update_field_value(&field_id, &new_value, storage.clock().unix_seconds())
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .contacts()
                    .save_own_card(&card)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                // Repropagation retry queue: arm the durable marker, then
                // attempt an immediate flush. The edit never fails on a
                // propagation error; the next sync tick retries
                // (2026-06-14-own-card-propagation-retry-queue).
                engine.vauchi().mark_own_card_repropagate().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                #[allow(clippy::let_underscore_must_use)]
                let _ = engine.vauchi().run_owed_repropagation();
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::RemoveField { label } => {
                let storage = engine.vauchi().storage();
                let old_card = storage
                    .contacts()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let mut card = old_card.clone();
                let field_id = match card.fields().iter().find(|f| f.label() == label) {
                    Some(f) => f.id().to_string(),
                    None => return Ok(DomainCommandResult::Bool { value: false }),
                };
                card.remove_field(&field_id)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .contacts()
                    .save_own_card(&card)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                // Repropagation retry queue: arm the durable marker, then
                // attempt an immediate flush. The edit never fails on a
                // propagation error; the next sync tick retries
                // (2026-06-14-own-card-propagation-retry-queue).
                engine.vauchi().mark_own_card_repropagate().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                #[allow(clippy::let_underscore_must_use)]
                let _ = engine.vauchi().run_owed_repropagation();
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Bool { value: true })
            }
            DomainCommand::SetDisplayName { name } => {
                // Route through `Vauchi::update_display_name` so the
                // identity's `display_name` column is updated in addition
                // to the own_card. The prior implementation mutated only
                // `own_card` and called `storage.save_own_card`, leaving
                // the identity column stale — which surfaced as the
                // Samsung S7 rename failure tracked by
                // `_private/docs/problems/2026-04-06-display-name-rename-fails/`.
                engine
                    .vauchi_mut()
                    .update_display_name(&name)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetOwnAvatar { avatar_bytes } => {
                let storage = engine.vauchi().storage();
                let mut card = storage
                    .contacts()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                card.set_avatar(avatar_bytes)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .contacts()
                    .save_own_card(&card)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ClearOwnAvatar => {
                let storage = engine.vauchi().storage();
                let mut card = storage
                    .contacts()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                card.clear_avatar();
                storage
                    .contacts()
                    .save_own_card(&card)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::CreateIdentity { display_name } => {
                engine
                    .vauchi_mut()
                    .create_identity(&display_name)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_all();
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetPublicId => {
                let value = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?
                    .public_id();
                Ok(DomainCommandResult::Text { value })
            }
            DomainCommand::GetDisplayName => {
                let value = engine
                    .vauchi()
                    .storage()
                    .contacts()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?
                    .display_name()
                    .to_string();
                Ok(DomainCommandResult::Text { value })
            }
            DomainCommand::GetOwnFingerprint => {
                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                let hex = hex::encode(identity.signing_public_key());
                let formatted = hex
                    .chars()
                    .collect::<Vec<_>>()
                    .chunks(4)
                    .map(|c| c.iter().collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_uppercase();
                Ok(DomainCommandResult::Text { value: formatted })
            }
            DomainCommand::DisplayNameSuggestions { full_name } => {
                Ok(DomainCommandResult::Strings {
                    values: vauchi_core::display_name_suggestions(&full_name),
                })
            }
            DomainCommand::ResetOnboarding => {
                let storage = engine.vauchi().storage();
                let mut progress =
                    storage
                        .ux()
                        .load_or_create_onboarding_progress()
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })?;
                progress.reset(engine.vauchi().clock().unix_seconds());
                storage
                    .ux()
                    .save_onboarding_progress(&progress)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Onboarding);
                Ok(DomainCommandResult::Unit)
            }

            // ── Contact Verification + Duplicates + Notes + Misc (B7 batch 11) ──
            DomainCommand::GetOnboardingProgress => {
                let progress = engine
                    .vauchi()
                    .storage()
                    .ux()
                    .load_or_create_onboarding_progress()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::OnboardingProgress {
                    progress: crate::types::MobileOnboardingProgress::from(&progress),
                })
            }
            DomainCommand::CurrentOnboardingStep => {
                let progress = engine
                    .vauchi()
                    .storage()
                    .ux()
                    .load_or_create_onboarding_progress()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::OnboardingStep {
                    step: progress.current_step().into(),
                })
            }
            DomainCommand::IsOnboardingComplete => {
                let progress = engine
                    .vauchi()
                    .storage()
                    .ux()
                    .load_or_create_onboarding_progress()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Bool {
                    value: progress.is_complete(),
                })
            }
            DomainCommand::AdvanceOnboarding => {
                let storage = engine.vauchi().storage();
                let mut progress =
                    storage
                        .ux()
                        .load_or_create_onboarding_progress()
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })?;
                progress.advance(engine.vauchi().clock().unix_seconds());
                storage
                    .ux()
                    .save_onboarding_progress(&progress)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::OnboardingProgress {
                    progress: crate::types::MobileOnboardingProgress::from(&progress),
                })
            }
            DomainCommand::SkipOnboardingStep => {
                let storage = engine.vauchi().storage();
                let mut progress =
                    storage
                        .ux()
                        .load_or_create_onboarding_progress()
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })?;
                progress.skip_step(engine.vauchi().clock().unix_seconds());
                storage
                    .ux()
                    .save_onboarding_progress(&progress)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::OnboardingProgress {
                    progress: crate::types::MobileOnboardingProgress::from(&progress),
                })
            }

            // ── Contact display options + paginated/archived lists (B7 batch 17) ──
            other => unreachable!(
                "non-own_card_identity command {other:?} routed to own_card_identity dispatcher"
            ),
        }
    }
}

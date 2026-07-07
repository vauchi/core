// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `engagement` arm group of [`PlatformAppEngine::dispatch_domain_command`] —
//! split out of `platform_app_engine.rs` (pure code motion).

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::rng::SecureRngExt;

use crate::domain_command::{DomainCommand, DomainCommandResult};
use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

impl PlatformAppEngine {
    pub(crate) fn dispatch_engagement(
        &self,
        engine: &mut AppEngine,
        command: DomainCommand,
    ) -> Result<DomainCommandResult, MobileError> {
        match command {
            DomainCommand::GrantConsent { consent_type } => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::ConsentManager::new(storage);
                let id = engine.vauchi().rng().uuid_v4();
                manager
                    .grant(id, vauchi_core::api::ConsentType::from(consent_type))
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::RevokeConsent { consent_type } => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::ConsentManager::new(storage);
                let id = engine.vauchi().rng().uuid_v4();
                manager
                    .revoke(id, vauchi_core::api::ConsentType::from(consent_type))
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::CheckConsent { consent_type } => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::ConsentManager::new(storage);
                let value = manager
                    .check(&vauchi_core::api::ConsentType::from(consent_type))
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Bool { value })
            }
            DomainCommand::GetConsentStatus { consent_type } => {
                let status = engine
                    .vauchi()
                    .get_consent_status(vauchi_core::api::ConsentType::from(consent_type))
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::ConsentStatus {
                    status: crate::types::MobileConsentStatus::from(status),
                })
            }
            DomainCommand::GetConsentRecords => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::ConsentManager::new(storage);
                let records =
                    manager
                        .export_consent_log_with_version()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::ConsentRecords {
                    records: records
                        .iter()
                        .map(crate::types::MobileConsentRecord::from)
                        .collect(),
                })
            }

            // ── Content Updates (B7 batch 2) ──
            DomainCommand::RunContentUpdateCycle => {
                let outcome = self.run_content_update_cycle_dispatch();
                if outcome.applied {
                    invalidate_content_screens(engine);
                }
                Ok(DomainCommandResult::ContentUpdateCycle { outcome })
            }

            // ── Aha Moments (B7 batch 5) ──
            DomainCommand::HasSeenAhaMoment { moment_type } => {
                let tracker = self.load_aha_tracker_engine();
                Ok(DomainCommandResult::Bool {
                    value: tracker.has_seen(moment_type.into()),
                })
            }
            DomainCommand::TryTriggerAhaMoment { moment_type } => {
                let mut tracker = self.load_aha_tracker_engine();
                let core_type: vauchi_core::AhaMomentType = moment_type.into();
                let moment = if let Some(m) = tracker.try_trigger(core_type) {
                    self.save_aha_tracker_engine(&tracker)?;
                    Some(crate::types::MobileAhaMoment {
                        moment_type,
                        title: m.title().to_string(),
                        message: m.message(),
                        has_animation: m.has_animation(),
                    })
                } else {
                    None
                };
                Ok(DomainCommandResult::AhaMomentOpt { moment })
            }
            DomainCommand::TryTriggerAhaMomentWithContext {
                moment_type,
                context,
            } => {
                let mut tracker = self.load_aha_tracker_engine();
                let core_type: vauchi_core::AhaMomentType = moment_type.into();
                let moment = if let Some(m) = tracker.try_trigger_with_context(core_type, context) {
                    self.save_aha_tracker_engine(&tracker)?;
                    Some(crate::types::MobileAhaMoment {
                        moment_type,
                        title: m.title().to_string(),
                        message: m.message(),
                        has_animation: m.has_animation(),
                    })
                } else {
                    None
                };
                Ok(DomainCommandResult::AhaMomentOpt { moment })
            }
            DomainCommand::AhaMomentsSeenCount => {
                let tracker = self.load_aha_tracker_engine();
                Ok(DomainCommandResult::Count {
                    value: tracker.seen_count() as u32,
                })
            }
            DomainCommand::AhaMomentsTotalCount => {
                let tracker = self.load_aha_tracker_engine();
                Ok(DomainCommandResult::Count {
                    value: tracker.total_count() as u32,
                })
            }
            DomainCommand::ResetAhaMoments => {
                let mut tracker = self.load_aha_tracker_engine();
                tracker.reset();
                self.save_aha_tracker_engine(&tracker)?;
                Ok(DomainCommandResult::Unit)
            }

            // ── Demo Contact (B7 batch 5) ──
            DomainCommand::InitDemoContactIfNeeded => {
                let storage = engine.vauchi().storage();
                let contacts =
                    storage
                        .contacts()
                        .list_contacts()
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })?;
                if !contacts.is_empty() {
                    return Ok(DomainCommandResult::DemoContactOpt { contact: None });
                }
                let mut state = self.load_demo_state_engine();
                if state.was_dismissed || state.auto_removed {
                    return Ok(DomainCommandResult::DemoContactOpt { contact: None });
                }
                if !state.is_active {
                    state = vauchi_core::DemoContactState::new_active(
                        engine.vauchi().clock().unix_seconds(),
                    );
                    self.save_demo_state_engine(&state)?;
                }
                let contact = state.current_tip().map(|tip| {
                    let card = vauchi_core::generate_demo_contact_card(&tip);
                    card.into()
                });
                Ok(DomainCommandResult::DemoContactOpt { contact })
            }
            DomainCommand::GetDemoContact => {
                let state = self.load_demo_state_engine();
                let contact = if state.is_active {
                    state.current_tip().map(|tip| {
                        let card = vauchi_core::generate_demo_contact_card(&tip);
                        card.into()
                    })
                } else {
                    None
                };
                Ok(DomainCommandResult::DemoContactOpt { contact })
            }
            DomainCommand::GetDemoContactState => {
                let state = self.load_demo_state_engine();
                Ok(DomainCommandResult::DemoContactState {
                    state: crate::types::MobileDemoContactState {
                        is_active: state.is_active,
                        was_dismissed: state.was_dismissed,
                        auto_removed: state.auto_removed,
                        update_count: state.update_count,
                    },
                })
            }
            DomainCommand::IsDemoUpdateAvailable => {
                let state = self.load_demo_state_engine();
                Ok(DomainCommandResult::Bool {
                    value: state.is_update_due(engine.vauchi().clock().unix_seconds()),
                })
            }
            DomainCommand::TriggerDemoUpdate => {
                let mut state = self.load_demo_state_engine();
                if !state.is_active {
                    return Ok(DomainCommandResult::DemoContactOpt { contact: None });
                }
                let contact = if let Some(tip) =
                    state.advance_to_next_tip(engine.vauchi().clock().unix_seconds())
                {
                    self.save_demo_state_engine(&state)?;
                    let card = vauchi_core::generate_demo_contact_card(&tip);
                    Some(card.into())
                } else {
                    None
                };
                Ok(DomainCommandResult::DemoContactOpt { contact })
            }
            DomainCommand::DismissDemoContact => {
                let mut state = self.load_demo_state_engine();
                state.dismiss();
                self.save_demo_state_engine(&state)?;
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::AutoRemoveDemoContact => {
                let mut state = self.load_demo_state_engine();
                let removed = if state.is_active {
                    state.auto_remove();
                    self.save_demo_state_engine(&state)?;
                    true
                } else {
                    false
                };
                Ok(DomainCommandResult::Bool { value: removed })
            }
            DomainCommand::RestoreDemoContact => {
                let mut state = self.load_demo_state_engine();
                state.restore();
                self.save_demo_state_engine(&state)?;
                let contact = state.current_tip().map(|tip| {
                    let card = vauchi_core::generate_demo_contact_card(&tip);
                    card.into()
                });
                Ok(DomainCommandResult::DemoContactOpt { contact })
            }

            // ── Contact Card + CRUD (B7 batch 10) ──
            //
            // Cache invalidation: own-card writes invalidate `MyInfo`;
            // contact writes invalidate `Contacts` + the specific
            // `ContactDetail { contact_id }` (where applicable);
            // archive writes invalidate `ArchivedContacts`. Reads
            // invalidate nothing.
            other => {
                unreachable!("non-engagement command {other:?} routed to engagement dispatcher")
            }
        }
    }
}

/// A completed `RunContentUpdateCycle` refreshes the on-disk content
/// cache; every screen reading social-network labels re-renders. Named
/// helper so the invalidation set stays in one place when a new content
/// type needs a wider one.
fn invalidate_content_screens(engine: &mut AppEngine) {
    engine.invalidate_screen(&AppScreen::Settings);
    engine.invalidate_screen(&AppScreen::MyInfo);
}

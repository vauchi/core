// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `contacts` arm group of [`PlatformAppEngine::dispatch_domain_command`] —
//! split out of `platform_app_engine.rs` (pure code motion).

use vauchi_app::ui::{AppEngine, AppScreen};

use crate::domain_command::{DomainCommand, DomainCommandResult};
use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

impl PlatformAppEngine {
    pub(crate) fn dispatch_contacts(
        &self,
        engine: &mut AppEngine,
        command: DomainCommand,
    ) -> Result<DomainCommandResult, MobileError> {
        match command {
            DomainCommand::ListContacts => {
                let storage = engine.vauchi().storage();
                let contacts =
                    storage
                        .contacts()
                        .list_contacts()
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::Contacts {
                    contacts: crate::mobile_contacts::enrich_contacts_batch(storage, &contacts),
                })
            }
            DomainCommand::GetContact { id } => {
                let storage = engine.vauchi().storage();
                let contact = storage.contacts().load_contact(&id).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                Ok(DomainCommandResult::ContactOpt {
                    contact: contact
                        .as_ref()
                        .map(|c| crate::mobile_contacts::enrich_contact(storage, c)),
                })
            }
            DomainCommand::SearchContacts { query } => {
                let storage = engine.vauchi().storage();
                let contacts = storage.contacts().search_contacts(&query).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                Ok(DomainCommandResult::Contacts {
                    contacts: crate::mobile_contacts::enrich_contacts_batch(storage, &contacts),
                })
            }
            DomainCommand::ContactCount => {
                let count = engine
                    .vauchi()
                    .storage()
                    .contacts()
                    .list_contacts()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .len() as u32;
                Ok(DomainCommandResult::Count { value: count })
            }
            DomainCommand::RemoveContact { id } => {
                let removed = engine.vauchi().storage().delete_contact(&id).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                if removed {
                    engine.invalidate_screen(&AppScreen::Contacts);
                    engine.invalidate_screen(&AppScreen::ContactDetail {
                        contact_id: id.clone(),
                    });
                }
                Ok(DomainCommandResult::Bool { value: removed })
            }
            DomainCommand::SoftDeleteImportedContact { id } => {
                engine
                    .vauchi()
                    .soft_delete_imported_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::UndoDeleteImportedContact { id } => {
                engine
                    .vauchi()
                    .undo_delete_imported_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::HardDeleteImportedContact { id } => {
                engine
                    .vauchi()
                    .hard_delete_imported_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ArchiveContact { id } => {
                engine
                    .vauchi()
                    .archive_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::UnarchiveContact { id } => {
                engine
                    .vauchi()
                    .unarchive_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ListArchivedContacts => {
                let storage = engine.vauchi().storage();
                let contacts =
                    engine
                        .vauchi()
                        .list_archived_contacts()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::Contacts {
                    contacts: crate::mobile_contacts::enrich_contacts_batch(storage, &contacts),
                })
            }
            DomainCommand::HideContact { contact_id } => {
                engine
                    .vauchi()
                    .hide_contact(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::UnhideContact { contact_id } => {
                engine
                    .vauchi()
                    .unhide_contact(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }

            // ── GDPR / Deletion + shred-status (B7 batch 3) ──
            DomainCommand::VerifyContact { id } => {
                let storage = engine.vauchi().storage();
                let mut contact = storage
                    .contacts()
                    .load_contact(&id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {id}"),
                    })?;
                contact
                    .mark_fingerprint_verified()
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage.contacts().save_contact(&contact).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetProposalTrusted {
                contact_id,
                trusted,
            } => {
                let storage = engine.vauchi().storage();
                let mut contact = storage
                    .contacts()
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {contact_id}"),
                    })?;
                contact
                    .set_proposal_trusted(trusted)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage.contacts().save_contact(&contact).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::FindDuplicates => {
                let pairs = engine
                    .vauchi()
                    .find_duplicates()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DuplicatePairs {
                    pairs: pairs
                        .into_iter()
                        .map(|p| crate::types::MobileDuplicatePair {
                            id1: p.id1,
                            id2: p.id2,
                            similarity: p.similarity,
                        })
                        .collect(),
                })
            }
            DomainCommand::DismissDuplicate { id1, id2 } => {
                engine
                    .vauchi()
                    .dismiss_duplicate(&id1, &id2)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDuplicates);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetContactNote { contact_id, note } => {
                engine
                    .vauchi()
                    .storage()
                    .contacts()
                    .save_personal_notes(&contact_id, note.as_bytes())
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetContactNote { contact_id } => {
                let bytes = engine
                    .vauchi()
                    .storage()
                    .contacts()
                    .load_personal_notes(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::StringOpt {
                    value: bytes.and_then(|b| String::from_utf8(b).ok()),
                })
            }
            DomainCommand::DeleteContactNote { contact_id } => {
                engine
                    .vauchi()
                    .storage()
                    .contacts()
                    .delete_personal_notes(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetContactFieldNote {
                contact_id,
                field_id,
                note,
            } => {
                engine
                    .vauchi()
                    .storage()
                    .field_notes()
                    .save_contact_field_note(&contact_id, &field_id, note.as_bytes())
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetContactFieldNotes { contact_id } => {
                let map = engine
                    .vauchi()
                    .storage()
                    .field_notes()
                    .load_contact_field_notes(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let mut notes: Vec<crate::types::MobileFieldNote> = map
                    .into_iter()
                    .filter_map(|(field_id, bytes)| {
                        String::from_utf8(bytes)
                            .ok()
                            .map(|note| crate::types::MobileFieldNote { field_id, note })
                    })
                    .collect();
                notes.sort_by(|a, b| a.field_id.cmp(&b.field_id));
                Ok(DomainCommandResult::FieldNotes { notes })
            }
            DomainCommand::DeleteContactFieldNote {
                contact_id,
                field_id,
            } => {
                engine
                    .vauchi()
                    .storage()
                    .field_notes()
                    .delete_contact_field_note(&contact_id, &field_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetContactNickname { contact_id, name } => {
                engine
                    .vauchi()
                    .set_contact_nickname(&contact_id, &name)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ClearContactNickname { contact_id } => {
                engine
                    .vauchi()
                    .clear_contact_nickname(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetContactCustomAvatar { contact_id, data } => {
                engine
                    .vauchi()
                    .set_contact_custom_avatar(&contact_id, &data)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ClearContactCustomAvatar { contact_id } => {
                engine
                    .vauchi()
                    .clear_contact_custom_avatar(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetContactCustomAvatar { contact_id } => {
                let data = engine
                    .vauchi()
                    .get_contact_custom_avatar(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::AvatarOpt { data })
            }
            DomainCommand::SearchSocialNetworks { query } => {
                let registry = vauchi_core::SocialNetworkRegistry::with_defaults();
                let networks = registry
                    .search(&query)
                    .iter()
                    .map(|sn| crate::types::MobileSocialNetwork {
                        id: sn.id().to_string(),
                        display_name: sn.display_name().to_string(),
                        url_template: sn.profile_url_template().to_string(),
                    })
                    .collect();
                Ok(DomainCommandResult::SocialNetworks { networks })
            }
            DomainCommand::GetProfileUrl {
                network_id,
                username,
            } => {
                let registry = vauchi_core::SocialNetworkRegistry::with_defaults();
                Ok(DomainCommandResult::StringOpt {
                    value: registry.profile_url(&network_id, &username),
                })
            }
            DomainCommand::ListHiddenContacts => {
                let storage = engine.vauchi().storage();
                let contacts =
                    engine
                        .vauchi()
                        .list_hidden_contacts()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::Contacts {
                    contacts: crate::mobile_contacts::enrich_contacts_batch(storage, &contacts),
                })
            }
            DomainCommand::ContactDetailFooterActionId { contact_id } => {
                let contact = engine
                    .vauchi()
                    .storage()
                    .contacts()
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: "contact_id".into(),
                        detail: format!("contact not found: {contact_id}"),
                    })?;
                let value = vauchi_app::ui::contact_detail_footer_action_id(contact.is_imported())
                    .to_string();
                Ok(DomainCommandResult::Text { value })
            }

            // ── Backup + Import (B7 batch 12) ──
            DomainCommand::SetDisplayNamePreference {
                contact_id,
                pref_json,
            } => {
                let pref: vauchi_core::DisplayNamePreference = serde_json::from_str(&pref_json)
                    .map_err(|e| MobileError::InvalidInput {
                        field: "pref_json".into(),
                        detail: format!("Invalid preference JSON: {e}"),
                    })?;
                engine
                    .vauchi()
                    .set_display_name_preference(&contact_id, pref)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetAvatarPreference {
                contact_id,
                pref_json,
            } => {
                let pref: vauchi_core::AvatarPreference = serde_json::from_str(&pref_json)
                    .map_err(|e| MobileError::InvalidInput {
                        field: "pref_json".into(),
                        detail: format!("Invalid preference JSON: {e}"),
                    })?;
                engine
                    .vauchi()
                    .set_avatar_preference(&contact_id, pref)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::MergeContacts {
                primary_id,
                secondary_id,
            } => {
                let merged = engine
                    .vauchi()
                    .merge_contacts(&primary_id, &secondary_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                let storage = engine.vauchi().storage();
                let contact = crate::mobile_contacts::enrich_contact(storage, &merged);
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDuplicates);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: primary_id.clone(),
                });
                Ok(DomainCommandResult::ContactSingle { contact })
            }

            // ── Onboarding state ops (B7 batch 16) ──
            DomainCommand::GetContactDisplayOptions { contact_id } => {
                let opts = engine
                    .vauchi()
                    .get_contact_display_options(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                let names = opts
                    .names
                    .into_iter()
                    .map(|n| {
                        Ok(crate::types::MobileNameOption {
                            source: serde_json::to_string(&n.source).map_err(|e| {
                                MobileError::Other {
                                    detail: format!("Serialize name source: {e}"),
                                }
                            })?,
                            name: n.name,
                            is_primary: n.is_primary,
                        })
                    })
                    .collect::<Result<Vec<_>, MobileError>>()?;
                let avatars = opts
                    .avatars
                    .into_iter()
                    .map(|a| {
                        Ok(crate::types::MobileAvatarOption {
                            source: serde_json::to_string(&a.source).map_err(|e| {
                                MobileError::Other {
                                    detail: format!("Serialize avatar source: {e}"),
                                }
                            })?,
                            has_data: a.has_data,
                            is_primary: a.is_primary,
                        })
                    })
                    .collect::<Result<Vec<_>, MobileError>>()?;
                let active_name_preference = serde_json::to_string(&opts.active_name_preference)
                    .map_err(|e| MobileError::Other {
                        detail: format!("Serialize name pref: {e}"),
                    })?;
                let active_avatar_preference =
                    serde_json::to_string(&opts.active_avatar_preference).map_err(|e| {
                        MobileError::Other {
                            detail: format!("Serialize avatar pref: {e}"),
                        }
                    })?;
                Ok(DomainCommandResult::ContactDisplayOptions {
                    options: crate::types::MobileContactDisplayOptions {
                        names,
                        avatars,
                        active_name_preference,
                        active_avatar_preference,
                    },
                })
            }
            DomainCommand::ListContactsPaginated { offset, limit } => {
                let storage = engine.vauchi().storage();
                let contacts = storage
                    .contacts()
                    .list_contacts_paginated(offset as usize, limit as usize)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Contacts {
                    contacts: crate::mobile_contacts::enrich_contacts_batch(storage, &contacts),
                })
            }

            // ── Contact detail view state + social registry (B7 batch 19) ──
            DomainCommand::ContactDetailViewState { contact_id } => {
                use vauchi_app::i18n::Locale;
                use vauchi_app::ui::{
                    ReciprocityBannerKind, reciprocity_banner, show_recovery_trusted_indicator,
                    show_verified_badge, verify_button_visible,
                };
                let storage = engine.vauchi().storage();
                let contact = storage
                    .contacts()
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: "contact_id".to_string(),
                        detail: format!("contact not found: {contact_id}"),
                    })?;

                let mut badges = Vec::new();
                if show_verified_badge(contact.is_fingerprint_verified()) {
                    badges.push(crate::mobile_contact_detail::MobileContactDetailBadge::Verified);
                }
                if show_recovery_trusted_indicator(contact.is_recovery_trusted()) {
                    badges.push(
                        crate::mobile_contact_detail::MobileContactDetailBadge::RecoveryTrusted,
                    );
                }

                let mut banners = Vec::new();
                if let Some(kind) = reciprocity_banner(contact.reciprocity(0)) {
                    banners.push(match kind {
                        ReciprocityBannerKind::Pending => {
                            crate::mobile_contact_detail::MobileContactDetailBanner::ReciprocityPending {
                                label: "Waiting for them to share their info".to_string(),
                            }
                        }
                        ReciprocityBannerKind::Unreciprocated => {
                            crate::mobile_contact_detail::MobileContactDetailBanner::ReciprocityUnreciprocated {
                                label: "They haven't shared their info".to_string(),
                            }
                        }
                    });
                }

                let mut actions = Vec::new();
                if verify_button_visible(contact.is_fingerprint_verified(), contact.trust_level()) {
                    actions.push(crate::mobile_contact_detail::MobileContactDetailAction::Verify);
                }
                actions.push(
                    crate::mobile_contact_detail::MobileContactDetailAction::ToggleRecoveryTrust {
                        currently_trusted: contact.is_recovery_trusted(),
                    },
                );
                actions.push(
                    crate::mobile_contact_detail::MobileContactDetailAction::ToggleHidden {
                        currently_hidden: contact.is_hidden(),
                    },
                );
                actions.push(crate::mobile_contact_detail::MobileContactDetailAction::Edit);
                actions.push(
                    crate::mobile_contact_detail::MobileContactDetailAction::VerifyFingerprint,
                );
                actions.push(
                    crate::mobile_contact_detail::MobileContactDetailAction::PreviewAs {
                        contact_id: contact_id.clone(),
                    },
                );
                if contact.is_imported() {
                    actions.push(crate::mobile_contact_detail::MobileContactDetailAction::Delete);
                } else {
                    actions.push(crate::mobile_contact_detail::MobileContactDetailAction::Archive);
                }
                actions.push(crate::mobile_contact_detail::MobileContactDetailAction::Back);

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let added_time_display = crate::mobile_contact_detail::compute_added_time_display(
                    &contact,
                    now,
                    Locale::English,
                );

                Ok(DomainCommandResult::ContactDetailView {
                    state: crate::mobile_contact_detail::MobileContactDetailViewState {
                        badges,
                        banners,
                        actions,
                        added_time_display,
                    },
                })
            }
            DomainCommand::ListSocialNetworks => {
                let registry = vauchi_core::SocialNetworkRegistry::with_defaults();
                let networks = registry
                    .all()
                    .iter()
                    .map(|sn| crate::types::MobileSocialNetwork {
                        id: sn.id().to_string(),
                        display_name: sn.display_name().to_string(),
                        url_template: sn.profile_url_template().to_string(),
                    })
                    .collect();
                Ok(DomainCommandResult::SocialNetworks { networks })
            }

            // ── Multipart QR encoding (B7 batch 20) ──
            other => unreachable!("non-contacts command {other:?} routed to contacts dispatcher"),
        }
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `groups_visibility` arm group of [`PlatformAppEngine::dispatch_domain_command`] —
//! split out of `platform_app_engine.rs` (pure code motion).

use vauchi_app::ui::{AppEngine, AppScreen};

use crate::domain_command::{DomainCommand, DomainCommandResult};
use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

/// Maps `Vauchi` visibility errors onto the exact `MobileError`
/// shapes the inline arm bodies produced pre-G3, so frontend-visible
/// error text is unchanged.
fn map_visibility_error(e: vauchi_core::VauchiError) -> MobileError {
    use vauchi_core::VauchiError;
    match e {
        VauchiError::NotFound(detail) if detail.starts_with("contact: ") => MobileError::Other {
            detail: format!("Contact not found: {}", &detail["contact: ".len()..]),
        },
        VauchiError::NotFound(detail) if detail.starts_with("field: ") => {
            MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Field not found: {}", &detail["field: ".len()..]),
            }
        }
        VauchiError::IdentityNotInitialized => MobileError::Other {
            detail: "Identity not found".into(),
        },
        VauchiError::InvalidState(detail) => MobileError::InvalidInput {
            field: String::new(),
            detail,
        },
        other => MobileError::StorageError {
            detail: other.to_string(),
        },
    }
}

impl PlatformAppEngine {
    pub(crate) fn dispatch_groups_visibility(
        &self,
        engine: &mut AppEngine,
        command: DomainCommand,
    ) -> Result<DomainCommandResult, MobileError> {
        match command {
            DomainCommand::ListLabels => {
                let labels = engine
                    .vauchi()
                    .storage()
                    .labels()
                    .load_all_groups()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Labels {
                    labels: labels
                        .iter()
                        .map(crate::types::MobileVisibilityLabel::from)
                        .collect(),
                })
            }
            DomainCommand::CreateLabel { name } => {
                let label = engine
                    .vauchi()
                    .storage()
                    .labels()
                    .create_group(&name)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Groups);
                Ok(DomainCommandResult::Label {
                    label: crate::types::MobileVisibilityLabel::from(&label),
                })
            }
            DomainCommand::GetLabel { label_id } => {
                let storage = engine.vauchi().storage();
                let label = storage.labels().load_group(&label_id).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                let mut detail = crate::types::MobileVisibilityLabelDetail::from(&label);
                let (rows, stale_count) =
                    crate::mobile_visibility::resolve_label_contacts(storage, &detail.contact_ids);
                detail.label_contacts = rows;
                detail.stale_reference_count = stale_count;
                Ok(DomainCommandResult::LabelDetail { detail })
            }
            DomainCommand::RenameLabel { label_id, new_name } => {
                engine
                    .vauchi()
                    .storage()
                    .labels()
                    .rename_group(&label_id, &new_name)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Groups);
                engine.invalidate_screen(&AppScreen::GroupDetail {
                    group_id: label_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::DeleteLabel { label_id } => {
                engine
                    .vauchi()
                    .storage()
                    .labels()
                    .delete_group(&label_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Groups);
                engine.invalidate_screen(&AppScreen::GroupDetail {
                    group_id: label_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::AddContactToGroup {
                label_id,
                contact_id,
            } => {
                engine
                    .vauchi()
                    .storage()
                    .labels()
                    .add_contact_to_group(&label_id, &contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Groups);
                engine.invalidate_screen(&AppScreen::GroupDetail {
                    group_id: label_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::RemoveContactFromGroup {
                label_id,
                contact_id,
            } => {
                engine
                    .vauchi()
                    .storage()
                    .labels()
                    .remove_contact_from_group(&label_id, &contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Groups);
                engine.invalidate_screen(&AppScreen::GroupDetail {
                    group_id: label_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetGroupsForContact { contact_id } => {
                let labels = engine
                    .vauchi()
                    .storage()
                    .labels()
                    .get_groups_for_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Labels {
                    labels: labels
                        .iter()
                        .map(crate::types::MobileVisibilityLabel::from)
                        .collect(),
                })
            }
            DomainCommand::SetGroupFieldVisibility {
                label_id,
                field_label,
                is_visible,
            } => {
                let storage = engine.vauchi().storage();
                let card = storage
                    .contacts()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == field_label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field not found: {field_label}"),
                    })?
                    .id()
                    .to_string();
                storage
                    .labels()
                    .set_group_field_visibility(&label_id, &field_id, is_visible)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::GroupDetail {
                    group_id: label_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetContactFieldOverride {
                contact_id,
                field_label,
                is_visible,
            } => {
                let storage = engine.vauchi().storage();
                let card = storage
                    .contacts()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == field_label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field not found: {field_label}"),
                    })?
                    .id()
                    .to_string();
                storage
                    .labels()
                    .save_contact_override(&contact_id, &field_id, is_visible)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactVisibility {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::RemoveContactFieldOverride {
                contact_id,
                field_label,
            } => {
                let storage = engine.vauchi().storage();
                let card = storage
                    .contacts()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == field_label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field not found: {field_label}"),
                    })?
                    .id()
                    .to_string();
                storage
                    .labels()
                    .delete_contact_override(&contact_id, &field_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactVisibility {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::HideFieldFromContact {
                contact_id,
                field_label,
            } => {
                engine
                    .vauchi()
                    .set_field_visibility_by_label(&contact_id, &field_label, false)
                    .map_err(map_visibility_error)?;
                engine.invalidate_screen(&AppScreen::ContactVisibility {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ShowFieldToContact {
                contact_id,
                field_label,
            } => {
                engine
                    .vauchi()
                    .set_field_visibility_by_label(&contact_id, &field_label, true)
                    .map_err(map_visibility_error)?;
                engine.invalidate_screen(&AppScreen::ContactVisibility {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::IsFieldVisibleToContact {
                contact_id,
                field_label,
            } => {
                let visible = engine
                    .vauchi()
                    .is_field_visible_by_label(&contact_id, &field_label)
                    .map_err(map_visibility_error)?;
                Ok(DomainCommandResult::Bool { value: visible })
            }
            DomainCommand::GetSuggestedLabels => {
                let values = vauchi_core::SUGGESTED_LABELS
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                Ok(DomainCommandResult::Strings { values })
            }

            // ── Passcode + Duress + Decoy (B7 batch 7) ──
            //
            // The legacy VauchiPlatform code calls `set_identity` per
            // method because each call opens a fresh Vauchi instance.
            // PlatformAppEngine's persistent Vauchi already holds the
            // identity from construction, so the wrappers can call the
            // password / duress methods directly without
            // re-installation.
            other => unreachable!(
                "non-groups_visibility command {other:?} routed to groups_visibility dispatcher"
            ),
        }
    }
}

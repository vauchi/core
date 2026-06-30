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

/// Resolves an own-card field's id from its label, mapping the not-found and
/// no-identity cases to the `MobileError` shapes the visibility arms produced
/// pre-G3 (so frontend-visible error text is unchanged).
fn resolve_own_field_id(engine: &AppEngine, field_label: &str) -> Result<String, MobileError> {
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
    card.fields()
        .iter()
        .find(|f| f.label() == field_label)
        .map(|f| f.id().to_string())
        .ok_or_else(|| MobileError::InvalidInput {
            field: String::new(),
            detail: format!("Field not found: {field_label}"),
        })
}

/// Hide, show, and per-contact override are one operation: set the contact's
/// Layer-C override (the layer the effective resolver reads) and repropagate
/// so the revocation/grant reaches the peer. Routing the bare Layer-A
/// `set_field_visibility_by_label` here was the visibility leak — for a grouped
/// contact the resolver ignored it, so a "hide" never reached the wire
/// (`2026-06-14-visibility-changes-not-fully-propagated`).
fn set_contact_override(
    engine: &mut AppEngine,
    contact_id: &str,
    field_label: &str,
    is_visible: bool,
) -> Result<DomainCommandResult, MobileError> {
    let field_id = resolve_own_field_id(engine, field_label)?;
    engine
        .vauchi()
        .set_contact_visibility_override_and_repropagate(contact_id, &field_id, is_visible)
        .map_err(map_visibility_error)?;
    engine.invalidate_screen(&AppScreen::ContactVisibility {
        contact_id: contact_id.to_string(),
    });
    engine.invalidate_screen(&AppScreen::ContactDetail {
        contact_id: contact_id.to_string(),
    });
    Ok(DomainCommandResult::Unit)
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
                    .delete_group_and_repropagate(&label_id)
                    .map_err(map_visibility_error)?;
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
                    .add_contact_to_group_and_repropagate(&label_id, &contact_id)
                    .map_err(map_visibility_error)?;
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
                    .remove_contact_from_group_and_repropagate(&label_id, &contact_id)
                    .map_err(map_visibility_error)?;
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
                let field_id = resolve_own_field_id(engine, &field_label)?;
                engine
                    .vauchi()
                    .set_group_field_visibility_and_repropagate(&label_id, &field_id, is_visible)
                    .map_err(map_visibility_error)?;
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
            } => set_contact_override(engine, &contact_id, &field_label, is_visible),
            DomainCommand::RemoveContactFieldOverride {
                contact_id,
                field_label,
            } => {
                let field_id = resolve_own_field_id(engine, &field_label)?;
                engine
                    .vauchi()
                    .remove_contact_visibility_override_and_repropagate(&contact_id, &field_id)
                    .map_err(map_visibility_error)?;
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
            } => set_contact_override(engine, &contact_id, &field_label, false),
            DomainCommand::ShowFieldToContact {
                contact_id,
                field_label,
            } => set_contact_override(engine, &contact_id, &field_label, true),
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

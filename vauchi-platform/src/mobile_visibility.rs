// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility operations and labels for mobile.

use super::VauchiPlatform;
use super::error::MobileError;
use super::types::{
    MobileLabelContactRow, MobileLabelContactStatus, MobileVisibilityLabel,
    MobileVisibilityLabelDetail,
};

/// Resolve raw contact IDs against storage into rendered rows.
///
/// Active contacts produce `MobileLabelContactRow` entries with the same
/// display-name pipeline `enrich_contact()` uses for `list_contacts` (so a
/// contact with a nickname renders the same in both surfaces). Missing or
/// errored IDs are dropped from the rows and counted in the second tuple
/// member; this is the conservative default per the planning record's
/// missing-contact policy decision (`omit + stale_reference_count`).
///
/// Order is preserved: the i-th row corresponds to the next active id from
/// `contact_ids` left-to-right. The invariant
/// `rows.len() + stale_count as usize == contact_ids.len()` is verified in
/// `mobile_visibility_resolve_tests`.
fn resolve_label_contacts(
    storage: &vauchi_core::Storage,
    contact_ids: &[String],
) -> (Vec<MobileLabelContactRow>, u32) {
    let mut rows = Vec::with_capacity(contact_ids.len());
    let mut stale: u32 = 0;

    for id in contact_ids {
        match storage.load_contact(id) {
            Ok(Some(contact)) => {
                let nickname = storage.load_contact_nickname(id).ok().flatten();
                let shared_names = storage.list_shared_names(id).unwrap_or_default();
                let (name_pref, _) = storage.load_display_preferences(id).unwrap_or((
                    vauchi_core::DisplayNamePreference::Primary,
                    vauchi_core::AvatarPreference::Primary,
                ));
                let display_name = vauchi_core::contact::display::resolve_display_name(
                    contact.display_name(),
                    &name_pref,
                    &shared_names,
                    nickname.as_deref(),
                );
                rows.push(MobileLabelContactRow {
                    id: id.clone(),
                    display_name,
                    trust_level: contact.trust_level().into(),
                    status: MobileLabelContactStatus::Active,
                });
            }
            // Missing or error → omit from rows and bump stale_reference_count.
            // Per the planning record (G2 missing-contact policy default):
            // never expose unresolved contact IDs to the UI; surface the
            // count instead so the frontend can render a footer hint.
            _ => {
                stale = stale.saturating_add(1);
            }
        }
    }

    (rows, stale)
}

#[uniffi::export]
impl VauchiPlatform {
    // === Visibility Operations ===

    /// Hide field from contact.
    pub fn hide_field_from_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::Other {
                detail: format!("Contact not found: {}", contact_id.clone()),
            })?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            detail: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Field not found: {}", field_label),
            })?;

        contact
            .visibility_rules_mut()
            .ok_or(MobileError::InvalidInput {
                field: String::new(),
                detail: "Visibility rules require an exchanged contact".to_string(),
            })?
            .set_nobody(field.id());
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Show field to contact.
    pub fn show_field_to_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::Other {
                detail: format!("Contact not found: {}", contact_id.clone()),
            })?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            detail: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Field not found: {}", field_label),
            })?;

        contact
            .visibility_rules_mut()
            .ok_or(MobileError::InvalidInput {
                field: String::new(),
                detail: "Visibility rules require an exchanged contact".to_string(),
            })?
            .set_everyone(field.id());
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Check if field is visible to contact.
    pub fn is_field_visible_to_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;

        let contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::Other {
                detail: format!("Contact not found: {}", contact_id.clone()),
            })?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            detail: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Field not found: {}", field_label),
            })?;

        Ok(contact
            .visibility_rules()
            .is_some_and(|rules| rules.can_see(field.id(), &contact_id)))
    }

    // === Visibility Labels ===

    /// List all visibility labels.
    pub fn list_labels(&self) -> Result<Vec<MobileVisibilityLabel>, MobileError> {
        let storage = self.open_storage()?;
        let labels = storage.load_all_groups()?;
        Ok(labels.iter().map(MobileVisibilityLabel::from).collect())
    }

    /// Create a new visibility label.
    pub fn create_label(&self, name: String) -> Result<MobileVisibilityLabel, MobileError> {
        let storage = self.open_storage()?;
        let label = storage.create_group(&name)?;
        Ok(MobileVisibilityLabel::from(&label))
    }

    /// Get a label by ID with full details.
    ///
    /// Populates `label_contacts` and `stale_reference_count` by resolving
    /// the label's `contact_ids` against storage — frontends should render
    /// `label_contacts` instead of joining `contact_ids` against the
    /// contacts list themselves (ADR-021/043 Humble UI). See
    /// `resolve_label_contacts` for the missing-contact policy.
    pub fn get_label(&self, label_id: String) -> Result<MobileVisibilityLabelDetail, MobileError> {
        let storage = self.open_storage()?;
        let label = storage.load_group(&label_id)?;
        let mut detail = MobileVisibilityLabelDetail::from(&label);
        let (rows, stale_count) = resolve_label_contacts(&storage, &detail.contact_ids);
        detail.label_contacts = rows;
        detail.stale_reference_count = stale_count;
        Ok(detail)
    }

    /// Rename a label.
    pub fn rename_label(&self, label_id: String, new_name: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.rename_group(&label_id, &new_name)?;
        Ok(())
    }

    /// Delete a label.
    pub fn delete_label(&self, label_id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.delete_group(&label_id)?;
        Ok(())
    }

    /// Add a contact to a label.
    pub fn add_contact_to_group(
        &self,
        label_id: String,
        contact_id: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.add_contact_to_group(&label_id, &contact_id)?;
        Ok(())
    }

    /// Remove a contact from a label.
    pub fn remove_contact_from_group(
        &self,
        label_id: String,
        contact_id: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.remove_contact_from_group(&label_id, &contact_id)?;
        Ok(())
    }

    /// Get all labels that contain a contact.
    pub fn get_groups_for_contact(
        &self,
        contact_id: String,
    ) -> Result<Vec<MobileVisibilityLabel>, MobileError> {
        let storage = self.open_storage()?;
        let labels = storage.get_groups_for_contact(&contact_id)?;
        Ok(labels.iter().map(MobileVisibilityLabel::from).collect())
    }

    /// Set whether a field is visible to contacts in a label.
    pub fn set_group_field_visibility(
        &self,
        label_id: String,
        field_label: String,
        is_visible: bool,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            detail: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Field not found: {}", field_label),
            })?;

        storage.set_group_field_visibility(&label_id, field.id(), is_visible)?;
        Ok(())
    }

    /// Set a per-contact override for field visibility.
    ///
    /// Per-contact overrides take precedence over label-based visibility.
    pub fn set_contact_field_override(
        &self,
        contact_id: String,
        field_label: String,
        is_visible: bool,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            detail: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Field not found: {}", field_label),
            })?;

        storage.save_contact_override(&contact_id, field.id(), is_visible)?;
        Ok(())
    }

    /// Remove a per-contact override for field visibility.
    pub fn remove_contact_field_override(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let card = storage.load_own_card()?.ok_or(MobileError::Other {
            detail: "Identity not found".to_string(),
        })?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Field not found: {}", field_label),
            })?;

        storage.delete_contact_override(&contact_id, field.id())?;
        Ok(())
    }

    /// Get suggested default labels.
    pub fn get_suggested_labels(&self) -> Vec<String> {
        vauchi_core::SUGGESTED_LABELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
}

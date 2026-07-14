// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility labels and field visibility management.

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

impl Vauchi {
    pub(crate) fn record_group_change(&self, group: &crate::contact::Group) {
        self.record_sync_item(crate::sync::SyncItem::GroupChanged {
            group_data: crate::sync::GroupSyncData::from_group(group),
            timestamp: group.modified_at(),
        });
    }

    pub(crate) fn record_group_deletion(&self, group_id: &str) {
        self.record_sync_item(crate::sync::SyncItem::LabelChange {
            label_id: group_id.to_string(),
            label_name: String::new(),
            contacts: Vec::new(),
            visible_fields: Vec::new(),
            is_deleted: true,
            timestamp: self.clock.unix_seconds(),
        });
    }

    // === Visibility Labels ===

    /// Lists all visibility labels.
    pub fn list_groups(&self) -> VauchiResult<Vec<crate::contact::Group>> {
        Ok(self.storage.labels().load_all_groups()?)
    }

    /// Creates a new visibility label.
    pub fn create_group(&self, name: &str) -> VauchiResult<crate::contact::Group> {
        let group = self.storage.labels().create_group(name)?;
        self.record_group_change(&group);
        Ok(group)
    }

    /// Renames a visibility label.
    pub fn rename_group(&self, label_id: &str, new_name: &str) -> VauchiResult<()> {
        self.storage.labels().rename_group(label_id, new_name)?;
        self.record_group_change(&self.storage.labels().load_group(label_id)?);
        Ok(())
    }

    /// Sets or clears the per-group display name override.
    ///
    /// When set, contacts in this group see this name instead of the
    /// user's default display name. Pass `None` to clear.
    /// Persists the updated label to storage.
    pub fn set_group_display_name_override(
        &self,
        label_id: &str,
        name_override: Option<&str>,
    ) -> VauchiResult<()> {
        let mut label = self.storage.labels().load_group(label_id)?;
        label
            .set_display_name_override(name_override, self.clock.unix_seconds())
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.labels().save_group(&label)?;
        self.record_group_change(&label);
        Ok(())
    }

    /// Sets or clears the per-group bio override (ADR-054 D2).
    ///
    /// When set, contacts in this group see this bio instead of the card's
    /// default bio. Pass `None` to clear. Persists the updated label.
    pub fn set_group_bio_override(
        &self,
        label_id: &str,
        bio_override: Option<&str>,
    ) -> VauchiResult<()> {
        let mut label = self.storage.labels().load_group(label_id)?;
        label
            .set_bio_override(bio_override, self.clock.unix_seconds())
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.labels().save_group(&label)?;
        self.record_group_change(&label);
        Ok(())
    }

    /// Sets or clears the per-group avatar override (WebP, ADR-042).
    ///
    /// Accepts any common image format; core normalizes to WebP <= 32 KB.
    /// When set, contacts in this group see this avatar instead of the card's
    /// default avatar. Pass `None` to clear. Persists the updated label.
    pub fn set_group_avatar_override(
        &self,
        label_id: &str,
        avatar_override: Option<&[u8]>,
    ) -> VauchiResult<()> {
        let mut label = self.storage.labels().load_group(label_id)?;
        label
            .set_avatar_override(avatar_override, self.clock.unix_seconds())
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.labels().save_group(&label)?;
        self.record_group_change(&label);
        Ok(())
    }

    /// Deletes a visibility label.
    ///
    /// Contacts in the label remain in the contact list; they just lose
    /// their label membership.
    ///
    /// When deleting the last label (transition to no-group mode), visible
    /// fields from the deleted label are migrated to `field_visibility` on the
    /// own card so field visibility is preserved.
    ///
    /// Note: Only the *last-deleted* label's fields are migrated. If labels
    /// are deleted sequentially, fields assigned only to earlier-deleted
    /// labels won't carry over. This is intentional — deleting a label
    /// removes its field assignments. To preserve all field visibility when
    /// transitioning, delete all labels in a single operation or re-assign
    /// fields before deletion.
    pub fn delete_group(&self, label_id: &str) -> VauchiResult<()> {
        // Load the label before deletion to capture its visible fields
        let label = self.storage.labels().load_group(label_id)?;
        let visible_fields = label.visible_fields().clone();

        // Delete the label from storage
        self.storage.labels().delete_group(label_id)?;

        // Check if this was the last label
        let remaining_labels = self.storage.labels().load_all_groups()?;
        if remaining_labels.is_empty() && !visible_fields.is_empty() {
            // Transitioning to no-group mode:
            // Migrate visible fields from the deleted label to field_visibility
            if let Some(mut card) = self.storage.contacts().load_own_card()? {
                for field_id in &visible_fields {
                    card.set_field_shown(field_id, true);
                }
                self.storage.contacts().save_own_card(&card)?;
            }
        }

        self.record_group_deletion(label_id);

        Ok(())
    }

    /// Gets a visibility label by ID.
    pub fn get_group(&self, label_id: &str) -> VauchiResult<crate::contact::Group> {
        Ok(self.storage.labels().load_group(label_id)?)
    }

    /// Gets all contacts that are members of a visibility label.
    ///
    /// Loads the label, extracts its contact IDs, then loads the actual
    /// `Contact` objects. Contacts that no longer exist in storage are
    /// silently skipped.
    pub fn get_group_members(&self, label_id: &str) -> VauchiResult<Vec<crate::contact::Contact>> {
        let label = self.storage.labels().load_group(label_id)?;
        let mut members = Vec::new();
        for contact_id in label.contacts() {
            if let Some(contact) = self.storage.contacts().load_contact(contact_id)? {
                members.push(contact);
            }
        }
        Ok(members)
    }

    /// Adds a contact to a visibility label.
    pub fn add_contact_to_group(&self, label_id: &str, contact_id: &str) -> VauchiResult<()> {
        self.storage
            .labels()
            .add_contact_to_group(label_id, contact_id)?;
        self.record_group_change(&self.storage.labels().load_group(label_id)?);
        Ok(())
    }

    /// Removes a contact from a visibility label.
    pub fn remove_contact_from_group(&self, label_id: &str, contact_id: &str) -> VauchiResult<()> {
        self.storage
            .labels()
            .remove_contact_from_group(label_id, contact_id)?;
        self.record_group_change(&self.storage.labels().load_group(label_id)?);
        Ok(())
    }

    /// Gets all labels that contain a specific contact.
    pub fn get_groups_for_contact(
        &self,
        contact_id: &str,
    ) -> VauchiResult<Vec<crate::contact::Group>> {
        Ok(self.storage.labels().get_groups_for_contact(contact_id)?)
    }

    /// Sets field visibility for a label.
    ///
    /// When `is_visible` is true, contacts in this label will see the field.
    /// When false, the field is hidden from contacts in this label.
    pub fn set_group_field_visibility(
        &self,
        label_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        self.storage
            .labels()
            .set_group_field_visibility(label_id, field_id, is_visible)?;
        self.record_group_change(&self.storage.labels().load_group(label_id)?);
        Ok(())
    }

    /// Sets a per-contact visibility override for a field.
    ///
    /// Per-contact overrides take precedence over label-based visibility.
    pub fn set_contact_visibility_override(
        &self,
        contact_id: &str,
        field_id: &str,
        is_visible: bool,
    ) -> VauchiResult<()> {
        Ok(self
            .storage
            .labels()
            .save_contact_override(contact_id, field_id, is_visible)?)
    }

    /// Removes a per-contact visibility override.
    pub fn remove_contact_visibility_override(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> VauchiResult<()> {
        Ok(self
            .storage
            .labels()
            .delete_contact_override(contact_id, field_id)?)
    }

    /// Marks an own-card field as part of the **public base** — visible to an
    /// ungrouped contact. The public base is per-field (not per-contact);
    /// per-contact control is `set_field_*` (overrides, Layer C).
    /// (2026-06-14 visibility layering.)
    pub fn set_own_field_public(&self, field_id: &str) -> VauchiResult<()> {
        let mut card = self
            .storage
            .contacts()
            .load_own_card()?
            .ok_or(VauchiError::IdentityNotInitialized)?;
        card.field_visibility_mut().set_everyone(field_id);
        self.storage.contacts().save_own_card(&card)?;
        // A public-base change is what ungrouped contacts may see — arm the
        // repropagation marker so the next sync pass sends the grant/revoke.
        self.mark_own_card_repropagate()?;
        Ok(())
    }

    /// Removes an own-card field from the **public base** — hidden from an
    /// ungrouped contact, so leaving a granting group revokes it. Still visible
    /// to grouped contacts via their groups (and to anyone via an override).
    pub fn set_own_field_private(&self, field_id: &str) -> VauchiResult<()> {
        let mut card = self
            .storage
            .contacts()
            .load_own_card()?
            .ok_or(VauchiError::IdentityNotInitialized)?;
        card.field_visibility_mut().set_nobody(field_id);
        self.storage.contacts().save_own_card(&card)?;
        // See set_own_field_public: arm the marker so the revoke propagates.
        self.mark_own_card_repropagate()?;
        Ok(())
    }

    /// Gets all per-contact visibility overrides for a contact.
    pub fn get_contact_visibility_overrides(
        &self,
        contact_id: &str,
    ) -> VauchiResult<std::collections::HashMap<String, bool>> {
        Ok(self.storage.labels().load_contact_overrides(contact_id)?)
    }

    /// Determines the effective visibility of a field for a contact.
    ///
    /// Field-centric model (owner decision 2026-07-10,
    /// `2026-07-05-ungrouped-contacts-default-open`), in priority order:
    /// 1. Layer C — per-contact override (if set), always wins
    /// 2. A group the contact belongs to grants the field → visible
    /// 3. The field is assigned to ANY group → hidden (group-governed;
    ///    this contact holds no grant), regardless of the toggle
    /// 4. Unassigned field — the own card's Visible/Hidden toggle
    ///    (`set_own_field_*` / `set_field_shown`) applies to every contact
    ///    alike; unruled defaults to hidden. The contact's legacy
    ///    `visibility_rules` stay a pure restrictor (imported contacts have
    ///    none → hidden)
    pub fn get_effective_field_visibility(
        &self,
        contact_id: &str,
        field_id: &str,
    ) -> VauchiResult<bool> {
        // Load the contact's visibility rules as fallback
        let contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("contact: {}", contact_id)))?;

        // Check per-contact override first
        let overrides = self.storage.labels().load_contact_overrides(contact_id)?;
        if let Some(&is_visible) = overrides.get(field_id) {
            return Ok(is_visible);
        }

        // A group the contact is in that exposes this field grants it.
        let labels = self.storage.labels().get_groups_for_contact(contact_id)?;
        if labels.iter().any(|l| l.is_field_visible(field_id)) {
            return Ok(true);
        }

        // Field-centric partition: a field any group exposes is
        // group-audience data — closed to every contact without a grant,
        // matching exchange-time filtering so initial share and propagation
        // agree (ADR-054 D3 generalized 2026-07-10). The gate keys on ALL
        // groups, not this contact's: assignment moves the field out of the
        // toggle's reach.
        let all_groups = self.storage.labels().load_all_groups()?;
        if all_groups.iter().any(|g| g.is_field_visible(field_id)) {
            return Ok(false);
        }

        // Unassigned field: the Visible/Hidden toggle. Visible only on an
        // explicit `Everyone` rule — unruled defaults to hidden
        // (privacy-first, matches `ContactCard::is_field_shown`). The legacy
        // per-contact rules remain a restrictor so imported contacts (no
        // rules) stay excluded.
        let legacy_allows = contact
            .visibility_rules()
            .is_some_and(|rules| rules.can_see(field_id, contact_id));
        if !legacy_allows {
            return Ok(false);
        }
        Ok(self
            .storage
            .contacts()
            .load_own_card()?
            .is_some_and(|card| card.field_visibility().is_explicitly_everyone(field_id)))
    }

    /// One-time grandfathering sweep for the field-centric visibility model
    /// (2026-07-05-ungrouped-contacts-default-open, owner decision
    /// 2026-07-10). Unruled own-card entries that no group governs
    /// materialize to explicit `Everyone` — re-encoding what contacts
    /// observably received under the retired default-open model — while
    /// explicit toggles are never touched. Runs at startup once the storage
    /// key is available; `create_identity` pre-sets the marker so fresh
    /// installs never sweep. Returns whether the sweep ran.
    pub fn migrate_field_centric_visibility(&self) -> VauchiResult<bool> {
        let mut flags = self.load_settings_flags()?;
        if flags.field_centric_visibility_migrated {
            return Ok(false);
        }

        if let Some(mut card) = self.storage.contacts().load_own_card()? {
            let groups = self.storage.labels().load_all_groups()?;
            let assigned = |fid: &str| groups.iter().any(|g| g.is_field_visible(fid));
            let unruled_unassigned: Vec<String> = card
                .fields()
                .iter()
                .map(|f| f.id().to_string())
                .filter(|fid| !card.field_visibility().contains(fid) && !assigned(fid))
                .collect();
            for fid in &unruled_unassigned {
                card.field_visibility_mut().set_everyone(fid);
            }
            if !unruled_unassigned.is_empty() {
                self.storage.contacts().save_own_card(&card)?;
            }
            // With groups present the algorithm change moves audiences even
            // though no toggle moved (non-members lose group-assigned
            // entries; members gain toggled ones) — deliver it now instead
            // of on the next unrelated edit. Without groups the sweep is a
            // no-op re-encoding: no traffic.
            if !groups.is_empty() {
                self.mark_own_card_repropagate()?;
            }
        }

        flags.field_centric_visibility_migrated = true;
        self.save_settings_flags(&flags)?;
        Ok(true)
    }

    /// Resolves an own-card field label to its field id.
    ///
    /// Layer-A visibility rules are keyed by field *id*; UniFFI
    /// surfaces address fields by *label* — this is the one
    /// resolution point (was duplicated inline per dispatch arm in
    /// vauchi-platform).
    fn own_field_id_by_label(&self, field_label: &str) -> VauchiResult<String> {
        let card = self
            .storage
            .contacts()
            .load_own_card()?
            .ok_or(VauchiError::IdentityNotInitialized)?;
        Ok(card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| VauchiError::NotFound(format!("field: {field_label}")))?
            .id()
            .to_string())
    }

    /// Reads a field's **effective** visibility for one contact, addressed by
    /// the field's own-card label — the same verdict the repropagation path
    /// sends to the peer (override → group → default-closed → public base), so
    /// a frontend readout never contradicts what the contact actually receives.
    pub fn is_field_visible_by_label(
        &self,
        contact_id: &str,
        field_label: &str,
    ) -> VauchiResult<bool> {
        let field_id = self.own_field_id_by_label(field_label)?;
        self.get_effective_field_visibility(contact_id, &field_id)
    }

    /// Toggles a field's per-contact visibility, addressed by own-card label.
    ///
    /// Reads the field's current *effective* visibility, then writes the
    /// negation as a per-contact **override** (Layer C). Routing through the
    /// override — not the legacy `visibility_rules` — makes the toggle robust:
    /// it wins over a group grant and the default-closed gate and persists
    /// through group-membership changes, matching `set_field_private`/`public`
    /// (2026-06-14 visibility layering, G2/F3). The override is keyed by field
    /// **id**, the key every read path resolves to; writing it by label was a
    /// no-op (F1). Returns the new visibility state.
    pub fn toggle_field_visibility(
        &self,
        contact_id: &str,
        field_label: &str,
    ) -> VauchiResult<bool> {
        let contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::InvalidState("Contact not found".into()))?;
        if !contact.is_exchanged() {
            return Err(VauchiError::InvalidState(
                "Visibility rules require an exchanged contact".into(),
            ));
        }

        let field_id = self.own_field_id_by_label(field_label)?;
        let new_visible = !self.get_effective_field_visibility(contact_id, &field_id)?;
        self.storage
            .labels()
            .save_contact_override(contact_id, &field_id, new_visible)?;
        self.record_sync_item(crate::sync::SyncItem::VisibilityChanged {
            contact_id: contact_id.to_string(),
            field_id: field_id.clone(),
            is_visible: new_visible,
            timestamp: self.now_timestamp(),
        });
        Ok(new_visible)
    }
}

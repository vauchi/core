// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility labels and field visibility management.

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

impl Vauchi {
    // === Visibility Labels ===

    /// Lists all visibility labels.
    pub fn list_groups(&self) -> VauchiResult<Vec<crate::contact::Group>> {
        Ok(self.storage.labels().load_all_groups()?)
    }

    /// Creates a new visibility label.
    pub fn create_group(&self, name: &str) -> VauchiResult<crate::contact::Group> {
        Ok(self.storage.labels().create_group(name)?)
    }

    /// Renames a visibility label.
    pub fn rename_group(&self, label_id: &str, new_name: &str) -> VauchiResult<()> {
        Ok(self.storage.labels().rename_group(label_id, new_name)?)
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
        Ok(self
            .storage
            .labels()
            .add_contact_to_group(label_id, contact_id)?)
    }

    /// Removes a contact from a visibility label.
    pub fn remove_contact_from_group(&self, label_id: &str, contact_id: &str) -> VauchiResult<()> {
        Ok(self
            .storage
            .labels()
            .remove_contact_from_group(label_id, contact_id)?)
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
        Ok(self
            .storage
            .labels()
            .set_group_field_visibility(label_id, field_id, is_visible)?)
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
    /// Returns visibility determined by (in priority order):
    /// 1. Layer C — per-contact override (if set), always wins
    /// 2. Layer B — group union (visible if any of the contact's groups
    ///    exposes the field)
    /// 3. Grouped contacts are default-closed (ADR-054 D3): a grouped contact
    ///    sees only what its groups grant
    /// 4. Layer A — public base for an *ungrouped* contact: the contact's
    ///    legacy `visibility_rules` AND the own card's `field_visibility`
    ///    (`set_own_field_*`)
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

        // A group the contact is in that exposes this field grants it (Layer B).
        let labels = self.storage.labels().get_groups_for_contact(contact_id)?;
        if labels.iter().any(|l| l.is_field_visible(field_id)) {
            return Ok(true);
        }

        // ADR-054 D3: a *grouped* contact is default-closed — a field none of
        // their groups grants is hidden, matching exchange-time filtering so
        // initial share and propagation agree
        // (2026-06-08-sync-card-update-not-group-filtered, decision A). The gate
        // keys on `labels` (THIS contact's groups), not on groups existing
        // globally: an ungrouped contact is not default-closed but falls through
        // to the Layer-A public base card below, even while others are grouped.
        if !labels.is_empty() {
            return Ok(false);
        }

        // No group membership for this contact: the public base card. Visible
        // iff (a) the contact's legacy Layer-A rules allow it — imported
        // contacts have none → hidden — AND (b) the own card's per-field public
        // base (`field_visibility`) marks it visible. `set_own_field_*` curates
        // the public base; per-contact `set_field_*` goes through overrides
        // (Layer C, checked above). Empty `field_visibility` defaults to
        // `Everyone`, so this is a no-op until the public base is curated.
        // (2026-06-14 visibility layering.)
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
            .is_some_and(|card| card.field_visibility().can_see(field_id, contact_id)))
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

    /// Sets a field's Layer-A visibility for one contact, addressed
    /// by the field's own-card label.
    ///
    /// `visible == false` sets the rule to nobody; `true` to
    /// everyone. Requires an exchanged contact (imported contacts
    /// carry no visibility rules).
    pub fn set_field_visibility_by_label(
        &self,
        contact_id: &str,
        field_label: &str,
        visible: bool,
    ) -> VauchiResult<()> {
        let mut contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("contact: {contact_id}")))?;
        let field_id = self.own_field_id_by_label(field_label)?;
        let rules = contact
            .visibility_rules_mut()
            .ok_or(VauchiError::InvalidState(
                "Visibility rules require an exchanged contact".into(),
            ))?;
        if visible {
            rules.set_everyone(&field_id);
        } else {
            rules.set_nobody(&field_id);
        }
        self.storage.contacts().save_contact(&contact)?;
        Ok(())
    }

    /// Reads a field's Layer-A visibility for one contact, addressed
    /// by the field's own-card label.
    ///
    /// Imported contacts (no visibility rules) read as not visible.
    pub fn is_field_visible_by_label(
        &self,
        contact_id: &str,
        field_label: &str,
    ) -> VauchiResult<bool> {
        let contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("contact: {contact_id}")))?;
        let field_id = self.own_field_id_by_label(field_label)?;
        Ok(contact
            .visibility_rules()
            .is_some_and(|r| r.can_see(&field_id, contact_id)))
    }

    /// Toggles field visibility for a contact.
    ///
    /// If the field is currently visible to this contact, hides it.
    /// If hidden, makes it visible. Returns the new visibility state.
    pub fn toggle_field_visibility(
        &self,
        contact_id: &str,
        field_label: &str,
    ) -> VauchiResult<bool> {
        let mut contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::InvalidState("Contact not found".into()))?;

        let rules = contact.visibility_rules().ok_or(VauchiError::InvalidState(
            "Visibility rules require an exchanged contact".into(),
        ))?;
        let current_can_see = rules.can_see(field_label, contact_id);

        if current_can_see {
            contact
                .visibility_rules_mut()
                .expect("checked above")
                .set_nobody(field_label);
        } else {
            contact
                .visibility_rules_mut()
                .expect("checked above")
                .set_everyone(field_label);
        }

        self.storage.contacts().save_contact(&contact)?;
        let new_visible = !current_can_see;
        self.record_sync_item(crate::sync::SyncItem::VisibilityChanged {
            contact_id: contact_id.to_string(),
            field_label: field_label.to_string(),
            is_visible: new_visible,
            timestamp: self.now_timestamp(),
        });
        Ok(new_visible)
    }
}

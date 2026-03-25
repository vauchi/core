// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact duplicate detection and merge operations.
//!
//! Exposes `contact::merge` functions as Vauchi API methods, integrating
//! with storage for dismissed-duplicate tracking and event dispatching.

use crate::contact::merge::{
    DuplicatePair, compute_similarity, filter_dismissed, find_duplicates, merge_contacts,
    normalize_pair_key,
};

use super::super::error::{VauchiError, VauchiResult};
use super::super::events::VauchiEvent;
use super::Vauchi;

impl Vauchi {
    /// Scans all contacts for potential duplicates.
    ///
    /// Returns duplicate pairs ordered by similarity (highest first),
    /// excluding pairs the user has previously dismissed.
    pub fn find_duplicates(&self) -> VauchiResult<Vec<DuplicatePair>> {
        let contacts = self.storage.list_contacts()?;
        let all_duplicates = find_duplicates(&contacts);

        // Load dismissed pairs and filter them out
        let dismissed = self.storage.load_dismissed_duplicates()?;
        Ok(filter_dismissed(all_duplicates, &dismissed))
    }

    /// Returns the similarity score between two contacts.
    ///
    /// Score ranges from 0.0 (no similarity) to 1.0 (identical).
    /// Returns an error if either contact is not found.
    pub fn get_duplicate_score(&self, id1: &str, id2: &str) -> VauchiResult<f64> {
        let contact1 = self
            .storage
            .load_contact(id1)?
            .ok_or_else(|| VauchiError::ContactNotFound(id1.to_string()))?;
        let contact2 = self
            .storage
            .load_contact(id2)?
            .ok_or_else(|| VauchiError::ContactNotFound(id2.to_string()))?;

        Ok(compute_similarity(&contact1, &contact2))
    }

    /// Dismisses a duplicate suggestion so it no longer appears.
    ///
    /// The pair key is normalized (id1 < id2) so dismissing (A, B) is the
    /// same as dismissing (B, A).
    pub fn dismiss_duplicate(&self, id1: &str, id2: &str) -> VauchiResult<()> {
        let (norm1, norm2) = normalize_pair_key(id1, id2);
        self.storage.dismiss_duplicate(&norm1, &norm2)?;
        Ok(())
    }

    /// Merges two contacts, keeping the primary and incorporating fields
    /// from the secondary.
    ///
    /// After merge:
    /// - The primary contact has all unique fields from both contacts
    /// - The secondary contact is deleted from storage
    /// - A `ContactRemoved` event is dispatched for the secondary
    /// - A `ContactUpdated` event is dispatched for the primary
    ///
    /// Returns the merged contact.
    pub fn merge_contacts(
        &self,
        primary_id: &str,
        secondary_id: &str,
    ) -> VauchiResult<crate::contact::Contact> {
        let primary = self
            .storage
            .load_contact(primary_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(primary_id.to_string()))?;
        let secondary = self
            .storage
            .load_contact(secondary_id)?
            .ok_or_else(|| VauchiError::ContactNotFound(secondary_id.to_string()))?;

        // W5: Prevent merging contacts of different kinds (exchanged vs imported).
        if primary.is_exchanged() != secondary.is_exchanged() {
            return Err(VauchiError::InvalidState(
                "Cannot merge exchanged and imported contacts".into(),
            ));
        }

        let merged = merge_contacts(&primary, &secondary);

        // Save merged contact
        self.storage.save_contact(&merged)?;

        // Delete secondary
        self.storage.delete_contact(secondary_id)?;

        // Dispatch events
        self.events.dispatch(VauchiEvent::ContactRemoved {
            contact_id: secondary_id.to_string(),
        });
        self.events.dispatch(VauchiEvent::ContactUpdated {
            contact_id: primary_id.to_string(),
            changed_fields: vec!["merged".to_string()],
        });

        Ok(merged)
    }
}

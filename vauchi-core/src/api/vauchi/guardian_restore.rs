// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Restoring the guardian backup the guardians handed back into the new
//! identity (vauchi/private#796, step 3, D6).
//!
//! The new identity keeps its own key: the backup brings back the
//! contacts, labels and own card, and each restored exchanged contact
//! waits for its rekey (steps 4 to 6). A contact already here, such as a
//! guardian just met again, keeps the relationship it has.

use std::collections::HashSet;

use crate::api::error::{VauchiError, VauchiResult};
use crate::backup::FullBackupEnvelope;
use crate::contact::Group;

use super::Vauchi;

impl Vauchi {
    /// How many guardians have handed a backup back to this identity.
    ///
    /// # Errors
    /// Storage errors.
    pub fn guardians_handed_back(&self) -> VauchiResult<usize> {
        Ok(self.storage.returned_backups().all_kept()?.len())
    }

    /// Whether the backup the guardians handed back is sealed under the
    /// owner's passphrase, so the restore screen asks for it.
    ///
    /// # Errors
    /// Storage errors.
    pub fn returned_backup_needs_passphrase(&self) -> VauchiResult<bool> {
        Ok(self
            .storage
            .returned_backups()
            .all_kept()?
            .iter()
            .any(|delivery| {
                crate::backup::passphrase_layer::is_passphrase_sealed(delivery.backup())
            }))
    }

    /// Opens the newest backup the returned shares open and restores its
    /// contacts, labels and own card into this identity, marking each
    /// restored exchanged contact as waiting for its rekey. Returns how
    /// many contacts it restored; restoring again restores none.
    ///
    /// # Errors
    /// The sanitized guardian recovery refusal when nothing was handed back
    /// or no backup opens (a share of a group missing, a missing or wrong
    /// passphrase); storage errors otherwise.
    pub fn restore_from_returned_backups(&self, passphrase: Option<&str>) -> VauchiResult<usize> {
        let envelope = self.open_returned_backup(passphrase)?;
        let contacts = crate::backup::restore_contacts_from_envelope(&envelope)
            .map_err(|_| restore_refused())?;
        let now = self.clock.unix_seconds();
        self.storage.with_savepoint(|| -> VauchiResult<usize> {
            let mut restored = 0;
            for contact in &contacts {
                if self
                    .storage
                    .contacts()
                    .load_contact(contact.id())?
                    .is_some()
                {
                    continue;
                }
                self.storage.contacts().save_contact(contact)?;
                if contact.public_key().is_some() {
                    self.storage.recovery().mark_rekey_pending(contact.id())?;
                }
                restored += 1;
            }
            if let Some(card) = &envelope.sections.own_card {
                self.storage.contacts().save_own_card(card)?;
            }
            let existing: HashSet<String> = self
                .storage
                .labels()
                .load_all_groups()?
                .iter()
                .map(|group| group.id().to_string())
                .collect();
            for label in &envelope.sections.labels {
                if existing.contains(&label.label_id) {
                    continue;
                }
                let mut members = HashSet::new();
                for id in &label.contacts {
                    if self.storage.contacts().load_contact(id)?.is_some() {
                        members.insert(id.clone());
                    }
                }
                self.storage.labels().save_group(&Group::from_storage(
                    label.label_id.clone(),
                    label.name.clone(),
                    members,
                    label.visible_fields.iter().cloned().collect(),
                    None,
                    None,
                    None,
                    now,
                    now,
                ))?;
            }
            Ok(restored)
        })
    }

    /// The contacts restored from the guardian backup that still wait for
    /// their rekey to this identity.
    ///
    /// # Errors
    /// Storage errors.
    pub fn rekey_pending_contacts(&self) -> VauchiResult<Vec<String>> {
        Ok(self.storage.recovery().rekey_pending()?)
    }

    /// Every returned delivery's share tried on each returned backup,
    /// newest first: guardians keep the newest delivery they had, so their
    /// backups may differ, and a share opens only its own backup.
    fn open_returned_backup(&self, passphrase: Option<&str>) -> VauchiResult<FullBackupEnvelope> {
        let mut deliveries = self.storage.returned_backups().all_kept()?;
        deliveries.sort_by_key(|delivery| std::cmp::Reverse(delivery.created_at()));
        let shares: Vec<Vec<u8>> = deliveries
            .iter()
            .map(|delivery| delivery.sealed_share().to_vec())
            .collect();
        let mut tried: Vec<&[u8]> = Vec::new();
        for delivery in &deliveries {
            if tried.contains(&delivery.backup()) {
                continue;
            }
            tried.push(delivery.backup());
            if let Ok(envelope) = self.recover_guardian_backup_with_passphrase(
                &hex::encode(delivery.backup()),
                &shares,
                passphrase,
            ) {
                return Ok(envelope);
            }
        }
        Err(restore_refused())
    }
}

fn restore_refused() -> VauchiError {
    VauchiError::Configuration("Guardian backup recovery failed".into())
}

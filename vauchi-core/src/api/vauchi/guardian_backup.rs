// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Sending the guardian backup to the guardians (vauchi/private#796).
//!
//! Each recovery-trusted contact keeps the encrypted backup next to its own
//! sealed share (D7); the delivery travels to it in ratchet-sealed mailbox
//! pieces (D9). The shards open at the voucher threshold, capped at the
//! guardian count and raised to Shamir's minimum of two (D10). Shamir also
//! needs three shares at least, so fewer than three guardians get nothing.

use crate::api::error::{VauchiError, VauchiResult};
use crate::backup::KeyShardConfig;
use crate::contact::Contact;
use crate::sync::delta::{encode_guardian_backup_piece, encode_guardian_release};
use crate::sync::guardian_piece::{GuardianDelivery, split_into_pieces};

use super::ProfileEngine;

/// A changed guardian backup is sent again at most once a day (D11): each
/// send is about 2 MB per guardian, and a day-old copy still restores.
const GUARDIAN_BACKUP_REFRESH_SECS: u64 = 86_400;

impl ProfileEngine {
    /// Exports a fresh guardian backup and queues each recovery-trusted
    /// contact its share and the backup, in pieces. Returns how many
    /// guardians it was queued for; a guardian with no session to send on
    /// yet is skipped, and fewer than three guardians make no backup.
    ///
    /// # Errors
    /// [`VauchiError::IdentityNotInitialized`] without an identity; storage,
    /// export and sealing errors otherwise.
    pub fn send_guardian_backup(&self) -> VauchiResult<usize> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let guardians: Vec<_> = self
            .storage
            .contacts()
            .list_contacts()?
            .into_iter()
            .filter(|contact| contact.is_recovery_trusted() && !contact.is_blocked())
            .collect();
        // Each group the owner formed, with the guardians in it (D17); a
        // group whose members are all gone no longer counts.
        let grouped: Vec<Vec<&Contact>> = self
            .storage
            .recovery()
            .guardian_groups()?
            .into_iter()
            .map(|(_, members)| {
                members
                    .iter()
                    .filter_map(|id| guardians.iter().find(|guardian| guardian.id() == id))
                    .collect::<Vec<_>>()
            })
            .filter(|members| !members.is_empty())
            .collect();
        if grouped.len() < usize::from(KeyShardConfig::MIN_COUNT) {
            return Ok(0);
        }
        let keys = grouped
            .iter()
            .map(|members| {
                members
                    .iter()
                    .map(|guardian| {
                        guardian.public_key().copied().ok_or_else(|| {
                            VauchiError::InvalidState(format!(
                                "Guardian {} has no public key",
                                guardian.id()
                            ))
                        })
                    })
                    .collect::<VauchiResult<Vec<[u8; 32]>>>()
            })
            .collect::<VauchiResult<Vec<_>>>()?;
        let groups = crate::backup::GuardianGroups::new(keys)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        let (backup_hex, sealed) = self.export_guardian_backup_for_groups(&groups)?;
        let mut backup =
            hex::decode(&backup_hex).map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        if let Some((salt, key)) = self.storage.recovery().load_recovery_passphrase()? {
            backup = crate::backup::passphrase_layer::seal(&key, &salt, &backup)
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        }
        let mut backup_id = [0u8; 16];
        self.rng.fill_bytes(&mut backup_id);
        let created_at = self.clock.unix_seconds();

        let mut sent = 0;
        let each_member = grouped.iter().flatten().zip(sealed.into_iter().flatten());
        for (guardian, share) in each_member {
            let guardian_pk = guardian
                .public_key()
                .copied()
                .ok_or_else(|| VauchiError::InvalidState("A guardian has no public key".into()))?;
            // The token lets a contact tell this guardian's in-person vouch
            // from anyone's (D19).
            let token = crate::recovery::guardian::GuardianToken::create(
                identity.signing_keypair(),
                crate::crypto::PublicKey::from_bytes(guardian_pk),
                created_at,
            );
            let delivery = GuardianDelivery::new(created_at, share, backup.clone())
                .and_then(|delivery| delivery.with_guardian_token(token.to_bytes()))
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
            let pieces = split_into_pieces(backup_id, &delivery.encode())
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
            let queued = pieces.iter().try_for_each(|piece| {
                self.queue_sealed_payload(identity, guardian, &encode_guardian_backup_piece(piece))
            });
            match queued {
                Ok(()) => sent += 1,
                // No session to send on yet: the next guardian set change
                // or contact change sends again (step 1e).
                Err(VauchiError::SendDeferred)
                | Err(VauchiError::NotFound(_))
                | Err(VauchiError::InvalidState(_)) => {}
                Err(error) => return Err(error),
            }
        }
        if sent > 0 {
            self.storage
                .guardian_backups()
                .record_sent(&self.guardian_backup_digest()?, created_at)?;
        }
        Ok(sent)
    }

    /// Sends the guardian backup again if what it holds changed since the
    /// last send and a day has passed (step 1e, D11); never from a duress
    /// session, whose stale mark waits for the next normal one (D12,
    /// ADR-032). Returns whether a guardian was sent one.
    ///
    /// # Errors
    /// Storage, export and sealing errors.
    pub fn refresh_guardian_backup_if_stale(&self) -> VauchiResult<bool> {
        if self.identity.is_none() || self.in_duress_mode() {
            return Ok(false);
        }
        let last_sent = self.storage.guardian_backups().last_sent()?;
        let now = self.clock.unix_seconds();
        if let Some((digest, sent_at)) = last_sent {
            if now < sent_at.saturating_add(GUARDIAN_BACKUP_REFRESH_SECS) {
                return Ok(false);
            }
            if digest == self.guardian_backup_digest()? {
                return Ok(false);
            }
        }
        Ok(self.send_guardian_backup()? > 0)
    }

    /// A digest of what the guardian backup holds and how it is split: the
    /// voucher threshold, the own card, each contact's id, key, card and
    /// guardian flag, and the groups. Fields are length-prefixed so that
    /// no two contents hash alike.
    fn guardian_backup_digest(&self) -> VauchiResult<[u8; 32]> {
        use sha2::{Digest, Sha256};

        fn field(hasher: &mut Sha256, bytes: &[u8]) {
            hasher.update((bytes.len() as u64).to_be_bytes());
            hasher.update(bytes);
        }
        fn json(card: &crate::ContactCard) -> VauchiResult<Vec<u8>> {
            serde_json::to_vec(card).map_err(|e| VauchiError::Serialization(e.to_string()))
        }

        let mut hasher = Sha256::new();
        hasher.update(b"vauchi-guardian-backup-content-v1");
        let threshold = self
            .storage
            .recovery()
            .load_recovery_settings()?
            .unwrap_or_default()
            .recovery_threshold();
        hasher.update(threshold.to_be_bytes());
        // A passphrase set, changed or cleared needs a new seal (D16).
        match self.storage.recovery().load_recovery_passphrase()? {
            Some((salt, _)) => field(&mut hasher, &salt),
            None => field(&mut hasher, &[]),
        }
        match self.storage.contacts().load_own_card()? {
            Some(card) => field(&mut hasher, &json(&card)?),
            None => field(&mut hasher, &[]),
        }
        let mut contacts = self.storage.contacts().list_contacts()?;
        contacts.sort_by(|a, b| a.id().cmp(b.id()));
        // A guardian moved between groups needs a new split (D17).
        let groups = self.storage.recovery().guardian_groups()?;
        let group_of = |id: &str| {
            groups
                .iter()
                .find(|(_, members)| members.iter().any(|member| member == id))
                .map_or(0, |(group_id, _)| *group_id)
        };
        for contact in &contacts {
            hasher.update([group_of(contact.id())]);
            field(&mut hasher, contact.id().as_bytes());
            field(
                &mut hasher,
                contact.public_key().map_or(&[][..], |pk| &pk[..]),
            );
            field(
                &mut hasher,
                contact
                    .shared_key()
                    .map_or(&[][..], |key| &key.as_bytes()[..]),
            );
            field(&mut hasher, &json(contact.card())?);
            hasher.update([u8::from(contact.is_recovery_trusted())]);
        }
        let mut groups = self.storage.labels().load_all_groups()?;
        groups.sort_by(|a, b| a.id().cmp(b.id()));
        for group in &groups {
            field(&mut hasher, group.id().as_bytes());
            field(&mut hasher, group.name().as_bytes());
            let mut members: Vec<&String> = group.contacts().iter().collect();
            members.sort();
            for member in members {
                field(&mut hasher, member.as_bytes());
            }
        }
        Ok(hasher.finalize().into())
    }

    /// Tells `contact`, just taken out of the guardian set, to forget what
    /// it held for us (step 1d'). A contact with no session yet is skipped:
    /// the next backup does not reach it, and what it holds stays.
    /// In the recovery meeting, after vouching and exchanging with the new
    /// identity, hands back this guardian's share re-sealed to
    /// `contact_id`'s key together with the backup it keeps (step 2b, D13).
    ///
    /// Only to the new key of a claim this device vouched for, and only
    /// while that claim may live ([`crate::recovery::RecoveryClaim::MAX_AGE_SECS`]
    /// from the vouch); the old device's key is a vouched claim's old key,
    /// never its new one. A duress session holds no backup to hand back
    /// (ADR-032). Every refusal is the same error, so none tells the
    /// others apart.
    ///
    /// # Errors
    /// [`VauchiError::InvalidState`] when there is nothing to hand back to
    /// this contact; storage, sealing and send errors otherwise.
    pub fn return_guardian_backup(&self, contact_id: &str) -> VauchiResult<()> {
        let refused =
            || VauchiError::InvalidState("No guardian backup to hand back to this contact".into());
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        if self.in_duress_mode() {
            return Err(refused());
        }
        let contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(refused)?;
        let new_pk = *contact.public_key().ok_or_else(refused)?;
        let not_before = self
            .clock
            .unix_seconds()
            .saturating_sub(crate::recovery::RecoveryClaim::MAX_AGE_SECS);
        let old_pk = self
            .storage
            .recovery()
            .vouched_old_key_for(&new_pk, not_before)?
            .ok_or_else(refused)?;
        let kept = self
            .storage
            .guardian_backups()
            .kept(&hex::encode(old_pk))?
            .ok_or_else(refused)?;
        self.queue_hand_back(identity, &contact, &new_pk, &kept)?;
        self.storage
            .recovery()
            .mark_handed_back(&old_pk, &new_pk, self.clock.unix_seconds())?;
        Ok(())
    }

    /// Sends every hand-back this guardian still owes (step 2c, D14): a
    /// vouched claim, still within its age, whose new key is an exchanged
    /// contact and whose old key's backup is kept. Tried as an exchange
    /// completes and on every sync, so a guardian that answered the
    /// exchange and has no sending chain yet sends once the new phone has
    /// written. Returns how many went out; a duress session sends none.
    ///
    /// # Errors
    /// Storage and sealing errors; a contact that cannot be sent to yet is
    /// owed, not an error.
    pub fn hand_back_owed(&self) -> VauchiResult<usize> {
        let Some(identity) = self.identity.as_ref() else {
            return Ok(0);
        };
        if self.in_duress_mode() {
            return Ok(0);
        }
        let now = self.clock.unix_seconds();
        let not_before = now.saturating_sub(crate::recovery::RecoveryClaim::MAX_AGE_SECS);
        let mut sent = 0;
        for (old_pk, new_pk) in self.storage.recovery().owed_hand_backs(not_before)? {
            let Some(contact) = self.storage.contacts().load_contact(&hex::encode(new_pk))? else {
                continue;
            };
            if contact.public_key() != Some(&new_pk) {
                continue;
            }
            let Some(kept) = self.storage.guardian_backups().kept(&hex::encode(old_pk))? else {
                continue;
            };
            match self.queue_hand_back(identity, &contact, &new_pk, &kept) {
                Ok(()) => {
                    self.storage
                        .recovery()
                        .mark_handed_back(&old_pk, &new_pk, now)?;
                    sent += 1;
                }
                Err(VauchiError::SendDeferred) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(sent)
    }

    /// Re-seals the kept share to `new_pk` and queues it with the backup to
    /// `contact` as `0x0d` pieces, all or none.
    fn queue_hand_back(
        &self,
        identity: &crate::identity::Identity,
        contact: &Contact,
        new_pk: &[u8; 32],
        kept: &GuardianDelivery,
    ) -> VauchiResult<()> {
        let resealed = self.respond_to_recovery(kept.sealed_share(), new_pk)?;
        let returned = GuardianDelivery::new(kept.created_at(), resealed, kept.backup().to_vec())
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        let mut backup_id = [0u8; 16];
        self.rng.fill_bytes(&mut backup_id);
        let pieces = split_into_pieces(backup_id, &returned.encode())
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.with_savepoint(|| {
            pieces.iter().try_for_each(|piece| {
                self.queue_sealed_payload(
                    identity,
                    contact,
                    &crate::sync::delta::encode_returned_backup_piece(piece),
                )
            })
        })
    }

    /// Trusts `contact_id` for recovery in the group the owner chose as it
    /// is added (D17): `Some(group)` joins an existing group, `None` starts
    /// a new one. Returns the group's number. A guardian already trusted
    /// moves to the chosen group. The new grouping, and the backup split
    /// over it, are one change.
    ///
    /// # Errors
    /// [`VauchiError::InvalidState`] for an unknown or blocked contact, a
    /// group that does not exist, a tenth guardian, or a contact not
    /// verified in person; storage and send errors otherwise.
    pub fn trust_for_recovery_in_group(
        &self,
        contact_id: &str,
        group: Option<u8>,
    ) -> VauchiResult<u8> {
        let mut contact = self
            .storage
            .contacts()
            .load_contact(contact_id)?
            .ok_or_else(|| VauchiError::InvalidState("Contact not found".into()))?;
        if contact.is_blocked() {
            return Err(VauchiError::InvalidState(
                "Blocked contacts cannot be trusted for recovery".into(),
            ));
        }
        let groups = self.storage.recovery().guardian_groups()?;
        // The guardian limit is checked before a group is chosen, so a
        // tenth guardian is told the limit, not that no group is free.
        if !contact.is_recovery_trusted() {
            let trusted = groups
                .iter()
                .map(|(_, members)| members.len())
                .sum::<usize>();
            if trusted >= crate::backup::GuardianGroups::MAX_GUARDIANS {
                return Err(VauchiError::InvalidState(format!(
                    "At most {} contacts can be trusted for recovery; release one first",
                    crate::backup::GuardianGroups::MAX_GUARDIANS
                )));
            }
            contact
                .trust_for_recovery()
                .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        }
        let group_id = match group {
            Some(group_id) if groups.iter().any(|(id, _)| *id == group_id) => group_id,
            Some(_) => {
                return Err(VauchiError::InvalidState(
                    "That guardian group does not exist".into(),
                ));
            }
            None => (1..=crate::backup::GuardianGroups::MAX_GUARDIANS as u8)
                .find(|id| !groups.iter().any(|(group_id, _)| group_id == id))
                .ok_or_else(|| VauchiError::InvalidState("No guardian group is free".into()))?,
        };
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            self.storage.contacts().save_contact(&contact)?;
            self.storage
                .recovery()
                .set_guardian_group(contact_id, group_id)?;
            self.send_guardian_backup()?;
            Ok(())
        })?;
        self.upload_guardian_entries_best_effort();
        Ok(group_id)
    }

    /// The guardian groups by number, each with its members' contact ids,
    /// for the screen where the owner adds a guardian (D17).
    ///
    /// # Errors
    /// Storage errors.
    pub fn guardian_groups(&self) -> VauchiResult<Vec<(u8, Vec<String>)>> {
        Ok(self.storage.recovery().guardian_groups()?)
    }

    /// Re-uploads the guardian tokens after a change to the guardian set;
    /// the local change stands when the relay cannot be reached, and the
    /// next change or sync uploads again.
    pub(super) fn upload_guardian_entries_best_effort(&self) {
        #[cfg(feature = "network-http")]
        if let Err(_relay_unreachable) = self.upload_guardian_entries() {}
    }

    pub(super) fn send_guardian_release(&self, contact: &Contact) -> VauchiResult<()> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        match self.queue_sealed_payload(identity, contact, &encode_guardian_release()) {
            Ok(())
            | Err(VauchiError::SendDeferred)
            | Err(VauchiError::NotFound(_))
            | Err(VauchiError::InvalidState(_)) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

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

use super::Vauchi;

/// A changed guardian backup is sent again at most once a day (D11): each
/// send is about 2 MB per guardian, and a day-old copy still restores.
const GUARDIAN_BACKUP_REFRESH_SECS: u64 = 86_400;

impl Vauchi {
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
        let count = u8::try_from(guardians.len()).unwrap_or(u8::MAX);
        if count < KeyShardConfig::MIN_COUNT {
            return Ok(0);
        }

        let voucher_threshold = self
            .storage
            .recovery()
            .load_recovery_settings()?
            .unwrap_or_default()
            .recovery_threshold();
        let threshold = u8::try_from(voucher_threshold)
            .unwrap_or(u8::MAX)
            .clamp(KeyShardConfig::MIN_THRESHOLD, count);
        let keys = guardians
            .iter()
            .map(|guardian| {
                guardian.public_key().copied().ok_or_else(|| {
                    VauchiError::InvalidState(format!(
                        "Guardian {} has no public key",
                        guardian.id()
                    ))
                })
            })
            .collect::<VauchiResult<Vec<[u8; 32]>>>()?;
        let (backup_hex, shares) = self.export_guardian_backup_with_shards(&keys, threshold)?;
        let backup =
            hex::decode(&backup_hex).map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        let mut backup_id = [0u8; 16];
        self.rng.fill_bytes(&mut backup_id);
        let created_at = self.clock.unix_seconds();

        let mut sent = 0;
        for (guardian, share) in guardians.iter().zip(shares) {
            let delivery = GuardianDelivery::new(created_at, share, backup.clone())
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
        match self.storage.contacts().load_own_card()? {
            Some(card) => field(&mut hasher, &json(&card)?),
            None => field(&mut hasher, &[]),
        }
        let mut contacts = self.storage.contacts().list_contacts()?;
        contacts.sort_by(|a, b| a.id().cmp(b.id()));
        for contact in &contacts {
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

        let resealed = self.respond_to_recovery(kept.sealed_share(), &new_pk)?;
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
                    &contact,
                    &crate::sync::delta::encode_returned_backup_piece(piece),
                )
            })
        })
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

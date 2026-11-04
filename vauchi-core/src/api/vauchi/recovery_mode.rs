// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Social recovery that restores the same identity (vauchi/private#796,
//! P28, P32), the last rung after linking from a working device and the
//! password backup.
//!
//! The new phone starts with no identity and a temporary recovery key; its
//! claim names the old identity's key and that recovery key. In person,
//! each guardian hands back its share re-sealed to the recovery key with
//! the backup it keeps; nothing is exchanged as contacts and no relay is
//! involved. With enough hand-backs the phone opens the backup and becomes
//! the old identity as a new device of it; the recovery key goes. Contacts
//! keep their shared keys and nothing waits for a rekey: if the lost phone
//! was stolen unlocked, compromise recovery follows (P29).

use base64::Engine as _;
use sha2::Digest as _;

use super::Vauchi;
use crate::api::error::{VauchiError, VauchiResult};
use crate::crypto::SigningKeyPair;
use crate::identity::{DeviceInfo, DeviceRegistry, Identity};
use crate::recovery::RecoveryClaim;
use crate::sync::guardian_piece::GuardianDelivery;

/// The device name a restored identity takes until the owner renames it.
const RESTORED_DEVICE_NAME: &str = "Restored device";

/// At most one share per guardian, so a flood of hand-backs cannot grow
/// the store, and the backup opens from at most this many shares.
const MAX_HAND_BACKS: usize = crate::backup::KeyShardConfig::MAX_COUNT as usize;

/// The signed registry of the restored identity: the lost device, derived
/// from the backup's seed and device index and revoked, and this device.
fn restored_registry(
    master_seed: &[u8; 32],
    identity: &Identity,
    lost_index: u32,
    lost_name: String,
    now: u64,
) -> VauchiResult<DeviceRegistry> {
    let signing = identity.signing_keypair();
    let lost = DeviceInfo::derive(master_seed, lost_index, lost_name, 0);
    let mut registry = DeviceRegistry::new(lost.to_registered(master_seed), signing);
    let device_error = |e: crate::identity::DeviceError| VauchiError::InvalidState(e.to_string());
    registry
        .add_device(identity.device_info().to_registered(master_seed), signing)
        .map_err(device_error)?;
    registry
        .revoke_device(lost.device_id(), signing, now)
        .map_err(device_error)?;
    Ok(registry)
}

impl Vauchi {
    /// Begins a social recovery of the identity `old_pk_hex` on this phone,
    /// which has no identity yet: makes the temporary recovery key, or
    /// keeps the one made before, and returns the claim (base64) the
    /// guardians are shown.
    ///
    /// # Errors
    /// [`VauchiError::AlreadyInitialized`] when this phone has an identity;
    /// [`VauchiError::Serialization`] for a malformed key; storage errors.
    pub fn begin_recovery(&self, old_pk_hex: &str) -> VauchiResult<String> {
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }
        let old_pk: [u8; 32] = hex::decode(old_pk_hex.trim())
            .ok()
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| VauchiError::Serialization("public key must be 32 bytes".into()))?;
        let recovery_key = self.recovery_mode_key(&old_pk)?;
        let claim = RecoveryClaim::new(
            old_pk,
            recovery_key.public_key().as_bytes(),
            self.clock.unix_seconds(),
        );
        Ok(base64::engine::general_purpose::STANDARD.encode(claim.to_bytes()))
    }

    /// The guardian's side, in the recovery meeting: once the owner is
    /// recognized, re-seals this guardian's share of the backup it keeps
    /// for the claim's old identity to the claim's recovery key, and
    /// returns it with that backup, for the meeting's channel. Records the
    /// vouch. Every refusal is the same error.
    ///
    /// # Errors
    /// [`VauchiError::InvalidState`] for a malformed or expired claim, an
    /// identity whose backup this guardian does not keep, or a duress
    /// session (ADR-032); storage and sealing errors.
    pub fn hand_back_in_person(&self, claim_b64: &str) -> VauchiResult<Vec<u8>> {
        let refused = || VauchiError::InvalidState("Nothing to hand back for this claim".into());
        if self.identity.is_none() || self.in_duress_mode() {
            return Err(refused());
        }
        let now = self.clock.unix_seconds();
        let claim = self
            .parse_recovery_claim_b64(claim_b64)
            .map_err(|_| refused())?;
        if claim.is_expired(now) {
            return Err(refused());
        }
        let old_pk = *claim.old_pk().as_bytes();
        let recovery_pk = *claim.new_pk().as_bytes();
        let kept = self
            .storage
            .guardian_backups()
            .kept(&hex::encode(old_pk))?
            .ok_or_else(refused)?;
        let resealed = self.respond_to_recovery(kept.sealed_share(), &recovery_pk)?;
        let hand_back = GuardianDelivery::new(kept.created_at(), resealed, kept.backup().to_vec())
            .map_err(|_| refused())?;
        self.storage
            .recovery()
            .record_vouched_claim(&old_pk, &recovery_pk, now)?;
        Ok(hand_back.encode())
    }

    /// Keeps a guardian's hand-back on this phone in recovery mode, and
    /// returns how many it holds. Its share is opened as it is taken and the
    /// hand-back keyed by that share, so a retried hand-back, sealed afresh,
    /// and two guardians of one group (who hold the same share) count once.
    ///
    /// # Errors
    /// [`VauchiError::InvalidState`] when no recovery is under way, the
    /// bytes are not a hand-back, its share does not open for this phone's
    /// recovery key, or a new share would pass the bound; storage errors.
    pub fn take_hand_back(&self, hand_back: &[u8]) -> VauchiResult<usize> {
        let not_a_hand_back = || VauchiError::InvalidState("Not a guardian hand-back".into());
        let recovery = self.storage.recovery();
        let seed = match (&self.identity, recovery.load_recovery_mode()?) {
            (None, Some((seed, _))) => seed,
            _ => return Err(VauchiError::InvalidState("No recovery under way".into())),
        };
        let delivery = GuardianDelivery::decode(hand_back).map_err(|_| not_a_hand_back())?;
        let secret = SigningKeyPair::from_seed(&seed).to_x25519_secret();
        let share = crate::backup::open_share_for_guardian(delivery.sealed_share(), &secret)
            .map_err(|_| not_a_hand_back())?;
        let digest: [u8; 32] = sha2::Sha256::digest(share.to_bytes()).into();
        if !recovery.has_hand_back(&digest)? && recovery.hand_back_count()? >= MAX_HAND_BACKS {
            return Err(not_a_hand_back());
        }
        recovery.keep_hand_back(&digest, hand_back, self.clock.unix_seconds())?;
        Ok(recovery.hand_back_count()?)
    }

    /// Opens the backup the hand-backs open and becomes its identity, as a
    /// new device of it; restores its contacts, labels and own card, and
    /// discards the recovery key. Returns how many contacts it restored.
    ///
    /// # Errors
    /// [`VauchiError::AlreadyInitialized`] when this phone has an identity;
    /// the sanitized guardian recovery refusal when no recovery is under
    /// way or no backup opens (a group's share missing, a missing or wrong
    /// passphrase); storage errors otherwise.
    pub fn restore_identity_from_hand_backs(
        &mut self,
        passphrase: Option<&str>,
    ) -> VauchiResult<usize> {
        let refused = || VauchiError::Configuration("Guardian backup recovery failed".into());
        if self.identity.is_some() {
            return Err(VauchiError::AlreadyInitialized);
        }
        let (seed, target_pk) = self
            .storage
            .recovery()
            .load_recovery_mode()?
            .ok_or_else(refused)?;
        let secret = SigningKeyPair::from_seed(&seed).to_x25519_secret();
        let envelope = self.open_hand_backs(passphrase, &secret)?;
        let contacts =
            crate::backup::restore_contacts_from_envelope(&envelope).map_err(|_| refused())?;
        let master_seed = crate::backup::extract_master_seed(&envelope.sections.identity)
            .map_err(|_| refused())?;
        let now = self.clock.unix_seconds();
        let lost_index = envelope.sections.identity.device_index;
        let identity = Identity::from_device_link(
            *master_seed,
            envelope.sections.identity.display_name.clone(),
            self.new_device_index(lost_index),
            RESTORED_DEVICE_NAME.into(),
            now,
        );
        // A backup that opens is not yet the claimed one: anyone can seal a
        // share of another identity's backup to the recovery key a claim
        // shows.
        if identity.signing_public_key() != &target_pk {
            return Err(refused());
        }
        let registry = restored_registry(
            &master_seed,
            &identity,
            lost_index,
            envelope.sections.identity.device_name.clone(),
            now,
        )?;
        let restored = self.storage.with_savepoint(|| -> VauchiResult<usize> {
            self.storage
                .identity()
                .save_identity(&identity.to_storage_bytes())?;
            self.storage.device().save_device_registry(&registry)?;
            for contact in &contacts {
                self.storage.contacts().save_contact(contact)?;
            }
            self.restore_labels_and_card(&envelope, now)?;
            self.storage.recovery().clear_recovery_mode_seed()?;
            Ok(contacts.len())
        })?;
        self.identity = Some(identity);
        Ok(restored)
    }

    /// The recovery key, made once and kept until the restore, now
    /// recovering `target_pk`. Hand-backs for another target are dropped.
    fn recovery_mode_key(&self, target_pk: &[u8; 32]) -> VauchiResult<SigningKeyPair> {
        let recovery = self.storage.recovery();
        let seed = match recovery.load_recovery_mode()? {
            Some((seed, target)) if &target == target_pk => {
                return Ok(SigningKeyPair::from_seed(&seed));
            }
            Some((seed, _)) => {
                recovery.clear_hand_backs()?;
                seed
            }
            None => {
                let mut seed = zeroize::Zeroizing::new([0u8; 32]);
                self.rng.fill_bytes(seed.as_mut());
                seed
            }
        };
        recovery.save_recovery_mode(&seed, target_pk)?;
        Ok(SigningKeyPair::from_seed(&seed))
    }

    /// A device index other than the lost device's, so the lost device can
    /// be revoked in the signed registry. Random, since no surviving device
    /// tells which indices are taken.
    fn new_device_index(&self, lost_index: u32) -> u32 {
        let mut bytes = [0u8; 4];
        loop {
            self.rng.fill_bytes(&mut bytes);
            let index = u32::from_le_bytes(bytes) >> 1;
            if index != lost_index {
                return index;
            }
        }
    }

    /// Every hand-back's share tried on each handed-back backup, newest
    /// first: guardians keep the newest delivery they had, so their backups
    /// may differ, and a share opens only its own backup.
    fn open_hand_backs(
        &self,
        passphrase: Option<&str>,
        secret: &x25519_dalek::StaticSecret,
    ) -> VauchiResult<crate::backup::FullBackupEnvelope> {
        let mut deliveries: Vec<GuardianDelivery> = self
            .storage
            .recovery()
            .hand_backs()?
            .iter()
            .filter_map(|bytes| GuardianDelivery::decode(bytes).ok())
            .collect();
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
            if let Ok(envelope) = super::recovery_passphrase::open_guardian_backup_with_passphrase(
                &hex::encode(delivery.backup()),
                &shares,
                passphrase,
                secret,
            ) {
                return Ok(envelope);
            }
        }
        Err(VauchiError::Configuration(
            "Guardian backup recovery failed".into(),
        ))
    }
}

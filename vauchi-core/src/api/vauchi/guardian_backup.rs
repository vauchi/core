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
use crate::sync::delta::encode_guardian_backup_piece;
use crate::sync::guardian_piece::{GuardianDelivery, split_into_pieces};

use super::Vauchi;

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

        let mut sent = 0;
        for (guardian, share) in guardians.iter().zip(shares) {
            let delivery = GuardianDelivery::new(share, backup.clone())
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
        Ok(sent)
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashSet;

use super::Vauchi;
use crate::api::VauchiResult;
use crate::backup::{guardian_backup_metadata, open_share_for_guardian, passphrase_layer};
use crate::crypto::SigningKeyPair;

/// Durable prerequisites for attempting the same-identity restore.
/// Share metadata is a readiness hint; only the restore authenticates the
/// backup and verifies the recovered identity matches the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandBackRestoreStatus {
    pub handed_back: usize,
    pub can_attempt_restore: bool,
    pub needs_passphrase: bool,
}

impl Vauchi {
    /// Reads identity-free hand-backs without creating a claim, changing
    /// its lifetime, writing storage or restoring an identity.
    /// Returns `None` outside identity-free recovery.
    ///
    /// # Errors
    /// Storage errors, including unreadable encrypted recovery state.
    pub fn hand_back_restore_status(&self) -> VauchiResult<Option<HandBackRestoreStatus>> {
        if self.identity.is_some() {
            return Ok(None);
        }
        let Some((seed, _)) = self.storage.recovery().load_recovery_mode()? else {
            return Ok(None);
        };
        let secret = SigningKeyPair::from_seed(&seed).to_x25519_secret();
        let shares = self.hand_back_shares()?;
        let opened: Vec<_> = shares
            .iter()
            .filter_map(|share| open_share_for_guardian(share.sealed_share(), &secret).ok())
            .collect();
        let mut tried = HashSet::new();
        for share in &shares {
            let hash = share.backup_hash();
            if !tried.insert(hash) {
                continue;
            }
            let Some(backup) = self.whole_backup(&hash)? else {
                continue;
            };
            let needs_passphrase = passphrase_layer::is_passphrase_sealed(&backup);
            let Ok(candidate) = open_share_for_guardian(share.sealed_share(), &secret) else {
                continue;
            };
            let metadata = candidate.metadata();
            if !needs_passphrase && guardian_backup_metadata(&backup).ok() != Some(metadata) {
                continue;
            }
            let indices: HashSet<_> = opened
                .iter()
                .filter(|shard| shard.metadata() == metadata)
                .map(|shard| shard.index())
                .collect();
            if indices.len() >= usize::from(metadata.threshold()) {
                return Ok(Some(HandBackRestoreStatus {
                    handed_back: indices.len(),
                    can_attempt_restore: true,
                    needs_passphrase,
                }));
            }
        }
        Ok(Some(HandBackRestoreStatus {
            handed_back: shares.len(),
            can_attempt_restore: false,
            needs_passphrase: false,
        }))
    }
}

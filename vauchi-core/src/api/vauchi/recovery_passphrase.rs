// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The owner's optional recovery passphrase over the guardian backup
//! (vauchi/private#796, D16): with it set, guardians that collude still
//! need it to open the backup.

use crate::api::error::{VauchiError, VauchiResult};
use crate::backup::passphrase_layer;

use super::Vauchi;

/// A sealed backup is the guardian backup plus the layer's magic, salt
/// and AEAD overhead; anything longer is refused before it is decoded.
const MAX_SEALED_HEX_LEN: usize = 2 * (crate::backup::full_backup::MAX_GUARDIAN_BACKUP_BYTES + 128);

impl Vauchi {
    /// Sets the owner's recovery passphrase and sends the guardians a
    /// backup sealed under it, replacing an earlier passphrase.
    ///
    /// # Errors
    /// [`VauchiError::InvalidState`] for an empty or weak passphrase; storage,
    /// derivation and sending errors otherwise.
    pub fn set_recovery_passphrase(&self, passphrase: &str) -> VauchiResult<()> {
        if passphrase.is_empty() {
            return Err(VauchiError::InvalidState(
                "The recovery passphrase is empty".into(),
            ));
        }
        // Guardians who collude hold the backup and every share and can
        // guess offline, so the bar is the backup password's (D16).
        crate::identity::password::validate_password(passphrase)
            .map_err(|_| VauchiError::InvalidState("The recovery passphrase is too weak".into()))?;
        let mut salt = [0u8; 16];
        self.rng.fill_bytes(&mut salt);
        let key = passphrase_layer::passphrase_key(passphrase, &salt)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            self.storage
                .recovery()
                .save_recovery_passphrase(&salt, &key)?;
            self.send_guardian_backup()?;
            Ok(())
        })
    }

    /// Forgets the owner's recovery passphrase and sends the guardians a
    /// backup their shares alone open.
    ///
    /// # Errors
    /// Storage and sending errors.
    pub fn clear_recovery_passphrase(&self) -> VauchiResult<()> {
        self.storage.with_savepoint(|| -> VauchiResult<()> {
            self.storage.recovery().clear_recovery_passphrase()?;
            self.send_guardian_backup()?;
            Ok(())
        })
    }

    /// Whether the owner set a recovery passphrase.
    ///
    /// # Errors
    /// Storage errors.
    pub fn has_recovery_passphrase(&self) -> VauchiResult<bool> {
        Ok(self
            .storage
            .recovery()
            .load_recovery_passphrase()?
            .is_some())
    }

    /// Whether a guardian backup (hex) also needs the owner's passphrase,
    /// so the restore screen knows to ask for it.
    pub fn guardian_backup_needs_passphrase(backup_data: &str) -> bool {
        backup_data
            .trim()
            .get(..passphrase_layer::MAGIC_HEX_LEN)
            .and_then(|magic| hex::decode(magic).ok())
            .is_some_and(|magic| passphrase_layer::is_passphrase_sealed(&magic))
    }

    /// Opens a guardian backup with the guardians' re-sealed shares and,
    /// when the backup is sealed under it, the owner's passphrase.
    ///
    /// # Errors
    /// The same sanitized refusal as [`Self::recover_guardian_backup`] for a
    /// missing or wrong passphrase, and for shares that do not open it.
    pub fn recover_guardian_backup_with_passphrase(
        &self,
        backup_data: &str,
        re_sealed_shares: &[Vec<u8>],
        passphrase: Option<&str>,
    ) -> VauchiResult<crate::backup::FullBackupEnvelope> {
        if !Self::guardian_backup_needs_passphrase(backup_data) {
            return self.recover_guardian_backup(backup_data, re_sealed_shares);
        }
        let refused = || VauchiError::Configuration("Guardian backup recovery failed".into());
        if backup_data.trim().len() > MAX_SEALED_HEX_LEN {
            return Err(refused());
        }
        let bytes = hex::decode(backup_data.trim()).map_err(|_| refused())?;
        let inner = passphrase_layer::open(passphrase.ok_or_else(refused)?, &bytes)
            .map_err(|_| refused())?;
        self.recover_guardian_backup(&hex::encode(inner), re_sealed_shares)
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The mask profile's own seed, used only for a backup made under duress
//! (ADR-080 point 7). Every install with an app password has one, not only
//! those with a duress PIN: a seed only duress users had could move the
//! sealed blob to a larger size bucket, which is readable without the SMK.

use rusqlite::OptionalExtension;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::super::{Sealed, Storage, StorageError};

/// The mask profile's seed.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MaskSeed([u8; 32]);

impl PartialEq for MaskSeed {
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl Eq for MaskSeed {}

impl MaskSeed {
    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for MaskSeed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MaskSeed(..)")
    }
}

/// The one `mask_seed` row of the mask database.
pub struct MaskSeedStore<'a> {
    mask: &'a Storage,
}

impl Storage {
    /// The mask profile's seed (ADR-080).
    pub fn mask_seed(&self) -> Sealed<'_, MaskSeedStore<'_>> {
        let mask = self.mask_profile();
        self.mask_sealed(mask.sealed(MaskSeedStore { mask }))
    }
}

impl MaskSeedStore<'_> {
    pub fn load(&self) -> Result<Option<MaskSeed>, StorageError> {
        let Some(sealed) = self
            .mask
            .conn
            .query_row(
                "SELECT seed_encrypted FROM mask_seed WHERE id = 1",
                [],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let plain = Zeroizing::new(
            crate::crypto::decrypt(&self.mask.encryption_key, &sealed)
                .map_err(|e| StorageError::Encryption(e.to_string()))?,
        );
        let seed: [u8; 32] = plain
            .as_slice()
            .try_into()
            .map_err(|_| StorageError::InvalidData("mask seed has invalid length".into()))?;
        Ok(Some(MaskSeed(seed)))
    }

    /// Makes the seed if there is none; an existing seed stays.
    pub(crate) fn ensure(&self) -> Result<(), StorageError> {
        if self.load()?.is_some() {
            return Ok(());
        }
        let seed = MaskSeed(crate::crypto::random_bytes::<32>());
        let sealed = crate::crypto::encrypt(&self.mask.encryption_key, &seed.0)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.mask.conn.execute(
            "INSERT INTO mask_seed (id, seed_encrypted) VALUES (1, ?1)",
            [sealed],
        )?;
        Ok(())
    }
}

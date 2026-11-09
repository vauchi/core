// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The real profile's public ID and name, held by the mask profile so a
//! duress foreground shows them without the real identity (ADR-080 point 4,
//! amended 2026-11-08).

use rusqlite::{OptionalExtension, params};

use super::super::{Sealed, Storage, StorageError};
use crate::crypto::{decrypt, encrypt};
use crate::identity::Identity;

/// What the mask displays of the real identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskPublicValues {
    signing_public_key: [u8; 32],
    display_name: String,
}

impl MaskPublicValues {
    pub fn new(signing_public_key: [u8; 32], display_name: &str) -> Self {
        MaskPublicValues {
            signing_public_key,
            display_name: display_name.to_string(),
        }
    }

    pub fn signing_public_key(&self) -> &[u8; 32] {
        &self.signing_public_key
    }

    /// The public ID, as the real identity's `public_id` gives it.
    pub fn public_id(&self) -> String {
        hex::encode(self.signing_public_key)
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

/// The one `public_values` row of the mask database.
pub struct MaskPublicValuesStore<'a> {
    mask: &'a Storage,
    /// Sealed after each write; a write whose seal fails returns the error
    /// (vauchi/private#1003).
    sealer: &'a Storage,
}

impl Storage {
    /// The real profile's public values, as the mask profile holds them.
    pub fn mask_public_values(&self) -> Sealed<'_, MaskPublicValuesStore<'_>> {
        let mask = self.mask_profile();
        self.mask_sealed(mask.sealed(MaskPublicValuesStore { mask, sealer: self }))
    }

    /// Makes the mask's public values those of `identity`, the real one,
    /// and of this database's user-presence enrolment.
    pub fn keep_mask_public_values(&self, identity: &Identity) -> Result<(), StorageError> {
        self.mask_public_values().save(&MaskPublicValues::new(
            *identity.signing_public_key(),
            identity.display_name(),
        ))?;
        self.keep_mask_presence_digest(self.ux().load_user_presence_digest()?.as_ref())
    }

    /// Makes the mask's user-presence digest `digest`. The digest only
    /// verifies the keystore's secret, so the mask holds it as a public
    /// value: under duress an export reveals the enrolled secret as it
    /// always does, and never seals a new one over it (owner, 2026-11-09).
    pub fn keep_mask_presence_digest(&self, digest: Option<&[u8; 32]>) -> Result<(), StorageError> {
        let mask = self.mask_profile();
        if mask.ux().load_user_presence_digest()?.as_ref() == digest {
            return Ok(());
        }
        match digest {
            Some(digest) => mask.ux().save_user_presence_digest(digest)?,
            None => mask.ux().clear_user_presence_digest()?,
        }
        self.seal_mask()
    }
}

impl MaskPublicValuesStore<'_> {
    pub fn load(&self) -> Result<Option<MaskPublicValues>, StorageError> {
        let Some((key, name)) = self
            .mask
            .conn
            .query_row(
                "SELECT signing_public_key_encrypted, display_name_encrypted
                 FROM public_values WHERE id = 1",
                [],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let open = |sealed: &[u8]| {
            decrypt(&self.mask.encryption_key, sealed)
                .map_err(|e| StorageError::Encryption(e.to_string()))
        };
        let signing_public_key: [u8; 32] = open(&key)?
            .as_slice()
            .try_into()
            .map_err(|_| StorageError::InvalidData("public key has invalid length".into()))?;
        let display_name = String::from_utf8(open(&name)?)
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
        Ok(Some(MaskPublicValues {
            signing_public_key,
            display_name,
        }))
    }

    /// Stores `values` unless they are already held, so a profile that opens
    /// unchanged leaves the seal as it was.
    pub fn save(&self, values: &MaskPublicValues) -> Result<(), StorageError> {
        if self.load()?.as_ref() == Some(values) {
            return Ok(());
        }
        let seal = |plain: &[u8]| {
            encrypt(&self.mask.encryption_key, plain)
                .map_err(|e| StorageError::Encryption(e.to_string()))
        };
        self.mask.conn.execute(
            "INSERT OR REPLACE INTO public_values
             (id, signing_public_key_encrypted, display_name_encrypted)
             VALUES (1, ?1, ?2)",
            params![
                seal(&values.signing_public_key)?,
                seal(values.display_name.as_bytes())?
            ],
        )?;
        self.sealer.seal_mask()
    }

    /// Forgets the public values, as when the real identity is wiped.
    pub fn clear(&self) -> Result<(), StorageError> {
        if self
            .mask
            .conn
            .execute("DELETE FROM public_values WHERE id = 1", [])?
            > 0
        {
            self.sealer.seal_mask()?;
        }
        Ok(())
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The duress section of a full backup's real payload (ADR-080 point 7,
//! amended 2026-11-09): the mask profile under a transfer key, and the app
//! password and duress PIN hashes, so a restore locks as the old install
//! did and the duress PIN opens the same mask. Storage composes it
//! (`storage/backup_snapshot.rs`); this is the pure representation.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::backup::BackupError;
use crate::crypto::SymmetricKey;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DuressSection {
    mask_key_b64: String,
    mask_body_b64: String,
    password_hash_b64: String,
    password_salt_b64: String,
    duress: Option<DuressCredential>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DuressCredential {
    hash_b64: String,
    salt_b64: String,
    copy_in_use: bool,
}

/// A stored credential's hash and salt, borrowed.
pub(crate) type CredentialRef<'a> = (&'a [u8; 32], &'a [u8; 16]);

/// A credential hash and salt as the identity row stores them.
pub(crate) struct Credential {
    pub hash: Zeroizing<[u8; 32]>,
    pub salt: [u8; 16],
}

/// The duress PIN's credential, and whether a duress session had taken
/// the own-card copy over.
pub(crate) struct DuressPin {
    pub credential: Credential,
    pub copy_in_use: bool,
}

impl Drop for DuressSection {
    fn drop(&mut self) {
        self.mask_key_b64.zeroize();
        self.password_hash_b64.zeroize();
        if let Some(duress) = self.duress.as_mut() {
            duress.hash_b64.zeroize();
        }
    }
}

fn invalid(what: &str) -> BackupError {
    BackupError::Deserialization(format!("Invalid backup duress {what}"))
}

fn decode_fixed<const N: usize>(b64: &str, what: &str) -> Result<Zeroizing<[u8; N]>, BackupError> {
    let bytes = Zeroizing::new(BASE64.decode(b64).map_err(|_| invalid(what))?);
    let array: [u8; N] = bytes.as_slice().try_into().map_err(|_| invalid(what))?;
    Ok(Zeroizing::new(array))
}

fn decode_credential(hash_b64: &str, salt_b64: &str) -> Result<Credential, BackupError> {
    Ok(Credential {
        hash: decode_fixed::<32>(hash_b64, "hash")?,
        salt: *decode_fixed::<16>(salt_b64, "salt")?,
    })
}

impl DuressSection {
    pub(crate) fn new(
        mask_key: &SymmetricKey,
        mask_body: &[u8],
        password: CredentialRef<'_>,
        duress_pin: Option<(CredentialRef<'_>, bool)>,
    ) -> Self {
        Self {
            mask_key_b64: BASE64.encode(mask_key.as_bytes()),
            mask_body_b64: BASE64.encode(mask_body),
            password_hash_b64: BASE64.encode(password.0),
            password_salt_b64: BASE64.encode(password.1),
            duress: duress_pin.map(|((hash, salt), copy_in_use)| DuressCredential {
                hash_b64: BASE64.encode(hash),
                salt_b64: BASE64.encode(salt),
                copy_in_use,
            }),
        }
    }

    pub(crate) fn mask_key(&self) -> Result<SymmetricKey, BackupError> {
        Ok(SymmetricKey::from_bytes(*decode_fixed::<32>(
            &self.mask_key_b64,
            "mask key",
        )?))
    }

    pub(crate) fn mask_body(&self) -> Result<Vec<u8>, BackupError> {
        BASE64
            .decode(&self.mask_body_b64)
            .map_err(|_| invalid("mask body"))
    }

    pub(crate) fn password(&self) -> Result<Credential, BackupError> {
        decode_credential(&self.password_hash_b64, &self.password_salt_b64)
    }

    pub(crate) fn duress_pin(&self) -> Result<Option<DuressPin>, BackupError> {
        self.duress
            .as_ref()
            .map(|d| {
                Ok(DuressPin {
                    credential: decode_credential(&d.hash_b64, &d.salt_b64)?,
                    copy_in_use: d.copy_in_use,
                })
            })
            .transpose()
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which key opens storage when secure storage holds the keys (ADR-033,
//! vauchi/private#580).
//!
//! Every key that can open the database lives in secure storage, so a shred
//! that deletes them there destroys access. Before an identity exists the
//! database is under a bootstrap key; afterwards under the SEK derived from
//! the SMK. The SMK is saved before the rekey that moves the data to it, so
//! a boot can find an SMK while the data is still under the bootstrap key —
//! the storage key check tells which, and the move is finished here.

use std::path::Path;

use crate::crypto::{ShreddingMasterKey, SymmetricKey};
use crate::storage::{SecureStorage, Storage, StorageError};

use super::super::{VauchiError, VauchiResult};
use super::SMK_KEY_NAME;

pub(super) const BOOTSTRAP_KEY_NAME: &str = "storage_bootstrap";

fn load_key(secure: &dyn SecureStorage, name: &str) -> VauchiResult<Option<[u8; 32]>> {
    let Some(bytes) = secure.load_key(name).map_err(|e| {
        VauchiError::Configuration(format!("Failed to load {name} from SecureStorage: {e}"))
    })?
    else {
        return Ok(None);
    };
    let bytes = zeroize::Zeroizing::new(bytes);
    let key: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
        VauchiError::Configuration(format!("{name} in SecureStorage has invalid length"))
    })?;
    Ok(Some(key))
}

/// Opens with the first candidate the data is under and returns that key;
/// [`StorageError::WrongKey`] when none is.
fn open_with_first(
    path: &Path,
    candidates: Vec<SymmetricKey>,
) -> VauchiResult<(Storage, SymmetricKey)> {
    for key in candidates {
        match Storage::open(path, key.clone()) {
            Ok(storage) => return Ok((storage, key)),
            Err(StorageError::WrongKey) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(StorageError::WrongKey.into())
}

/// Deletes the bootstrap key once the data is under the SEK.
///
/// Best-effort: the data no longer opens under it, and every shred deletes
/// it again, so a failed delete must not stop the app from starting.
pub(super) fn delete_bootstrap_key(secure: &dyn SecureStorage) {
    if let Err(e) = secure.secure_delete_key(BOOTSTRAP_KEY_NAME) {
        tracing::warn!(
            target: "vauchi.storage.keys",
            error = %e,
            "failed to delete the storage bootstrap key; the next boot retries"
        );
    }
}

/// Opens storage under secure-storage keys. `shell_key` is a storage key a
/// shell kept itself before keys moved to secure storage; it is adopted, so
/// the shell can drop it once Core has opened.
pub(super) fn open_storage(
    path: &Path,
    shell_key: Option<SymmetricKey>,
    secure: &dyn SecureStorage,
) -> VauchiResult<Storage> {
    let bootstrap = load_key(secure, BOOTSTRAP_KEY_NAME)?
        .map(|bytes| {
            SymmetricKey::try_from_bytes(bytes).map_err(|_| {
                VauchiError::Configuration(format!(
                    "{BOOTSTRAP_KEY_NAME} in SecureStorage is degenerate"
                ))
            })
        })
        .transpose()?;

    if let Some(smk) = load_key(secure, SMK_KEY_NAME)? {
        let sek = ShreddingMasterKey::from_bytes(smk).derive_sek();
        match Storage::open(path, sek.clone()) {
            Ok(storage) => {
                delete_bootstrap_key(secure);
                return Ok(storage);
            }
            Err(StorageError::WrongKey) => {}
            Err(e) => return Err(e.into()),
        }
        let (mut storage, _) =
            open_with_first(path, bootstrap.into_iter().chain(shell_key).collect())?;
        storage.rekey(sek)?;
        delete_bootstrap_key(secure);
        return Ok(storage);
    }

    let stored_bootstrap = bootstrap.clone();
    let mut candidates: Vec<SymmetricKey> = bootstrap.into_iter().chain(shell_key).collect();
    if candidates.is_empty() {
        candidates.push(SymmetricKey::generate());
    }
    let (storage, key) = open_with_first(path, candidates)?;
    if stored_bootstrap.as_ref().map(SymmetricKey::as_bytes) != Some(key.as_bytes()) {
        secure
            .save_key(BOOTSTRAP_KEY_NAME, key.as_bytes())
            .map_err(|e| {
                VauchiError::Configuration(format!("Failed to store the bootstrap key: {e}"))
            })?;
    }
    Ok(storage)
}

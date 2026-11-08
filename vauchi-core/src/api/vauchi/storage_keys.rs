// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
use crate::storage::{KeyAccess, SecureStorage, Storage, StorageError};

use super::super::{VauchiError, VauchiResult};
use super::SMK_KEY_NAME;

use crate::api::shred::{BOOTSTRAP_KEY_NAME, BRK_KEY_NAME};

/// Keeps every keychain failure typed, so Core can choose the unlock,
/// recovery or try-again screen (ADR-045). The platform's message is
/// dropped: it is for logs, and may name paths.
pub(super) fn keychain_error(error: StorageError) -> VauchiError {
    VauchiError::Storage(match error {
        StorageError::SecureStorageLocked | StorageError::SecureStorageKeyInvalidated => error,
        _ => StorageError::SecureStorageUnavailable,
    })
}

fn load_key(secure: &dyn SecureStorage, name: &str) -> VauchiResult<Option<[u8; 32]>> {
    let Some(bytes) = secure.load_key(name).map_err(keychain_error)? else {
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

/// Gives `storage` the Background Receive Key, first making one for an
/// identity that has an SMK and no BRK yet; an existing one is never
/// replaced (ADR-033 amendment 2026-11-07). Random rather than derived,
/// because the SMK it would come from cannot be read on a locked phone.
///
/// Best-effort: without the key the app still opens, receive state written
/// meanwhile stays under the storage key, and the next open retries. A
/// failed read makes nothing, so a key that is there but unreadable is not
/// replaced, and a key that could not be saved seals nothing, because data
/// under it would be lost with it.
pub(super) fn attach_background_receive_key(storage: &mut Storage, secure: &dyn SecureStorage) {
    let Some(brk) = background_receive_key(secure) else {
        return;
    };
    if let Err(e) = storage.set_receive_key(brk) {
        tracing::warn!(
            target: "vauchi.storage.keys",
            error = %e,
            "the background receive key does not open this file's receive state"
        );
    }
}

fn background_receive_key(secure: &dyn SecureStorage) -> Option<SymmetricKey> {
    match secure.load_key_with_access(BRK_KEY_NAME, KeyAccess::AfterFirstUnlock) {
        Ok(Some(bytes)) => {
            let bytes = zeroize::Zeroizing::new(bytes);
            return SymmetricKey::try_from_bytes(bytes.as_slice().try_into().ok()?).ok();
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(
                target: "vauchi.storage.keys",
                error = %e,
                "could not read the background receive key; the next open retries"
            );
            return None;
        }
    }
    let brk = SymmetricKey::generate();
    match secure.save_key_with_access(BRK_KEY_NAME, brk.as_bytes(), KeyAccess::AfterFirstUnlock) {
        Ok(()) => Some(brk),
        Err(e) => {
            tracing::warn!(
                target: "vauchi.storage.keys",
                error = %e,
                "could not store the background receive key; the next open retries"
            );
            None
        }
    }
}

/// Opens storage under secure-storage keys and returns the key the data is
/// under. `shell_key` is a storage key a shell kept itself before keys moved
/// to secure storage; it is adopted, so the shell can drop it once Core has
/// opened.
pub(super) fn open_storage(
    path: &Path,
    shell_key: Option<SymmetricKey>,
    secure: &dyn SecureStorage,
) -> VauchiResult<(Storage, SymmetricKey)> {
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
            Ok(mut storage) => {
                delete_bootstrap_key(secure);
                attach_background_receive_key(&mut storage, secure);
                return Ok((storage, sek));
            }
            Err(StorageError::WrongKey) => {}
            Err(e) => return Err(e.into()),
        }
        let (mut storage, _) =
            open_with_first(path, bootstrap.into_iter().chain(shell_key).collect())?;
        storage.rekey(sek.clone())?;
        delete_bootstrap_key(secure);
        attach_background_receive_key(&mut storage, secure);
        return Ok((storage, sek));
    }

    let stored_bootstrap = bootstrap.clone();
    let candidates: Vec<SymmetricKey> = bootstrap.into_iter().chain(shell_key).collect();
    if candidates.is_empty() {
        if path.exists() {
            return Err(StorageError::WrongKey.into());
        }
        // Saved before the database exists, so a failed save leaves no data
        // under a key nobody holds.
        let fresh = SymmetricKey::generate();
        secure
            .save_key(BOOTSTRAP_KEY_NAME, fresh.as_bytes())
            .map_err(keychain_error)?;
        return Ok((Storage::open(path, fresh.clone())?, fresh));
    }
    let (storage, key) = open_with_first(path, candidates)?;
    if stored_bootstrap.as_ref().map(SymmetricKey::as_bytes) != Some(key.as_bytes()) {
        secure
            .save_key(BOOTSTRAP_KEY_NAME, key.as_bytes())
            .map_err(keychain_error)?;
    }
    Ok((storage, key))
}

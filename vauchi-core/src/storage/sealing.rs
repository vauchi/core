// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A store handed out by a `Storage` whose database lives sealed in another
//! file's row (the mask profile, ADR-080) reseals when it drops after a
//! write, so every write persists before the call that made it returns.
//! A `Storage` with no seal target hands out the bare store.

use std::ops::{Deref, DerefMut};

use super::Storage;

/// A store, and the reseal it owes its `Storage` when it drops.
pub struct Sealed<'a, S> {
    store: S,
    _reseal: Option<ResealOnDrop<'a>>,
}

impl<S> Deref for Sealed<'_, S> {
    type Target = S;

    fn deref(&self) -> &S {
        &self.store
    }
}

impl<S> DerefMut for Sealed<'_, S> {
    fn deref_mut(&mut self) -> &mut S {
        &mut self.store
    }
}

struct ResealOnDrop<'a> {
    storage: &'a Storage,
    changes_before: u64,
}

impl Drop for ResealOnDrop<'_> {
    fn drop(&mut self) {
        // Inside a transaction the rows may still be rolled back;
        // `Storage::commit` reseals once they are not.
        if self.storage.conn.is_autocommit()
            && self.storage.conn.total_changes() != self.changes_before
        {
            self.storage.reseal_logged();
        }
    }
}

impl Storage {
    /// Hands out `store`, resealing on drop when this `Storage` is sealed
    /// into another file.
    pub(super) fn sealed<S>(&self, store: S) -> Sealed<'_, S> {
        Sealed {
            store,
            _reseal: self.seal_target.as_ref().map(|_| ResealOnDrop {
                storage: self,
                changes_before: self.conn.total_changes(),
            }),
        }
    }

    pub(super) fn reseal_logged(&self) {
        if let Err(error) = self.reseal() {
            tracing::error!(%error, "[Storage] resealing the mask profile failed");
        }
    }
}

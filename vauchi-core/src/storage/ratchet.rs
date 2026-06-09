// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage forwarders to [`RatchetStore`](super::RatchetStore).

use super::{Storage, StorageError};
use crate::crypto::ratchet::DoubleRatchetState;

impl Storage {
    /// Saves a Double Ratchet state for a contact.
    pub fn save_ratchet_state(
        &self,
        contact_id: &str,
        state: &DoubleRatchetState,
        is_initiator: bool,
    ) -> Result<(), StorageError> {
        self.ratchets()
            .save_ratchet_state(contact_id, state, is_initiator)
    }
    /// Loads a Double Ratchet state for a contact.
    ///
    /// Returns the ratchet state and whether this side was the initiator.
    pub fn load_ratchet_state(
        &self,
        contact_id: &str,
    ) -> Result<Option<(DoubleRatchetState, bool)>, StorageError> {
        self.ratchets().load_ratchet_state(contact_id)
    }
}

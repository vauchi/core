// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! What duress mode shows of the duress and decoy setup (#462, ADR-032).
//!
//! After the duress PIN unlocks the app, the coercer must not learn that
//! duress protection exists, nor be able to change or disable it. The
//! public duress and decoy methods therefore answer from this session-only
//! stand-in while `auth_mode` is `Duress`: it starts as "not set up", takes
//! whatever the coercer enters so the screens look like they work, and is
//! dropped at the next unlock. The real configuration stays behind the
//! storage reads used by `authenticate` (covert alert) and the biometric
//! PIN decision.

use std::sync::{Mutex, MutexGuard};

use crate::contact_card::ContactCard;
use crate::emergency::DuressSettings;
use crate::storage::Storage;

use super::{AuthMode, Vauchi};

#[derive(Default)]
pub(super) struct ConcealedDuress {
    pub(super) enabled: bool,
    pub(super) settings: Option<DuressSettings>,
    pub(super) decoys: Vec<(String, String, ContactCard)>,
}

pub(super) type ConcealedDuressCell = Mutex<ConcealedDuress>;

impl Vauchi {
    pub(super) fn in_duress_mode(&self) -> bool {
        self.auth_mode == AuthMode::Duress
    }

    /// Whether `contact_id` names a contact the duress session hides: in
    /// duress mode only decoy ids resolve, as in `get_contact`, so reads keyed
    /// by a real id answer as if it were unknown (ADR-032, private#388).
    pub(super) fn conceals_contact(
        &self,
        contact_id: &str,
    ) -> crate::api::error::VauchiResult<bool> {
        if !self.in_duress_mode() {
            return Ok(false);
        }
        Ok(!self
            .decoy_contacts_as_contacts()?
            .iter()
            .any(|decoy| decoy.id() == contact_id))
    }

    /// Poisoning cannot leave this state half-written in a way that
    /// matters: it holds nothing real, so a poisoned lock is recovered.
    pub(super) fn concealed_duress(&self) -> MutexGuard<'_, ConcealedDuress> {
        self.concealed_duress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn reset_concealed_duress(&self) {
        *self.concealed_duress() = ConcealedDuress::default();
    }

    /// Every unlock starts a fresh session: nothing a duress session saw or
    /// made outlives it, whichever unlock path ends it.
    pub(super) fn enter_auth_mode(&mut self, mode: AuthMode) {
        self.auth_mode = mode;
        self.reset_concealed_duress();
        self.reset_duress_store();
    }

    /// Where screens read and write groups, tags and places: in duress mode
    /// an empty session store, so the coercer sees none and what they make
    /// never reaches the real ones (#468). Engine internals that decide what
    /// real contacts receive keep using `self.storage`. Fails closed: without
    /// a session store, duress mode gets an error, never the real data.
    pub(super) fn vocabulary_store(&self) -> crate::api::error::VauchiResult<&Storage> {
        if !self.in_duress_mode() {
            return Ok(&self.storage);
        }
        self.duress_store.as_ref().ok_or_else(|| {
            crate::api::error::VauchiError::InvalidState("no duress session store".into())
        })
    }

    /// Starts or ends the duress session store at an unlock.
    fn reset_duress_store(&mut self) {
        self.duress_store = if self.in_duress_mode() {
            Storage::in_memory(crate::crypto::SymmetricKey::generate()).ok()
        } else {
            None
        };
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What duress mode shows of the duress and decoy setup (#462, ADR-032).
//!
//! After the duress PIN unlocks the app, the coercer must not learn that
//! duress protection exists, nor be able to change or disable it. The
//! public duress and decoy methods therefore answer from this session-only
//! stand-in while `auth_mode` is `Duress`: it starts as "not set up", takes
//! whatever the coercer enters so the screens look like they work, and is
//! dropped at the next unlock. The real configuration stays behind the
//! storage reads used by `authenticate` and the biometric PIN decision.

use std::sync::{Mutex, MutexGuard};

use crate::api::error::{VauchiError, VauchiResult};
use crate::card::ContactCard;
use crate::contact::place::ExchangeLocation;

use super::{AuthMode, Vauchi};

#[derive(Default)]
pub(super) struct ConcealedDuress {
    pub(super) enabled: bool,
    pub(super) decoys: Vec<(String, String, ContactCard)>,
    /// A duress edit's owed cover pass (`duress_cover.rs`), where a real
    /// edit's marker is durable: the marker itself must not outlive the
    /// session.
    pub(super) owed_cover: crate::reminders::OwnCardRepropagateState,
}

pub(super) type ConcealedDuressCell = Mutex<ConcealedDuress>;

impl Vauchi {
    pub(super) fn in_duress_mode(&self) -> bool {
        self.auth_mode == AuthMode::Duress
    }

    /// Whether `contact_id` names a contact the duress session hides: in
    /// duress mode only decoy ids resolve, as in `get_contact`, so reads keyed
    /// by a real id answer as if it were unknown (ADR-032, private#388).
    pub(super) fn conceals_contact(&self, contact_id: &str) -> VauchiResult<bool> {
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

    /// Every unlock starts a fresh session: what a duress session saw of
    /// the setup does not outlive it, whichever unlock path ends it. What
    /// it made (card, groups, tags, places) persists in the duress twins.
    pub(super) fn enter_auth_mode(&mut self, mode: AuthMode) {
        self.auth_mode = mode;
        self.reset_concealed_duress();
    }

    /// Where screens read and write groups: the real tables, or under
    /// duress their persistent twins (#468, vauchi/private#813), so the
    /// coercer sees none of the real ones, what they make never reaches
    /// them, and it is still there after a lock. Engine internals that
    /// decide what real contacts receive keep using `self.storage.labels()`.
    pub(super) fn labels_slot(&self) -> crate::storage::LabelStore<'_> {
        if self.in_duress_mode() {
            self.storage.duress_labels()
        } else {
            self.storage.labels()
        }
    }

    pub(super) fn tags_slot(&self) -> crate::storage::TagStore<'_> {
        if self.in_duress_mode() {
            self.storage.duress_tags()
        } else {
            self.storage.tags()
        }
    }

    pub(super) fn places_slot(&self) -> crate::storage::PlaceStore<'_> {
        if self.in_duress_mode() {
            self.storage.duress_places()
        } else {
            self.storage.places()
        }
    }

    /// Exchange locations live on the real contact rows, which a duress
    /// session never sees: it reads none and records none, the same answer
    /// an unknown contact gets.
    pub(super) fn visible_exchange_location(
        &self,
        contact_id: &str,
    ) -> VauchiResult<Option<ExchangeLocation>> {
        if self.in_duress_mode() {
            return Ok(None);
        }
        Ok(self.storage.load_exchange_location(contact_id)?)
    }

    pub(super) fn visible_exchange_locations(
        &self,
    ) -> VauchiResult<Vec<(String, ExchangeLocation)>> {
        if self.in_duress_mode() {
            return Ok(Vec::new());
        }
        Ok(self.storage.list_exchange_locations()?)
    }

    pub(super) fn reject_concealed_exchange_write(&self, contact_id: &str) -> VauchiResult<()> {
        if self.in_duress_mode() {
            return Err(VauchiError::ContactNotFound(contact_id.to_string()));
        }
        Ok(())
    }

    /// The own card a screen reads and writes: the real card, or under
    /// duress a copy of its own (vauchi/private#811), so what a coercer
    /// types reaches no contact. The copy persists across duress unlocks:
    /// an edit that vanished after a lock would give the mode away. Engine
    /// paths that decide what contacts receive keep
    /// `self.storage.contacts().own_card()`.
    pub(super) fn own_card_slot(&self) -> crate::storage::OwnCardStore<'_> {
        if self.in_duress_mode() {
            self.storage.contacts().duress_own_card()
        } else {
            self.storage.contacts().own_card()
        }
    }

    /// Starts the duress copy from the real card the first time a duress
    /// session needs it; later sessions keep what earlier ones edited.
    pub(super) fn seed_duress_own_card(&self) -> VauchiResult<()> {
        let contacts = self.storage.contacts();
        if contacts.duress_own_card().load()?.is_none()
            && let Some(real) = contacts.own_card().load()?
        {
            contacts.duress_own_card().save(&real)?;
        }
        Ok(())
    }
}

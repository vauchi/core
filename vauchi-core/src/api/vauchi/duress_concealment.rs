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

use super::{AuthMode, ProfileEngine};

#[derive(Default)]
pub(super) struct ConcealedDuress {
    pub(super) enabled: bool,
    pub(super) decoys: Vec<(String, String, ContactCard)>,
    /// A duress edit's owed cover pass (`duress_cover.rs`), where a real
    /// edit's marker is durable: the marker itself must not outlive the
    /// session.
    pub(super) owed_cover: crate::reminders::OwnCardRepropagateState,
    /// Cover a mask engine's edits owe the owner's other devices, journaled
    /// by the real engine when it next runs (ADR-080 point 5, amended
    /// 2026-11-09): random bytes, each the length of an edit's item.
    pub(super) owed_journal_cover: Vec<crate::sync::SyncItem>,
    /// The branch a duress session set duress up under, while `enabled`.
    pub(super) unlock: crate::emergency::DuressUnlock,
}

pub(super) type ConcealedDuressCell = Mutex<ConcealedDuress>;

impl ProfileEngine {
    pub(super) fn in_duress_mode(&self) -> bool {
        self.auth_mode == AuthMode::Duress
    }

    /// Whether this is the mask profile's engine (ADR-080 point 4).
    pub(super) fn is_mask(&self) -> bool {
        self.shown.is_some()
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

    /// Starts the duress copy from the real card the first time a duress
    /// session needs it; later sessions keep what earlier ones edited.
    pub(super) fn seed_duress_own_card(&self) -> VauchiResult<()> {
        let identity = self.storage.identity();
        if !identity.duress_copy_in_use()? {
            self.restart_duress_own_card()?;
            identity.mark_duress_copy_in_use()?;
        }
        Ok(())
    }

    /// Makes the duress copy the real card again. Every identity with an app
    /// password has the copy, so its row does not say whether a duress PIN
    /// was ever set (ADR-032, vauchi/private#947).
    pub(super) fn restart_duress_own_card(&self) -> VauchiResult<()> {
        let contacts = self.storage.contacts();
        if let Some(real) = contacts.own_card().load()? {
            self.storage.duress_own_card().save(&real)?;
        }
        Ok(())
    }
}

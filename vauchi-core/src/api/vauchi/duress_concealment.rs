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

    /// Poisoning cannot leave this state half-written in a way that
    /// matters: it holds nothing real, so a poisoned lock is recovered.
    pub(super) fn concealed_duress(&self) -> MutexGuard<'_, ConcealedDuress> {
        self.concealed_duress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(super) fn reset_concealed_duress(&self) {
        *self.concealed_duress() = ConcealedDuress::default();
    }
}

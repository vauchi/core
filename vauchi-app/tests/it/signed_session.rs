// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A multi-stage session as production builds one: for an identity and its
//! card, with a signed payload (vauchi/private#1217).

use vauchi_core::exchange::MultiStageSession;
use vauchi_core::{ContactCard, Identity};

pub fn signed_session(name: &str) -> MultiStageSession {
    MultiStageSession::for_identity(&Identity::create(name, 0), &ContactCard::new(name), None)
}

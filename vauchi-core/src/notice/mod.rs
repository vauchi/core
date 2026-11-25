// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The notice core: per-direction keys and records for mailbox cohorts.
//!
//! What it is for (ADR-081, owner 2026-11-24): Vauchi's relays must not
//! be able to reconstruct who is in touch with whom, alone or with the
//! OHTTP relay, at any growth stage; the alpha ships with this path
//! (release gate RG-16, vauchi/private#1281).
//!
//! Not yet wired into the app (vauchi/private#1375) and built only behind
//! the `notice-core` feature, so no shipped build contains it until then.

pub mod card;
pub mod chain;
pub mod content;
pub mod direction;
pub mod groups;
pub mod keys;
pub mod pairing;
pub mod publish;
pub mod record;
pub mod saved;
pub mod seal;
pub mod session;

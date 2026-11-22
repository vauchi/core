// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The notice core: per-direction keys and records for mailbox cohorts.
//!
//! Experimental and wired to nothing. It is the executable form of a
//! protocol still under review (vauchi/private#1281), built behind the
//! `notice-core` feature so that no shipped build contains it.

pub mod chain;
pub mod content;
pub mod direction;
pub mod keys;
pub mod record;
pub mod seal;

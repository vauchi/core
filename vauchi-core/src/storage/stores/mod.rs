// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-domain persistence views.
//!
//! Each `*Store<'a>` borrows the shared connection, encryption key, and clock
//! from [`Storage`](super::Storage) and exposes only one domain's operations,
//! turning the domain↔persistence boundary into a type-level one. Stores are
//! constructed solely by accessors on `Storage` (e.g. `Storage::recovery`).
//!
//! Rollout is phased — see problem record
//! `2026-06-09-storage-per-domain-store-boundaries`.

mod recovery;
mod sync;

pub use recovery::RecoveryStore;
pub use sync::SyncStore;

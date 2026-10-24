// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Compatibility exports for pure relay trust verification.

pub use crate::crypto::ohttp_key_trust::{
    HeldOhttpKey, OhttpAnchor, OhttpKeyRejection, RolloverRejection, accept_anchor_rollover,
    accept_signed_key, backup_commitment,
};

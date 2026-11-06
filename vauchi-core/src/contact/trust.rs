// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Trust level computation for contacts.
//!
//! `TrustLevel` is a pure, deterministic derivation from cryptographic
//! exchange facts already stored on `Contact`. It is never user-editable.

use serde::{Deserialize, Serialize};

/// Computed trust level derived from cryptographic exchange facts.
///
/// Derived deterministically from `Contact` fields; not user-editable.
/// Evaluated in order: Cautious, Verified, Remote, High, Standard.
///
/// Exhaustive on purpose: every consumer must place a new level, or a level
/// falls through a catch-all and reads as a stronger one (ADR-034).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustLevel {
    /// Identity was recovered — ratchet may have reset. Highest priority.
    Cautious,
    /// User manually verified the key fingerprint out-of-band.
    Verified,
    /// High proximity confidence + close-range transport (NFC or BLE).
    High,
    /// Normal exchange, no special indicators. Default.
    Standard,
    /// Exchanged over a link, so nobody met. Ranks below `Standard`; only a
    /// fingerprint comparison raises it (ADR-034 amendment 2026-11-06).
    Remote,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustLevel::Cautious => write!(f, "Cautious"),
            TrustLevel::Verified => write!(f, "Verified"),
            TrustLevel::High => write!(f, "High"),
            TrustLevel::Standard => write!(f, "Standard"),
            TrustLevel::Remote => write!(f, "Remote"),
        }
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Trust level computation for contacts.
//!
//! `TrustLevel` is a pure, deterministic derivation from cryptographic
//! exchange facts already stored on `Contact`. It is never user-editable.

use serde::{Deserialize, Serialize};

/// Computed trust level derived from cryptographic exchange facts.
///
/// Derived deterministically from `Contact` fields; not user-editable.
/// Priority order (highest wins): Cautious > Verified > High > Standard.
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
}

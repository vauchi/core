// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange reciprocity tracking.
//!
//! Tracks whether both parties completed a bilateral exchange.
//! Orthogonal to trust scoring (ADR-034).

use serde::{Deserialize, Serialize};

/// Whether the other party also completed the exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Reciprocity {
    /// Both sides confirmed via local or relay channel.
    Confirmed,
    /// Exchange completed locally, awaiting async confirmation.
    Pending,
    /// Confirmation window expired without reciprocation.
    Unreciprocated,
    /// Pre-feature contacts or hardware-limited fallback.
    Unknown,
}

/// Which confirmation channel resolved reciprocity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConfirmationChannel {
    /// Ultrasonic chirp (sub-second, in-person).
    Audio,
    /// BLE advertisement (sub-second, in-person).
    Ble,
    /// Relay escrow deposit/poll (seconds, requires internet).
    RelayEscrow,
    /// Encrypted update via relay sync (hours/days, async).
    RelaySync,
}

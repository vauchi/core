// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Duress Alert System
//!
//! When a user unlocks the app with their duress PIN, a silent alert is
//! queued and sent to trusted contacts via the normal sync channel.
//! The alert is serialized as a card update to be indistinguishable from
//! regular sync traffic.

use serde::{Deserialize, Serialize};

/// A duress alert to be sent to trusted contacts.
///
/// Queued when the user authenticates with the duress PIN.
/// Serialized as a card update to be indistinguishable from normal sync traffic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuressAlert {
    /// Unix timestamp (seconds) when the alert was generated.
    pub timestamp: u64,
    /// Device identifier string.
    pub device_id: String,
    /// Type of duress event that triggered the alert.
    pub alert_type: DuressAlertType,
}

/// The type of duress event that triggered the alert.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum DuressAlertType {
    /// The app was unlocked with the duress PIN.
    Unlock,
    /// A panic shred was triggered.
    Shred,
}

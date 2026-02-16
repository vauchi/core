// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Emergency Broadcast System
//!
//! One-tap encrypted alerts to trusted contacts. The alert is sent as
//! an `EncryptedUpdate` payload, making it indistinguishable from normal
//! card sync traffic on the wire.
//!
//! Constraints:
//! - Max 10 trusted contacts
//! - Default message: "I may be in danger. Please check on me."
//! - Alerts are E2E encrypted with each contact's shared key

use serde::{Deserialize, Serialize};

/// Maximum number of trusted contacts for emergency broadcast.
pub const MAX_TRUSTED_CONTACTS: usize = 10;

/// Default emergency message.
pub const DEFAULT_EMERGENCY_MESSAGE: &str = "I may be in danger. Please check on me.";

/// Configuration for emergency broadcast.
///
/// Stored in the `emergency_config` table (migration V22).
/// Determines which contacts receive alerts, what message is sent,
/// and whether device location is included.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergencyBroadcastConfig {
    /// Contact IDs of trusted contacts who receive emergency alerts.
    pub trusted_contact_ids: Vec<String>,
    /// Custom alert message included in the alert payload.
    pub message: String,
    /// Whether to include device location in the alert.
    pub include_location: bool,
}

/// Result of a broadcast operation.
#[derive(Debug, Clone)]
pub struct BroadcastResult {
    /// Number of alerts successfully queued for delivery.
    pub sent: usize,
    /// Total number of trusted contacts in the config.
    pub total: usize,
}

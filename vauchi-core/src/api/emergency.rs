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

/// Emergency wipe readiness status.
///
/// Aggregates the configuration state of all emergency-related features
/// so clients can display readiness at a glance.
#[derive(Debug, Clone)]
pub struct EmergencyWipeStatus {
    /// Whether emergency broadcast is configured (trusted contacts selected).
    pub broadcast_configured: bool,
    /// Whether duress PIN settings are configured.
    pub duress_configured: bool,
    /// Whether a soft shred (deletion) is currently scheduled.
    pub deletion_scheduled: bool,
    /// Whether a hard shred (deletion) has been executed.
    pub deletion_executed: bool,
    /// Whether there is at least one recovery-trusted contact.
    pub has_trusted_contacts: bool,
    /// Number of recovery-trusted contacts.
    pub trusted_contact_count: usize,
    /// Whether an app password is set (required for duress).
    pub password_enabled: bool,
}

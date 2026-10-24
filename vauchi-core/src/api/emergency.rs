// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
//!
//! `MAX_TRUSTED_CONTACTS` and `DEFAULT_EMERGENCY_MESSAGE` live in
//! `crate::emergency`, which is also available without network features.
//! Internal callers import from that owning module directly.

/// Minimum seconds between emergency broadcasts (UX guard).
///
/// Prevents accidental double-sends. Frontends should check this before
/// calling `send_emergency_broadcast()`. A future core version should
/// enforce this server-side.
pub const BROADCAST_COOLDOWN_SECS: u64 = 60;

/// Result of a broadcast operation.
#[derive(Debug, Clone)]
pub struct BroadcastResult {
    /// Number of alerts successfully queued for delivery.
    pub sent: usize,
    /// Total number of trusted contacts in the config.
    pub total: usize,
}

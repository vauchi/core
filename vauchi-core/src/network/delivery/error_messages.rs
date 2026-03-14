// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Human-readable failure message mapping for delivery errors.
//!
//! Converts delivery failure reasons to user-friendly explanations
//! that help users understand why a message delivery failed.

/// Converts a delivery failure reason to a user-friendly message.
///
/// # Arguments
/// * `reason` - Failure reason code (e.g. "connection_timeout", "key_mismatch")
///
/// # Returns
/// A human-readable explanation suitable for displaying to end users.
pub fn failure_to_user_message(reason: &str) -> String {
    match reason {
        "connection_timeout" => {
            "Could not reach relay server. Check your internet connection.".to_string()
        }
        "recipient_not_found" => {
            "Recipient not found. They may have deleted their identity.".to_string()
        }
        "key_mismatch" => {
            "Recipient's security key has changed. Please re-verify this contact.".to_string()
        }
        "quota_exceeded" => "Relay storage is full. Please try again later.".to_string(),
        "expired" => "Message expired before delivery (30-day limit).".to_string(),
        _ => "Delivery failed. Please try again or contact support if the problem persists."
            .to_string(),
    }
}

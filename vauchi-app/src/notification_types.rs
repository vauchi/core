// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared types for the notification and activity log system.

use serde::{Deserialize, Serialize};

pub use vauchi_core::EventOrigin;

/// Notification categories that produce OS notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub enum NotificationCategory {
    /// Always fires, no toggle.
    EmergencyAlert,
    /// Always fires, no toggle. Distinct from `EmergencyAlert` so the recipient
    /// can respond to a coerced sender appropriately (the sender entered their
    /// duress PIN — 2026-07-04-coercion-safety-alerts-never-received).
    DuressAlert,
    /// Opt-in, off by default.
    ContactAdded,
}

/// User preferences for OS notifications.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct NotificationPreferences {
    /// Whether contact-added notifications are enabled. Default: false.
    pub contact_added_enabled: bool,
}

/// An OS notification for the frontend to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
pub struct PendingNotification {
    pub event_key: String,
    pub category: NotificationCategory,
    pub title: String,
    pub body: String,
    pub contact_id: String,
}

/// Activity log entry types stored in the payload column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[serde(tag = "type")]
pub enum ActivityLogEntry {
    CardUpdateReceived {
        contact_id: String,
        changed_fields: Vec<String>,
    },
    CardUpdateDelivered {
        contact_id: String,
    },
    CardUpdatePending {
        contact_id: String,
    },
    CardUpdateFailed {
        contact_id: String,
        reason: String,
    },
    ContactAdded {
        contact_id: String,
        origin: EventOrigin,
    },
    EmergencyAlertReceived {
        contact_id: String,
    },
    DuressAlertReceived {
        contact_id: String,
    },
    OwnCardUpdated {
        changed_fields: Vec<String>,
    },
    ContactRemoved {
        contact_id: String,
    },
}

impl ActivityLogEntry {
    /// Returns the category string for the DB column.
    pub fn category_str(&self) -> &'static str {
        match self {
            Self::CardUpdateReceived { .. } => "card_update_received",
            Self::CardUpdateDelivered { .. } => "card_update_delivered",
            Self::CardUpdatePending { .. } => "card_update_pending",
            Self::CardUpdateFailed { .. } => "card_update_failed",
            Self::ContactAdded { .. } => "contact_added",
            Self::EmergencyAlertReceived { .. } => "emergency_alert_received",
            Self::DuressAlertReceived { .. } => "duress_alert_received",
            Self::OwnCardUpdated { .. } => "own_card_updated",
            Self::ContactRemoved { .. } => "contact_removed",
        }
    }

    /// Returns the contact_id from any variant.
    ///
    /// For `OwnCardUpdated`, returns "me".
    pub fn contact_id(&self) -> &str {
        match self {
            Self::CardUpdateReceived { contact_id, .. }
            | Self::CardUpdateDelivered { contact_id }
            | Self::CardUpdatePending { contact_id }
            | Self::CardUpdateFailed { contact_id, .. }
            | Self::ContactAdded { contact_id, .. }
            | Self::EmergencyAlertReceived { contact_id }
            | Self::DuressAlertReceived { contact_id }
            | Self::ContactRemoved { contact_id } => contact_id,
            Self::OwnCardUpdated { .. } => "me",
        }
    }
}

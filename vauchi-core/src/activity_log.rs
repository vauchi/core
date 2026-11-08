// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The activity log: what happened, as rows in a profile's own database.
//!
//! An [`ActivityLogEntry`] is one row's payload; `api::ActivityLogWriter`
//! turns events into rows.

use serde::{Deserialize, Serialize};

use crate::exchange_types::EventOrigin;

/// Retention window: 7 days in seconds.
pub const RETENTION_SECS: u64 = 7 * 24 * 3600;

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
            | Self::ContactRemoved { contact_id } => contact_id,
            Self::OwnCardUpdated { .. } => "me",
        }
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Maps `VauchiEvent` to affected screen IDs for the C ABI event callback.

use vauchi_core::api::VauchiEvent;

/// Returns a JSON array of screen IDs affected by the given event.
///
/// Example: `["contacts","contact_detail"]`
pub fn affected_screens_json(event: &VauchiEvent) -> String {
    let ids = affected_screens(event);
    if ids.is_empty() {
        return "[]".to_string();
    }
    let inner: Vec<String> = ids.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", inner.join(","))
}

fn affected_screens(event: &VauchiEvent) -> Vec<&'static str> {
    match event {
        VauchiEvent::ContactAdded { .. }
        | VauchiEvent::ContactUpdated { .. }
        | VauchiEvent::ContactRemoved { .. }
        | VauchiEvent::ContactHidden { .. }
        | VauchiEvent::ContactUnhidden { .. }
        | VauchiEvent::ContactBlocked { .. }
        | VauchiEvent::ContactUnblocked { .. }
        | VauchiEvent::ContactSoftDeleted { .. }
        | VauchiEvent::ContactArchived { .. }
        | VauchiEvent::ContactUnarchived { .. } => {
            vec!["contacts", "contact_detail"]
        }
        VauchiEvent::OwnCardUpdated { .. } => vec!["my_info"],
        VauchiEvent::SyncStateChanged { .. }
        | VauchiEvent::SyncProgress { .. }
        | VauchiEvent::LabelSyncCompleted { .. } => {
            vec!["sync", "contacts"]
        }
        VauchiEvent::MessageDelivered { .. }
        | VauchiEvent::MessageFailed { .. }
        | VauchiEvent::DeliveryStatusUpdate { .. }
        | VauchiEvent::PreExpiryWarning { .. } => {
            vec!["delivery_status"]
        }
        VauchiEvent::ConnectionStateChanged { .. }
        | VauchiEvent::RelayHealthChanged { .. }
        | VauchiEvent::RelayFailover { .. } => {
            vec!["sync"]
        }
        VauchiEvent::IncomingUpdate { .. } => {
            vec!["contacts", "contact_detail"]
        }
        VauchiEvent::VisibilityChanged { .. } => {
            vec!["my_info", "contacts"]
        }
        VauchiEvent::EmergencyAlertReceived { .. } | VauchiEvent::EmergencyBroadcastSent { .. } => {
            vec!["contacts"]
        }
        VauchiEvent::DowngradeDetected { .. } | VauchiEvent::Error { .. } => vec![],
        // VauchiEvent is #[non_exhaustive]
        _ => vec![],
    }
}

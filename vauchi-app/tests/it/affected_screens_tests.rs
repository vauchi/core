// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for `affected_screens` — the canonical event→screen-ID mapping.

use vauchi_app::ui::affected_screens;
use vauchi_core::api::{EventOrigin, VauchiEvent};
use vauchi_core::{ConnectionState, SyncState};

// @internal
#[test]
fn contact_events_invalidate_contacts_and_detail() {
    let events = [
        VauchiEvent::contact_added("c1".into(), EventOrigin::Local),
        VauchiEvent::ContactUpdated {
            contact_id: "c1".into(),
            changed_fields: vec!["name".into()],
        },
        VauchiEvent::ContactRemoved {
            contact_id: "c1".into(),
        },
        VauchiEvent::ContactHidden {
            contact_id: "c1".into(),
        },
        VauchiEvent::ContactUnhidden {
            contact_id: "c1".into(),
        },
        VauchiEvent::ContactBlocked {
            contact_id: "c1".into(),
        },
        VauchiEvent::ContactUnblocked {
            contact_id: "c1".into(),
        },
        VauchiEvent::ContactSoftDeleted {
            contact_id: "c1".into(),
        },
        VauchiEvent::ContactArchived {
            contact_id: "c1".into(),
        },
        VauchiEvent::ContactUnarchived {
            contact_id: "c1".into(),
        },
    ];

    for event in &events {
        let ids = affected_screens(event);
        assert_eq!(
            ids,
            vec!["contacts", "contact_detail"],
            "event {event:?} should invalidate contacts + contact_detail"
        );
    }
}

// @internal
#[test]
fn own_card_updated_invalidates_my_info() {
    let event = VauchiEvent::OwnCardUpdated {
        changed_fields: vec!["phone".into()],
    };
    assert_eq!(affected_screens(&event), vec!["my_info"]);
}

// @internal
#[test]
fn sync_lifecycle_events_invalidate_nothing() {
    // Lifecycle chatter (progress ticks, per-contact state, label-sync
    // completion) does NOT touch contact data — applied changes dispatch
    // their own precise per-item events (5d13a463). Mapping chatter to
    // "contacts" was the pre-5d13a463 coarse catch-all; keeping it would
    // wipe in-progress list state (search query, facets) on every
    // background-sync tick. The standalone Sync screen was retired
    // (M4 S2); the chrome sync chip reflects status via
    // `sync_chrome_status`, updated in the sync handler — not by
    // invalidating a screen. So these events now invalidate nothing.
    let events = [
        VauchiEvent::SyncStateChanged {
            contact_id: "c1".into(),
            state: SyncState::Synced { last_sync: 0 },
        },
        VauchiEvent::SyncProgress {
            total: 10,
            processed: 5,
            contact_id: "c1".into(),
        },
        VauchiEvent::LabelSyncCompleted {
            label_id: "l1".into(),
        },
    ];

    for event in &events {
        let ids = affected_screens(event);
        assert!(
            ids.is_empty(),
            "event {event:?} should invalidate no screen (Sync screen retired), got {ids:?}"
        );
    }
}

// @internal
#[test]
fn delivery_events_invalidate_delivery_status() {
    let events = [
        VauchiEvent::MessageDelivered {
            contact_id: "c1".into(),
            message_id: "m1".into(),
        },
        VauchiEvent::MessageFailed {
            contact_id: "c1".into(),
            error: "timeout".into(),
        },
        VauchiEvent::DeliveryStatusUpdate {
            message_id: "m1".into(),
            status: "delivered".into(),
        },
        VauchiEvent::PreExpiryWarning {
            message_id: "m1".into(),
            expires_at: 1234567890,
        },
    ];

    for event in &events {
        let ids = affected_screens(event);
        assert_eq!(
            ids,
            vec!["delivery_status"],
            "event {event:?} should invalidate delivery_status"
        );
    }
}

// @internal
#[test]
fn connection_events_invalidate_nothing() {
    // Connection/relay-health events drove the retired Sync screen (M4 S2).
    // The chrome sync chip does not re-render off these events, so they now
    // invalidate nothing.
    let events = [
        VauchiEvent::ConnectionStateChanged {
            state: ConnectionState::Connected,
        },
        VauchiEvent::RelayHealthChanged {
            relay_url: "https://relay.example.com".into(),
            healthy: true,
        },
        VauchiEvent::RelayFailover {
            from: "https://old.example.com".into(),
            to: "https://new.example.com".into(),
        },
    ];

    for event in &events {
        let ids = affected_screens(event);
        assert!(
            ids.is_empty(),
            "event {event:?} should invalidate no screen (Sync screen retired), got {ids:?}"
        );
    }
}

// @internal
#[test]
fn incoming_update_invalidates_contacts_and_detail() {
    let event = VauchiEvent::IncomingUpdate {
        contact_id: "c1".into(),
    };
    assert_eq!(affected_screens(&event), vec!["contacts", "contact_detail"]);
}

// @internal
#[test]
fn visibility_changed_invalidates_my_info_and_contacts() {
    let event = VauchiEvent::VisibilityChanged {
        contact_id: "c1".into(),
        field: "phone".into(),
    };
    assert_eq!(affected_screens(&event), vec!["my_info", "contacts"]);
}

// @internal
#[test]
fn emergency_events_invalidate_contacts() {
    let events = [
        VauchiEvent::EmergencyAlertReceived {
            contact_id: "c1".into(),
            message: "help".into(),
            timestamp: 1234567890,
            location: Some((47.0, 8.0)),
        },
        VauchiEvent::EmergencyBroadcastSent {
            sent_count: 3,
            total: 5,
        },
    ];

    for event in &events {
        let ids = affected_screens(event);
        assert_eq!(
            ids,
            vec!["contacts"],
            "event {event:?} should invalidate contacts"
        );
    }
}

// @internal
#[test]
fn non_screen_events_return_empty() {
    let events = [
        VauchiEvent::DowngradeDetected {
            contact_id: "c1".into(),
            expected_version: 2,
            received_version: 1,
        },
        VauchiEvent::Error {
            message: "something".into(),
        },
    ];

    for event in &events {
        let ids = affected_screens(event);
        assert!(
            ids.is_empty(),
            "event {event:?} should not invalidate any screen"
        );
    }
}

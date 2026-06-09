// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for AppEngine delivery status screen population.
//!
//! Verifies that navigating to `AppScreen::DeliveryStatus` shows actual
//! delivery records from storage, not an empty placeholder.
//!
//! Traces to: features/message_delivery.feature @delivery @status

use vauchi_app::ui::{AppEngine, AppScreen, Component, Status};
use vauchi_core::api::Vauchi;
use vauchi_core::storage::{DeliveryRecord, DeliveryStatus};

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// @scenario: message_delivery :: Delivery status screen shows pending and failed records
#[test]
fn test_delivery_status_screen_shows_records_from_storage() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let timestamp = now();

    vauchi
        .storage()
        .deliveries()
        .create_delivery_record(&DeliveryRecord {
            message_id: "msg-pending".to_string(),
            recipient_id: "contact-bob".to_string(),
            status: DeliveryStatus::Sent,
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        })
        .unwrap();

    vauchi
        .storage()
        .deliveries()
        .create_delivery_record(&DeliveryRecord {
            message_id: "msg-failed".to_string(),
            recipient_id: "contact-carol".to_string(),
            status: DeliveryStatus::Failed {
                reason: "timeout".to_string(),
            },
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        })
        .unwrap();

    vauchi
        .storage()
        .deliveries()
        .create_delivery_record(&DeliveryRecord {
            message_id: "msg-delivered".to_string(),
            recipient_id: "contact-dave".to_string(),
            status: DeliveryStatus::Delivered,
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        })
        .unwrap();

    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::DeliveryStatus);

    assert_eq!(screen.screen_id, "delivery_status");

    // With 3 delivery records in storage, we must see StatusIndicator
    // components — NOT the "All Delivered" empty InfoPanel.
    let status_indicators: Vec<_> = screen
        .components
        .iter()
        .filter(|c| matches!(c, Component::StatusIndicator { .. }))
        .collect();

    assert!(
        !status_indicators.is_empty(),
        "Expected StatusIndicator components for delivery records, \
         but got: {:?}",
        screen.components
    );

    assert_eq!(
        status_indicators.len(),
        3,
        "Expected 3 StatusIndicator components (one per delivery record)"
    );
}

// @scenario: message_delivery :: Delivery status screen shows empty state when no records
#[test]
fn test_delivery_status_screen_empty_when_no_records() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::DeliveryStatus);

    assert_eq!(screen.screen_id, "delivery_status");

    // No delivery records → show "All Delivered" InfoPanel
    let info_panels: Vec<_> = screen
        .components
        .iter()
        .filter(|c| matches!(c, Component::InfoPanel { .. }))
        .collect();

    assert_eq!(
        info_panels.len(),
        1,
        "Expected exactly 1 InfoPanel for empty delivery status"
    );
}

// @scenario: message_delivery :: Failed deliveries show retry action
#[test]
fn test_delivery_status_screen_shows_retry_for_failed() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let timestamp = now();

    vauchi
        .storage()
        .deliveries()
        .create_delivery_record(&DeliveryRecord {
            message_id: "msg-fail".to_string(),
            recipient_id: "contact-eve".to_string(),
            status: DeliveryStatus::Failed {
                reason: "relay unreachable".to_string(),
            },
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        })
        .unwrap();

    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::DeliveryStatus);

    assert_eq!(screen.screen_id, "delivery_status");

    let retry_action = screen.actions.iter().find(|a| a.id == "retry_all");
    assert!(
        retry_action.is_some(),
        "Expected 'retry_all' action for failed deliveries, got actions: {:?}",
        screen.actions
    );
}

// @scenario: message_delivery :: Delivery status maps storage statuses to UI statuses correctly
#[test]
fn test_delivery_status_maps_statuses_correctly() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let timestamp = now();

    // Queued → Pending
    vauchi
        .storage()
        .deliveries()
        .create_delivery_record(&DeliveryRecord {
            message_id: "msg-queued".to_string(),
            recipient_id: "contact-a".to_string(),
            status: DeliveryStatus::Queued,
            created_at: timestamp,
            updated_at: timestamp,
            expires_at: None,
        })
        .unwrap();

    // Delivered → Success
    vauchi
        .storage()
        .deliveries()
        .create_delivery_record(&DeliveryRecord {
            message_id: "msg-delivered".to_string(),
            recipient_id: "contact-b".to_string(),
            status: DeliveryStatus::Delivered,
            created_at: timestamp + 1,
            updated_at: timestamp + 1,
            expires_at: None,
        })
        .unwrap();

    // Failed → Failed
    vauchi
        .storage()
        .deliveries()
        .create_delivery_record(&DeliveryRecord {
            message_id: "msg-failed".to_string(),
            recipient_id: "contact-c".to_string(),
            status: DeliveryStatus::Failed {
                reason: "timeout".to_string(),
            },
            created_at: timestamp + 2,
            updated_at: timestamp + 2,
            expires_at: None,
        })
        .unwrap();

    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::DeliveryStatus);

    // The screen now groups records into sections (Recent / Failed /
    // Pending Retries). Within "Recent" the storage created_at DESC
    // order is preserved; "Failed" is its own section, so the failed
    // record appears after the recent ones (not interleaved).
    let statuses: Vec<&Status> = screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::StatusIndicator { status, .. } => Some(status),
            _ => None,
        })
        .collect();

    assert_eq!(statuses.len(), 3, "Expected 3 status indicators");
    // Recent section: delivered (ts+1), queued (ts) — failed is excluded
    assert_eq!(statuses[0], &Status::Success, "Delivered → Status::Success");
    assert_eq!(statuses[1], &Status::Pending, "Queued → Status::Pending");
    // Failed section: failed
    assert_eq!(statuses[2], &Status::Failed, "Failed → Status::Failed");
}

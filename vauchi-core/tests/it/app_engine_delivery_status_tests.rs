// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for AppEngine delivery status screen population.
//!
//! Verifies that navigating to `AppScreen::DeliveryStatus` shows actual
//! delivery records from storage, sorted into the Recent / Pending /
//! Failed lists with localized details (#445), not an empty placeholder.
//!
//! Traces to: features/message_delivery.feature @delivery @status

use vauchi_app::ui::{AppEngine, AppScreen, Component, ScreenModel, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::storage::{DeliveryRecord, DeliveryStatus};

const TWO_HOURS: u64 = 2 * 60 * 60;

fn two_hours_ago() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - TWO_HOURS
}

fn record(vauchi: &Vauchi, message_id: &str, recipient_id: &str, status: DeliveryStatus) {
    let at = two_hours_ago();
    vauchi
        .storage()
        .deliveries()
        .create_delivery_record(&DeliveryRecord {
            message_id: message_id.to_string(),
            recipient_id: recipient_id.to_string(),
            status,
            created_at: at,
            updated_at: at,
            expires_at: None,
        })
        .unwrap();
}

fn switch_labels(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Dropdown { id, options, .. } if id == "delivery_filter" => {
                Some(options.iter().map(|o| o.label.clone()).collect())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn row_details(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "deliveries" => Some(
                items
                    .iter()
                    .map(|i| i.subtitle.clone().unwrap_or_default())
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn show(engine: &mut AppEngine, list: &str) -> ScreenModel {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: "delivery_filter".into(),
        item_id: list.into(),
    });
    engine.current_screen()
}

// @scenario: message_delivery :: Delivery status screen shows pending and failed records
#[test]
fn test_delivery_status_screen_shows_records_from_storage() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    record(&vauchi, "msg-pending", "contact-bob", DeliveryStatus::Sent);
    record(
        &vauchi,
        "msg-failed",
        "contact-carol",
        DeliveryStatus::Failed {
            reason: "timeout".to_string(),
        },
    );
    record(
        &vauchi,
        "msg-delivered",
        "contact-dave",
        DeliveryStatus::Delivered,
    );

    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::DeliveryStatus);

    assert_eq!(screen.screen_id, "delivery_status");
    assert_eq!(
        switch_labels(&screen),
        ["Recent (1)", "Pending (1)", "Failed (1)"],
        "each stored record is counted in exactly one list"
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
    record(
        &vauchi,
        "msg-fail",
        "contact-eve",
        DeliveryStatus::Failed {
            reason: "relay unreachable".to_string(),
        },
    );

    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::DeliveryStatus);

    assert_eq!(screen.screen_id, "delivery_status");
    let retry_all = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::ButtonList { id, items } if id == "delivery_actions"
                && items.iter().any(|i| i.id == "retry_all")
        )
    });
    assert!(
        retry_all,
        "Expected a 'retry_all' button in the Failed list, got: {:?}",
        screen.components
    );
}

// @scenario: message_delivery :: Delivery status maps storage statuses to UI statuses correctly
#[test]
fn test_delivery_status_maps_statuses_correctly() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    record(&vauchi, "msg-queued", "contact-a", DeliveryStatus::Queued);
    record(
        &vauchi,
        "msg-delivered",
        "contact-b",
        DeliveryStatus::Delivered,
    );
    record(
        &vauchi,
        "msg-failed",
        "contact-c",
        DeliveryStatus::Failed {
            reason: "timeout".to_string(),
        },
    );

    let mut engine = AppEngine::new(vauchi);
    let failed = engine.navigate_to(AppScreen::DeliveryStatus);

    assert_eq!(row_details(&failed), ["Failed: timeout · 2 hours ago"]);
    assert_eq!(
        row_details(&show(&mut engine, "pending")),
        ["Queued · 2 hours ago"]
    );
    assert_eq!(
        row_details(&show(&mut engine, "recent")),
        ["Delivered · 2 hours ago"]
    );
}

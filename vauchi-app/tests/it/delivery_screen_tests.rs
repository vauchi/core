// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Update Delivery screen (#445, design pass #419 item 5; owner
//! decisions 2026-09-30): a Recent / Pending / Failed switch with counts
//! that opens on the most urgent non-empty list, so a failure is never
//! hidden behind a tab; rows that open the contact; one "Retry all
//! failed" button in the body of the Failed list; canvas copy.

use vauchi_app::ui::delivery::RetryEntry;
use vauchi_app::ui::{
    ActionResult, Component, DeliveryItem, DeliveryStatusEngine, ScreenModel, Status, UserAction,
    WorkflowEngine,
};

fn delivered(name: &str) -> DeliveryItem {
    DeliveryItem {
        message_id: format!("msg-{name}"),
        contact_id: format!("contact-{name}"),
        contact_name: name.into(),
        status: Status::Success,
        detail: Some("Delivered to 2 of 2 devices · 5 minutes ago".into()),
        retryable: false,
    }
}

fn failed(name: &str) -> DeliveryItem {
    DeliveryItem {
        message_id: format!("msg-{name}"),
        contact_id: format!("contact-{name}"),
        contact_name: name.into(),
        status: Status::Failed,
        detail: Some("Failed: relay unreachable · 1 hour ago".into()),
        retryable: true,
    }
}

fn retry(name: &str, attempt: u32, in_secs: Option<u64>, max_exceeded: bool) -> RetryEntry {
    RetryEntry {
        message_id: format!("retry-{name}"),
        contact_id: format!("contact-{name}"),
        contact_name: name.into(),
        attempt,
        max_attempts: 5,
        max_exceeded,
        next_retry_in_secs: in_secs,
    }
}

fn switch(screen: &ScreenModel) -> Option<(Option<String>, Vec<String>)> {
    screen.components.iter().find_map(|c| match c {
        Component::Dropdown {
            id,
            selected,
            options,
            ..
        } if id == "delivery_filter" => Some((
            selected.clone(),
            options.iter().map(|o| o.label.clone()).collect(),
        )),
        _ => None,
    })
}

fn rows(screen: &ScreenModel) -> Vec<(String, Option<String>)> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "deliveries" => Some(
                items
                    .iter()
                    .map(|i| (i.name.clone(), i.subtitle.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn has_retry_button(screen: &ScreenModel) -> bool {
    screen.components.iter().any(|c| {
        matches!(
            c,
            Component::ButtonList { id, items } if id == "delivery_actions"
                && items.iter().any(|i| i.id == "retry_all" && i.label == "Retry all failed")
        )
    })
}

fn texts(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Text { content, .. } => Some(content.clone()),
            Component::InfoPanel { title, .. } => Some(title.clone()),
            _ => None,
        })
        .collect()
}

fn select(engine: &mut DeliveryStatusEngine, component: &str, item: &str) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: component.into(),
        item_id: item.into(),
    })
}

fn mixed() -> DeliveryStatusEngine {
    DeliveryStatusEngine::new(vec![delivered("Amira"), failed("Léa")]).with_retries(vec![retry(
        "Jonas",
        2,
        Some(200),
        false,
    )])
}

// @internal
#[test]
fn the_screen_is_update_delivery_with_nothing_in_the_toolbar() {
    let screen = mixed().current_screen();
    assert_eq!(screen.title, "Update Delivery");
    assert!(
        screen.contextual_actions.is_empty(),
        "shells fold toolbar actions into menus"
    );
}

// @internal
#[test]
fn the_switch_counts_each_list_and_opens_on_the_most_urgent() {
    let screen = mixed().current_screen();
    let (selected, labels) = switch(&screen).expect("the Recent/Pending/Failed switch");
    assert_eq!(labels, ["Recent (1)", "Pending (1)", "Failed (1)"]);
    assert_eq!(
        selected.as_deref(),
        Some("failed"),
        "a failure is never hidden behind a tab"
    );
    assert_eq!(
        rows(&screen),
        [(
            "Léa".to_string(),
            Some("Failed: relay unreachable · 1 hour ago".to_string())
        )]
    );

    let no_failures = DeliveryStatusEngine::new(vec![delivered("Amira")])
        .with_retries(vec![retry("Jonas", 2, Some(200), false)])
        .current_screen();
    assert_eq!(switch(&no_failures).unwrap().0.as_deref(), Some("pending"));

    let only_recent = DeliveryStatusEngine::new(vec![delivered("Amira")]).current_screen();
    assert_eq!(switch(&only_recent).unwrap().0.as_deref(), Some("recent"));
}

// @internal
#[test]
fn choosing_a_list_shows_its_rows() {
    let mut engine = mixed();
    let result = select(&mut engine, "delivery_filter", "recent");
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("choosing a list updates the screen, got {result:?}");
    };
    assert_eq!(switch(&screen).unwrap().0.as_deref(), Some("recent"));
    assert_eq!(
        rows(&screen),
        [(
            "Amira".to_string(),
            Some("Delivered to 2 of 2 devices · 5 minutes ago".to_string())
        )]
    );
}

// @internal
#[test]
fn a_pending_retry_says_when_it_tries_again() {
    let mut engine = DeliveryStatusEngine::new(vec![]).with_retries(vec![
        retry("Jonas", 2, Some(200), false),
        retry("Priya", 3, Some(0), false),
    ]);
    let screen = engine.current_screen();
    assert_eq!(
        rows(&screen),
        [
            (
                "Jonas".to_string(),
                Some("Attempt 2 of 5 · next retry in 4 min".to_string())
            ),
            (
                "Priya".to_string(),
                Some("Attempt 3 of 5 · retrying now".to_string())
            ),
        ]
    );
    let _ = select(&mut engine, "delivery_filter", "failed");
    assert!(texts(&engine.current_screen()).contains(&"No failed deliveries.".to_string()));
}

// @internal
#[test]
fn a_retry_out_of_attempts_is_a_failure() {
    let screen = DeliveryStatusEngine::new(vec![])
        .with_retries(vec![retry("Tomás", 5, None, true)])
        .current_screen();
    assert_eq!(switch(&screen).unwrap().0.as_deref(), Some("failed"));
    assert_eq!(
        rows(&screen),
        [(
            "Tomás".to_string(),
            Some("Max attempts (5) exceeded".to_string())
        )]
    );
}

// @internal
#[test]
fn a_row_opens_its_contact_not_its_message() {
    let mut engine = mixed();
    let result = select(&mut engine, "deliveries", "msg-Léa");
    assert_eq!(
        result,
        ActionResult::OpenContact {
            contact_id: "contact-Léa".into()
        }
    );
}

// @internal
#[test]
fn only_the_failed_list_offers_retry_all() {
    let mut engine = mixed();
    assert!(has_retry_button(&engine.current_screen()));
    let result = select(&mut engine, "delivery_actions", "retry_all");
    assert_eq!(
        result,
        ActionResult::RetryFailedDeliveries {
            message_ids: vec!["msg-Léa".into()]
        }
    );
    let _ = select(&mut engine, "delivery_filter", "recent");
    assert!(!has_retry_button(&engine.current_screen()));
}

// @internal
#[test]
fn nothing_to_deliver_says_everything_arrived() {
    let screen = DeliveryStatusEngine::new(vec![]).current_screen();
    assert!(switch(&screen).is_none(), "no switch without deliveries");
    assert!(
        texts(&screen).contains(
            &"Everything you changed has reached everyone who should have it.".to_string()
        )
    );
}

// @internal
#[test]
fn help_explains_what_a_delivery_is() {
    let mut engine = mixed();
    let result = select(&mut engine, "delivery_help", "help");
    match result {
        ActionResult::ShowInfoOverlay { title, body } => {
            assert_eq!(title, "What a delivery is");
            assert!(body.starts_with("When you change your card"), "{body}");
        }
        other => panic!("expected the help overlay, got {other:?}"),
    }
}

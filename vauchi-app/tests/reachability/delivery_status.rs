// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `DeliveryStatusEngine`.

use vauchi_app::ui::testing::{assert_reachability_across_screens, check_reachability};
use vauchi_app::ui::{DeliveryItem, DeliveryStatusEngine, Status, WorkflowEngine};

/// Action ids handled by `DeliveryStatusEngine` —
/// `core/vauchi-app/src/ui/delivery.rs:RETRY_ALL_ACTION_ID`.
const HANDLED: &[&str] = &["retry_all"];

fn factory() -> DeliveryStatusEngine {
    DeliveryStatusEngine::new(vec![
        DeliveryItem {
            message_id: "msg-alice".into(),
            contact_id: "c-alice".into(),
            contact_name: "Alice".into(),
            status: Status::Success,
            detail: None,
            retryable: false,
        },
        DeliveryItem {
            message_id: "msg-bob".into(),
            contact_id: "c-bob".into(),
            contact_name: "Bob".into(),
            status: Status::Failed,
            detail: Some("network error".into()),
            retryable: true,
        },
    ])
}

// @internal
#[test]
fn delivery_status_screen_is_fully_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "delivery_status");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn delivery_status_has_no_orphans() {
    let report = check_reachability(factory, HANDLED);
    assert!(report.is_reachable(), "unexpected orphans: {report:?}");
}

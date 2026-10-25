// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(feature = "testing")]

use std::sync::Arc;
use std::time::{Duration, SystemTime};
use vauchi_app::notification_types::{ActivityLogEntry, PendingNotification};
use vauchi_app::ui::{AppEngine, UserAction, WorkflowEngine};
use vauchi_core::clock::FakeClock;
use vauchi_core::{Vauchi, VauchiEvent, storage::ActivityLogRow};

const NOW: u64 = 1_700_000_000;

fn engine() -> (AppEngine, Arc<FakeClock>) {
    let clock = Arc::new(FakeClock::new(at(NOW)));
    let mut vauchi = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    vauchi.create_identity("Alice").unwrap();
    (AppEngine::new(vauchi), clock)
}

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

fn alert(engine: &AppEngine, nonce: u8) {
    engine
        .vauchi()
        .dispatch_event(VauchiEvent::EmergencyAlertReceived {
            contact_id: "bob".into(),
            message: "HELP".into(),
            timestamp: NOW,
            location: None,
            alert_nonce: [nonce; 32],
        });
}

fn assert_alert(notifications: &[PendingNotification], nonce: u8) {
    assert_eq!(notifications.len(), 1);
    assert_eq!(
        notifications[0].event_key,
        format!("alert:bob:{}", hex::encode([nonce; 32]))
    );
    assert_eq!(notifications[0].title, "Emergency Alert");
}

// @internal — both legacy entry points share one handoff ledger (#764).
#[test]
fn notification_delivery_drain_then_poll_does_not_repeat() {
    let (mut engine, _) = engine();
    alert(&engine, 1);
    assert_alert(&engine.drain_pending_notifications(), 1);
    assert_eq!(engine.poll_notifications(), Vec::new());
    assert_eq!(engine.drain_pending_notifications(), Vec::new());
}

// @internal — inclusive wall-clock queries must not replay the same row (#764).
#[test]
fn notification_delivery_repeated_poll_is_empty() {
    let (mut engine, _) = engine();
    alert(&engine, 1);
    assert_alert(&engine.poll_notifications(), 1);
    assert_eq!(engine.drain_pending_notifications(), Vec::new());
    assert_eq!(engine.poll_notifications(), Vec::new());
}

// @internal — persistence during an action must not consume notification delivery (#764).
#[test]
fn notification_delivery_action_before_drain_preserves_pending_alert() {
    let (mut engine, _) = engine();
    alert(&engine, 1);
    engine.handle_action(UserAction::ActionPressed {
        action_id: "unknown".into(),
    });
    assert_alert(&engine.drain_pending_notifications(), 1);
    assert_eq!(engine.poll_notifications(), Vec::new());
}

// @internal — distinct entries inserted in one second each get one handoff (#764).
#[test]
fn notification_delivery_later_same_second_insert_only_returns_new_entry() {
    let (mut engine, _) = engine();
    alert(&engine, 1);
    assert_alert(&engine.poll_notifications(), 1);
    alert(&engine, 2);
    assert_alert(&engine.poll_notifications(), 2);
    assert_eq!(engine.poll_notifications(), Vec::new());
}

// @internal — delivery order cannot depend on the wall clock (#764).
#[test]
fn notification_delivery_clock_rollback_does_not_drop_or_repeat() {
    let (mut engine, clock) = engine();
    alert(&engine, 1);
    assert_alert(&engine.poll_notifications(), 1);
    clock.set(at(NOW - 60));
    alert(&engine, 2);
    assert_alert(&engine.poll_notifications(), 2);
    assert_eq!(engine.poll_notifications(), Vec::new());
}

// @internal — callback draining honors the same configured preferences as polling (#764).
#[test]
fn notification_delivery_disabled_updates_are_not_replayed_when_enabled() {
    let (mut engine, clock) = engine();
    engine.vauchi_mut().config_mut().card_update_notifications = false;
    engine.vauchi().dispatch_event(VauchiEvent::IncomingUpdate {
        contact_id: "bob".into(),
    });
    assert_eq!(engine.drain_pending_notifications(), Vec::new());
    engine.vauchi_mut().config_mut().card_update_notifications = true;
    assert_eq!(engine.poll_notifications(), Vec::new());
    clock.advance(Duration::from_secs(1));
    engine.vauchi().dispatch_event(VauchiEvent::IncomingUpdate {
        contact_id: "bob".into(),
    });
    let pending = engine.poll_notifications();
    assert_eq!(pending.len(), 1);
    assert_eq!(
        pending[0].event_key,
        format!("card_received:bob:{}", NOW + 1)
    );
}

// @internal — deleting every retained row must not invalidate delivery accounting (#764).
#[test]
fn notification_delivery_after_pruning_all_rows_still_returns_new_entries() {
    let (mut engine, clock) = engine();
    alert(&engine, 1);
    assert_alert(&engine.poll_notifications(), 1);
    clock.advance(Duration::from_secs(8 * 86400));
    assert_eq!(
        engine
            .vauchi()
            .storage()
            .activity_log()
            .activity_log_prune(NOW + 8 * 86400, 7 * 86400)
            .unwrap(),
        1
    );
    alert(&engine, 2);
    assert_alert(&engine.poll_notifications(), 2);
    assert_eq!(engine.poll_notifications(), Vec::new());
}

// @internal — a non-ASCII identifier must not panic during fallback copy generation (#764).
#[test]
fn notification_delivery_unicode_unknown_contact_does_not_panic() {
    let (mut engine, _) = engine();
    engine.vauchi().dispatch_event(VauchiEvent::IncomingUpdate {
        contact_id: "日本語の連絡先".into(),
    });
    let pending = engine.drain_pending_notifications();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].contact_id, "日本語の連絡先");
}

fn stored_alert(event_key: &str, created_at: u64) -> ActivityLogRow {
    ActivityLogRow {
        event_key: event_key.into(),
        category: "emergency".into(),
        contact_id: Some("bob".into()),
        payload: serde_json::to_string(&ActivityLogEntry::EmergencyAlertReceived {
            contact_id: "bob".into(),
        })
        .unwrap(),
        created_at,
    }
}

// @internal — collection honors retention even if no new domain event triggers pruning (#764).
#[test]
fn notification_delivery_replays_pending_recent_rows_but_not_expired_history() {
    let (mut engine, _) = engine();
    for row in [
        stored_alert("expired", NOW - 8 * 86400),
        stored_alert("pending", NOW - 60),
    ] {
        engine
            .vauchi()
            .storage()
            .activity_log()
            .activity_log_insert(&row)
            .unwrap();
    }
    let pending = engine.drain_pending_notifications();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].event_key, "pending");
    assert_eq!(engine.poll_notifications(), Vec::new());
}

// @internal — malformed JSON is consumed once without losing neighboring valid entries (#764).
#[test]
fn notification_delivery_consumes_malformed_payload_without_blocking_valid_rows() {
    let (mut engine, _) = engine();
    let mut malformed = stored_alert("malformed", NOW);
    malformed.payload = "not JSON".into();
    engine
        .vauchi()
        .storage()
        .activity_log()
        .activity_log_insert(&malformed)
        .unwrap();
    alert(&engine, 1);
    assert_alert(&engine.drain_pending_notifications(), 1);
    assert_eq!(
        engine
            .vauchi()
            .storage()
            .activity_log()
            .claim_notifications()
            .unwrap(),
        Vec::new()
    );
}

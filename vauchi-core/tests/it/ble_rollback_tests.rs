// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for BLE exchange rollback tracking.

use vauchi_core::ExchangeError;
use vauchi_core::exchange::BleRollback;

// @scenario: ble_exchange :: Rollback clears pending contact data
#[test]
fn test_rollback_clears_pending_data() {
    let mut rb = BleRollback::new();
    rb.record_pending("contact-1".to_string(), vec![1, 2, 3]);
    assert!(rb.has_pending("contact-1"));

    let result = rb.rollback("contact-1");
    assert!(result.is_ok(), "expected success");
    assert!(!rb.has_pending("contact-1"));
}

// @scenario: ble_exchange :: Rollback on nonexistent contact is a no-op
#[test]
fn test_rollback_nonexistent_is_noop() {
    let mut rb = BleRollback::new();
    let result = rb.rollback("does-not-exist");
    assert!(result.is_ok(), "expected success");
}

// @scenario: ble_exchange :: Commit returns pending data and removes it
#[test]
fn test_commit_removes_pending() {
    let mut rb = BleRollback::new();
    let data = vec![10, 20, 30];
    rb.record_pending("contact-2".to_string(), data.clone());

    let committed = rb.commit("contact-2");
    assert!(committed.is_ok(), "expected success");
    assert_eq!(committed.unwrap(), data);
    assert!(!rb.has_pending("contact-2"));
}

// @scenario: ble_exchange :: Commit on nonexistent contact returns error
#[test]
fn test_commit_nonexistent_returns_error() {
    let mut rb = BleRollback::new();
    let result = rb.commit("missing");
    assert!(result.is_err(), "expected error");
    match result {
        Err(ExchangeError::InvalidState(msg)) => {
            assert!(msg.contains("missing"), "error should contain contact_id");
        }
        other => panic!("expected InvalidState, got: {:?}", other),
    }
}

// @scenario: ble_exchange :: Rollback all clears everything
#[test]
fn test_rollback_all_clears_everything() {
    let mut rb = BleRollback::new();
    rb.record_pending("a".to_string(), vec![1]);
    rb.record_pending("b".to_string(), vec![2]);
    rb.record_pending("c".to_string(), vec![3]);

    assert!(rb.has_pending("a"));
    assert!(rb.has_pending("b"));
    assert!(rb.has_pending("c"));

    rb.rollback_all();

    assert!(!rb.has_pending("a"));
    assert!(!rb.has_pending("b"));
    assert!(!rb.has_pending("c"));
}

// @scenario: ble_exchange :: Default rollback manager is empty
#[test]
fn test_default_is_empty() {
    let rb = BleRollback::default();
    assert!(!rb.has_pending("anything"));
}

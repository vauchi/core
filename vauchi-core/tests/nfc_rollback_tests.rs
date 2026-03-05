// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use vauchi_core::exchange::{ExchangeError, NfcRollback, NoopNfcRollback};

#[test]
fn test_noop_rollback_succeeds() {
    let rollback = NoopNfcRollback;
    rollback
        .rollback_contact("test-id")
        .expect("noop contact rollback should succeed");
    rollback
        .rollback_ratchet("test-id")
        .expect("noop ratchet rollback should succeed");
    rollback
        .rollback_all("test-id")
        .expect("noop full rollback should succeed");
}

/// Mock rollback that counts calls for verification.
struct CountingRollback {
    contact_count: AtomicU32,
    ratchet_count: AtomicU32,
}

impl CountingRollback {
    fn new() -> Self {
        Self {
            contact_count: AtomicU32::new(0),
            ratchet_count: AtomicU32::new(0),
        }
    }
}

impl NfcRollback for CountingRollback {
    fn rollback_contact(&self, _contact_id: &str) -> Result<(), ExchangeError> {
        self.contact_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn rollback_ratchet(&self, _contact_id: &str) -> Result<(), ExchangeError> {
        self.ratchet_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn test_rollback_all_calls_both() {
    let rollback = CountingRollback::new();
    rollback
        .rollback_all("test-id")
        .expect("rollback_all should succeed");
    assert_eq!(
        rollback.contact_count.load(Ordering::SeqCst),
        1,
        "contact rollback should be called once"
    );
    assert_eq!(
        rollback.ratchet_count.load(Ordering::SeqCst),
        1,
        "ratchet rollback should be called once"
    );
}

#[test]
fn test_rollback_trait_is_object_safe() {
    // Verify NfcRollback can be used as a trait object
    let rollback: Box<dyn NfcRollback> = Box::new(NoopNfcRollback);
    rollback
        .rollback_all("test-id")
        .expect("trait object rollback should succeed");
}

/// Mock that fails on ratchet rollback.
struct FailingRatchetRollback;

impl NfcRollback for FailingRatchetRollback {
    fn rollback_contact(&self, _contact_id: &str) -> Result<(), ExchangeError> {
        Ok(())
    }

    fn rollback_ratchet(&self, _contact_id: &str) -> Result<(), ExchangeError> {
        Err(ExchangeError::CryptoError)
    }
}

#[test]
fn test_rollback_all_propagates_ratchet_error() {
    let rollback = FailingRatchetRollback;
    let result = rollback.rollback_all("test-id");
    assert!(
        result.is_err(),
        "rollback_all should propagate ratchet error"
    );
}

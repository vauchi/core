// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_core::Vauchi;
#[cfg(feature = "testing")]
use vauchi_core::{StorageError, VauchiError};

// @internal — P32: reading resume state cannot create a recovery or expose it after initialization.
#[test]
fn pending_recovery_claim_requires_identity_free_persisted_recovery() {
    let mut phone = Vauchi::in_memory().unwrap();
    assert_eq!(phone.pending_recovery_claim().unwrap(), None);
    let original = phone.begin_recovery(&"01".repeat(32)).unwrap();
    let original = phone.parse_recovery_claim_b64(&original).unwrap();
    let resumed = phone.pending_recovery_claim().unwrap().unwrap();
    let resumed = phone.parse_recovery_claim_b64(&resumed).unwrap();
    assert_eq!(resumed.old_pk(), original.old_pk());
    assert_eq!(resumed.new_pk(), original.new_pk());
    assert!(!phone.has_identity());

    phone.create_identity("Alice").unwrap();
    assert_eq!(phone.pending_recovery_claim().unwrap(), None);
    assert_eq!(phone.identity().unwrap().display_name(), "Alice");
}

// @internal — P32: resume can read persisted state without rewriting it.
#[cfg(feature = "testing")]
#[test]
fn pending_recovery_claim_works_when_storage_refuses_writes() {
    let phone = Vauchi::in_memory().unwrap();
    let original = phone.begin_recovery(&"01".repeat(32)).unwrap();
    let original = phone.parse_recovery_claim_b64(&original).unwrap();
    phone
        .storage()
        .connection()
        .execute_batch("PRAGMA query_only = ON")
        .unwrap();

    let resumed = phone.pending_recovery_claim().unwrap().unwrap();
    let resumed = phone.parse_recovery_claim_b64(&resumed).unwrap();
    assert_eq!(resumed.old_pk(), original.old_pk());
    assert_eq!(resumed.new_pk(), original.new_pk());
    assert!(!phone.has_identity());
}

// @internal — DC-02: unreadable recovery state must be refused, not replaced with a new key.
#[cfg(feature = "testing")]
#[test]
fn pending_recovery_claim_propagates_corrupt_storage_refusal() {
    let phone = Vauchi::in_memory().unwrap();
    phone.begin_recovery(&"01".repeat(32)).unwrap();
    phone
        .storage()
        .connection()
        .execute("UPDATE recovery_mode_key SET seed_encrypted = X'00'", [])
        .unwrap();

    assert!(matches!(
        phone.pending_recovery_claim(),
        Err(VauchiError::Storage(StorageError::Encryption(_)))
    ));
    assert!(!phone.has_identity());
}

#[cfg(feature = "testing")]
mod claim_lifetime {
    use std::sync::Arc;
    use std::time::{Duration, UNIX_EPOCH};

    use vauchi_core::Vauchi;
    use vauchi_core::clock::FakeClock;
    use vauchi_core::recovery::RecoveryClaim;

    const BEGUN_AT: u64 = 1_800_000_000;

    fn recovering_phone() -> (Vauchi, Arc<FakeClock>, String) {
        let clock = Arc::new(FakeClock::new(UNIX_EPOCH + Duration::from_secs(BEGUN_AT)));
        let phone = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
        let claim = phone.begin_recovery(&"01".repeat(32)).unwrap();
        (phone, clock, claim)
    }

    // @internal — P32: presenting a claim is a read; only an explicit act extends what guardians will accept.
    #[test]
    fn reading_a_pending_claim_returns_the_issued_claim_unchanged() {
        let (phone, clock, issued) = recovering_phone();
        clock.advance(Duration::from_secs(3_600));
        assert_eq!(
            phone.pending_recovery_claim().unwrap().as_deref(),
            Some(issued.as_str())
        );
        clock.advance(Duration::from_secs(3_600));
        assert_eq!(
            phone.pending_recovery_claim().unwrap().as_deref(),
            Some(issued.as_str())
        );
    }

    // @internal — P32: repeated reads cannot keep a claim alive past its lifetime.
    #[test]
    fn a_pending_claim_expires_on_schedule_however_often_it_is_read() {
        let (phone, clock, _) = recovering_phone();
        let read_at = |phone: &Vauchi| {
            let claim = phone.pending_recovery_claim().unwrap().unwrap();
            phone.parse_recovery_claim_b64(&claim).unwrap()
        };

        clock.advance(Duration::from_secs(RecoveryClaim::MAX_AGE_SECS));
        let at_limit = read_at(&phone);
        assert_eq!(at_limit.timestamp(), BEGUN_AT);
        assert!(!at_limit.is_expired(BEGUN_AT + RecoveryClaim::MAX_AGE_SECS));

        clock.advance(Duration::from_secs(1));
        let past_limit = read_at(&phone);
        assert_eq!(past_limit.timestamp(), BEGUN_AT);
        assert!(past_limit.is_expired(BEGUN_AT + RecoveryClaim::MAX_AGE_SECS + 1));
    }
}

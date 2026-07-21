// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Genesis-decrypt rate limiting (ADR-068, migration v63, plan §REVISION F6).
//!
//! A genesis decrypt derives keys from `shared_key` before any signature check,
//! so the receive path bounds attempts per contact and globally, durably, using
//! a mockable clock (never a real sleep — CC-06).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_core::Storage;
use vauchi_core::clock::FakeClock;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::{
    GENESIS_CONTACT_ATTEMPTS_PER_WINDOW, GENESIS_GLOBAL_ATTEMPTS_PER_WINDOW, GENESIS_WINDOW_SECS,
};

fn fixed_clock() -> Arc<FakeClock> {
    Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ))
}

fn storage_with_clock(clock: Arc<FakeClock>) -> Storage {
    Storage::in_memory(SymmetricKey::generate())
        .unwrap()
        .with_clock(clock)
}

fn saved_contact(storage: &Storage, seed: u8, name: &str) -> String {
    let contact = Contact::from_exchange(
        [seed; 32],
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    storage.contacts().save_contact(&contact).unwrap();
    id
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// @internal
#[test]
fn per_contact_budget_caps_then_window_reset_restores() {
    let clock = fixed_clock();
    let storage = storage_with_clock(clock.clone());
    let contact = saved_contact(&storage, 1, "Alice");

    for i in 0..GENESIS_CONTACT_ATTEMPTS_PER_WINDOW {
        assert!(
            storage
                .genesis_limits()
                .consume_decrypt_budget(&contact)
                .unwrap(),
            "attempt {i} must be within the per-contact cap"
        );
    }
    assert!(
        !storage
            .genesis_limits()
            .consume_decrypt_budget(&contact)
            .unwrap(),
        "the attempt past the per-contact cap must be denied"
    );

    // A denied attempt is not charged; advancing past the window resets it.
    clock.advance(Duration::from_secs(GENESIS_WINDOW_SECS));
    assert!(
        storage
            .genesis_limits()
            .consume_decrypt_budget(&contact)
            .unwrap(),
        "a fresh window must restore the budget"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// @internal
#[test]
fn global_budget_caps_across_contacts() {
    let storage = storage_with_clock(fixed_clock());

    // One attempt each across enough distinct contacts to exhaust the global
    // cap without any single contact hitting its own cap.
    let mut allowed = 0u32;
    for n in 0..(GENESIS_GLOBAL_ATTEMPTS_PER_WINDOW + 5) {
        let contact = saved_contact(&storage, (n % 250) as u8 + 1, &format!("c{n}"));
        if storage
            .genesis_limits()
            .consume_decrypt_budget(&contact)
            .unwrap()
        {
            allowed += 1;
        }
    }
    assert_eq!(
        allowed, GENESIS_GLOBAL_ATTEMPTS_PER_WINDOW,
        "exactly the global cap of attempts may be allowed within one window"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// @internal
#[test]
fn global_cap_denial_leaves_the_contact_counter_uncharged() {
    // The two-counter charge is one SAVEPOINT: a denial by the GLOBAL cap must
    // roll back the already-applied per-contact charge, so the contact is not
    // silently burned by traffic that never counted against its own budget.
    let storage = storage_with_clock(fixed_clock());
    for n in 0..GENESIS_GLOBAL_ATTEMPTS_PER_WINDOW {
        let contact = saved_contact(&storage, (n % 250) as u8 + 1, &format!("c{n}"));
        assert!(
            storage
                .genesis_limits()
                .consume_decrypt_budget(&contact)
                .unwrap()
        );
    }

    // A brand-new contact is now denied purely by the exhausted global cap.
    let victim = saved_contact(&storage, 251, "victim");
    assert!(
        !storage
            .genesis_limits()
            .consume_decrypt_budget(&victim)
            .unwrap(),
        "global cap is exhausted, so the new contact is denied"
    );
    assert_eq!(
        storage
            .genesis_limits()
            .contact_attempts_in_window(&victim)
            .unwrap(),
        0,
        "a global-cap denial must not charge the contact's own counter"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// @internal
#[test]
fn backward_clock_correction_resets_an_exhausted_window() {
    let clock = fixed_clock();
    let storage = storage_with_clock(clock.clone());
    let contact = saved_contact(&storage, 1, "Alice");

    for _ in 0..GENESIS_CONTACT_ATTEMPTS_PER_WINDOW {
        assert!(
            storage
                .genesis_limits()
                .consume_decrypt_budget(&contact)
                .unwrap()
        );
    }
    assert!(
        !storage
            .genesis_limits()
            .consume_decrypt_budget(&contact)
            .unwrap(),
        "budget is exhausted before the clock moves"
    );

    // A backward wall-clock correction (NTP step, manual set) is a
    // discontinuity — it must reset the window, not freeze genesis until wall
    // time catches back up.
    clock.set(SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000));
    assert!(
        storage
            .genesis_limits()
            .consume_decrypt_budget(&contact)
            .unwrap(),
        "a backward clock correction must reset the exhausted window"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
// @internal
#[test]
fn budget_counters_are_durable_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("genesis.db");
    let key = SymmetricKey::generate();
    let clock = fixed_clock();

    let contact = {
        let storage = Storage::open(&path, key.clone())
            .unwrap()
            .with_clock(clock.clone());
        let contact = saved_contact(&storage, 1, "Alice");
        for _ in 0..GENESIS_CONTACT_ATTEMPTS_PER_WINDOW {
            assert!(
                storage
                    .genesis_limits()
                    .consume_decrypt_budget(&contact)
                    .unwrap()
            );
        }
        contact
    };

    // Reopen the same database: the exhausted counter must survive, so a
    // process restart cannot reset an in-progress attack.
    let storage = Storage::open(&path, key).unwrap().with_clock(clock);
    assert!(
        !storage
            .genesis_limits()
            .consume_decrypt_budget(&contact)
            .unwrap(),
        "the per-contact cap must persist across a storage reopen"
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Own-card repropagation retry queue
//! (2026-06-14-own-card-propagation-retry-queue).
//!
//! An own-card edit sets a durable marker; the sync loop runs a group-aware
//! repropagation pass (via `repropagate_to_contact`) and clears the marker only
//! once every contact's update has been queued. A permanent error backs off via
//! `failed_attempts` instead of hot-looping. These tests cover the durable
//! marker; the pass + sync wiring are covered alongside the implementation.

use vauchi_core::OwnCardRepropagateState;
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;

// @internal
#[test]
fn own_card_repropagate_marker_roundtrips_and_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let key = SymmetricKey::generate();
    let db = dir.path().join("vauchi.db");

    // Fresh DB → the marker defaults to "not owed".
    {
        let config = VauchiConfig::with_storage_path(&db).with_storage_key(key.clone());
        let vauchi = Vauchi::new(config).unwrap();
        let state = vauchi.storage().ux().load_own_card_repropagate().unwrap();
        assert_eq!(state, OwnCardRepropagateState::default());
        assert!(!state.needs_repropagate, "fresh DB owes no repropagation");
        assert_eq!(state.failed_attempts, 0);

        let dirty = OwnCardRepropagateState {
            needs_repropagate: true,
            failed_attempts: 3,
        };
        vauchi
            .storage()
            .ux()
            .save_own_card_repropagate(&dirty)
            .unwrap();
    }

    // Reopen the same DB → the persisted marker round-trips exactly.
    {
        let config = VauchiConfig::with_storage_path(&db).with_storage_key(key);
        let vauchi = Vauchi::new(config).unwrap();
        let state = vauchi.storage().ux().load_own_card_repropagate().unwrap();
        assert!(state.needs_repropagate, "dirty marker survives reopen");
        assert_eq!(state.failed_attempts, 3, "failed_attempts survives reopen");
        assert!(state.should_run(), "owed + under cap → runs");
    }
}

// @internal
#[test]
fn own_card_repropagate_backs_off_at_cap() {
    let at_cap = OwnCardRepropagateState {
        needs_repropagate: true,
        failed_attempts: OwnCardRepropagateState::MAX_FAILED_ATTEMPTS,
    };
    assert!(
        !at_cap.should_run(),
        "at the retry cap the marker backs off (no hot-loop)"
    );

    let under_cap = OwnCardRepropagateState {
        needs_repropagate: true,
        failed_attempts: OwnCardRepropagateState::MAX_FAILED_ATTEMPTS - 1,
    };
    assert!(under_cap.should_run(), "one below the cap still runs");

    assert!(
        !OwnCardRepropagateState::default().should_run(),
        "not owed → does not run"
    );
}

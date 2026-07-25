// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 activation state machine (ADR-064 Amendment 2026-07-25).
//!
//! `Active` is the only state in which the send path may fan out
//! per-device — and it is reachable exclusively through a matching ack of
//! our current pushed registry version (the bilaterality invariant: registry
//! *presence* never activates, confirmation does). Invalid transitions are
//! rejected explicitly (DC-02), never coerced.

use proptest::prelude::*;
use vauchi_core::sync::registry_activation::{ActivationState, ActivationTracker};

#[test]
fn starts_dormant_with_nothing_held() {
    let tracker = ActivationTracker::new();
    assert_eq!(tracker.state(), ActivationState::Dormant);
    assert_eq!(tracker.peer_version_held(), None);
    assert_eq!(tracker.our_version_acked(), None);
}

#[test]
fn push_then_matching_ack_activates() {
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1u8; 32], 4);
    assert_eq!(tracker.state(), ActivationState::Pushed);

    tracker.record_ack(&[1u8; 32], 4).expect("matching ack");
    assert_eq!(tracker.state(), ActivationState::Active);
    assert_eq!(tracker.our_version_acked(), Some(4));
}

#[test]
fn ack_without_outstanding_push_is_rejected() {
    let mut tracker = ActivationTracker::new();
    assert!(tracker.record_ack(&[1u8; 32], 1).is_err());
    assert_eq!(tracker.state(), ActivationState::Dormant);
}

#[test]
fn ack_with_wrong_nonce_or_stale_version_is_rejected_and_changes_nothing() {
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1u8; 32], 4);

    assert!(tracker.record_ack(&[2u8; 32], 4).is_err(), "wrong nonce");
    assert_eq!(tracker.state(), ActivationState::Pushed);

    assert!(tracker.record_ack(&[1u8; 32], 3).is_err(), "stale version");
    assert_eq!(tracker.state(), ActivationState::Pushed);
    assert_eq!(tracker.our_version_acked(), None);
}

#[test]
fn registry_change_after_activation_demotes_until_confirmed_again() {
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1u8; 32], 4);
    tracker.record_ack(&[1u8; 32], 4).expect("ack");
    assert_eq!(tracker.state(), ActivationState::Active);

    // Our registry changed (device linked/revoked) — the peer can no longer
    // be assumed to resolve the new topology, so sends must drop back to the
    // legacy path until the new version is confirmed.
    tracker.record_push_sent([2u8; 32], 5);
    assert_eq!(tracker.state(), ActivationState::Pushed);

    tracker.record_ack(&[2u8; 32], 5).expect("re-ack");
    assert_eq!(tracker.state(), ActivationState::Active);
}

#[test]
fn repushing_the_acked_version_stays_active() {
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([1u8; 32], 4);
    tracker.record_ack(&[1u8; 32], 4).expect("ack");

    // Idempotent re-push of an already-confirmed version (e.g. repair
    // trigger) must not bounce the send path through the legacy fallback.
    tracker.record_push_sent([3u8; 32], 4);
    assert_eq!(tracker.state(), ActivationState::Active);
}

#[test]
fn peer_registry_is_held_monotonically_and_emptying_deactivates() {
    let mut tracker = ActivationTracker::new();
    tracker.record_peer_registry(2);
    assert_eq!(tracker.peer_version_held(), Some(2));

    tracker.record_peer_registry(1);
    assert_eq!(
        tracker.peer_version_held(),
        Some(2),
        "stale peer registry ignored"
    );

    tracker.record_push_sent([1u8; 32], 4);
    tracker.record_ack(&[1u8; 32], 4).expect("ack");
    assert_eq!(tracker.state(), ActivationState::Active);

    tracker.record_peer_registry_emptied();
    assert_eq!(tracker.state(), ActivationState::Dormant);
    assert_eq!(tracker.peer_version_held(), None);
}

/// CC-13: invariants over arbitrary operation sequences.
#[derive(Debug, Clone)]
enum Op {
    Push { nonce: [u8; 32], version: u64 },
    Ack { nonce: [u8; 32], version: u64 },
    PeerRegistry { version: u64 },
    PeerEmptied,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    // Tiny value spaces so sequences actually collide (matching nonces/versions).
    let nonce = prop::sample::select(vec![[1u8; 32], [2u8; 32]]);
    let version = 1..4u64;
    prop_oneof![
        (nonce.clone(), version.clone()).prop_map(|(nonce, version)| Op::Push { nonce, version }),
        (nonce, version.clone()).prop_map(|(nonce, version)| Op::Ack { nonce, version }),
        version.prop_map(|version| Op::PeerRegistry { version }),
        Just(Op::PeerEmptied),
    ]
}

proptest! {
    #[test]
    fn invariants_hold_over_arbitrary_sequences(ops in prop::collection::vec(op_strategy(), 1..40)) {
        let mut tracker = ActivationTracker::new();
        let mut last_pushed: Option<([u8; 32], u64)> = None;
        let mut held: Option<u64> = None;

        for op in ops {
            let before = tracker.state();
            match op {
                Op::Push { nonce, version } => {
                    tracker.record_push_sent(nonce, version);
                    last_pushed = Some((nonce, version));
                }
                Op::Ack { nonce, version } => {
                    let accepted = tracker.record_ack(&nonce, version).is_ok();
                    let matches = last_pushed == Some((nonce, version));
                    // An ack is accepted iff it answers the outstanding push
                    // (or re-confirms while already Active on that version).
                    if accepted {
                        prop_assert!(matches, "accepted ack must match the outstanding push");
                    } else {
                        prop_assert_eq!(tracker.state(), before, "rejected ack must not change state");
                    }
                }
                Op::PeerRegistry { version } => {
                    tracker.record_peer_registry(version);
                    held = Some(held.map_or(version, |h| h.max(version)));
                    prop_assert_eq!(tracker.peer_version_held(), held, "peer version held is monotonic");
                }
                Op::PeerEmptied => {
                    tracker.record_peer_registry_emptied();
                    held = None;
                }
            }

            // The bilaterality invariant: Active requires the outstanding
            // push version to be exactly the acked version.
            if tracker.state() == ActivationState::Active {
                prop_assert_eq!(
                    tracker.our_version_acked(),
                    last_pushed.map(|(_, v)| v),
                    "Active implies the current pushed version is acked"
                );
            }
        }
    }
}

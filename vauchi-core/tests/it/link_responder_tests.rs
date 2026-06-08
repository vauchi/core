// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! State-machine tests for `LinkResponderSession`.
//!
//! The pure-Rust state machine (no threads, no sleeps) drives the
//! responder side of link-mode contact exchange after the consent
//! gate grants. Inputs are `apply_hardware_event` (relay events
//! arriving from the cycle thread) and `tick(now_unix)` (deadline
//! checks); outputs are `drain_pending_commands` (commands the cycle
//! thread should dispatch) and `current_state`.

use proptest::prelude::{Strategy, any};
use vauchi_core::exchange::link_mode::*;
use vauchi_core::exchange::link_responder::{
    LinkResponderFailureReason, LinkResponderSession, LinkResponderState,
};
use vauchi_core::{Command, Event};

/// Fixed unix-seconds base for deterministic deadline tests.
const NOW: u64 = 1_700_000_000;

/// Synthetic responder session — derives keys + commands the same way
/// the production cycle thread will, but stays inside the test.
fn make_session(deadline_unix: u64) -> LinkResponderSession {
    let (init, _) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    // `responder_respond` returns the keys + 2 RelayEscrowDeposit commands.
    let (keys, deposits) = responder_respond(&parsed, b"responder_card".to_vec()).unwrap();
    LinkResponderSession::new(keys, deposits, deadline_unix)
}

// ================================================================
// Construction + initial state
// ================================================================

// @internal
#[test]
fn fresh_session_emits_deposit_and_check_commands() {
    let now = NOW;
    let mut session = make_session(now + 300);

    // Initial state is Polling — deposits are fire-and-forget; the
    // responder waits for the gate-count `RelayEscrowReady` ack rather
    // than per-deposit confirmation.
    assert!(
        matches!(session.current_state(), LinkResponderState::Polling),
        "fresh session must start in Polling, got {:?}",
        session.current_state()
    );

    let cmds = session.drain_pending_commands();
    // Two RelayEscrowDeposit (handshake epk + encrypted card) plus
    // one RelayEscrowCheck so the relay starts watching the gate.
    let deposits = cmds
        .iter()
        .filter(|c| matches!(c, Command::RelayEscrowDeposit { .. }))
        .count();
    let checks = cmds
        .iter()
        .filter(|c| matches!(c, Command::RelayEscrowCheck { .. }))
        .count();
    assert_eq!(deposits, 2, "expected 2 deposits, got {deposits}");
    assert_eq!(checks, 1, "expected 1 check, got {checks}");

    // Drain is one-shot — second drain is empty.
    let cmds_again = session.drain_pending_commands();
    assert!(
        cmds_again.is_empty(),
        "drain_pending_commands must be one-shot, got {} extras",
        cmds_again.len()
    );
}

// ================================================================
// Polling → Retrieving
// ================================================================

// @internal
#[test]
fn relay_escrow_ready_on_our_gate_transitions_to_retrieving() {
    let now = NOW;
    let mut session = make_session(now + 300);
    let _ = session.drain_pending_commands();

    let our_gate = session.gate_hash_bytes();
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: our_gate,
    });

    assert!(
        matches!(session.current_state(), LinkResponderState::Retrieving),
        "RelayEscrowReady on our gate must move to Retrieving, got {:?}",
        session.current_state()
    );

    // Retrieving emits a RelayEscrowRetrieve command on entry.
    let cmds = session.drain_pending_commands();
    assert_eq!(cmds.len(), 1, "expected 1 command, got {}", cmds.len());
    assert!(
        matches!(cmds[0], Command::RelayEscrowRetrieve { .. }),
        "expected RelayEscrowRetrieve, got {:?}",
        cmds[0]
    );
}

// @internal
#[test]
fn relay_escrow_ready_on_unrelated_gate_is_noop() {
    let now = NOW;
    let mut session = make_session(now + 300);
    let _ = session.drain_pending_commands();

    // Different gate hash — must not affect this session's state.
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: vec![0xAA; 32],
    });

    assert!(
        matches!(session.current_state(), LinkResponderState::Polling),
        "unrelated RelayEscrowReady must stay in Polling, got {:?}",
        session.current_state()
    );
    assert!(session.drain_pending_commands().is_empty());
}

// ================================================================
// Retrieving → Finalized
// ================================================================

// @internal
#[test]
fn blob_received_with_valid_ciphertext_transitions_to_finalized() {
    let now = NOW;
    let mut session = make_session(now + 300);
    let _ = session.drain_pending_commands();

    // Drive into Retrieving first.
    let our_gate = session.gate_hash_bytes();
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: our_gate.clone(),
    });
    let _ = session.drain_pending_commands();

    // Encrypt a synthetic card payload with the session's keys so the
    // decrypt path round-trips. Production: this blob comes from the
    // initiator's deposit, which was encrypted with the same card_key
    // both sides derived.
    let plaintext = b"alice card bytes";
    let ciphertext = session.test_encrypt_card(plaintext).expect("encrypt");

    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: our_gate,
        blob: ciphertext,
    });

    match session.current_state() {
        LinkResponderState::Finalized { card_bytes } => {
            assert_eq!(
                card_bytes.as_slice(),
                plaintext,
                "Finalized must carry the decrypted card bytes"
            );
        }
        other => panic!("expected Finalized, got {other:?}"),
    }
}

// @internal
#[test]
fn blob_received_with_garbage_ciphertext_transitions_to_failed_decrypt() {
    let now = NOW;
    let mut session = make_session(now + 300);
    let _ = session.drain_pending_commands();

    let our_gate = session.gate_hash_bytes();
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: our_gate.clone(),
    });
    let _ = session.drain_pending_commands();

    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: our_gate,
        blob: vec![0xFF; 4], // too short for AEAD nonce
    });

    assert!(
        matches!(
            session.current_state(),
            LinkResponderState::Failed(LinkResponderFailureReason::DecryptError { .. })
        ),
        "garbage blob must surface DecryptError, got {:?}",
        session.current_state()
    );
}

// ================================================================
// ================================================================

// @internal
#[test]
fn relay_escrow_failed_on_our_gate_transitions_to_failed_deposit_rejected() {
    let now = NOW;
    let mut session = make_session(now + 300);
    let _ = session.drain_pending_commands();

    let our_gate = session.gate_hash_bytes();
    session.apply_hardware_event(Event::RelayEscrowFailed {
        gate_hash: our_gate,
        reason: "slot already occupied".into(),
    });

    assert!(
        matches!(
            session.current_state(),
            LinkResponderState::Failed(LinkResponderFailureReason::DepositRejected)
        ),
        "RelayEscrowFailed on our gate must surface DepositRejected, got {:?}",
        session.current_state()
    );
}

// @internal
#[test]
fn tick_past_deadline_transitions_to_failed_polling_timed_out() {
    let now = NOW;
    let deadline = now + 1;
    let mut session = make_session(deadline);
    let _ = session.drain_pending_commands();

    // Tick before deadline → still Polling.
    session.tick(now);
    assert!(matches!(
        session.current_state(),
        LinkResponderState::Polling
    ));

    // Tick past deadline → PollingTimedOut.
    session.tick(deadline + 1);
    assert!(
        matches!(
            session.current_state(),
            LinkResponderState::Failed(LinkResponderFailureReason::PollingTimedOut)
        ),
        "tick past deadline must surface PollingTimedOut, got {:?}",
        session.current_state()
    );
}

// @internal
#[test]
fn cancel_from_polling_transitions_to_failed_cancelled() {
    let now = NOW;
    let mut session = make_session(now + 300);
    let _ = session.drain_pending_commands();

    session.cancel();

    assert!(
        matches!(
            session.current_state(),
            LinkResponderState::Failed(LinkResponderFailureReason::Cancelled)
        ),
        "cancel must surface Cancelled, got {:?}",
        session.current_state()
    );
}

// ================================================================
// Terminal-state idempotency
// ================================================================

// @internal
#[test]
fn finalized_is_terminal() {
    let now = NOW;
    let mut session = make_session(now + 300);
    let _ = session.drain_pending_commands();

    let our_gate = session.gate_hash_bytes();
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: our_gate.clone(),
    });
    let _ = session.drain_pending_commands();
    let plaintext = b"x";
    let ciphertext = session.test_encrypt_card(plaintext).unwrap();
    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: our_gate.clone(),
        blob: ciphertext,
    });
    assert!(matches!(
        session.current_state(),
        LinkResponderState::Finalized { .. }
    ));

    // Subsequent events are inert.
    session.tick(now + 3600);
    session.cancel();
    session.apply_hardware_event(Event::RelayEscrowFailed {
        gate_hash: our_gate,
        reason: "should be ignored".into(),
    });

    assert!(matches!(
        session.current_state(),
        LinkResponderState::Finalized { .. }
    ));
    assert!(session.drain_pending_commands().is_empty());
}

// @internal
#[test]
fn failed_is_terminal() {
    let now = NOW;
    let mut session = make_session(now + 300);
    let _ = session.drain_pending_commands();

    session.cancel();
    assert!(matches!(
        session.current_state(),
        LinkResponderState::Failed(LinkResponderFailureReason::Cancelled)
    ));

    // Subsequent events do not flip the variant.
    let our_gate = session.gate_hash_bytes();
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: our_gate,
    });
    session.tick(now + 3600);

    assert!(matches!(
        session.current_state(),
        LinkResponderState::Failed(LinkResponderFailureReason::Cancelled)
    ));
}

// ================================================================
// Stateful property test (CC-13)
// ================================================================

// @internal
proptest::proptest! {
    /// Random sequences of events against the state machine never
    /// violate terminal-state stability or produce ill-typed commands.
    /// Property test (CC-13) covering event-ordering fuzz.
    #[test]
    fn random_event_sequences_preserve_invariants(
        events in proptest::collection::vec(
            proptest::prop_oneof![
                proptest::strategy::Just(EventKind::ReadyOurs),
                proptest::strategy::Just(EventKind::ReadyOther),
                proptest::strategy::Just(EventKind::FailedOurs),
                proptest::strategy::Just(EventKind::FailedOther),
                proptest::strategy::Just(EventKind::TickPast),
                proptest::strategy::Just(EventKind::Cancel),
                any::<u8>().prop_map(EventKind::BlobOurs),
            ],
            0..32,
        )
    ) {
        let now = NOW;
        let mut session = make_session(now + 300);
        let _ = session.drain_pending_commands();

        let our_gate = session.gate_hash_bytes();
        let mut transitioned_terminal: Option<bool> = None;

        for ev in events {
            let was_terminal = matches!(
                session.current_state(),
                LinkResponderState::Finalized { .. } | LinkResponderState::Failed(_)
            );

            match ev {
                EventKind::ReadyOurs => {
                    session.apply_hardware_event(Event::RelayEscrowReady {
                        gate_hash: our_gate.clone(),
                    });
                }
                EventKind::ReadyOther => {
                    session.apply_hardware_event(Event::RelayEscrowReady {
                        gate_hash: vec![0u8; 32],
                    });
                }
                EventKind::FailedOurs => {
                    session.apply_hardware_event(Event::RelayEscrowFailed {
                        gate_hash: our_gate.clone(),
                        reason: String::new(),
                    });
                }
                EventKind::FailedOther => {
                    session.apply_hardware_event(Event::RelayEscrowFailed {
                        gate_hash: vec![0u8; 32],
                        reason: String::new(),
                    });
                }
                EventKind::TickPast => {
                    session.tick(now + 7200);
                }
                EventKind::Cancel => {
                    session.cancel();
                }
                EventKind::BlobOurs(byte) => {
                    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
                        gate_hash: our_gate.clone(),
                        blob: vec![byte; 4],
                    });
                }
            }

            // Terminal states are sticky.
            if was_terminal {
                let still_terminal = matches!(
                    session.current_state(),
                    LinkResponderState::Finalized { .. } | LinkResponderState::Failed(_)
                );
                proptest::prop_assert!(
                    still_terminal,
                    "terminal state must stay terminal, got {:?}",
                    session.current_state()
                );
            }
            transitioned_terminal = Some(matches!(
                session.current_state(),
                LinkResponderState::Finalized { .. } | LinkResponderState::Failed(_)
            ));
        }

        let _ = transitioned_terminal;
    }
}

#[derive(Debug, Clone, Copy)]
enum EventKind {
    ReadyOurs,
    ReadyOther,
    FailedOurs,
    FailedOther,
    TickPast,
    Cancel,
    BlobOurs(u8),
}

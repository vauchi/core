// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! State-machine tests for `LinkResponderSession`.
//!
//! The pure-Rust state machine (no threads, no sleeps) drives the
//! responder side of link-mode contact exchange after the consent
//! gate grants and the code is typed. Inputs are `apply_hardware_event` (relay events
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

fn deposit_blob(command: &Command) -> Vec<u8> {
    match command {
        Command::RelayEscrowDeposit { encrypted_card, .. } => encrypted_card.clone(),
        other => panic!("expected a deposit, got {other:?}"),
    }
}

/// A responder session past the handshake: its keys are derived and its card
/// deposited, so it polls the escrow gate. The state most tests start from.
fn make_session(deadline_unix: u64) -> LinkResponderSession {
    let (init, presence) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    let opening = responder_open(&parsed, &init.code).unwrap();
    let mut session = LinkResponderSession::new(opening, deadline_unix);
    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: handshake_gate(&parsed.nonce),
        blob: deposit_blob(&presence[0]),
    });
    session.deposit_card(b"responder_card");
    session
}

/// A responder session that has only just opened the link, with the sender's
/// deposit and the link's handshake gate.
fn make_session_for_link(deadline_unix: u64) -> (LinkResponderSession, Vec<u8>, Vec<u8>) {
    let (init, presence) = initiator_generate();
    let parsed = parse_link_url(&init.url).unwrap();
    let opening = responder_open(&parsed, &init.code).unwrap();
    (
        LinkResponderSession::new(opening, deadline_unix),
        deposit_blob(&presence[0]),
        handshake_gate(&parsed.nonce),
    )
}

/// The relay authenticates a retrieval by the requester's *own* slot and
/// then returns the *other* slot's blob (`relay/src/escrow.rs::get`).
/// Passing the peer's slot therefore hands back our own deposit, which
/// decrypts perfectly — both parties share one `card_key` — and only
/// fails later, as "cannot complete a link exchange with our own
/// identity" surfaced to the user as a decryption error
/// (2026-08-14-link-exchange-responder-cannot-decrypt-the-card).
// @scenario: link_exchange :: the responder retrieves the initiator's deposit
#[test]
fn responder_retrieves_using_its_own_slot_so_the_relay_returns_the_peers_blob() {
    let (mut init, presence) = initiator_generate();
    let parsed = parse_link_url(&init.url).expect("a freshly generated url must parse");
    let opening = responder_open(&parsed, &init.code).unwrap();
    let responder_deposit = deposit_blob(&opening.deposit_command());
    // The sender's keys name the two slots from the other side.
    let (_, sender_keys) = initiator_derive_keys(
        &init.secret_key_bytes,
        init.pake.take().unwrap(),
        &responder_deposit,
    )
    .unwrap();
    let gate_hash = hex_bytes(&sender_keys.gate_hash);
    let our_slot = hex_bytes(&sender_keys.their_slot);
    let their_slot = hex_bytes(&sender_keys.our_slot);

    let mut session = LinkResponderSession::new(opening, NOW + 300);
    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: handshake_gate(&parsed.nonce),
        blob: deposit_blob(&presence[0]),
    });
    session.deposit_card(b"responder_card");
    let _ = session.drain_pending_commands();

    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: gate_hash.clone(),
    });

    let retrieve_slot = session
        .drain_pending_commands()
        .into_iter()
        .find_map(|c| match c {
            Command::RelayEscrowRetrieve { slot_hash, .. } => Some(slot_hash),
            _ => None,
        })
        .expect("a ready gate must queue a RelayEscrowRetrieve");

    assert_ne!(
        retrieve_slot, their_slot,
        "retrieving with the peer's slot makes the relay return our own \
         deposit, so the responder decrypts its own card and rejects it as \
         a self-exchange"
    );
    assert_eq!(
        retrieve_slot, our_slot,
        "the relay verifies the requester owns the slot it names and then \
         returns the other slot's blob, so the responder must name its own"
    );
}

/// Decode a hex slot/gate identifier the way the session does internally.
fn hex_bytes(hex_string: &str) -> Vec<u8> {
    (0..hex_string.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_string[i..i + 2], 16).expect("valid hex"))
        .collect()
}

// ================================================================
// Construction + initial state
// ================================================================

// @internal
#[test]
fn a_fresh_session_deposits_its_handshake_blob_and_no_card() {
    let (mut session, _presence, handshake) = make_session_for_link(NOW + 300);

    assert_eq!(
        session.current_state(),
        &LinkResponderState::AwaitingPresence
    );

    let cmds = session.drain_pending_commands();
    match &cmds[..] {
        [
            Command::RelayEscrowDeposit {
                gate_hash,
                encrypted_card,
                ..
            },
            Command::RelayEscrowCheck {
                gate_hash: checked, ..
            },
        ] => {
            assert_eq!(*gate_hash, handshake);
            assert_eq!(*checked, handshake);
            assert_eq!(encrypted_card.len(), 32 + 33);
        }
        other => panic!("expected the handshake deposit and a check, got {other:?}"),
    }
    assert!(
        session.drain_pending_commands().is_empty(),
        "drain_pending_commands must be one-shot"
    );
}

// ================================================================
// The handshake: AwaitingPresence → AwaitingCard → Polling
// ================================================================

// The relay answers with the other slot's blob, so the session names its own.
// @internal
#[test]
fn a_ready_handshake_gate_retrieves_the_senders_deposit_with_our_slot() {
    let (mut session, _presence, handshake) = make_session_for_link(NOW + 300);
    let deposited_to = match &session.drain_pending_commands()[0] {
        Command::RelayEscrowDeposit { slot_hash, .. } => slot_hash.clone(),
        other => panic!("expected the deposit, got {other:?}"),
    };

    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: handshake.clone(),
    });

    let cmds = session.drain_pending_commands();
    assert!(
        matches!(
            &cmds[..],
            [Command::RelayEscrowRetrieve { gate_hash, slot_hash }]
                if *gate_hash == handshake && *slot_hash == deposited_to
        ),
        "{cmds:?}"
    );
}

// @internal
#[test]
fn the_senders_deposit_derives_keys_and_waits_for_the_signed_card() {
    let (mut session, presence, handshake) = make_session_for_link(NOW + 300);
    let _ = session.drain_pending_commands();

    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: handshake,
        blob: presence,
    });

    assert_eq!(session.current_state(), &LinkResponderState::AwaitingCard);
    assert!(session.escrow_gate_bytes().is_some());
    assert!(
        session.drain_pending_commands().is_empty(),
        "nothing is deposited before the card is signed"
    );
}

// @internal
#[test]
fn the_signed_card_is_deposited_and_the_escrow_gate_polled() {
    let (mut session, presence, handshake) = make_session_for_link(NOW + 300);
    let _ = session.drain_pending_commands();
    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: handshake,
        blob: presence,
    });

    session.deposit_card(b"responder_card");

    assert_eq!(session.current_state(), &LinkResponderState::Polling);
    let escrow_gate = session.escrow_gate_bytes().unwrap();
    let cmds = session.drain_pending_commands();
    assert!(
        matches!(
            &cmds[..],
            [
                Command::RelayEscrowDeposit { gate_hash, .. },
                Command::RelayEscrowCheck { gate_hash: checked, .. },
            ] if *gate_hash == escrow_gate && *checked == escrow_gate
        ),
        "{cmds:?}"
    );
}

// @internal
#[test]
fn a_card_before_the_senders_deposit_is_ignored() {
    let (mut session, _presence, _handshake) = make_session_for_link(NOW + 300);
    let _ = session.drain_pending_commands();

    session.deposit_card(b"responder_card");

    assert_eq!(
        session.current_state(),
        &LinkResponderState::AwaitingPresence
    );
    assert!(session.drain_pending_commands().is_empty());
}

// @internal
#[test]
fn a_sender_deposit_naming_another_key_fails_the_handshake() {
    let (mut session, _presence, handshake) = make_session_for_link(NOW + 300);
    let (_, foreign_presence, _) = make_session_for_link(NOW + 300);
    let _ = session.drain_pending_commands();

    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: handshake,
        blob: foreign_presence,
    });

    assert!(
        matches!(
            session.current_state(),
            LinkResponderState::Failed(LinkResponderFailureReason::HandshakeFailed { .. })
        ),
        "got {:?}",
        session.current_state()
    );
    assert!(session.drain_pending_commands().is_empty());
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

    let our_gate = session.escrow_gate_bytes().unwrap();
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
    let our_gate = session.escrow_gate_bytes().unwrap();
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: our_gate.clone(),
    });
    let _ = session.drain_pending_commands();

    // Encrypt a synthetic card payload with the session's keys so the
    // decrypt path round-trips. Production: this blob comes from the
    // initiator's deposit, which was encrypted with the same card_key
    // both sides derived.
    let plaintext = b"alice card bytes";
    let ciphertext = session
        .test_encrypt_card(plaintext)
        .unwrap()
        .expect("encrypt");

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

    let our_gate = session.escrow_gate_bytes().unwrap();
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

    let our_gate = session.escrow_gate_bytes().unwrap();
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

// Two people opened the link: the second one's epk deposit finds the slot
// taken. It is told the link was used, not left to time out (#942).
// @internal
#[test]
fn a_taken_epk_slot_fails_as_link_already_used() {
    let (mut session, _presence, handshake) = make_session_for_link(NOW + 300);
    let _ = session.drain_pending_commands();

    session.apply_hardware_event(Event::RelayEscrowFailed {
        gate_hash: handshake,
        reason: "slot_taken".into(),
    });

    assert_eq!(
        session.current_state(),
        &LinkResponderState::Failed(LinkResponderFailureReason::LinkAlreadyUsed)
    );
    assert!(session.drain_pending_commands().is_empty());
}

// @internal
#[test]
fn any_other_epk_deposit_failure_is_a_rejected_deposit() {
    let (mut session, _presence, handshake) = make_session_for_link(NOW + 300);
    let _ = session.drain_pending_commands();

    session.apply_hardware_event(Event::RelayEscrowFailed {
        gate_hash: handshake,
        reason: "blob_too_large".into(),
    });

    assert_eq!(
        session.current_state(),
        &LinkResponderState::Failed(LinkResponderFailureReason::DepositRejected)
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

// An accepted link whose gate already held both cards from an earlier
// exchange moved the s7 to Retrieving, and it waited there past 7 minutes
// with nothing left to answer (rig, 2026-10-16).
// @internal
#[test]
fn tick_past_deadline_fails_a_responder_stuck_retrieving() {
    let now = NOW;
    let deadline = now + 1;
    let mut session = make_session(deadline);
    let _ = session.drain_pending_commands();
    let our_gate = session.escrow_gate_bytes().unwrap();
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: our_gate,
    });
    assert!(matches!(
        session.current_state(),
        LinkResponderState::Retrieving
    ));

    session.tick(deadline + 1);

    assert!(
        matches!(
            session.current_state(),
            LinkResponderState::Failed(LinkResponderFailureReason::PollingTimedOut)
        ),
        "a retrieve that never answers must end at the deadline, got {:?}",
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

    let our_gate = session.escrow_gate_bytes().unwrap();
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: our_gate.clone(),
    });
    let _ = session.drain_pending_commands();
    let plaintext = b"x";
    let ciphertext = session.test_encrypt_card(plaintext).unwrap().unwrap();
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
    let our_gate = session.escrow_gate_bytes().unwrap();
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

        let our_gate = session.escrow_gate_bytes().unwrap();
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

proptest::proptest! {
    // CC-14: the relay's answer on the handshake gate is untrusted. Whatever
    // it holds, the session never panics and never moves on with a deposit
    // that is not the sender's.
    // @internal
    #[test]
    fn a_garbage_sender_deposit_never_derives_keys(
        blob in proptest::collection::vec(any::<u8>(), 0..128),
    ) {
        let (mut session, presence, handshake) = make_session_for_link(NOW + 300);
        proptest::prop_assume!(blob != presence);
        let _ = session.drain_pending_commands();

        session.apply_hardware_event(Event::RelayEscrowBlobReceived {
            gate_hash: handshake,
            blob,
        });

        proptest::prop_assert!(session.escrow_gate_bytes().is_none());
        let is_handshake_failure = matches!(
            session.current_state(),
            LinkResponderState::Failed(LinkResponderFailureReason::HandshakeFailed { .. })
        );
        proptest::prop_assert!(is_handshake_failure, "got {:?}", session.current_state());
    }
}

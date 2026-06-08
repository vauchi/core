// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! State-machine tests for `LinkInitiatorSession`.
//!
//! Mirror of `link_responder_tests.rs` for the initiator half. The
//! pure-Rust state machine (no threads, no sleeps) drives the initiator
//! side of link-mode contact exchange: it shares a URL, polls the
//! handshake gate for the responder's ephemeral public key, performs
//! ECDH, deposits its own encrypted card, then polls the escrow gate to
//! retrieve + decrypt the responder's card.
//!
//! Inputs are `apply_hardware_event` (relay events) and `tick(now_unix)`
//! (deadline checks); outputs are `drain_pending_commands` (commands the
//! engine dispatches) and `current_state`. Real crypto throughout
//! (ADR-002 — no mocks).

use proptest::prelude::{Strategy, any};
use vauchi_core::exchange::link_initiator::{
    LinkInitiatorFailureReason, LinkInitiatorSession, LinkInitiatorState,
};
use vauchi_core::exchange::link_mode::*;
use vauchi_core::{Command, Event};

/// Fixed unix-seconds base for deterministic deadline tests.
const NOW: u64 = 1_700_000_000;

/// Build a fresh initiator session, returning it plus the URL the
/// responder needs to respond. The presence-deposit command is the
/// session's initial drain.
fn make_session(deadline_unix: u64) -> (LinkInitiatorSession, String) {
    let (init, presence_commands) = initiator_generate();
    let url = init.url.clone();
    let own_card = b"initiator card payload bytes".to_vec();
    let session = LinkInitiatorSession::new(init, presence_commands, own_card, deadline_unix);
    (session, url)
}

/// Synthesize the responder side for `url`: returns the responder's
/// ephemeral public key (peer_public_key for `LinkOpened`) and its
/// encrypted card blob (for `RelayEscrowBlobReceived`).
fn responder_side(url: &str, responder_card: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let parsed = parse_link_url(url).unwrap();
    let (_keys, commands) = responder_respond_with_card_bytes(&parsed, responder_card).unwrap();
    // commands[0] = epk deposit (encrypted_card holds the raw public key),
    // commands[1] = encrypted card deposit.
    let epk = match &commands[0] {
        Command::RelayEscrowDeposit { encrypted_card, .. } => encrypted_card.clone(),
        other => panic!("expected epk deposit, got {other:?}"),
    };
    let card_blob = match &commands[1] {
        Command::RelayEscrowDeposit { encrypted_card, .. } => encrypted_card.clone(),
        other => panic!("expected card deposit, got {other:?}"),
    };
    (epk, card_blob)
}

// ================================================================
// Construction + initial state
// ================================================================

// @internal
#[test]
fn fresh_session_starts_polling_and_drains_presence_deposit() {
    let (mut session, _url) = make_session(NOW + 300);

    assert!(
        matches!(session.current_state(), LinkInitiatorState::Polling),
        "fresh session must start in Polling, got {:?}",
        session.current_state()
    );

    let cmds = session.drain_pending_commands();
    let deposits = cmds
        .iter()
        .filter(|c| matches!(c, Command::RelayEscrowDeposit { .. }))
        .count();
    assert_eq!(deposits, 1, "expected 1 presence deposit, got {deposits}");

    // Drain is one-shot.
    assert!(
        session.drain_pending_commands().is_empty(),
        "drain must be one-shot"
    );
}

// @internal
#[test]
fn link_shared_emits_handshake_check() {
    let (mut session, _url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();

    session.apply_hardware_event(Event::LinkShared);

    let cmds = session.drain_pending_commands();
    assert_eq!(
        cmds.len(),
        1,
        "expected 1 check command, got {}",
        cmds.len()
    );
    match &cmds[0] {
        Command::RelayEscrowCheck { gate_hash, .. } => {
            assert_eq!(
                gate_hash,
                &session.handshake_gate_bytes(),
                "check must watch the handshake gate"
            );
        }
        other => panic!("expected RelayEscrowCheck, got {other:?}"),
    }
    assert!(matches!(
        session.current_state(),
        LinkInitiatorState::Polling
    ));
}

// ================================================================
// Handshake phase: Polling → retrieve epk
// ================================================================

// @internal
#[test]
fn relay_escrow_ready_on_handshake_gate_emits_retrieve() {
    let (mut session, _url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();
    session.apply_hardware_event(Event::LinkShared);
    let _ = session.drain_pending_commands();

    let gate = session.handshake_gate_bytes();
    session.apply_hardware_event(Event::RelayEscrowReady { gate_hash: gate });

    let cmds = session.drain_pending_commands();
    assert_eq!(cmds.len(), 1, "expected 1 retrieve, got {}", cmds.len());
    assert!(
        matches!(cmds[0], Command::RelayEscrowRetrieve { .. }),
        "expected RelayEscrowRetrieve, got {:?}",
        cmds[0]
    );
    // Still polling — escrow keys not yet derived.
    assert!(matches!(
        session.current_state(),
        LinkInitiatorState::Polling
    ));
}

// @internal
#[test]
fn relay_escrow_ready_on_unrelated_gate_is_noop() {
    let (mut session, _url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();
    session.apply_hardware_event(Event::LinkShared);
    let _ = session.drain_pending_commands();

    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: vec![0xAA; 32],
    });

    assert!(matches!(
        session.current_state(),
        LinkInitiatorState::Polling
    ));
    assert!(session.drain_pending_commands().is_empty());
}

// ================================================================
// Escrow phase: LinkOpened → Retrieving
// ================================================================

// @internal
#[test]
fn link_opened_derives_keys_deposits_card_and_polls_escrow() {
    let (mut session, url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();
    let (epk, _card) = responder_side(&url, b"responder card");

    session.apply_hardware_event(Event::LinkOpened {
        peer_public_key: epk,
    });

    assert!(
        matches!(session.current_state(), LinkInitiatorState::Retrieving),
        "LinkOpened must move to Retrieving, got {:?}",
        session.current_state()
    );

    let cmds = session.drain_pending_commands();
    let deposits = cmds
        .iter()
        .filter(|c| matches!(c, Command::RelayEscrowDeposit { .. }))
        .count();
    let checks = cmds
        .iter()
        .filter(|c| matches!(c, Command::RelayEscrowCheck { .. }))
        .count();
    assert_eq!(deposits, 1, "expected 1 card deposit, got {deposits}");
    assert_eq!(checks, 1, "expected 1 escrow check, got {checks}");
    assert!(
        session.escrow_gate_bytes().is_some(),
        "escrow keys must be derived after LinkOpened"
    );
}

// @internal
#[test]
fn link_opened_with_malformed_epk_fails_handshake() {
    let (mut session, _url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();

    session.apply_hardware_event(Event::LinkOpened {
        peer_public_key: vec![0x01, 0x02, 0x03], // not 32 bytes
    });

    assert!(
        matches!(
            session.current_state(),
            LinkInitiatorState::Failed(LinkInitiatorFailureReason::HandshakeFailed { .. })
        ),
        "malformed epk must surface HandshakeFailed, got {:?}",
        session.current_state()
    );
}

// ================================================================
// Escrow phase: Retrieving → Finalized (full round-trip, real crypto)
// ================================================================

// @internal
#[test]
fn full_round_trip_finalizes_with_responder_card() {
    let (mut session, url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();
    let responder_card = b"alice contact card bytes";
    let (epk, card_blob) = responder_side(&url, responder_card);

    // Share → handshake ready → retrieve epk.
    session.apply_hardware_event(Event::LinkShared);
    let _ = session.drain_pending_commands();
    let handshake_gate = session.handshake_gate_bytes();
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: handshake_gate,
    });
    let _ = session.drain_pending_commands();

    // LinkOpened → derive keys + deposit our card + poll escrow.
    session.apply_hardware_event(Event::LinkOpened {
        peer_public_key: epk,
    });
    let _ = session.drain_pending_commands();
    let escrow_gate = session.escrow_gate_bytes().expect("escrow gate");

    // Escrow ready → retrieve.
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: escrow_gate.clone(),
    });
    let cmds = session.drain_pending_commands();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::RelayEscrowRetrieve { .. })),
        "escrow ready must emit a retrieve"
    );

    // Blob received → decrypt → Finalized with the responder's card.
    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: escrow_gate,
        blob: card_blob,
    });

    match session.current_state() {
        LinkInitiatorState::Finalized { card_bytes } => {
            assert_eq!(
                card_bytes.as_slice(),
                responder_card,
                "Finalized must carry the responder's decrypted card bytes"
            );
        }
        other => panic!("expected Finalized, got {other:?}"),
    }
}

// @internal
#[test]
fn blob_with_garbage_ciphertext_fails_decrypt() {
    let (mut session, url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();
    let (epk, _card) = responder_side(&url, b"card");

    session.apply_hardware_event(Event::LinkOpened {
        peer_public_key: epk,
    });
    let _ = session.drain_pending_commands();
    let escrow_gate = session.escrow_gate_bytes().unwrap();

    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: escrow_gate,
        blob: vec![0xFF; 4], // too short for AEAD nonce
    });

    assert!(
        matches!(
            session.current_state(),
            LinkInitiatorState::Failed(LinkInitiatorFailureReason::DecryptError { .. })
        ),
        "garbage blob must surface DecryptError, got {:?}",
        session.current_state()
    );
}

// ================================================================
// ================================================================

// @internal
#[test]
fn relay_escrow_failed_transitions_to_deposit_rejected() {
    let (mut session, _url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();

    session.apply_hardware_event(Event::RelayEscrowFailed {
        gate_hash: session.handshake_gate_bytes(),
        reason: "slot already occupied".into(),
    });

    assert!(
        matches!(
            session.current_state(),
            LinkInitiatorState::Failed(LinkInitiatorFailureReason::DepositRejected)
        ),
        "RelayEscrowFailed must surface DepositRejected, got {:?}",
        session.current_state()
    );
}

// @internal
#[test]
fn tick_past_deadline_transitions_to_polling_timed_out() {
    let deadline = NOW + 1;
    let (mut session, _url) = make_session(deadline);
    let _ = session.drain_pending_commands();

    session.tick(NOW);
    assert!(matches!(
        session.current_state(),
        LinkInitiatorState::Polling
    ));

    session.tick(deadline + 1);
    assert!(
        matches!(
            session.current_state(),
            LinkInitiatorState::Failed(LinkInitiatorFailureReason::PollingTimedOut)
        ),
        "tick past deadline must surface PollingTimedOut, got {:?}",
        session.current_state()
    );
}

// @internal
#[test]
fn cancel_from_polling_transitions_to_cancelled() {
    let (mut session, _url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();

    session.cancel();

    assert!(
        matches!(
            session.current_state(),
            LinkInitiatorState::Failed(LinkInitiatorFailureReason::Cancelled)
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
    let (mut session, url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();
    let responder_card = b"y";
    let (epk, card_blob) = responder_side(&url, responder_card);

    session.apply_hardware_event(Event::LinkOpened {
        peer_public_key: epk,
    });
    let _ = session.drain_pending_commands();
    let escrow_gate = session.escrow_gate_bytes().unwrap();
    session.apply_hardware_event(Event::RelayEscrowBlobReceived {
        gate_hash: escrow_gate.clone(),
        blob: card_blob,
    });
    assert!(matches!(
        session.current_state(),
        LinkInitiatorState::Finalized { .. }
    ));

    // Subsequent events are inert.
    session.tick(NOW + 3600);
    session.cancel();
    session.apply_hardware_event(Event::RelayEscrowFailed {
        gate_hash: escrow_gate,
        reason: "ignored".into(),
    });

    assert!(matches!(
        session.current_state(),
        LinkInitiatorState::Finalized { .. }
    ));
    assert!(session.drain_pending_commands().is_empty());
}

// @internal
#[test]
fn failed_is_terminal() {
    let (mut session, _url) = make_session(NOW + 300);
    let _ = session.drain_pending_commands();

    session.cancel();
    assert!(matches!(
        session.current_state(),
        LinkInitiatorState::Failed(LinkInitiatorFailureReason::Cancelled)
    ));

    // Subsequent events do not flip the variant.
    session.apply_hardware_event(Event::LinkShared);
    session.apply_hardware_event(Event::RelayEscrowReady {
        gate_hash: session.handshake_gate_bytes(),
    });
    session.tick(NOW + 3600);

    assert!(matches!(
        session.current_state(),
        LinkInitiatorState::Failed(LinkInitiatorFailureReason::Cancelled)
    ));
}

// ================================================================
// Property test (CC-04): card round-trip survives real ECDH + AEAD
// ================================================================

// @internal
proptest::proptest! {
    /// For any non-empty responder card payload, a full initiator round
    /// trip (real X25519 ECDH on both sides + XChaCha20-Poly1305 card
    /// encrypt/decrypt) finalizes with byte-exact card recovery.
    #[test]
    fn card_round_trip_recovers_exact_bytes(
        responder_card in proptest::collection::vec(any::<u8>(), 1..512),
    ) {
        let (mut session, url) = make_session(NOW + 300);
        let _ = session.drain_pending_commands();
        let (epk, card_blob) = responder_side(&url, &responder_card);

        session.apply_hardware_event(Event::LinkOpened {
            peer_public_key: epk,
        });
        let _ = session.drain_pending_commands();
        let escrow_gate = session.escrow_gate_bytes().unwrap();
        session.apply_hardware_event(Event::RelayEscrowBlobReceived {
            gate_hash: escrow_gate,
            blob: card_blob,
        });

        match session.current_state() {
            LinkInitiatorState::Finalized { card_bytes } => {
                proptest::prop_assert_eq!(card_bytes.as_slice(), responder_card.as_slice());
            }
            other => proptest::prop_assert!(false, "expected Finalized, got {:?}", other),
        }
    }
}

// ================================================================
// Stateful property test (CC-13): random sequences preserve invariants
// ================================================================

// @internal
proptest::proptest! {
    /// Random sequences of events never violate terminal-state stability.
    #[test]
    fn random_event_sequences_preserve_invariants(
        events in proptest::collection::vec(
            proptest::prop_oneof![
                proptest::strategy::Just(EventKind::Shared),
                proptest::strategy::Just(EventKind::ReadyHandshake),
                proptest::strategy::Just(EventKind::ReadyOther),
                proptest::strategy::Just(EventKind::Failed),
                proptest::strategy::Just(EventKind::TickPast),
                proptest::strategy::Just(EventKind::Cancel),
                any::<u8>().prop_map(EventKind::Blob),
            ],
            0..32,
        )
    ) {
        let (mut session, _url) = make_session(NOW + 300);
        let _ = session.drain_pending_commands();
        let handshake_gate = session.handshake_gate_bytes();

        for ev in events {
            let was_terminal = matches!(
                session.current_state(),
                LinkInitiatorState::Finalized { .. } | LinkInitiatorState::Failed(_)
            );

            match ev {
                EventKind::Shared => session.apply_hardware_event(Event::LinkShared),
                EventKind::ReadyHandshake => session.apply_hardware_event(
                    Event::RelayEscrowReady { gate_hash: handshake_gate.clone() },
                ),
                EventKind::ReadyOther => session.apply_hardware_event(
                    Event::RelayEscrowReady { gate_hash: vec![0u8; 32] },
                ),
                EventKind::Failed => session.apply_hardware_event(Event::RelayEscrowFailed {
                    gate_hash: handshake_gate.clone(),
                    reason: String::new(),
                }),
                EventKind::TickPast => session.tick(NOW + 7200),
                EventKind::Cancel => session.cancel(),
                EventKind::Blob(byte) => session.apply_hardware_event(
                    Event::RelayEscrowBlobReceived {
                        gate_hash: handshake_gate.clone(),
                        blob: vec![byte; 4],
                    },
                ),
            }

            if was_terminal {
                let still_terminal = matches!(
                    session.current_state(),
                    LinkInitiatorState::Finalized { .. } | LinkInitiatorState::Failed(_)
                );
                proptest::prop_assert!(
                    still_terminal,
                    "terminal state must stay terminal, got {:?}",
                    session.current_state()
                );
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum EventKind {
    Shared,
    ReadyHandshake,
    ReadyOther,
    Failed,
    TickPast,
    Cancel,
    Blob(u8),
}

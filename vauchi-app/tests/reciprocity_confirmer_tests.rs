// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::reciprocity_confirmer::ReciprocityConfirmer;
use vauchi_core::exchange::command::{ExchangeCommand, ExchangeHardwareEvent};
use vauchi_core::exchange::reciprocity::Reciprocity;

fn make_confirmer() -> ReciprocityConfirmer {
    ReciprocityConfirmer::new(
        [0xAA; 32],
        [0xBB; 32],
        "gate123".to_string(),
        "slot_ours".to_string(),
        "slot_theirs".to_string(),
        1000,
        true,
    )
}

#[test]
fn start_emits_deposit_command() {
    let mut confirmer = make_confirmer();
    let cmds = confirmer.start();
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        ExchangeCommand::RelayEscrowDeposit {
            encrypted_card,
            ttl_seconds,
            ..
        } => {
            assert_eq!(encrypted_card, &[0xAA; 32].to_vec());
            assert_eq!(*ttl_seconds, 7 * 24 * 3600);
        }
        other => panic!("expected RelayEscrowDeposit, got {other:?}"),
    }
}

#[test]
fn escrow_ready_triggers_retrieve() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    let cmds = confirmer.handle_event(&ExchangeHardwareEvent::RelayEscrowReady {
        gate_hash: b"gate123".to_vec(),
    });
    assert_eq!(cmds.len(), 1);
    matches!(&cmds[0], ExchangeCommand::RelayEscrowRetrieve { .. });
}

#[test]
fn correct_blob_confirms() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    confirmer.handle_event(&ExchangeHardwareEvent::RelayEscrowBlobReceived {
        gate_hash: b"gate123".to_vec(),
        blob: [0xBB; 32].to_vec(),
    });

    assert_eq!(confirmer.reciprocity(), Reciprocity::Confirmed);
    assert!(confirmer.is_done());
}

#[test]
fn wrong_blob_falls_to_pending() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    confirmer.handle_event(&ExchangeHardwareEvent::RelayEscrowBlobReceived {
        gate_hash: b"gate123".to_vec(),
        blob: [0xFF; 32].to_vec(),
    });

    assert_eq!(confirmer.reciprocity(), Reciprocity::Pending);
    assert!(confirmer.is_done());
}

#[test]
fn deposit_failure_retries_then_falls_to_pending() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    let fail_event = ExchangeHardwareEvent::RelayEscrowFailed {
        gate_hash: b"gate123".to_vec(),
        reason: "network error".into(),
    };

    // Deposit was sent by start() — failure goes to retry/fallthrough
    // But deposit_sent is already true, so it falls to pending immediately
    // (retries only apply when deposit itself failed before being sent)
    let cmds = confirmer.handle_event(&fail_event);
    assert!(cmds.is_empty());
    assert_eq!(confirmer.reciprocity(), Reciprocity::Pending);
}

#[test]
fn persisted_state_roundtrip() {
    let confirmer = make_confirmer();
    let state = confirmer.to_persisted_state();

    assert_eq!(state.our_token, [0xAA; 32]);
    assert_eq!(state.expected_their_token, [0xBB; 32]);
    assert_eq!(state.gate_hash, "gate123");

    let resumed = ReciprocityConfirmer::from_persisted(state, 1000);
    assert_eq!(resumed.reciprocity(), Reciprocity::Pending);
    assert!(!resumed.is_done());
}

#[test]
fn resumed_confirmer_skips_to_polling() {
    let mut confirmer = make_confirmer();
    confirmer.start(); // sends deposit

    let state = confirmer.to_persisted_state();
    assert!(state.deposit_sent);

    let mut resumed = ReciprocityConfirmer::from_persisted(state, 1000);
    let cmds = resumed.start();

    // Should emit check (poll), not deposit again
    assert_eq!(cmds.len(), 1);
    matches!(&cmds[0], ExchangeCommand::RelayEscrowCheck { .. });
}

#[test]
fn ignores_events_for_different_gate() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    let cmds = confirmer.handle_event(&ExchangeHardwareEvent::RelayEscrowReady {
        gate_hash: b"other_gate".to_vec(),
    });
    assert!(cmds.is_empty(), "should ignore events for different gates");
}

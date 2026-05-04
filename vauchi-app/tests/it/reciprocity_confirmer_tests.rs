// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::reciprocity_confirmer::ReciprocityConfirmer;
use vauchi_core::exchange::reciprocity::Reciprocity;
use vauchi_core::{Command, Event};

// Use valid hex strings for gate/slot hashes (matches production encoding)
const GATE_HEX: &str = "aabbccdd00112233445566778899aabbccddeeff00112233445566778899aabb";
const SLOT_OURS_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const SLOT_THEIRS_HEX: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn gate_bytes() -> Vec<u8> {
    hex::decode(GATE_HEX).unwrap()
}

fn make_confirmer() -> ReciprocityConfirmer {
    ReciprocityConfirmer::new(
        [0xAA; 32],
        [0xBB; 32],
        GATE_HEX.to_string(),
        SLOT_OURS_HEX.to_string(),
        SLOT_THEIRS_HEX.to_string(),
        1000,
        true,
    )
}

// @internal
#[test]
fn start_emits_deposit_command() {
    let mut confirmer = make_confirmer();
    let cmds = confirmer.start();
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Command::RelayEscrowDeposit {
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

// @internal
#[test]
fn escrow_ready_triggers_retrieve() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    let cmds = confirmer.handle_event(&Event::RelayEscrowReady {
        gate_hash: gate_bytes(),
    });
    assert_eq!(cmds.len(), 1);
    matches!(&cmds[0], Command::RelayEscrowRetrieve { .. });
}

// @internal
#[test]
fn correct_blob_confirms() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    confirmer.handle_event(&Event::RelayEscrowBlobReceived {
        gate_hash: gate_bytes(),
        blob: [0xBB; 32].to_vec(),
    });

    assert_eq!(confirmer.reciprocity(), Reciprocity::Confirmed);
    assert!(confirmer.is_done());
}

// @internal
#[test]
fn wrong_blob_falls_to_pending() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    confirmer.handle_event(&Event::RelayEscrowBlobReceived {
        gate_hash: gate_bytes(),
        blob: [0xFF; 32].to_vec(),
    });

    assert_eq!(confirmer.reciprocity(), Reciprocity::Pending);
    assert!(confirmer.is_done());
}

// @internal
#[test]
fn deposit_failure_retries_up_to_3_times() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    let fail_event = Event::RelayEscrowFailed {
        gate_hash: gate_bytes(),
        reason: "network error".into(),
    };

    // First 3 failures should produce retry deposit commands
    for i in 0..3 {
        let cmds = confirmer.handle_event(&fail_event.clone());
        assert_eq!(cmds.len(), 1, "retry {i} should emit a deposit command");
        assert!(!confirmer.is_done(), "retry {i} should not be done");
    }

    // 4th failure exhausts retries — falls to pending
    let cmds = confirmer.handle_event(&fail_event);
    assert!(cmds.is_empty(), "exhausted retries should emit nothing");
    assert_eq!(confirmer.reciprocity(), Reciprocity::Pending);
    assert!(confirmer.is_done());
}

// @internal
#[test]
fn persisted_state_roundtrip() {
    let confirmer = make_confirmer();
    let state = confirmer.to_persisted_state();

    assert_eq!(state.our_token, [0xAA; 32]);
    assert_eq!(state.expected_their_token, [0xBB; 32]);
    assert_eq!(state.gate_hash, GATE_HEX);

    let resumed = ReciprocityConfirmer::from_persisted(state, 1000);
    assert_eq!(resumed.reciprocity(), Reciprocity::Pending);
    assert!(!resumed.is_done());
}

// @internal
#[test]
fn resumed_confirmer_skips_to_polling() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    let state = confirmer.to_persisted_state();
    assert!(state.deposit_sent);

    let mut resumed = ReciprocityConfirmer::from_persisted(state, 1000);
    let cmds = resumed.start();

    assert_eq!(cmds.len(), 1);
    matches!(&cmds[0], Command::RelayEscrowCheck { .. });
}

// @internal
#[test]
fn ignores_events_for_different_gate() {
    let mut confirmer = make_confirmer();
    confirmer.start();

    let cmds = confirmer.handle_event(&Event::RelayEscrowReady {
        gate_hash: b"other_gate".to_vec(),
    });
    assert!(cmds.is_empty(), "should ignore events for different gates");
}

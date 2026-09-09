// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the NFC exchange sub-flow. A `#[path]` child of `nfc.rs` so
//! they keep `pub(super)` access to `NfcStep`, `NfcHardwareOutcome`,
//! `RelayHandoff`, and `NfcExchangeFlow` while the flow file stays under
//! the source size limit.

use super::*;
use vauchi_core::Event;
use vauchi_core::exchange::nfc_apdu::{self, ApduError};

fn make_identity(name: &str) -> Identity {
    Identity::create(name, 0)
}

// @internal
#[test]
fn new_initiator_starts_idle() {
    let flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    assert_eq!(*flow.step(), NfcStep::Idle);
    assert!(flow.is_initiator);
}

// @internal
#[test]
fn new_responder_starts_idle() {
    let flow = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
    assert_eq!(*flow.step(), NfcStep::Idle);
    assert!(!flow.is_initiator);
}

// @internal
#[test]
fn initiator_activate_emits_nfc_activate_with_key_offer_payload() {
    let mut flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    let commands = flow.activate().expect("activate");
    assert_eq!(*flow.step(), NfcStep::AwaitingTap);
    assert_eq!(commands.len(), 1);
    match &commands[0] {
        Command::NfcActivate { payload, .. } => {
            assert_eq!(
                payload.len(),
                NFC_PAYLOAD_SIZE,
                "initiator activation must carry exactly one ExchangeNfc payload"
            );
        }
        other => panic!("expected NfcActivate, got {other:?}"),
    }
}

// @internal
#[test]
fn responder_activate_emits_nfc_activate_with_empty_payload() {
    let mut flow = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
    let commands = flow.activate().expect("activate");
    assert_eq!(*flow.step(), NfcStep::AwaitingTap);
    match &commands[0] {
        Command::NfcActivate { payload, .. } => {
            assert!(
                payload.is_empty(),
                "responder activation payload must be empty"
            );
        }
        other => panic!("expected NfcActivate, got {other:?}"),
    }
}

// @internal
#[test]
fn activate_from_non_idle_step_is_an_error() {
    let mut flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    flow.activate().expect("first activate");
    let err = flow.activate().expect_err("second activate must fail");
    assert!(matches!(err, NfcFlowError::WrongState));
}

// @internal
#[test]
fn full_handshake_initiator_to_complete() {
    // Two flows simulate a real 3-phase exchange via direct
    // command/event plumbing (no NFC transport — events are
    // synthesised from the peer's emitted Commands).
    let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());

    // Phase 1: Alice activates (sends key offer), Bob activates (waits).
    let alice_cmds = alice.activate().expect("alice activate");
    let _ = bob.activate().expect("bob activate");
    let alice_offer = match &alice_cmds[0] {
        Command::NfcActivate { payload, .. } => payload.clone(),
        other => panic!("expected NfcActivate, got {other:?}"),
    };

    // Bob receives Alice's offer.
    let bob_outcome = bob.handle_event(&Event::NfcDataReceived { data: alice_offer });
    let bob_response = match bob_outcome {
        NfcHardwareOutcome::StepAdvanced { commands } => match &commands[0] {
            Command::NfcSendApdu { data, .. } => data.clone(),
            other => panic!("bob expected NfcSendApdu, got {other:?}"),
        },
        other => panic!("bob expected StepAdvanced, got {other:?}"),
    };
    assert_eq!(*bob.step(), NfcStep::AckSent);

    // Phase 2: Alice receives Bob's (key_ack || encrypted_card).
    let alice_outcome = alice.handle_event(&Event::NfcDataReceived { data: bob_response });
    let alice_card = match alice_outcome {
        NfcHardwareOutcome::StepAdvanced { commands } => match &commands[0] {
            Command::NfcSendApdu { data, .. } => data.clone(),
            other => panic!("alice expected NfcSendApdu, got {other:?}"),
        },
        other => panic!("alice expected StepAdvanced, got {other:?}"),
    };
    assert_eq!(*alice.step(), NfcStep::PayloadSent);

    // Phase 3: Bob receives Alice's encrypted card → Complete.
    // Responder terminal ACK: must emit `NfcSendApdu(0x9000)` then
    // `NfcDeactivate` in that order so HCE's binder thread returns
    // via the same channel as Phase 1/2 (Option (a) of
    // `2026-05-20-nfc-hce-responder-sync-boundary`).
    let bob_final = bob.handle_event(&Event::NfcDataReceived { data: alice_card });
    match bob_final {
        NfcHardwareOutcome::Complete {
            card_bytes,
            commands,
        } => {
            assert!(!card_bytes.is_empty());
            assert_eq!(
                commands.len(),
                2,
                "responder terminal must emit both ACK + deactivate"
            );
            match &commands[0] {
                Command::NfcSendApdu { data, apdus } => {
                    assert_eq!(data, &vec![0x90, 0x00], "terminal ACK must be 0x9000");
                    assert_eq!(apdus, &vec![vec![0x90, 0x00]]);
                }
                other => panic!("bob expected NfcSendApdu(0x9000) first, got {other:?}"),
            }
            assert!(matches!(commands[1], Command::NfcDeactivate));
        }
        other => panic!("bob expected Complete, got {other:?}"),
    }
    assert_eq!(*bob.step(), NfcStep::Complete);

    // Alice's terminating event: ACK of her Phase 3 send. The
    // payload bytes are irrelevant — confirm_send_success doesn't
    // parse them.
    let alice_final = alice.handle_event(&Event::NfcDataReceived {
        data: vec![0x90, 0x00],
    });
    match alice_final {
        NfcHardwareOutcome::Complete {
            card_bytes,
            commands,
        } => {
            assert!(!card_bytes.is_empty());
            assert!(commands.iter().any(|c| matches!(c, Command::NfcDeactivate)));
        }
        other => panic!("alice expected Complete, got {other:?}"),
    }
    assert_eq!(*alice.step(), NfcStep::Complete);
}

fn activation_of(commands: &[Command]) -> (Vec<u8>, Vec<Vec<u8>>) {
    match &commands[0] {
        Command::NfcActivate { payload, apdus } => (payload.clone(), apdus.clone()),
        other => panic!("expected NfcActivate, got {other:?}"),
    }
}

fn send_of(outcome: NfcHardwareOutcome) -> (Vec<u8>, Vec<Vec<u8>>) {
    match outcome {
        NfcHardwareOutcome::StepAdvanced { commands } => match &commands[0] {
            Command::NfcSendApdu { data, apdus } => (data.clone(), apdus.clone()),
            other => panic!("expected NfcSendApdu, got {other:?}"),
        },
        other => panic!("expected StepAdvanced, got {other:?}"),
    }
}

fn short_exchange_apdu(payload: &[u8]) -> Vec<u8> {
    let mut apdu = vec![0x00, 0xE0, 0x00, 0x00, payload.len() as u8];
    apdu.extend_from_slice(payload);
    apdu
}

// @internal
#[test]
fn initiator_activate_carries_select_and_exchange_apdus() {
    let mut flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    let (payload, apdus) = activation_of(&flow.activate().expect("activate"));
    assert_eq!(
        apdus,
        vec![nfc_apdu::build_select(), short_exchange_apdu(&payload)]
    );
}

// @internal
#[test]
fn responder_activate_carries_no_apdus() {
    let mut flow = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
    let (payload, apdus) = activation_of(&flow.activate().expect("activate"));
    assert!(payload.is_empty());
    assert!(apdus.is_empty());
}

// @internal
#[test]
fn responder_decodes_exchange_apdu_and_replies_with_status_terminated_response() {
    let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
    let (_, mut apdus) = activation_of(&alice.activate().expect("alice activate"));
    let _ = bob.activate().expect("bob activate");

    let exchange_apdu = apdus.remove(1);
    let (data, reply_apdus) = send_of(bob.handle_event(&Event::NfcApduReceived {
        bytes: exchange_apdu,
    }));

    assert_eq!(*bob.step(), NfcStep::AckSent);
    assert!(
        data.len() > NFC_PAYLOAD_SIZE + 2,
        "key ack + encrypted card + status word, got {} bytes",
        data.len()
    );
    assert_eq!(&data[data.len() - 2..], &[0x90, 0x00]);
    assert_eq!(reply_apdus, vec![data]);
}

// @internal
#[test]
fn initiator_decodes_raw_response_and_frames_its_card_send() {
    let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
    let (offer, _) = activation_of(&alice.activate().expect("alice activate"));
    let _ = bob.activate().expect("bob activate");
    let (bob_response, _) = send_of(bob.handle_event(&Event::NfcDataReceived { data: offer }));

    let (card, apdus) = send_of(alice.handle_event(&Event::NfcApduReceived {
        bytes: bob_response,
    }));

    assert_eq!(*alice.step(), NfcStep::PayloadSent);
    assert_eq!(apdus, vec![short_exchange_apdu(&card)]);
}

// @internal
#[test]
fn initiator_fails_with_fallback_on_aid_not_found_status() {
    let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    let _ = alice.activate().expect("activate");

    let outcome = alice.handle_event(&Event::NfcApduReceived {
        bytes: vec![0x6A, 0x82],
    });

    match outcome {
        NfcHardwareOutcome::FailedWithFallback {
            reason,
            relay_handoff,
        } => {
            assert_eq!(reason, ApduError::AidNotFound.to_string());
            assert!(relay_handoff.is_none());
        }
        other => panic!("expected FailedWithFallback, got {other:?}"),
    }
    assert_eq!(*alice.step(), NfcStep::Complete);
}

// @internal
#[test]
fn responder_answers_select_without_advancing() {
    let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
    let _ = bob.activate().expect("activate");

    let outcome = bob.handle_event(&Event::NfcApduReceived {
        bytes: nfc_apdu::build_select(),
    });

    match outcome {
        NfcHardwareOutcome::Consumed { commands } => assert_eq!(
            commands,
            vec![Command::NfcSendApdu {
                data: vec![0x90, 0x00],
                apdus: vec![vec![0x90, 0x00]],
            }]
        ),
        other => panic!("expected Consumed, got {other:?}"),
    }
    assert_eq!(*bob.step(), NfcStep::AwaitingTap);
}

// @internal
#[test]
fn responder_reassembles_chained_exchange_apdus() {
    let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
    let (offer, _) = activation_of(&alice.activate().expect("alice activate"));
    let _ = bob.activate().expect("bob activate");
    let (head, tail) = offer.split_at(100);
    let mut first = vec![0x10, 0xE0, 0x00, 0x00, head.len() as u8];
    first.extend_from_slice(head);

    let outcome = bob.handle_event(&Event::NfcApduReceived { bytes: first });

    match outcome {
        NfcHardwareOutcome::Consumed { commands } => assert_eq!(
            commands,
            vec![Command::NfcSendApdu {
                data: vec![0x90, 0x00],
                apdus: vec![vec![0x90, 0x00]],
            }]
        ),
        other => panic!("expected Consumed for a non-final chunk, got {other:?}"),
    }
    assert_eq!(*bob.step(), NfcStep::AwaitingTap);

    let _ = send_of(bob.handle_event(&Event::NfcApduReceived {
        bytes: short_exchange_apdu(tail),
    }));
    assert_eq!(*bob.step(), NfcStep::AckSent);
}

// @internal
#[test]
fn responder_rejects_malformed_command_apdu() {
    let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
    let _ = bob.activate().expect("activate");

    let outcome = bob.handle_event(&Event::NfcApduReceived {
        bytes: vec![0x00, 0xE0, 0x00, 0x00, 0x05, 0x01],
    });

    match outcome {
        NfcHardwareOutcome::FailedWithFallback { reason, .. } => {
            assert_eq!(reason, ApduError::MalformedCommand.to_string());
        }
        other => panic!("expected FailedWithFallback, got {other:?}"),
    }
}

// @internal
#[test]
fn nfc_failed_event_routes_to_fail_with_fallback() {
    let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    let _ = alice.activate().expect("activate");

    let outcome = alice.handle_event(&Event::NfcFailed {
        reason: "tag lost".into(),
    });

    match outcome {
        NfcHardwareOutcome::FailedWithFallback {
            reason,
            relay_handoff,
        } => {
            assert_eq!(reason, "tag lost");
            assert!(relay_handoff.is_none());
        }
        other => panic!("expected FailedWithFallback, got {other:?}"),
    }
    assert_eq!(*alice.step(), NfcStep::Complete);
}

// @internal
#[test]
fn permission_denied_routes_to_fail_with_fallback() {
    let mut flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    flow.activate().expect("activate");
    let outcome = flow.handle_event(&Event::PermissionDenied {
        transport: "nfc".into(),
    });
    match outcome {
        NfcHardwareOutcome::FailedWithFallback {
            reason,
            relay_handoff,
        } => {
            assert!(reason.to_lowercase().contains("permission"));
            // Pre-shared-key failure: no relay handoff available.
            assert!(relay_handoff.is_none());
        }
        other => panic!("expected FailedWithFallback, got {other:?}"),
    }
    // Absorbing state.
    assert_eq!(*flow.step(), NfcStep::Complete);
}

// @internal
#[test]
fn hardware_error_for_other_transport_is_ignored() {
    let mut flow = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    flow.activate().expect("activate");
    let outcome = flow.handle_event(&Event::HardwareError {
        transport: "ble".into(),
        error: "ignored".into(),
    });
    assert!(matches!(outcome, NfcHardwareOutcome::Ignored));
    assert_eq!(*flow.step(), NfcStep::AwaitingTap);
}

// @internal
#[test]
fn responder_failure_after_key_ack_yields_relay_handoff() {
    // Drive Bob into AckSent / KeyAckReceived by feeding Alice's
    // key offer to him.
    let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
    let alice_cmds = alice.activate().expect("alice activate");
    let _ = bob.activate().expect("bob activate");
    let offer = match &alice_cmds[0] {
        Command::NfcActivate { payload, .. } => payload.clone(),
        other => panic!("expected NfcActivate, got {other:?}"),
    };
    let bob_outcome = bob.handle_event(&Event::NfcDataReceived { data: offer });
    assert!(matches!(
        bob_outcome,
        NfcHardwareOutcome::StepAdvanced { .. }
    ));
    assert_eq!(*bob.step(), NfcStep::AckSent);

    // Trigger a hardware error after the shared key exists.
    let outcome = bob.handle_event(&Event::HardwareError {
        transport: "nfc".into(),
        error: "tag lost mid-exchange".into(),
    });
    match outcome {
        NfcHardwareOutcome::FailedWithFallback {
            reason,
            relay_handoff,
        } => {
            assert!(reason.contains("tag lost"));
            let handoff =
                relay_handoff.expect("post-shared-key failure must yield a relay handoff");
            assert_eq!(handoff.gate_hash.len(), 32, "gate_hash is SHA-256");
            assert_eq!(handoff.slot_hash.len(), 32, "slot_hash is SHA-256");
            assert!(
                !handoff.encrypted_card.is_empty(),
                "encrypted_card must carry the blob"
            );
        }
        other => panic!("expected FailedWithFallback, got {other:?}"),
    }
}

// ── Screen-builder coverage ────────────────────────────────────────────

fn action_ids(screen: &ScreenModel) -> Vec<String> {
    screen
        .contextual_actions
        .iter()
        .map(|a| a.id.clone())
        .collect()
}

// @internal
#[test]
fn idle_screen_has_cancel_affordance() {
    let s = build_nfc_screen(&NfcStep::Idle, crate::i18n::Locale::English);
    assert_eq!(s.screen_id, "exchange_nfc_idle");
    assert_eq!(action_ids(&s), vec!["cancel".to_string()]);
    assert!(
        s.contextual_actions
            .iter()
            .any(|a| a.id == "cancel" && a.enabled)
    );
}

// @internal
#[test]
fn awaiting_tap_screen_has_cancel_affordance() {
    let s = build_nfc_screen(&NfcStep::AwaitingTap, crate::i18n::Locale::English);
    assert_eq!(s.screen_id, "exchange_nfc_awaiting_tap");
    assert_eq!(action_ids(&s), vec!["cancel".to_string()]);
    assert!(
        s.contextual_actions
            .iter()
            .any(|a| a.id == "cancel" && a.enabled)
    );
}

// @internal
#[test]
fn in_progress_screens_share_screen_id_and_keep_cancel_enabled() {
    let sent = build_nfc_screen(&NfcStep::PayloadSent, crate::i18n::Locale::English);
    let ack = build_nfc_screen(&NfcStep::AckSent, crate::i18n::Locale::English);
    assert_eq!(sent.screen_id, "exchange_nfc_in_progress");
    assert_eq!(ack.screen_id, "exchange_nfc_in_progress");
    assert_eq!(action_ids(&sent), vec!["cancel".to_string()]);
    assert_eq!(action_ids(&ack), vec!["cancel".to_string()]);
}

// @internal
#[test]
fn complete_screen_disables_cancel() {
    let s = build_nfc_screen(&NfcStep::Complete, crate::i18n::Locale::English);
    assert_eq!(s.screen_id, "exchange_nfc_complete");
    // Cancel is still listed (so the action surface is stable across
    // states) but disabled — the exchange has already completed.
    assert_eq!(action_ids(&s), vec!["cancel".to_string()]);
    assert!(
        s.contextual_actions
            .iter()
            .any(|a| a.id == "cancel" && !a.enabled)
    );
}

// ── CC-13 proptest: Complete is absorbing ──────────────────────────────
//
// Engine-walker reachability tests (the CC-22 pattern used by
// `core/vauchi-app/tests/reachability/exchange_ble.rs`): the INITIATOR
// entry IS wired (`ExchangeMode::TapTap` -> `start_taptap_mode`), but the
// RESPONDER entry (HCE-driven `new_responder`) is not yet wired into
// ExchangeEngine, so the walker cannot BFS the responder NFC steps.
// Tracked in `_private/docs/problems/2026-05-29-nfc-exchange-mode-entry-wiring`;
// until then we exercise the sub-flow invariants at the unit-test layer.

use proptest::prelude::*;

fn arb_post_complete_event() -> impl Strategy<Value = Event> {
    prop_oneof![
        // NFC data could keep arriving after Complete (stale APDUs,
        // re-tap, etc.) — must not re-open the state.
        (any::<Vec<u8>>()).prop_map(|data| Event::NfcDataReceived { data }),
        // Transport-level errors scoped to NFC.
        Just(Event::HardwareError {
            transport: "nfc".into(),
            error: "spurious".into(),
        }),
        Just(Event::PermissionDenied {
            transport: "nfc".into(),
        }),
        Just(Event::HardwareUnavailable {
            transport: "nfc".into(),
        }),
        // Cross-transport noise — must be Ignored.
        Just(Event::BleDeviceDiscovered {
            id: "stray".into(),
            rssi: -50,
            adv_data: vec![],
        }),
        Just(Event::HardwareError {
            transport: "ble".into(),
            error: "stray".into(),
        }),
    ]
}

fn drive_to_complete() -> NfcExchangeFlow {
    // Use the happy-path initiator drive (mirrors
    // `full_handshake_initiator_to_complete`).
    let mut alice = NfcExchangeFlow::new_initiator(make_identity("Alice"), "Alice".into());
    let mut bob = NfcExchangeFlow::new_responder(make_identity("Bob"), "Bob".into());
    let alice_cmds = alice.activate().expect("alice activate");
    let _ = bob.activate().expect("bob activate");
    let offer = match &alice_cmds[0] {
        Command::NfcActivate { payload, .. } => payload.clone(),
        _ => unreachable!(),
    };
    let bob_outcome = bob.handle_event(&Event::NfcDataReceived { data: offer });
    let bob_response = match bob_outcome {
        NfcHardwareOutcome::StepAdvanced { commands } => match &commands[0] {
            Command::NfcSendApdu { data, .. } => data.clone(),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    let alice_outcome = alice.handle_event(&Event::NfcDataReceived { data: bob_response });
    let alice_card = match alice_outcome {
        NfcHardwareOutcome::StepAdvanced { commands } => match &commands[0] {
            Command::NfcSendApdu { data, .. } => data.clone(),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    // Phase 3: Bob receives Alice's encrypted card → Complete.
    let _ = bob.handle_event(&Event::NfcDataReceived { data: alice_card });
    let _ = alice.handle_event(&Event::NfcDataReceived {
        data: vec![0x90, 0x00],
    });
    // Both Alice and Bob are now Complete; return Alice so the
    // proptest exercises an initiator that has finished its
    // handshake (Phase 3 confirm path).
    alice
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// CC-13 invariant: once `NfcExchangeFlow` is in `Complete`, no
    /// random subsequent event transitions it away. Symptom this
    /// would catch: a late `NfcDataReceived` after Complete
    /// silently re-running the handshake from a partial state, or
    /// a stray hardware error flipping the step back to a
    /// non-terminal value.
    // @internal
    #[test]
    fn complete_is_absorbing(events in prop::collection::vec(arb_post_complete_event(), 0..20)) {
        let mut flow = drive_to_complete();
        prop_assert_eq!(flow.step(), &NfcStep::Complete);
        for event in &events {
            let _ = flow.handle_event(event);
            prop_assert_eq!(
                flow.step(),
                &NfcStep::Complete,
                "Complete must be absorbing; event {:?} transitioned out",
                event,
            );
        }
    }
}

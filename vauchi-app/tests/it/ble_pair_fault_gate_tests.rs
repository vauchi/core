// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tier-1 exchange reliability gate — BLE pair slice (Phase B of the
//! exchange-mode program, `2026-07-20-three-tier-exchange-reliability-gate`).
//!
//! Two REAL `BleHandshakeMachine`s cross-fed through a fault-injectable
//! link: every `BleWriteCharacteristic` one peer emits is delivered to
//! the other on the physically faithful link view (sender's latched
//! link, inverted direction), under virtual time. The channel can drop,
//! duplicate, and reorder messages — the fault family GATT actually
//! produces across characteristics (`ble_handshake_machine.rs`
//! quarantine docs).
//!
//! Jurisdiction (per the three-tier record): this tier blocks *logic*
//! regressions — role/glare resolution, cross-characteristic ordering,
//! bounded terminal outcomes. It can never certify device reliability
//! (Tier 2 exercises the PAE two-machine wiring; Tier 3 is hardware
//! soak). Persistence + `TrustMetrics` assertions ride the AppEngine
//! slice, not this machine-level harness.

use std::collections::VecDeque;

use vauchi_app::orchestrator::ble_handshake_machine::{
    BleHandshakeMachine, BleMachineEvent, BleMachinePhase,
};
use vauchi_core::Command;
use vauchi_core::crypto::X3DHKeyPair;
use vauchi_core::exchange::{
    BleCardPayload, BleExchangeResult, CHAR_DATA_NOTIFY, CHAR_HANDSHAKE_NOTIFY,
};
use vauchi_core::platform::BleLinkDirection;

const ALICE_DEV: &str = "alice-dev";
const BOB_DEV: &str = "bob-dev";
const ALICE_IDENTITY: [u8; 32] = [1u8; 32];
const BOB_IDENTITY: [u8; 32] = [2u8; 32];
/// Delivery budget: a full exchange is ~a dozen messages; anything
/// beyond this is a livelock, not slowness (virtual time — no waits).
const MAX_STEPS: usize = 200;
/// Simulated seconds charged per delivery. BLE notifications arrive in
/// milliseconds, so this only has to advance monotonically; at 10s a
/// mere six deliveries burned the whole 60s handshake window and any
/// benign reorder read as an expiry
/// (`backlog/2026-07-28-ble-pair-benign-handshake-reorder-expires`).
const DELIVERY_SECS: u64 = 1;

fn card_for(identity: [u8; 32], secret: [u8; 32], name: &str) -> (BleCardPayload, X3DHKeyPair) {
    let x3dh = X3DHKeyPair::from_bytes(secret);
    let card = BleCardPayload::new(identity, name.into(), *x3dh.public_key(), vec![], None);
    (card, x3dh)
}

fn alice_machine(initiator: bool) -> BleHandshakeMachine {
    let (card, x3dh) = card_for(ALICE_IDENTITY, [11u8; 32], "Alice");
    if initiator {
        BleHandshakeMachine::new_initiator(ALICE_IDENTITY, x3dh, card, 0, None)
    } else {
        BleHandshakeMachine::new_responder(ALICE_IDENTITY, x3dh, card, 0, None)
    }
}

fn bob_machine(initiator: bool) -> BleHandshakeMachine {
    let (card, x3dh) = card_for(BOB_IDENTITY, [22u8; 32], "Bob");
    if initiator {
        BleHandshakeMachine::new_initiator(BOB_IDENTITY, x3dh, card, 0, None)
    } else {
        BleHandshakeMachine::new_responder(BOB_IDENTITY, x3dh, card, 0, None)
    }
}

/// In-flight message: characteristic uuid, payload, and the sender's
/// link direction at send time (the recipient sees it inverted).
type Msg = (String, Vec<u8>, BleLinkDirection);

fn invert(direction: BleLinkDirection) -> BleLinkDirection {
    match direction {
        BleLinkDirection::Inbound => BleLinkDirection::Outbound,
        // Outbound plus any future non-exhaustive variant: the peer of a
        // dialed link is the dialed-into side.
        _ => BleLinkDirection::Inbound,
    }
}

struct Pair {
    alice: BleHandshakeMachine,
    bob: BleHandshakeMachine,
    to_alice: VecDeque<Msg>,
    to_bob: VecDeque<Msg>,
    alice_result: Option<BleExchangeResult>,
    bob_result: Option<BleExchangeResult>,
    alice_confirmed: Option<[u8; 32]>,
    bob_confirmed: Option<[u8; 32]>,
    now: u64,
}

impl Pair {
    /// Alice dials Bob: Alice initiator (Outbound), Bob responder (Inbound).
    fn connected() -> Self {
        Self::build(
            alice_machine(true),
            bob_machine(false),
            BleLinkDirection::Outbound,
            BleLinkDirection::Inbound,
        )
    }

    /// Bob dials Alice — same protocol, roles swapped.
    fn connected_reversed() -> Self {
        Self::build(
            alice_machine(false),
            bob_machine(true),
            BleLinkDirection::Inbound,
            BleLinkDirection::Outbound,
        )
    }

    /// Symmetric discovery glare: BOTH dial out and send a KeyOffer; the
    /// identity-key tiebreak must converge to one initiator/responder pair.
    fn glare() -> Self {
        Self::build(
            alice_machine(true),
            bob_machine(true),
            BleLinkDirection::Outbound,
            BleLinkDirection::Outbound,
        )
    }

    fn build(
        mut alice: BleHandshakeMachine,
        mut bob: BleHandshakeMachine,
        alice_dir: BleLinkDirection,
        bob_dir: BleLinkDirection,
    ) -> Self {
        let (_, a_cmds) = alice.on_connected(BOB_DEV, alice_dir, 0);
        let (_, b_cmds) = bob.on_connected(ALICE_DEV, bob_dir, 0);
        let mut pair = Pair {
            alice,
            bob,
            to_alice: VecDeque::new(),
            to_bob: VecDeque::new(),
            alice_result: None,
            bob_result: None,
            alice_confirmed: None,
            bob_confirmed: None,
            now: 0,
        };
        Self::route(a_cmds, &mut pair.to_bob);
        Self::route(b_cmds, &mut pair.to_alice);
        pair
    }

    fn route(cmds: Vec<Command>, out: &mut VecDeque<Msg>) {
        for cmd in cmds {
            if let Command::BleWriteCharacteristic {
                uuid,
                data,
                direction,
                ..
            } = cmd
            {
                out.push_back((uuid, data, direction));
            }
        }
    }

    fn deliver_one_to_alice(&mut self) {
        let Some((uuid, data, sent_dir)) = self.to_alice.pop_front() else {
            return;
        };
        self.now += DELIVERY_SECS;
        let (event, cmds) =
            self.alice
                .on_data_received(BOB_DEV, invert(sent_dir), &uuid, &data, self.now);
        Self::record(event, &mut self.alice_result, &mut self.alice_confirmed);
        Self::route(cmds, &mut self.to_bob);
    }

    fn deliver_one_to_bob(&mut self) {
        let Some((uuid, data, sent_dir)) = self.to_bob.pop_front() else {
            return;
        };
        self.now += DELIVERY_SECS;
        let (event, cmds) =
            self.bob
                .on_data_received(ALICE_DEV, invert(sent_dir), &uuid, &data, self.now);
        Self::record(event, &mut self.bob_result, &mut self.bob_confirmed);
        Self::route(cmds, &mut self.to_alice);
    }

    fn record(
        event: BleMachineEvent,
        result: &mut Option<BleExchangeResult>,
        confirmed: &mut Option<[u8; 32]>,
    ) {
        match event {
            BleMachineEvent::Completed(r) => *result = Some(*r),
            BleMachineEvent::ReciprocityConfirmed { their_identity } => {
                *confirmed = Some(their_identity);
            }
            _ => {}
        }
    }

    /// Alternate FIFO delivery in both directions until quiet or budget.
    /// Returns the delivery count; a run that hits `MAX_STEPS` livelocked.
    fn pump(&mut self) -> usize {
        let mut steps = 0;
        while (!self.to_alice.is_empty() || !self.to_bob.is_empty()) && steps < MAX_STEPS {
            if !self.to_bob.is_empty() {
                self.deliver_one_to_bob();
                steps += 1;
            }
            if !self.to_alice.is_empty() {
                self.deliver_one_to_alice();
                steps += 1;
            }
        }
        steps
    }

    /// P1 reciprocity: each completed side sends its post-persist ack.
    fn exchange_reciprocity_acks(&mut self) {
        if let Some(cmd) = self.alice.build_reciprocity_ack_command() {
            Self::route(vec![cmd], &mut self.to_bob);
        }
        if let Some(cmd) = self.bob.build_reciprocity_ack_command() {
            Self::route(vec![cmd], &mut self.to_alice);
        }
        self.pump();
    }

    fn assert_both_completed_and_cross_verified(&self) {
        assert_eq!(self.alice.phase(), BleMachinePhase::Completed);
        assert_eq!(self.bob.phase(), BleMachinePhase::Completed);
        let a = self.alice_result.as_ref().expect("alice result");
        let b = self.bob_result.as_ref().expect("bob result");
        assert_eq!(a.remote_card.display_name, "Bob");
        assert_eq!(a.remote_card.identity_key, BOB_IDENTITY);
        assert_eq!(b.remote_card.display_name, "Alice");
        assert_eq!(b.remote_card.identity_key, ALICE_IDENTITY);
    }
}

/// Drop the first queued message on `queue` whose uuid is `uuid`.
fn drop_first(queue: &mut VecDeque<Msg>, uuid: &str) -> bool {
    if let Some(pos) = queue.iter().position(|(u, _, _)| u == uuid) {
        queue.remove(pos);
        return true;
    }
    false
}

/// Duplicate the first queued message on `queue` whose uuid is `uuid`,
/// inserting the copy directly behind the original.
fn duplicate_first(queue: &mut VecDeque<Msg>, uuid: &str) -> bool {
    if let Some(pos) = queue.iter().position(|(u, _, _)| u == uuid) {
        let msg = queue[pos].clone();
        queue.insert(pos + 1, msg);
        return true;
    }
    false
}

/// Move the first `uuid` message behind everything else in `queue` —
/// the cross-characteristic reorder GATT permits (order is preserved
/// per characteristic, not across characteristics).
fn reorder_to_back(queue: &mut VecDeque<Msg>, uuid: &str) -> bool {
    if let Some(pos) = queue.iter().position(|(u, _, _)| u == uuid) {
        let msg = queue.remove(pos).expect("position from iter");
        queue.push_back(msg);
        return true;
    }
    false
}

// @internal
#[test]
fn happy_path_pair_completes_and_confirms_reciprocity_both_sides() {
    let mut pair = Pair::connected();
    let steps = pair.pump();
    assert!(steps < MAX_STEPS, "exchange livelocked after {steps} steps");
    pair.assert_both_completed_and_cross_verified();

    pair.exchange_reciprocity_acks();
    assert_eq!(pair.alice_confirmed, Some(BOB_IDENTITY));
    assert_eq!(pair.bob_confirmed, Some(ALICE_IDENTITY));
}

// @internal
#[test]
fn reversed_roles_pair_completes_identically() {
    let mut pair = Pair::connected_reversed();
    let steps = pair.pump();
    assert!(steps < MAX_STEPS, "exchange livelocked after {steps} steps");
    pair.assert_both_completed_and_cross_verified();
}

// @internal
#[test]
fn symmetric_glare_tiebreak_converges_to_one_completed_exchange() {
    // Both dialed, both sent a KeyOffer. The larger identity (Bob) must
    // yield to responder; the pair must still complete on both sides
    // (`2026-07-22-role-tiebreak-and-glare-design.md`).
    let mut pair = Pair::glare();
    let steps = pair.pump();
    assert!(steps < MAX_STEPS, "glare livelocked after {steps} steps");
    pair.assert_both_completed_and_cross_verified();
}

// @internal
#[test]
fn key_ack_after_card_chunks_reorder_still_completes() {
    // GATT gives no cross-characteristic ordering: the responder's card
    // chunks (CHAR_DATA_NOTIFY) may all arrive before its KeyAck
    // (CHAR_HANDSHAKE_NOTIFY). The quarantine must absorb this.
    let mut pair = Pair::connected();
    pair.deliver_one_to_bob(); // KeyOffer → responder emits KeyAck + chunks
    assert!(
        reorder_to_back(&mut pair.to_alice, CHAR_HANDSHAKE_NOTIFY),
        "expected a KeyAck queued toward the initiator"
    );
    let steps = pair.pump();
    assert!(steps < MAX_STEPS, "reordered exchange livelocked");
    pair.assert_both_completed_and_cross_verified();
}

// @internal
#[test]
fn duplicated_key_ack_before_chunks_is_harmless() {
    // A link-layer retry can re-deliver the KeyAck while the card
    // chunks are still in flight; re-stashing the same bytes must not
    // disturb the exchange.
    let mut pair = Pair::connected();
    pair.deliver_one_to_bob();
    assert!(duplicate_first(&mut pair.to_alice, CHAR_HANDSHAKE_NOTIFY));
    let steps = pair.pump();
    assert!(steps < MAX_STEPS, "duplicated-KeyAck exchange livelocked");
    pair.assert_both_completed_and_cross_verified();
}

// RED (Phase C spec): a KeyAck duplicate that lands AFTER the card
// completed reassembly reaches `complete_with_reveal`, fails reveal
// verification, and poisons an otherwise-successful exchange. The
// orchestrator-unification quarantine design requires dedup
// (`backlog/2026-07-20-ble-exchange-orchestrator-unification`). Un-ignore
// with that fix.
// @internal
#[test]
#[ignore = "Phase C dedup: late duplicate KeyAck currently poisons completion (2026-07-20-ble-exchange-orchestrator-unification)"]
fn late_duplicate_key_ack_is_deduplicated_not_fatal() {
    let mut pair = Pair::connected();
    pair.deliver_one_to_bob();
    // Deliver the responder's chunks first, keep the KeyAck last, then
    // append its duplicate: the dup arrives post-PayloadsExchanged.
    assert!(reorder_to_back(&mut pair.to_alice, CHAR_HANDSHAKE_NOTIFY));
    assert!(duplicate_first(&mut pair.to_alice, CHAR_HANDSHAKE_NOTIFY));
    let steps = pair.pump();
    assert!(steps < MAX_STEPS);
    pair.assert_both_completed_and_cross_verified();
}

// Canary, not spec: the machine layer has NO deadline authority — a
// dropped KeyAck strands both peers non-terminal forever; only the
// chrome engine's timeout (the R3 two-machine split) rescues the user.
// Phase C moves deadline authority into the orchestrator; when it does,
// this test MUST break and be rewritten to assert a bounded failure.
// @internal
#[test]
fn dropped_key_ack_strands_pair_non_terminal_at_machine_layer() {
    let mut pair = Pair::connected();
    pair.deliver_one_to_bob();
    assert!(drop_first(&mut pair.to_alice, CHAR_HANDSHAKE_NOTIFY));
    let steps = pair.pump();
    assert!(steps < MAX_STEPS);
    assert!(pair.to_alice.is_empty() && pair.to_bob.is_empty());
    assert!(
        !pair.alice.is_terminal() && !pair.bob.is_terminal(),
        "machine layer has no deadline — if this fails, Phase C landed: \
         rewrite this canary to assert bounded failure instead"
    );
    assert!(pair.alice_result.is_none() && pair.bob_result.is_none());
}

mod fault_schedule_props {
    use super::*;
    use proptest::prelude::*;

    /// CC-13: any schedule of benign faults (no loss) applied at any
    /// point in the exchange must still converge to Completed on both
    /// sides within the delivery budget.
    ///
    /// KeyAck duplication is excluded from the random family: composed
    /// with a reorder it lands the duplicate after the card completes and
    /// poisons the exchange — the known Phase C dedup defect pinned by
    /// `late_duplicate_key_ack_is_deduplicated_not_fatal` (ignored RED).
    /// Widen this family to handshake duplicates when that fix lands.
    fn apply_fault(queue: &mut VecDeque<Msg>, op: u8, uuid: &str) {
        let duplicate_allowed = uuid == CHAR_DATA_NOTIFY;
        match op % 3 {
            0 if duplicate_allowed => {
                duplicate_first(queue, uuid);
            }
            1 => {
                reorder_to_back(queue, uuid);
            }
            _ => {}
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        // @internal
        #[test]
        fn benign_fault_schedules_always_complete(
            ops in proptest::collection::vec((0u8..3, prop::bool::ANY), 0..6)
        ) {
            let mut pair = Pair::connected();
            pair.deliver_one_to_bob();
            for (op, target_handshake) in ops {
                let uuid = if target_handshake {
                    CHAR_HANDSHAKE_NOTIFY
                } else {
                    CHAR_DATA_NOTIFY
                };
                apply_fault(&mut pair.to_alice, op, uuid);
            }
            let steps = pair.pump();
            prop_assert!(steps < MAX_STEPS, "livelock under fault schedule");
            prop_assert_eq!(pair.alice.phase(), BleMachinePhase::Completed);
            prop_assert_eq!(pair.bob.phase(), BleMachinePhase::Completed);
        }
    }
}

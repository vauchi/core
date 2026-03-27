// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Timing and scan-asymmetry property tests for the multi-stage QR protocol.
//!
//! Layer 1 (this file, non-ignored): State machine robustness under asymmetric
//! scan loss. Runs in normal CI.
//!
//! Layer 2 (#[ignore]): Wall-clock jitter tests for timeout tuning regression.
//! Run with `just test-slow` or nightly CI.
//!
//! See: _private/docs/designs/2026-03-27-self-healing-reasoning-errors-design.md

use proptest::prelude::*;
use std::time::{Duration, Instant};
use vauchi_core::exchange::MultiStageSession;
use vauchi_core::exchange::ProtocolState;

/// Run an exchange where each side only processes every Nth QR from the peer.
///
/// `scan_every_n = 1` means process every QR (normal).
/// `scan_every_n = 5` means process only every 5th QR (80% loss).
///
/// The skip counter is per-side and tracks how many QRs the *peer has
/// displayed since the last scan* — not the outer loop cycle. This avoids
/// resonance between the outer cycle modulo and the session's internal
/// `display_cycle % 3` VRFY-interleave pattern (which would cause Alice
/// to always land on VRFY when scan_every_n == 3, creating a deadlock).
///
/// Optional per-side delays simulate wall-clock asymmetry (Layer 2).
fn run_asymmetric_exchange(
    alice_payload: &[u8],
    bob_payload: &[u8],
    alice_scan_every_n: u8,
    bob_scan_every_n: u8,
    alice_delay: Option<Duration>,
    bob_delay: Option<Duration>,
) -> (ProtocolState, ProtocolState) {
    let mut alice = MultiStageSession::new(alice_payload.to_vec());
    let mut bob = MultiStageSession::new(bob_payload.to_vec());

    // Stage 1: INIT — both must scan each other's INIT QR to proceed.
    // No skipping here — INIT is a one-time handshake.
    let ai = alice.get_display_qr().expect("alice INIT");
    let bi = bob.get_display_qr().expect("bob INIT");
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    // Stages 2+: cycle with asymmetric scan rates and optional delays.
    // Layer 1 (no delays): tight loop completes in <1s, so 5s is generous.
    // Layer 2 (delays): up to 100ms * 2 sides * hundreds of cycles.
    let timeout = if alice_delay.is_some() || bob_delay.is_some() {
        Duration::from_secs(120) // Layer 2: wall-clock delays need more time
    } else {
        Duration::from_secs(5) // Layer 1: tight loop, 5s safety net
    };
    let deadline = Instant::now() + timeout;

    // Independent per-side display counters — not tied to the outer loop
    // cycle so they can't resonate with session-internal display_cycle patterns.
    //
    // Offset initialization: start alice_peer_displayed at (alice_scan_every_n - 1)
    // so the first scan happens at Bob's display_cycle=1, not display_cycle=N.
    // Without this, scan_every_n=3 always lands on display_cycle 3, 6, 9…
    // which are exactly the VRFY frames (display_cycle.is_multiple_of(3)),
    // creating a permanent deadlock in Confirming.
    let mut alice_peer_displayed: u32 = (alice_scan_every_n - 1) as u32;
    let mut alice_last_scan_at: u32 = 0;
    let mut bob_peer_displayed: u32 = (bob_scan_every_n - 1) as u32;
    let mut bob_last_scan_at: u32 = 0;

    loop {
        // Both always display QR (this is where timeout checks happen)
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();

        // Track how many QRs each peer has displayed
        if bq.is_some() {
            alice_peer_displayed += 1;
        }
        if aq.is_some() {
            bob_peer_displayed += 1;
        }

        // Alice processes Bob's QR only after alice_scan_every_n new displays
        if let Some(bq) = &bq {
            if alice_peer_displayed - alice_last_scan_at >= alice_scan_every_n as u32 {
                alice.process_scanned_qr(&bq.data);
                alice_last_scan_at = alice_peer_displayed;
            }
        }

        // Bob processes Alice's QR only after bob_scan_every_n new displays
        if let Some(aq) = &aq {
            if bob_peer_displayed - bob_last_scan_at >= bob_scan_every_n as u32 {
                bob.process_scanned_qr(&aq.data);
                bob_last_scan_at = bob_peer_displayed;
            }
        }

        // Optional per-side delays for wall-clock jitter (Layer 2)
        if let Some(d) = alice_delay {
            std::thread::sleep(d);
        }
        if let Some(d) = bob_delay {
            std::thread::sleep(d);
        }

        // Check terminal states
        let as_ = alice.get_state();
        let bs = bob.get_state();
        let alice_terminal = matches!(as_, ProtocolState::Finalized | ProtocolState::Failed(_));
        let bob_terminal = matches!(bs, ProtocolState::Finalized | ProtocolState::Failed(_));

        if alice_terminal && bob_terminal {
            return (as_, bs);
        }

        // Safety timeout to prevent infinite loops
        if Instant::now() > deadline {
            return (as_, bs);
        }
    }
}

// ─── Layer 1: Cycle-skipping (fast, runs in CI) ─────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(50))]

    /// Both sides must finalize despite asymmetric scan rates.
    ///
    /// scan_every_n=1 → process every QR (0% loss)
    /// scan_every_n=5 → process every 5th QR (80% loss)
    ///
    /// Deterministic: proptest can shrink scan_every_n to find minimal
    /// failing pattern.
    #[test]
    fn test_exchange_completes_with_asymmetric_scan_loss(
        alice_n in 1u8..6,
        bob_n in 1u8..6,
    ) {
        let alice_payload = vec![0xAA; 200];
        let bob_payload = vec![0xBB; 200];

        let (alice_state, bob_state) = run_asymmetric_exchange(
            &alice_payload,
            &bob_payload,
            alice_n,
            bob_n,
            None, // no delay — tight loop
            None,
        );

        prop_assert!(
            matches!(alice_state, ProtocolState::Finalized),
            "Alice did not finalize (scan_every_n={}), got: {:?}",
            alice_n, alice_state
        );
        prop_assert!(
            matches!(bob_state, ProtocolState::Finalized),
            "Bob did not finalize (scan_every_n={}), got: {:?}",
            bob_n, bob_state
        );
    }
}

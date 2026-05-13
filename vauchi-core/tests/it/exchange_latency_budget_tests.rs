// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! D5 exchange-latency budget test.
//!
//! MVP gate D5: "Exchange latency < 3 s QR scan to card display". This
//! target is user-perceived wall-clock time and so depends on device
//! hardware (camera focus, network RTT, UI render) that can't be
//! exercised from a core test. What *can* be enforced from core is an
//! algorithmic upper bound that leaves enough budget for the device
//! portion to meet the user-perceived target on realistic hardware.
//!
//! This test runs the full algorithmic path a mobile scanner executes
//! between "camera delivers luma frame" and `ExchangeSession::Complete`:
//!
//! 1. Both sides `StartQR` (session setup + ephemeral key generation)
//! 2. Each `ExchangeQR` serialised via `to_data_string()` and rendered
//!    to a grayscale QR bitmap (the output the platform camera would
//!    produce after focus)
//! 3. Each side runs `scan_qr_from_luma` (rxing/rqrr pipeline) on the
//!    other's bitmap to recover the payload string
//! 4. `ExchangeQR::from_data_string()` reconstructs the peer's QR
//! 5. State machine: `ProcessQR` → `TheyScannedOurQR` →
//!    `PerformKeyAgreement` → `CompleteExchange` (includes X3DH + HKDF)
//!
//! Budget: **1000 ms** on the bench host. This leaves ~2 s for
//! camera focus + network + UI render to hit the 3 s user-perceived
//! gate on typical mobile hardware. Blown budgets indicate regressions
//! in the QR decode pipeline, crypto, or state-machine transitions —
//! not device-side factors.

#![cfg(feature = "testing")]

use std::time::Instant;

use qrcode::{EcLevel, QrCode};
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState, MockProximityVerifier,
};
use vauchi_core::qr::scanner::{ScannerBackend, scan_qr_from_luma};
use vauchi_core::{ContactCard, Identity};

/// Core-algorithmic budget (ms) for the QR-scan-to-card-complete path.
/// The user-perceived D5 target is 3000 ms; the remaining ~2 s covers
/// device-side factors (camera focus, network RTT, UI render) that
/// are validated manually at release.
const CORE_ALGORITHMIC_BUDGET_MS: u128 = 1000;

/// Bitmap dimensions matching what mobile apps render (same default as
/// `vauchi_platform::generate_qr_bitmap` in production).
const BITMAP_SIZE_PX: u32 = 512;
const QR_QUIET_MARGIN: u32 = 4;

/// Render an `ExchangeQR` payload string into a grayscale bitmap
/// identical to what `vauchi_platform::generate_qr_bitmap` produces.
/// Inlined here (not imported) so this test remains a pure vauchi-core
/// test with no vauchi-platform dependency.
fn render_qr_bitmap(data: &str, size: u32, margin: u32) -> (u32, Vec<u8>) {
    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::M)
        .expect("QR encoding of valid exchange payload should not fail");
    let qr_width = code.width() as u32;
    let total_modules = qr_width + 2 * margin;
    let scale = size as f32 / total_modules as f32;
    let mut pixels = vec![255u8; (size * size) as usize];
    for (i, color) in code.to_colors().iter().enumerate() {
        if *color == qrcode::Color::Dark {
            let qx = (i as u32) % qr_width;
            let qy = (i as u32) / qr_width;
            let px0 = ((qx + margin) as f32 * scale) as u32;
            let py0 = ((qy + margin) as f32 * scale) as u32;
            let px1 = (((qx + margin + 1) as f32 * scale) as u32).min(size);
            let py1 = (((qy + margin + 1) as f32 * scale) as u32).min(size);
            for py in py0..py1 {
                let row_start = (py * size + px0) as usize;
                let row_end = (py * size + px1) as usize;
                pixels[row_start..row_end].fill(0);
            }
        }
    }
    (size, pixels)
}

/// D5 exchange-latency gate: algorithmic-only portion of the QR-scan
/// to card-display path stays well under the user-perceived 3 s budget.
///
/// See: `_private/docs/investigations/measurements/`
/// `2026-04-16-d5-performance-baseline.md` (Exchange Latency section).
// @scenario: contact_exchange :: Exchange completes under MVP latency budget
#[test]
fn d5_exchange_latency_algorithmic_core_under_budget() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");
    let alice_card = ContactCard::new("Alice");
    let bob_card = ContactCard::new("Bob");

    let start = Instant::now();

    // Phase 1: session setup + key generation
    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );
    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // Phase 2: render each ExchangeQR to a camera-equivalent grayscale bitmap
    let alice_payload = alice_session.qr().unwrap().to_data_string();
    let bob_payload = bob_session.qr().unwrap().to_data_string();
    let (alice_size, alice_bitmap) =
        render_qr_bitmap(&alice_payload, BITMAP_SIZE_PX, QR_QUIET_MARGIN);
    let (bob_size, bob_bitmap) = render_qr_bitmap(&bob_payload, BITMAP_SIZE_PX, QR_QUIET_MARGIN);

    // Phase 3: each side scans the other's bitmap via the production
    // rxing/rqrr pipeline (the same path platform cameras feed into)
    let alice_decoded = scan_qr_from_luma(
        ScannerBackend::RqrrPreprocessed,
        &bob_bitmap,
        bob_size,
        bob_size,
    )
    .decoded
    .expect("alice should decode bob's QR");
    let bob_decoded = scan_qr_from_luma(
        ScannerBackend::RqrrPreprocessed,
        &alice_bitmap,
        alice_size,
        alice_size,
    )
    .decoded
    .expect("bob should decode alice's QR");

    // Phase 4: reconstruct ExchangeQR from decoded strings. `alice_decoded`
    // came from scanning Bob's bitmap — so it is Bob's payload and becomes
    // `alice_scanned_bob`. Symmetric for Bob.
    let alice_scanned_bob = ExchangeQR::from_data_string(&alice_decoded)
        .expect("bob's payload should parse back into ExchangeQR");
    let bob_scanned_alice = ExchangeQR::from_data_string(&bob_decoded)
        .expect("alice's payload should parse back into ExchangeQR");

    // Phase 5: drive the state machine through to Complete (X3DH + HKDF)
    alice_session
        .apply(ExchangeEvent::ProcessQR(alice_scanned_bob))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(bob_scanned_alice))
        .unwrap();
    alice_session
        .apply(ExchangeEvent::TheyScannedOurQR)
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    alice_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    alice_session
        .apply(ExchangeEvent::CompleteExchange(bob_card))
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    let elapsed_ms = start.elapsed().as_millis();

    // Sanity: both sessions actually reached Complete.
    assert!(
        matches!(alice_session.state(), ExchangeState::Complete { .. }),
        "alice should reach Complete"
    );
    assert!(
        matches!(bob_session.state(), ExchangeState::Complete { .. }),
        "bob should reach Complete"
    );

    // Emit for regression tracking / baseline doc updates.
    eprintln!(
        "D5 exchange latency (algorithmic, no camera/network): {} ms \
         (budget {} ms; user-perceived target 3000 ms)",
        elapsed_ms, CORE_ALGORITHMIC_BUDGET_MS
    );

    assert!(
        elapsed_ms < CORE_ALGORITHMIC_BUDGET_MS,
        "D5 exchange latency {} ms exceeds core-algorithmic budget {} ms — \
         regressed the QR decode pipeline, crypto, or state-machine transitions. \
         (User-perceived target is 3000 ms; core must stay under {} ms to leave \
         headroom for device camera focus + relay RTT + UI render.)",
        elapsed_ms,
        CORE_ALGORITHMIC_BUDGET_MS,
        CORE_ALGORITHMIC_BUDGET_MS
    );
}

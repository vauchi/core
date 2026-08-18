// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Hover driven through the *shell-facing* seam only
//! (`2026-08-18-hover-decodes-the-peer-qr-but-never-advances`).
//!
//! Every other multi-stage test reaches past that seam by calling
//! `forward_multi_stage_hardware_event` directly, so all of them passed
//! while a real Pixel 3a decoded an iPhone's `INI2` frame 1146 times
//! without the exchange ever advancing. A shell cannot call that method:
//! it renders `Command::ReplaceSurface` and reports
//! `Event::ValueChanged` on the capture node's opaque binding (ADR-021 /
//! ADR-066 — no domain vocabulary crosses the boundary). These tests use
//! that route and nothing else.

#![cfg(feature = "testing")]

use std::sync::Arc;
use std::time::{Duration, SystemTime};
use vauchi_app::orchestrator::multi_stage_machine::MultiStagePhase;
use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::platform::{
    BindingId, Command, InputValue, PresentationNode, PresentationQrPurpose, SurfaceId, SurfaceSpec,
};
use vauchi_core::{Event, api::Vauchi};

/// A device sitting on Hover with a clock the test can step.
fn engine_on_hover(name: &str, clock: Arc<dyn Clock>) -> AppEngine {
    let mut vauchi = Vauchi::in_memory_with_clock(clock).expect("in-memory Vauchi");
    vauchi.create_identity(name).expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:hover".into(),
    });
    engine
}

/// The last surface the engine told the shell to render.
fn latest_surface(commands: &[Command]) -> Option<SurfaceSpec> {
    commands.iter().rev().find_map(|c| match c {
        Command::ReplaceSurface { surface } => Some(surface.clone()),
        _ => None,
    })
}

/// Hover nests the capture node inside the preview `Group`, so a flat
/// scan of `surface.nodes` would miss it.
fn qr_nodes(surface: &SurfaceSpec) -> Vec<(BindingId, PresentationQrPurpose, Vec<String>)> {
    fn walk(
        nodes: &[PresentationNode],
        out: &mut Vec<(BindingId, PresentationQrPurpose, Vec<String>)>,
    ) {
        for node in nodes {
            match node {
                PresentationNode::Qr {
                    id,
                    purpose,
                    payloads,
                    ..
                } => out.push((id.clone(), *purpose, payloads.clone())),
                PresentationNode::Group { children, .. } => walk(children, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&surface.nodes, &mut out);
    out
}

/// What the peer's camera would see: the payload of the display QR.
fn displayed_qr(surface: &SurfaceSpec) -> Option<String> {
    qr_nodes(surface)
        .into_iter()
        .find(|(_, purpose, _)| matches!(purpose, PresentationQrPurpose::Display))
        .and_then(|(_, _, payloads)| payloads.first().cloned())
}

/// Where a shell reports a decode: the capture QR's binding.
fn capture_binding(surface: &SurfaceSpec) -> Option<BindingId> {
    qr_nodes(surface)
        .into_iter()
        .find(|(_, purpose, _)| matches!(purpose, PresentationQrPurpose::Capture))
        .map(|(id, _, _)| id)
}

/// Report one decoded frame exactly as a shell does — an opaque text
/// value on the capture node's binding. No domain event, no direct
/// machine call. Returns the command batch the shell would render.
fn report_decode(
    engine: &mut AppEngine,
    surface_id: &SurfaceId,
    binding: &BindingId,
    qr: String,
) -> Vec<Command> {
    engine
        .dispatch(Event::ValueChanged {
            surface_id: surface_id.clone(),
            binding_id: binding.clone(),
            value: InputValue::Text(qr),
        })
        .unwrap_or_default()
}

fn is_celebrate(cmd: &Command) -> bool {
    matches!(cmd, Command::Celebrate { .. })
}

/// A decode reported on the capture binding must move the protocol off
/// `Advertising`.
///
/// The discriminating assertion: without the seam wiring the engine
/// still returns a fresh, well-formed surface, so any shape-level check
/// would pass. Only the phase distinguishes "consumed" from "parsed and
/// dropped" — which is exactly what the Pixel did 1146 times.
// @scenario: exchange :: a reported decode advances Hover past advertising
#[test]
fn a_reported_decode_advances_hover_past_advertising() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let clock_a = Arc::new(FakeClock::new(start));
    let clock_b = Arc::new(FakeClock::new(start));
    let mut alice = engine_on_hover("Alice", clock_a);
    let mut bob = engine_on_hover("Bob", clock_b);

    alice.poll_notifications();
    bob.poll_notifications();

    let a_surface = latest_surface(&alice.initial_commands().expect("alice commands"))
        .expect("Alice renders a surface");
    let b_surface =
        latest_surface(&bob.initial_commands().expect("bob commands")).expect("Bob renders");

    let bob_qr = displayed_qr(&b_surface).expect("Bob displays an INIT frame");
    let binding = capture_binding(&a_surface).expect("Alice renders a capture node");

    let before = alice.multi_stage_phase();
    report_decode(&mut alice, &a_surface.surface_id, &binding, bob_qr);
    let after = alice.multi_stage_phase();

    assert!(
        matches!(before, Some(MultiStagePhase::Advertising)),
        "Alice starts out advertising her own frame, got {before:?}"
    );
    assert!(
        !matches!(after, Some(MultiStagePhase::Advertising)),
        "reporting Bob's decoded frame must move Alice's machine on; staying in \
         Advertising is the bug — the payload is parsed and then dropped, got {after:?}"
    );
}

/// Two Hover devices aimed at each other, each decoding whatever the
/// peer displays, reach a persisted success — driven only through
/// `ReplaceSurface` out and `ValueChanged` in.
///
/// This is the hardware scenario that failed 2026-08-18:
/// `[QrScan] decoded type=INI2 len=139` repeating for 93 s against one
/// lone `[Exchange] started: Hover`.
// @scenario: exchange :: Hover completes when the shell reports decodes
#[test]
fn hover_completes_when_scans_arrive_through_the_presentation_seam() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let clock_a = Arc::new(FakeClock::new(start));
    let clock_b = Arc::new(FakeClock::new(start));
    let mut alice = engine_on_hover("Alice", clock_a.clone());
    let mut bob = engine_on_hover("Bob", clock_b.clone());

    let mut alice_celebrates = 0;
    let mut bob_celebrates = 0;

    for _ in 0..600 {
        alice.poll_notifications();
        bob.poll_notifications();

        let a_cmds = alice.initial_commands().expect("alice commands");
        let b_cmds = bob.initial_commands().expect("bob commands");
        alice_celebrates += a_cmds.iter().filter(|c| is_celebrate(c)).count();
        bob_celebrates += b_cmds.iter().filter(|c| is_celebrate(c)).count();

        if let (Some(a), Some(b)) = (latest_surface(&a_cmds), latest_surface(&b_cmds)) {
            // Each side decodes whatever the peer is currently showing.
            if let (Some(binding), Some(peer_qr)) = (capture_binding(&a), displayed_qr(&b)) {
                let out = report_decode(&mut alice, &a.surface_id, &binding, peer_qr);
                alice_celebrates += out.iter().filter(|c| is_celebrate(c)).count();
            }
            if let (Some(binding), Some(peer_qr)) = (capture_binding(&b), displayed_qr(&a)) {
                let out = report_decode(&mut bob, &b.surface_id, &binding, peer_qr);
                bob_celebrates += out.iter().filter(|c| is_celebrate(c)).count();
            }
        }

        if alice.vauchi().contact_count().unwrap_or(0) > 0
            && bob.vauchi().contact_count().unwrap_or(0) > 0
        {
            break;
        }

        clock_a.advance(Duration::from_millis(500));
        clock_b.advance(Duration::from_millis(500));
    }

    assert_eq!(
        alice.vauchi().contact_count().unwrap_or(0),
        1,
        "Alice must have stored Bob's card — she reported his frames through the only \
         route a shell has. Zero is the device symptom: decode succeeds, nothing consumes it."
    );
    assert_eq!(
        bob.vauchi().contact_count().unwrap_or(0),
        1,
        "Bob must have stored Alice's card for the same reason"
    );
    assert_eq!(
        (alice_celebrates, bob_celebrates),
        (1, 1),
        "each side celebrates exactly once on validated success"
    );
}

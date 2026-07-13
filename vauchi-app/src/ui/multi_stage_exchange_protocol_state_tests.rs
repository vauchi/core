// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-`ProtocolState` rendering tests for `multi_stage_exchange.rs`.
//! Split out of `multi_stage_exchange_tests.rs` (tidy, M3 S5-10) to
//! keep that file under the 1200-line test hard limit.

use super::*;

// @internal
#[test]
fn idle_emits_show_this_label_with_peer_scanner() {
    let engine = MultiStageExchangeEngine::new_glance();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, SCREEN_ID);
    // Idle without a QR payload yet — no own_qr component.
    assert!(
        !screen
            .components
            .iter()
            .any(|c| matches!(c, Component::QrCode { id, .. } if id == COMPONENT_ID_OWN_QR)),
    );
    // Peer scanner is always present in Active rendering (now inside
    // the preview Row, alongside the action buttons).
    assert!(
        find_peer_scan(&screen).is_some(),
        "Idle must compose camera scanner"
    );
    // The active screen no longer emits a StatusIndicator — the own-QR
    // label carries the status. In Idle that caption is "Show this".
    assert_eq!(
        own_qr_label(&ProtocolState::Idle, Locale::English),
        "Show this"
    );
    assert_eq!(
        action_ids(&screen),
        vec![SWITCH_CAMERA_ACTION_ID, CANCEL_ACTION_ID]
    );
}

// @internal
#[test]
fn advertising_renders_active_with_qr_when_payload_present() {
    let engine = engine_with_qr(ProtocolState::Advertising, "vauchi://INIT/abc");
    let screen = engine.current_screen();
    let has_own_qr = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::QrCode { id, mode: QrMode::Display, data, .. }
                if id == COMPONENT_ID_OWN_QR && data == "vauchi://INIT/abc",
        )
    });
    assert!(has_own_qr, "Advertising with payload must render own QR");
}

// The own-QR ships its payload in the opaque `frames` carrier (ADR-044
// Amendment 2a C2a) so animated QR is render data, not a frontend
// behavior. The engine holds one live frame at a time, so `frames` is
// that single frame; `data` stays populated for the pre-migration path.
// @internal
#[test]
fn active_own_qr_ships_current_frame_in_frames_carrier() {
    let engine = engine_with_qr(ProtocolState::Advertising, "vauchi://INIT/abc");
    let screen = engine.current_screen();
    let own_qr_frames = screen.components.iter().find_map(|c| match c {
        Component::QrCode { id, frames, .. } if id == COMPONENT_ID_OWN_QR => Some(frames.clone()),
        _ => None,
    });
    assert_eq!(
        own_qr_frames,
        Some(vec!["vauchi://INIT/abc".to_string()]),
        "own-QR must carry the current payload as a single opaque frame"
    );
}

// @internal
#[test]
fn discovered_state_narrates_starting_exchange() {
    assert_eq!(
        own_qr_label(&ProtocolState::Discovered, Locale::English),
        "Connecting…"
    );
}

// @internal
#[test]
fn transferring_state_includes_chunk_progress() {
    assert_eq!(
        own_qr_label(
            &ProtocolState::Transferring {
                chunks_sent: 3,
                chunks_total: 7,
                chunks_received: 5,
                peer_chunks_total: 9,
            },
            Locale::English
        ),
        "Transferring 3/7",
    );
}

// @internal
#[test]
fn transferring_with_zero_totals_omits_progress_detail() {
    assert_eq!(
        own_qr_label(
            &ProtocolState::Transferring {
                chunks_sent: 0,
                chunks_total: 0,
                chunks_received: 0,
                peer_chunks_total: 0,
            },
            Locale::English
        ),
        "Transferring…",
        "all-zero totals must omit the progress fraction",
    );
}

// @internal
#[test]
fn verifying_state_narrates_verifying() {
    assert_eq!(
        own_qr_label(&ProtocolState::Verifying, Locale::English),
        "Verifying…"
    );
}

// @internal
#[test]
fn confirming_state_narrates_confirming() {
    assert_eq!(
        own_qr_label(&ProtocolState::Confirming, Locale::English),
        "Confirming…"
    );
}

// @internal
#[test]
fn complete_before_session_ended_keeps_active_chrome() {
    let engine = engine_with_state(ProtocolState::Complete);
    // The active own-QR caption reads "Almost done" while Complete
    // before the session ends.
    assert_eq!(
        own_qr_label(&ProtocolState::Complete, Locale::English),
        "Almost done"
    );
    // Still active — switch_camera + cancel.
    assert_eq!(
        action_ids(&engine.current_screen()),
        vec![SWITCH_CAMERA_ACTION_ID, CANCEL_ACTION_ID],
    );
}

// @internal
#[test]
fn finalized_after_session_ended_renders_success() {
    let mut engine = engine_with_state(ProtocolState::Finalized);
    engine.set_finalized("Alice".into());
    engine.set_session_ended();
    let screen = engine.current_screen();
    let has_success = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::StatusIndicator { title, status: Status::Success, .. }
                if title == "Exchange Complete",
        )
    });
    assert!(
        has_success,
        "session_ended Finalized must show success indicator"
    );
    let has_name = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::Text { content, .. } if content == "Exchanged with Alice",
        )
    });
    assert!(has_name, "success screen must include peer name");
    assert_eq!(action_ids(&screen), vec![DONE_ACTION_ID]);
}

// @internal
#[test]
fn finalized_before_session_ended_shows_success_with_qr_broadcast() {
    // The contact is persisted at Finalized; the FINALIZED_GRACE
    // broadcast that follows exists only for the peer (two-generals
    // last-ack: a still-Complete peer needs our RDYY). The user must
    // see Success immediately — with the own-QR still broadcasting
    // under a keep-facing caption — instead of parking on
    // "Almost done" for the whole grace window
    // (2026-07-01-hover-exchange-completion-latency).
    let mut engine = engine_with_qr(ProtocolState::Finalized, "GRACE-QR");
    engine.set_finalized("Alice".into());
    let screen = engine.current_screen();

    let has_success = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::StatusIndicator { title, status: Status::Success, .. }
                if title == "Exchange Complete",
        )
    });
    assert!(
        has_success,
        "Finalized before session end must already show the success indicator"
    );

    // The peer's camera must always see the QR: FIRST component on a
    // fixed (non-scrolling) layout — same pinned-QR contract as the
    // active screen. Appending it below a scrollable success summary
    // would push it below the fold exactly when the peer needs it.
    let broadcast_qr = match screen.components.first() {
        Some(Component::QrCode {
            data,
            mode: QrMode::Display,
            label,
            ..
        }) => Some((data.clone(), label.clone())),
        _ => None,
    };
    assert_eq!(
        broadcast_qr,
        Some((
            "GRACE-QR".to_string(),
            Some("Keep screens facing each other until the other phone finishes".to_string())
        )),
        "the own-QR must keep broadcasting FIRST on the grace screen"
    );
    assert_eq!(
        screen.layout,
        ScreenLayout::Fixed,
        "the grace screen must not scroll — the QR must stay visible"
    );

    assert!(
        !screen.components.iter().any(|c| matches!(
            c,
            Component::QrCode {
                mode: QrMode::Scan,
                ..
            }
        )),
        "post-Finalized scans are no-ops — the camera must be dropped"
    );
    assert_eq!(action_ids(&screen), vec![DONE_ACTION_ID]);
    let done_style = screen
        .actions
        .iter()
        .find(|a| a.id == DONE_ACTION_ID)
        .map(|a| a.style.clone());
    assert_eq!(
        done_style,
        Some(ActionStyle::Secondary),
        "Done tears the broadcast down early — de-emphasize it while \
         the caption asks the user to hold position"
    );
}

// @internal
#[test]
fn grace_screen_defers_rich_summary_until_session_ends() {
    // The production path attaches the rich success summary at the
    // same Finalized event. During the grace the QR must win the
    // viewport (Fixed layout cannot scroll) — the summary renders
    // once session_ended drops the strip.
    let mut engine = engine_with_qr(ProtocolState::Finalized, "GRACE-QR");
    engine.set_finalized("Bob".into());
    engine.set_success_summary(crate::ui::exchange::success::ExchangeSuccessSummary {
        peer_name: "Bob".into(),
        received_fields: vec![("email".into(), "Email".into(), "bob@example.com".into())],
        my_visible_fields: vec!["Phone".into()],
        group_names: Vec::new(),
    });

    let grace = engine.current_screen();
    assert!(
        matches!(
            grace.components.first(),
            Some(Component::QrCode {
                mode: QrMode::Display,
                ..
            })
        ),
        "with a summary attached the QR must still be the first component"
    );
    assert!(
        !grace
            .components
            .iter()
            .any(|c| matches!(c, Component::FieldList { .. })),
        "the rich summary is deferred while the broadcast runs"
    );

    engine.set_session_ended();
    let ended = engine.current_screen();
    assert!(
        ended.components.iter().any(|c| matches!(
            c,
            Component::FieldList { id, .. } if id == "received_fields"
        )),
        "once the grace ends the rich summary renders"
    );
}

// @internal
#[test]
fn finalized_after_session_ended_drops_qr_broadcast() {
    // session_ended (grace expiry) is the stop condition: the QR
    // strip disappears from the success screen.
    let mut engine = engine_with_qr(ProtocolState::Finalized, "GRACE-QR");
    engine.set_finalized("Alice".into());
    engine.set_session_ended();
    let screen = engine.current_screen();
    assert!(
        !screen
            .components
            .iter()
            .any(|c| matches!(c, Component::QrCode { .. })),
        "after the grace expires the success screen carries no QR"
    );
}

// @internal
#[test]
fn finalized_with_session_ended_but_no_name_falls_back() {
    let mut engine = engine_with_state(ProtocolState::Finalized);
    engine.set_session_ended();
    let screen = engine.current_screen();
    let has_fallback = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::Text { content, .. } if content == "Exchange complete.",
        )
    });
    assert!(
        has_fallback,
        "missing peer name must fall back to generic copy"
    );
}

// @internal
#[test]
fn failed_state_renders_retry_and_cancel() {
    let engine = engine_with_state(ProtocolState::Failed("timeout".into()));
    let screen = engine.current_screen();
    match first_status_indicator(&screen).unwrap() {
        Component::StatusIndicator {
            title,
            detail,
            status,
            ..
        } => {
            assert_eq!(title, "Exchange Failed");
            assert_eq!(detail.as_deref(), Some("timeout"));
            assert_eq!(*status, Status::Failed);
        }
        _ => unreachable!(),
    }
    assert_eq!(action_ids(&screen), vec![RETRY_ACTION_ID, CANCEL_ACTION_ID]);
}

// Direct full-mapping coverage of the own-QR caption helper (CC-03:
// exact-value asserts on every arm). The active screen folds this
// string into the own-QR `label`; per-state tests above pin the
// engine-side wiring, this pins the pure mapping including the
// non-exhaustive fallback.
// @internal
#[test]
fn own_qr_label_maps_every_protocol_state() {
    assert_eq!(
        own_qr_label(&ProtocolState::Idle, Locale::English),
        "Show this"
    );
    assert_eq!(
        own_qr_label(&ProtocolState::Advertising, Locale::English),
        "Show this"
    );
    assert_eq!(
        own_qr_label(&ProtocolState::Discovered, Locale::English),
        "Connecting…"
    );
    assert_eq!(
        own_qr_label(
            &ProtocolState::Transferring {
                chunks_sent: 2,
                chunks_total: 5,
                chunks_received: 1,
                peer_chunks_total: 5,
            },
            Locale::English
        ),
        "Transferring 2/5",
    );
    assert_eq!(
        own_qr_label(
            &ProtocolState::Transferring {
                chunks_sent: 0,
                chunks_total: 0,
                chunks_received: 0,
                peer_chunks_total: 0,
            },
            Locale::English
        ),
        "Transferring…",
    );
    assert_eq!(
        own_qr_label(&ProtocolState::Verifying, Locale::English),
        "Verifying…"
    );
    assert_eq!(
        own_qr_label(&ProtocolState::Confirming, Locale::English),
        "Confirming…"
    );
    assert_eq!(
        own_qr_label(&ProtocolState::Complete, Locale::English),
        "Almost done"
    );
    assert_eq!(
        own_qr_label(&ProtocolState::RetryReady, Locale::English),
        "Almost done"
    );
    assert_eq!(
        own_qr_label(&ProtocolState::Finalized, Locale::English),
        "Almost done"
    );
    assert_eq!(
        own_qr_label(&ProtocolState::Failed("boom".into()), Locale::English),
        "Exchange failed",
    );
}

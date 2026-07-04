// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end exchange mode tests — full flows through ExchangeEngine.
//!
//! Tests the complete user journey: mode selection → group selection →
//! field preview → mode-specific sub-flow → result.

use vauchi_app::ui::*;
use vauchi_core::exchange::mode::ExchangeMode;

fn config_with_mode_selection() -> ExchangeConfig {
    ExchangeConfig {
        own_name: "Alice".to_string(),
        own_qr_data: "alice-qr-payload".to_string(),
        available_groups: vec![
            ("g1".to_string(), "Family".to_string()),
            ("g2".to_string(), "Friends".to_string()),
        ],
        device_capabilities: vauchi_core::exchange::capability::types::DeviceCapabilities {
            has_camera: true,
            has_internet: true,
            ..Default::default()
        },
        mode: None, // triggers mode selection
        last_used_group_ids: None,
        last_used_mode: None,
        card_snapshot: None,
        transport_readiness: Default::default(),
        available_group_data: Vec::new(),
    }
}

// ================================================================
// Glance: full flow
// ================================================================

// @internal
#[test]
fn glance_full_flow_mode_to_ble_handoff() {
    let mut engine = ExchangeEngine::new(
        config_with_mode_selection(),
        vauchi_core::clock::SystemClock::shared(),
    );

    // Step 1: G4 group-first — starts on group selection.
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_group_selection");

    // Step 2: select a group, Continue → mode selection. (M2 S7: the
    // unified button keys the preview off the selection, so an empty
    // selection would take the Skip path.)
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_picker".to_string(),
        item_id: "g1".to_string(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_mode_selection");

    // Step 3: Select Glance → field preview (continued with groups).
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".to_string(),
        item_id: "mode:glance".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_field_preview");

    // Step 5: Start exchange → Glance hands off to the multi-stage
    // engine; it no longer drives the legacy QR sub-flow on this
    // ExchangeEngine. The grouped path now matches the no-groups path —
    // before `2026-06-02-grouped-mode-routing-nfc` the group/field-preview
    // resume incorrectly collapsed Glance (and TapTap, Hover) back onto
    // `exchange_show_qr`. The end-to-end legacy-QR journey is covered by
    // `broadcast_full_flow_mode_to_qr_to_result`.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "start_exchange".to_string(),
    });
    assert_eq!(
        result,
        ActionResult::StartBleExchange {
            mode: ExchangeMode::Glance
        }
    );
}

// ================================================================
// Broadcast: full flow on the legacy QR sub-flow
// ================================================================
//
// Glance + Hover now both hand off to `MultiStageExchange`
// (Pair 4 + Phase 1.E of `2026-05-11-hover-graduation-plan.md`)
// and so leave the Exchange engine entirely; the Hover handoff is
// pinned by `exchange.rs::tests::hover_mode_routes_through_multi_stage_handoff`.
// Broadcast (one-to-many QR) is the next QR-legacy mode in line for
// graduation — until then it's what this test needs to exercise
// the full legacy ExchangeEngine flow end-to-end.

// @internal
#[test]
fn tap_hover_shake_mode_pick_hands_off_to_multi_stage() {
    // TapHoverShake graduated to the multi-stage engine (P2.D of the
    // TapHoverShake graduation plan): picking it then skipping groups hands
    // off via ActionResult::StartMultiStageExchange instead of the retired
    // legacy ExchangeEngine QR walk. No QR mode remains on the legacy
    // ExchangeEngine path (Broadcast was removed), so the old full-QR-flow
    // walk this test used to exercise is no longer reachable here.
    let mut engine = ExchangeEngine::new(
        config_with_mode_selection(),
        vauchi_core::clock::SystemClock::shared(),
    );

    // G4 group-first: skip groups → mode selection.
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".to_string(),
    });

    // Select TapHoverShake → multi-stage handoff (no groups → no preview).
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:fun".to_string(),
        item_id: "mode:tap_hover_shake".to_string(),
    });
    assert!(
        matches!(
            result,
            ActionResult::StartMultiStageExchange {
                mode: vauchi_core::exchange::mode::ExchangeMode::TapHoverShake,
            },
        ),
        "TapHoverShake must hand off to multi-stage; got {result:?}",
    );
}

// ================================================================
// Link: mode-pick + field-preview hand off to LinkExchangeEngine
//
// The link-mode initiator flow (share-url / waiting / retrieving /
// terminal screens, escrow polling, card decrypt) graduated to the
// pure `LinkExchangeEngine` + engine-owned `LinkInitiatorSession`
// (slice 32l Phase 3b). `ExchangeEngine` no longer enters a Link
// sub-flow — it hands off via `ActionResult::StartLinkExchange`,
// which AppEngine routes to construct the new engine. The full
// share → waiting → retrieving → terminal flow is covered in
// `vauchi-app/tests/reachability/link_exchange.rs` and the
// `link_initiator` unit tests.
// ================================================================

// @internal
#[test]
fn link_full_flow_mode_to_field_preview_hands_off() {
    let mut engine = ExchangeEngine::new(
        config_with_mode_selection(),
        vauchi_core::clock::SystemClock::shared(),
    );

    // G4 group-first: starts on group selection.
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_group_selection");

    // Select a group, Continue → mode selection (M2 S7: selection-keyed).
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_picker".to_string(),
        item_id: "g1".to_string(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_mode_selection");

    // Select Link → field preview (continued with groups).
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:remote".to_string(),
        item_id: "mode:link".to_string(),
    });
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "exchange_field_preview");

    // Start exchange → hand off to LinkExchangeEngine
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "start_exchange".to_string(),
    });
    assert_eq!(
        result,
        ActionResult::StartLinkExchange,
        "Start exchange in Link mode must hand off to LinkExchangeEngine"
    );
}

// ================================================================
// Cross-mode: failed + retry preserves mode
// ================================================================

// @internal
#[test]
fn failed_retry_preserves_glance_mode() {
    let mut engine = ExchangeEngine::new(
        ExchangeConfig {
            mode: Some(ExchangeMode::Glance),
            ..config_with_mode_selection()
        },
        vauchi_core::clock::SystemClock::shared(),
    );
    engine.mark_failed();
    // Glance retry hands back to the multi-stage engine (mode preserved),
    // not the legacy QR step — Retry routes through `enter_mode_sub_flow`
    // like the forward path (glance_full_flow_mode_to_multistage_handoff).
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "retry".to_string(),
    });
    assert_eq!(
        result,
        ActionResult::StartBleExchange {
            mode: ExchangeMode::Glance
        }
    );
}

// @internal
#[test]
fn failed_retry_preserves_link_mode() {
    let mut engine = ExchangeEngine::new(
        ExchangeConfig {
            mode: Some(ExchangeMode::Link),
            available_groups: vec![],
            ..config_with_mode_selection()
        },
        vauchi_core::clock::SystemClock::shared(),
    );
    engine.mark_failed();
    // Retry in Link mode hands off to LinkExchangeEngine rather than
    // re-entering a Link sub-flow on this engine.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "retry".to_string(),
    });
    assert_eq!(result, ActionResult::StartLinkExchange);
}

// ================================================================
//
// Link-flow progress (share-url → waiting → retrieving → terminal)
// graduated to `LinkExchangeEngine::progress`; its per-screen
// progression is covered by `vauchi-app/tests/reachability/link_exchange.rs`.
// ================================================================

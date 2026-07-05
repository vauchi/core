// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M4 S1a (`2026-07-03-placebo-accessibility-toggles`): the
//! `reduce_motion` and `large_touch` accessibility flags take real
//! effect as a pure `DesignTokens` transform applied at the single
//! render choke point `AppEngine::current_screen()`. Category-2
//! core-owned per ADR-047 Addendum 2026-07-05.
//!
//! Exact-value assertions (CC-03) from the bundled token defaults:
//! motion `200/150/300`, `touch_target.minimum = 44`,
//! `list_item_*` = `8/8/12/12`. Large-touch scale is `v*3/2` (1.5x).

use vauchi_app::theme::{DesignTokens, apply_accessibility_tokens};
use vauchi_app::ui::{AppEngine, WorkflowEngine};
use vauchi_core::api::Vauchi;

// ── Pure transform: exact values ────────────────────────────────────

// @internal
#[test]
fn no_flags_leaves_tokens_unchanged() {
    let base = DesignTokens::default();
    let out = apply_accessibility_tokens(base.clone(), false, false);
    assert_eq!(out, base);
}

// @internal
#[test]
fn reduce_motion_zeroes_all_motion_durations_only() {
    let base = DesignTokens::default();
    let out = apply_accessibility_tokens(base.clone(), true, false);
    assert_eq!(out.motion.enter_duration_ms, 0);
    assert_eq!(out.motion.exit_duration_ms, 0);
    assert_eq!(out.motion.emphasis_duration_ms, 0);
    // Touch/spacing untouched by reduce_motion.
    assert_eq!(out.touch_target.minimum, base.touch_target.minimum);
    assert_eq!(
        out.spacing_direction.list_item_start,
        base.spacing_direction.list_item_start
    );
}

// @internal
#[test]
fn large_touch_scales_touch_and_list_spacing_only() {
    let base = DesignTokens::default();
    // Guard the fixture defaults so the scaled expectations below are anchored.
    assert_eq!(base.touch_target.minimum, 44);
    assert_eq!(base.spacing_direction.list_item_start, 8);
    assert_eq!(base.spacing_direction.list_item_end, 8);
    assert_eq!(base.spacing_direction.list_item_inline_start, 12);
    assert_eq!(base.spacing_direction.list_item_inline_end, 12);

    let out = apply_accessibility_tokens(base.clone(), false, true);
    assert_eq!(out.touch_target.minimum, 66, "44 * 3 / 2");
    assert_eq!(out.spacing_direction.list_item_start, 12, "8 * 3 / 2");
    assert_eq!(out.spacing_direction.list_item_end, 12);
    assert_eq!(
        out.spacing_direction.list_item_inline_start, 18,
        "12 * 3 / 2"
    );
    assert_eq!(out.spacing_direction.list_item_inline_end, 18);
    // Motion untouched by large_touch.
    assert_eq!(out.motion.enter_duration_ms, base.motion.enter_duration_ms);
}

// @internal
#[test]
fn both_flags_compose() {
    let base = DesignTokens::default();
    let out = apply_accessibility_tokens(base, true, true);
    assert_eq!(out.motion.enter_duration_ms, 0);
    assert_eq!(out.motion.exit_duration_ms, 0);
    assert_eq!(out.motion.emphasis_duration_ms, 0);
    assert_eq!(out.touch_target.minimum, 66);
    assert_eq!(out.spacing_direction.list_item_inline_end, 18);
}

// ── Overlay wiring: config flag → rendered ScreenModel.tokens ────────

// @internal
#[test]
fn current_screen_applies_reduce_motion_from_config() {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    // Default: motion intact on the rendered screen.
    let before = engine.current_screen();
    assert_eq!(before.tokens.motion.enter_duration_ms, 200);

    engine.vauchi_mut().config_mut().reduce_motion = true;
    let after = engine.current_screen();
    assert_eq!(after.tokens.motion.enter_duration_ms, 0);
    assert_eq!(after.tokens.motion.exit_duration_ms, 0);
    assert_eq!(after.tokens.motion.emphasis_duration_ms, 0);
}

// @internal
#[test]
fn current_screen_applies_large_touch_from_config() {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    let before = engine.current_screen();
    assert_eq!(before.tokens.touch_target.minimum, 44);

    engine.vauchi_mut().config_mut().large_touch = true;
    let after = engine.current_screen();
    assert_eq!(after.tokens.touch_target.minimum, 66);
    assert_eq!(after.tokens.spacing_direction.list_item_inline_start, 18);
}

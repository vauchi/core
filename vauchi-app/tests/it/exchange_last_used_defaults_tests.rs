// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M2 S1 — last-used exchange defaults (design D2.1,
//! `2026-07-03-one-tap-exchange` goal 1): a repeat exchange skips the G4
//! group gate, opens straight on the mode picker with the prior groups
//! pre-applied and visible as a "Sharing: … " chip (Banner), and each
//! mode-commit persists the new defaults. First-run behavior (G4
//! group-first) is pinned unchanged.

use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::types::ExchangeDefaults;

/// In-memory Vauchi with an identity and a "Work" group. Returns the
/// Vauchi (pre-AppEngine, so tests can seed storage) and the group id.
fn vauchi_with_work_group() -> (Vauchi, String) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let work = vauchi.create_group("Work").expect("create group");
    let work_id = work.id().to_string();
    (vauchi, work_id)
}

fn sharing_chip(engine: &AppEngine) -> Option<(String, String)> {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::Banner {
                text, action_id, ..
            } if action_id == "sharing_chip" => Some((text.clone(), action_id.clone())),
            _ => None,
        })
}

// First run (groups exist, no stored defaults): the G4 group-first gate
// is unchanged. Pins the existing behavior S1 must not regress.
// @scenario: exchange :: first exchange with groups starts on group selection
// @internal
#[test]
fn first_run_with_groups_still_gates_on_group_selection() {
    let (vauchi, _work) = vauchi_with_work_group();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_group_selection",
        "no stored defaults: G4 group-first gate must be unchanged"
    );
}

// Repeat run: stored defaults skip the gate; the mode picker opens with
// the prior groups visible on the sharing chip.
// @scenario: exchange :: repeat exchange opens the picker with prior groups
// @internal
#[test]
fn repeat_run_skips_gate_and_shows_sharing_chip() {
    let (vauchi, work_id) = vauchi_with_work_group();
    vauchi
        .storage()
        .ux()
        .save_exchange_defaults(&ExchangeDefaults {
            group_ids: vec![work_id],
            mode: ExchangeMode::Hover,
        })
        .expect("seed defaults");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);

    assert_eq!(
        engine.current_screen().screen_id,
        // The mode-selection ROOT carries the canonical `exchange` id
        // (apply_screen_id_metadata) so frontends show the nav bar.
        "exchange",
        "stored defaults must skip the group gate"
    );
    let (text, _) = sharing_chip(&engine).expect("sharing chip present on repeat run");
    assert_eq!(text, "Sharing: Work", "chip names the pre-applied groups");
}

// The chip is a refinement entry: pressing it opens group selection;
// Continue returns to the mode picker.
// @scenario: exchange :: sharing chip opens group selection as a refinement
// @internal
#[test]
fn sharing_chip_opens_group_selection_and_continue_returns() {
    let (vauchi, work_id) = vauchi_with_work_group();
    vauchi
        .storage()
        .ux()
        .save_exchange_defaults(&ExchangeDefaults {
            group_ids: vec![work_id],
            mode: ExchangeMode::Hover,
        })
        .expect("seed defaults");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "sharing_chip".into(),
    });
    assert_eq!(
        engine.current_screen().screen_id,
        "exchange_group_selection",
        "chip opens group selection as an opt-in refinement"
    );
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    // Continue arms the field preview after mode selection (existing
    // behavior) — the immediate screen is the mode picker again.
    assert_eq!(
        engine.current_screen().screen_id,
        // The mode-selection ROOT carries the canonical `exchange` id
        // (apply_screen_id_metadata) so frontends show the nav bar.
        "exchange",
        "Continue returns to the mode picker"
    );
}

// Committing to a mode persists the (groups, mode) pair as the new
// defaults — the write seam of D2.1.
// @scenario: exchange :: picking a mode persists last-used defaults
// @internal
#[test]
fn mode_commit_persists_last_used_defaults() {
    let (vauchi, work_id) = vauchi_with_work_group();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);

    // First run: through the gate (select Work, continue), then pick Hover.
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_picker".into(),
        item_id: work_id.clone(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:hover".into(),
    });
    // "continue" armed the field preview; the commit fires on its
    // start_exchange action (the moment the user actually starts).
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "start_exchange".into(),
    });

    let stored = engine
        .vauchi()
        .storage()
        .ux()
        .load_exchange_defaults()
        .expect("load")
        .expect("defaults persisted on mode commit");
    assert_eq!(stored.mode, ExchangeMode::Hover, "mode persisted exactly");
    assert_eq!(
        stored.group_ids,
        vec![work_id],
        "selected groups persisted exactly"
    );
}

// A stored group that no longer exists is filtered out; the gate stays
// skipped (the user is a repeat user) and the chip falls back honestly.
// @scenario: exchange :: deleted default group degrades gracefully
// @internal
#[test]
fn deleted_default_group_is_filtered_not_fatal() {
    let (vauchi, _work) = vauchi_with_work_group();
    vauchi
        .storage()
        .ux()
        .save_exchange_defaults(&ExchangeDefaults {
            group_ids: vec!["gone-group-id".into()],
            mode: ExchangeMode::Glance,
        })
        .expect("seed defaults");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);

    assert_eq!(
        engine.current_screen().screen_id,
        // The mode-selection ROOT carries the canonical `exchange` id
        // (apply_screen_id_metadata) so frontends show the nav bar.
        "exchange",
        "repeat user: gate skipped even when the stored group is gone"
    );
    let (text, _) = sharing_chip(&engine).expect("chip still present");
    assert_eq!(
        text, "Sharing: default card",
        "chip falls back to the default-card label"
    );
}

// Storage round-trip exactness for the new UxStore surface (CC-03).
// @internal
#[test]
fn exchange_defaults_round_trip_exactly() {
    let vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    let defaults = ExchangeDefaults {
        group_ids: vec!["a".into(), "b".into()],
        mode: ExchangeMode::TapHoverShake,
    };
    vauchi
        .storage()
        .ux()
        .save_exchange_defaults(&defaults)
        .expect("save");
    let loaded = vauchi
        .storage()
        .ux()
        .load_exchange_defaults()
        .expect("load")
        .expect("present");
    assert_eq!(loaded.group_ids, defaults.group_ids);
    assert_eq!(loaded.mode, defaults.mode);
    assert!(
        Vauchi::in_memory()
            .expect("fresh")
            .storage()
            .ux()
            .load_exchange_defaults()
            .expect("load on fresh store")
            .is_none(),
        "fresh store has no defaults"
    );
}

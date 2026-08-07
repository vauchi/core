// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 (`2026-07-03-core-screens-bypass-i18n`): the exchange group gate,
//! field preview and failed terminal render in the user's locale. Keys in
//! `exchange.*` + shared `action.*` (locales!85).
//!
//! Asserts that the screens resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{action_label, assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{ExchangeConfig, ExchangeEngine, UserAction, WorkflowEngine};
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::ExchangeMode;

fn config_for(locale: Locale) -> ExchangeConfig {
    ExchangeConfig {
        own_name: "Alice".into(),
        own_qr_data: "qr".into(),
        available_groups: vec![("g1".into(), "Familie".into())],
        device_capabilities: DeviceCapabilities {
            has_camera: true,
            has_ble: true,
            has_internet: true,
            ..Default::default()
        },
        mode: Some(ExchangeMode::TapHoverShake),
        last_used_group_ids: None,
        last_used_mode: None,
        card_snapshot: None,
        transport_readiness: Default::default(),
        available_group_data: Vec::new(),
        locale,
    }
}

fn engine_for(locale: Locale) -> ExchangeEngine {
    ExchangeEngine::new(
        config_for(locale),
        vauchi_core::clock::SystemClock::shared(),
    )
}

/// Copy across the group gate and the field preview.
struct GateCopy {
    gate_screen_id: String,
    skip_action: String,
    cancel_action: String,
    continue_action: String,
    preview_screen_id: String,
    preview_title: String,
    start_action: String,
}

fn walk_gate(locale: Locale) -> GateCopy {
    let mut engine = engine_for(locale);

    let gate = engine.current_screen();
    let gate_screen_id = gate.screen_id.clone();
    // Unified button reads the localized Skip at 0 selected (M2 S7).
    let skip_action = action_label(&gate, "continue");
    let cancel_action = action_label(&gate, "cancel");

    // Select the group → the same button becomes Continue.
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_picker".into(),
        item_id: "g1".into(),
    });
    let continue_action = action_label(&engine.current_screen(), "continue");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let preview = engine.current_screen();

    GateCopy {
        gate_screen_id,
        skip_action,
        cancel_action,
        continue_action,
        preview_screen_id: preview.screen_id.clone(),
        preview_title: preview.title.clone(),
        start_action: action_label(&preview, "start_exchange"),
    }
}

/// `(title, retry action, cancel action)` for the failed terminal.
fn failed_copy(locale: Locale) -> (String, String, String) {
    let mut engine = engine_for(locale);
    engine.mark_failed();
    let failed = engine.current_screen();
    (
        failed.title.clone(),
        action_label(&failed, "retry"),
        action_label(&failed, "cancel"),
    )
}

// @scenario: exchange :: group gate and field preview render in the active locale
// @internal
#[test]
fn group_gate_and_preview_render_the_active_locale() {
    load_german();
    let de = walk_gate(Locale::German);
    let en = walk_gate(Locale::English);

    // Screen ids are identifiers, not copy — they must NOT translate.
    assert_eq!(de.gate_screen_id, "exchange_group_selection");
    assert_eq!(de.preview_screen_id, "exchange_field_preview");
    assert_eq!(de.gate_screen_id, en.gate_screen_id);
    assert_eq!(de.preview_screen_id, en.preview_screen_id);

    assert_translated("skip action", &de.skip_action, &en.skip_action);
    assert_translated("cancel action", &de.cancel_action, &en.cancel_action);
    assert_translated("continue action", &de.continue_action, &en.continue_action);
    assert_translated("preview title", &de.preview_title, &en.preview_title);
    assert_translated("start-exchange action", &de.start_action, &en.start_action);

    // The unified button must change label when a group is selected —
    // that state transition is core's, independent of wording.
    assert_ne!(
        de.skip_action, de.continue_action,
        "selecting a group must flip the unified button from Skip to Continue"
    );
}

// @scenario: exchange :: failed terminal screen renders in the active locale
// @internal
#[test]
fn failed_terminal_renders_the_active_locale() {
    load_german();
    let (de_title, de_retry, de_cancel) = failed_copy(Locale::German);
    let (en_title, en_retry, en_cancel) = failed_copy(Locale::English);

    assert_translated("failed title", &de_title, &en_title);
    assert_translated("retry action", &de_retry, &en_retry);
    assert_translated("cancel action", &de_cancel, &en_cancel);
}

// English stays exactly as before (regression pin). English is the source
// language and ships bundled, so pinning it here couples nothing external.
// @internal
#[test]
fn exchange_flow_english_copy_unchanged() {
    let mut engine = engine_for(Locale::English);
    let gate = engine.current_screen();
    assert_eq!(action_label(&gate, "continue"), "Skip");

    engine.mark_failed();
    let failed = engine.current_screen();
    assert_eq!(failed.title, "Failed");
    assert_eq!(action_label(&failed, "retry"), "Retry");
}

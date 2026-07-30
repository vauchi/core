// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S4b-2 (`2026-07-03-core-screens-bypass-i18n`): the remaining
//! exchange-flow screens render in the user's locale via the
//! ExchangeConfig.locale seam (S4b-1) — the group gate + sharing chip,
//! field preview, and the shared terminal Success/Failed block. Keys in
//! `exchange.*` + shared `action.*` (locales!85). Exact German
//! assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{ExchangeConfig, ExchangeEngine, ScreenModel, UserAction, WorkflowEngine};
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::ExchangeMode;

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

fn action_label(screen: &ScreenModel, id: &str) -> String {
    screen
        .contextual_actions
        .iter()
        .find(|a| a.id == id)
        .unwrap_or_else(|| panic!("action {id} present"))
        .label
        .clone()
}

fn german_config() -> ExchangeConfig {
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
        locale: Locale::German,
    }
}

// @scenario: exchange :: group gate and field preview render in the active locale
// @internal
#[test]
fn group_gate_and_preview_render_german() {
    load_german();
    let mut engine =
        ExchangeEngine::new(german_config(), vauchi_core::clock::SystemClock::shared());

    let gate = engine.current_screen();
    assert_eq!(gate.screen_id, "exchange_group_selection");
    // Unified button reads the localized Skip at 0 selected (M2 S7).
    assert_eq!(action_label(&gate, "continue"), "Überspringen");
    assert_eq!(action_label(&gate, "cancel"), "Abbrechen");

    // Select the group → Continue → field preview, localized.
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "group_picker".into(),
        item_id: "g1".into(),
    });
    assert_eq!(action_label(&engine.current_screen(), "continue"), "Weiter");
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    let preview = engine.current_screen();
    assert_eq!(preview.screen_id, "exchange_field_preview");
    assert_eq!(preview.title, "Sie teilen");
    assert_eq!(
        action_label(&preview, "start_exchange"),
        "Austausch starten"
    );
}

// @scenario: exchange :: failed terminal screen renders in the active locale
// @internal
#[test]
fn failed_terminal_renders_german() {
    load_german();
    let mut engine =
        ExchangeEngine::new(german_config(), vauchi_core::clock::SystemClock::shared());
    engine.mark_failed();
    let failed = engine.current_screen();
    assert_eq!(failed.title, "Fehlgeschlagen");
    assert_eq!(action_label(&failed, "retry"), "Erneut versuchen");
    assert_eq!(action_label(&failed, "cancel"), "Abbrechen");
}

// English stays exactly as before (regression pin).
// @internal
#[test]
fn exchange_flow_english_copy_unchanged() {
    let mut engine = ExchangeEngine::new(
        ExchangeConfig {
            locale: Locale::English,
            ..german_config()
        },
        vauchi_core::clock::SystemClock::shared(),
    );
    let gate = engine.current_screen();
    assert_eq!(action_label(&gate, "continue"), "Skip");

    engine.mark_failed();
    let failed = engine.current_screen();
    assert_eq!(failed.title, "Failed");
    assert_eq!(action_label(&failed, "retry"), "Retry");
}

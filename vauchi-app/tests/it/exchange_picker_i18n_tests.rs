// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S4b-1 (`2026-07-03-core-screens-bypass-i18n`): the exchange mode
//! picker renders in the user's locale — title/subtitle, the disclosure
//! entry, per-mode instructions, and the Recommended/Unauthenticated
//! markers via the `exchange.picker.*` templates (locales!84). Mode NAMES
//! stay the canonical product vocabulary in every locale. Locale threads
//! via `ExchangeConfig.locale` (the one seam every exchange sub-engine
//! reads). Exact German assertions per CC-03.

use vauchi_app::i18n::{Locale, load_locale_from_bytes};
use vauchi_app::ui::{
    ActionListItem, Component, ExchangeConfig, ExchangeEngine, ScreenModel, UserAction,
    WorkflowEngine,
};
use vauchi_core::exchange::capability::types::DeviceCapabilities;

fn load_german() {
    let bytes = std::fs::read("../../locales/de.json")
        .expect("locales checkout present as sibling repo (CI: .clone-locales)");
    load_locale_from_bytes("de", &bytes).expect("German locale parses");
}

fn german_config() -> ExchangeConfig {
    ExchangeConfig {
        own_name: "Alice".into(),
        own_qr_data: "qr".into(),
        available_groups: vec![],
        device_capabilities: DeviceCapabilities {
            has_camera: true,
            has_ble: true,
            has_internet: true,
            ..Default::default()
        },
        mode: None,
        last_used_group_ids: None,
        last_used_mode: None,
        card_snapshot: None,
        transport_readiness: Default::default(),
        available_group_data: Vec::new(),
        locale: Locale::German,
    }
}

fn find_item<'a>(screen: &'a ScreenModel, id_prefix: &str) -> Option<&'a ActionListItem> {
    screen.components.iter().find_map(|c| match c {
        Component::ActionList { items, .. } => items.iter().find(|i| i.id.starts_with(id_prefix)),
        _ => None,
    })
}

// @scenario: exchange :: mode picker renders in the active locale
// @internal
#[test]
fn mode_picker_renders_german() {
    load_german();
    let mut engine =
        ExchangeEngine::new(german_config(), vauchi_core::clock::SystemClock::shared());

    let picker = engine.current_screen();
    assert_eq!(picker.screen_id, "exchange_mode_selection");
    assert_eq!(picker.title, "Austauschmodus");
    assert_eq!(
        picker.subtitle.as_deref(),
        Some("Wählen Sie, wie Kontaktkarten ausgetauscht werden")
    );

    // Collapsed: the disclosure row is localized; the hero (first-run
    // Glance) keeps its product name and carries the localized
    // Recommended marker + instruction.
    let more = find_item(&picker, "show_other_modes").expect("disclosure entry present");
    assert_eq!(more.label, "Weitere Verbindungsmöglichkeiten");
    let hero = find_item(&picker, "mode:glance").expect("Glance hero present");
    assert_eq!(hero.label, "Glance");
    assert_eq!(
        hero.detail.as_deref(),
        Some("Empfohlen · Zeigen Sie Ihren Code oder scannen Sie ihren")
    );

    // Expanded: an unauthenticated mode carries the localized marker.
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "more".into(),
        item_id: "show_other_modes".into(),
    });
    let expanded = engine.current_screen();
    let bump = find_item(&expanded, "mode:bump").expect("Bump listed after disclosure");
    let detail = bump.detail.as_deref().unwrap_or_default();
    assert!(
        detail.starts_with("Nicht authentifiziert · "),
        "Bump carries the localized unauthenticated marker; got {detail:?}"
    );
}

// English stays exactly as before the threading (regression pin).
// @internal
#[test]
fn mode_picker_english_copy_unchanged() {
    let engine = ExchangeEngine::new(
        ExchangeConfig {
            locale: Locale::English,
            ..german_config()
        },
        vauchi_core::clock::SystemClock::shared(),
    );
    let picker = engine.current_screen();
    assert_eq!(picker.title, "Exchange Mode");
    assert_eq!(
        picker.subtitle.as_deref(),
        Some("Choose how to exchange contact cards")
    );
    let hero = find_item(&picker, "mode:glance").expect("hero present");
    assert_eq!(
        hero.detail.as_deref(),
        Some("Recommended · Show your code or scan theirs")
    );
}

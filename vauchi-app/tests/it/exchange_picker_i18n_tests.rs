// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M3 S4b (`2026-07-03-core-screens-bypass-i18n`): the exchange mode
//! picker renders in the user's locale, threaded from the engine entry
//! via `ExchangeConfig.locale` (the one seam every exchange sub-engine
//! reads).
//!
//! Asserts that the picker resolved a translation, not what the
//! translation says — see `i18n_support::assert_translated`.

use super::i18n_support::{assert_translated, load_german};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ActionListItem, Component, ExchangeConfig, ExchangeEngine, ScreenModel, UserAction,
    WorkflowEngine,
};
use vauchi_core::exchange::capability::types::DeviceCapabilities;

fn config_for(locale: Locale) -> ExchangeConfig {
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
        locale,
    }
}

fn find_item<'a>(screen: &'a ScreenModel, id_prefix: &str) -> Option<&'a ActionListItem> {
    screen.components.iter().find_map(|c| match c {
        Component::ActionList { items, .. } => items.iter().find(|i| i.id.starts_with(id_prefix)),
        _ => None,
    })
}

/// Copy a shell would show on the picker, collapsed then expanded.
struct PickerCopy {
    screen_id: String,
    title: String,
    subtitle: String,
    disclosure_label: String,
    hero_label: String,
    hero_detail: String,
    bump_detail: String,
}

fn walk_picker(locale: Locale) -> PickerCopy {
    let mut engine = ExchangeEngine::new(
        config_for(locale),
        vauchi_core::clock::SystemClock::shared(),
    );

    let picker = engine.current_screen();
    let screen_id = picker.screen_id.clone();
    let title = picker.title.clone();
    let subtitle = picker.subtitle.clone().expect("subtitle present");

    let more = find_item(&picker, "show_other_modes").expect("disclosure entry present");
    let disclosure_label = more.label.clone();
    let hero = find_item(&picker, "mode:glance").expect("Glance hero present");
    let hero_label = hero.label.clone();
    let hero_detail = hero.detail.clone().expect("hero detail present");

    // Expanded: an unauthenticated mode carries the localized marker.
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "more".into(),
        item_id: "show_other_modes".into(),
    });
    let expanded = engine.current_screen();
    let bump = find_item(&expanded, "mode:bump").expect("Bump listed after disclosure");

    PickerCopy {
        screen_id,
        title,
        subtitle,
        disclosure_label,
        hero_label,
        hero_detail,
        bump_detail: bump.detail.clone().unwrap_or_default(),
    }
}

// @scenario: exchange :: mode picker renders in the active locale
// @internal
#[test]
fn mode_picker_renders_the_active_locale() {
    load_german();
    let de = walk_picker(Locale::German);
    let en = walk_picker(Locale::English);

    // Screen ids are identifiers, not copy — they must NOT translate.
    assert_eq!(de.screen_id, "exchange_mode_selection");
    assert_eq!(de.screen_id, en.screen_id);

    assert_translated("picker title", &de.title, &en.title);
    assert_translated("picker subtitle", &de.subtitle, &en.subtitle);
    assert_translated("disclosure row", &de.disclosure_label, &en.disclosure_label);
    assert_translated("Glance hero detail", &de.hero_detail, &en.hero_detail);
    assert_translated("Bump detail marker", &de.bump_detail, &en.bump_detail);

    // Exemption: "Glance" is the product name for the mode and is
    // deliberately identical in every locale, so it cannot be asserted
    // as translated. Pinning it exactly is the point — a translated
    // product name would be the defect.
    assert_eq!(de.hero_label, "Glance");
    assert_eq!(de.hero_label, en.hero_label);
}

// English stays exactly as before the threading (regression pin). English
// is the source language and ships in this repo's bundled locale, so
// pinning it here couples nothing external.
// @internal
#[test]
fn mode_picker_english_copy_unchanged() {
    let engine = ExchangeEngine::new(
        config_for(Locale::English),
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

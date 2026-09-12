// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

mod screen_catalog;

use std::collections::{BTreeMap, BTreeSet};

use screen_catalog::{
    build_catalog, entry_surface_id, init_fixture_i18n, node_kinds, replaced_surface,
    replaced_surface_ids,
};
use vauchi_app::ui::{ScreenCatalogEntry, ScreenCatalogFixture};
use vauchi_core::{Command, PresentationNode, PresentationRow};

const REQUIRED_CODE_IDS: [&str; 53] = [
    "onboarding",
    "my_info-empty",
    "my_info",
    "contacts-empty",
    "contacts",
    "contact_detail",
    "contact_edit",
    "contact_visibility",
    "verify_fingerprint",
    "exchange",
    "exchange-no_groups",
    "settings",
    "settings_advanced",
    "settings_appearance",
    "settings_accessibility",
    "help",
    "backup",
    "lock",
    "device_linking",
    "device_management",
    "duress_pin",
    "change_password",
    "decoy_contacts",
    "emergency_shred",
    "delivery_status",
    "recovery",
    "recovery_help",
    "groups",
    "group_detail",
    "tags",
    "places",
    "privacy",
    "support",
    "entry_detail",
    "contact_duplicates",
    "contact_merge",
    "contact_limit",
    "activity_log",
    "archived_contacts",
    "device_replacement",
    "avatar_editor",
    "recovery_claim_review",
    "link_exchange",
    "nfc_exchange",
    "direct_transport",
    "multi_stage_exchange-glance",
    "ble_exchange-magic",
    "form_dialog-add_field",
    "form_dialog-edit_field",
    "form_dialog-edit_name",
    "form_dialog-edit_relay_url",
    "form_dialog-create_group",
    "form_dialog-rename_group",
];

fn checked_in_catalog() -> ScreenCatalogFixture {
    serde_json::from_str(vauchi_app::ui::screen_catalog_fixture_json())
        .expect("Core-owned screen catalog fixture must decode")
}

/// `(code_id, locale) -> (title, node kinds)`: the structure that is stable
/// across regenerations even though keys and ids are random per run.
fn structure(catalog: &ScreenCatalogFixture) -> BTreeMap<(String, String), (String, Vec<String>)> {
    catalog
        .screens
        .iter()
        .map(|entry| {
            let surface = replaced_surface(&entry.commands, entry_surface_id(entry));
            (
                (entry.code_id.clone(), entry.locale.clone()),
                (surface.title.clone(), node_kinds(&surface.nodes)),
            )
        })
        .collect()
}

fn assert_single_replacement_then_navigation(entry: &ScreenCatalogEntry) {
    let surface_id = entry_surface_id(entry);
    let replaced = replaced_surface_ids(&entry.commands);
    assert_eq!(
        replaced.iter().filter(|id| **id == surface_id).count(),
        1,
        "{}/{}: exactly one ReplaceSurface for {surface_id}, got {replaced:?}",
        entry.code_id,
        entry.locale
    );
    let surface = replaced_surface(&entry.commands, surface_id);
    let replacement_index = entry
        .commands
        .iter()
        .position(
            |command| matches!(command, Command::ReplaceSurface { surface: s } if s == surface),
        )
        .expect("replacement located");
    let navigation = entry
        .commands
        .iter()
        .skip(replacement_index)
        .position(|command| {
            matches!(
                command,
                Command::SetNavigation { surface_id, revision, .. }
                    if *surface_id == surface.surface_id && *revision == surface.revision
            )
        });
    assert!(
        navigation.is_some_and(|offset| offset > 0),
        "{}/{}: SetNavigation for surface {} revision {} must follow its ReplaceSurface",
        entry.code_id,
        entry.locale,
        surface.surface_id.as_str(),
        surface.revision
    );
    assert_eq!(
        entry.title, surface.title,
        "{}: entry title mirrors surface",
        entry.code_id
    );
}

// @internal
#[test]
fn screen_catalog_checked_in_catalog_decodes_with_schema_version_one() {
    let catalog = checked_in_catalog();
    assert_eq!(catalog.schema_version, 1);
    assert!(!catalog.screens.is_empty(), "catalog must list screens");
}

// @internal
#[test]
fn screen_catalog_checked_in_catalog_covers_every_reachable_screen() {
    let catalog = checked_in_catalog();
    let code_ids: BTreeSet<&str> = catalog
        .screens
        .iter()
        .map(|entry| entry.code_id.as_str())
        .collect();
    let missing: Vec<&str> = REQUIRED_CODE_IDS
        .iter()
        .copied()
        .filter(|required| !code_ids.contains(required))
        .collect();
    assert!(
        missing.is_empty(),
        "catalog is missing screens: {missing:?}"
    );
    assert!(
        code_ids.len() >= 35,
        "catalog lists {} distinct code_ids, expected at least 35",
        code_ids.len()
    );
    for locale_screen in ["onboarding", "contacts", "settings"] {
        let german_code_id = format!("{locale_screen}-de");
        assert!(
            catalog
                .screens
                .iter()
                .any(|entry| entry.code_id == german_code_id && entry.locale == "de"),
            "{locale_screen} needs a German entry under code_id {german_code_id}"
        );
    }
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn screen_catalog_every_catalog_entry_replaces_one_surface_then_installs_its_navigation() {
    let catalog = checked_in_catalog();
    // Shells write `<code_id>.png`, so a code_id repeated across locales
    // would make the second render overwrite the first.
    let mut seen = BTreeSet::new();
    for entry in &catalog.screens {
        assert!(
            seen.insert(entry.code_id.clone()),
            "code_id {} listed twice (locale {})",
            entry.code_id,
            entry.locale
        );
        assert_single_replacement_then_navigation(entry);
    }
}

// @internal
#[test]
fn screen_catalog_checked_in_catalog_matches_a_fresh_build_structurally() {
    init_fixture_i18n();
    let recorded = structure(&checked_in_catalog());
    let fresh = structure(&build_catalog());

    let recorded_keys: BTreeSet<_> = recorded.keys().cloned().collect();
    let fresh_keys: BTreeSet<_> = fresh.keys().cloned().collect();
    assert_eq!(
        recorded_keys, fresh_keys,
        "set of (code_id, locale) drifted"
    );
    for (key, fresh_shape) in &fresh {
        assert_eq!(
            &recorded[key], fresh_shape,
            "{key:?}: surface title or node kinds drifted; run the ignored regenerate_shared_fixture test"
        );
    }
}

// @internal
#[test]
#[ignore = "regenerates the checked-in Core screen catalog fixture"]
fn screen_catalog_regenerate_shared_fixture() {
    init_fixture_i18n();
    let json = serde_json::to_string_pretty(&build_catalog()).expect("serialize catalog");
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/screen_catalog_v1.json");
    std::fs::write(path, format!("{json}\n")).expect("write screen catalog fixture");
}

fn picker_rows(entry: &ScreenCatalogEntry) -> Vec<&PresentationRow> {
    replaced_surface(&entry.commands, "exchange")
        .nodes
        .iter()
        .filter_map(|node| match node {
            PresentationNode::List { rows, .. } => Some(rows),
            _ => None,
        })
        .flatten()
        .collect()
}

// @scenario: exchange :: picker offers only the alpha-reliable modes
// The catalog is what shells and the design canvas are compared against,
// so the picker must be recorded from a phone's capability set: Glance
// (QR) leads as the recommended hero and the NFC- and BLE-gated modes are
// offered as runnable, not "Requires camera, BLE" the way a camera-less
// engine renders them.
#[test]
fn screen_catalog_exchange_picker_offers_qr_nfc_and_bluetooth_from_a_phone_capability_set() {
    let catalog = checked_in_catalog();
    let entry = catalog
        .screens
        .iter()
        .find(|entry| entry.code_id == "exchange-no_groups")
        .expect("exchange-no_groups recorded");
    let rows = picker_rows(entry);

    let hero = rows.first().expect("picker leads with a hero row");
    assert_eq!(hero.icon_token.as_deref(), Some("qrcode"), "QR leads");
    assert!(
        hero.subtitle
            .as_deref()
            .is_some_and(|subtitle| subtitle.starts_with("Recommended")),
        "QR hero is marked recommended, got {:?}",
        hero.subtitle
    );

    let icons: BTreeSet<&str> = rows
        .iter()
        .filter_map(|row| row.icon_token.as_deref())
        .collect();
    for offered in ["qrcode", "nfc", "gesture"] {
        assert!(
            icons.contains(offered),
            "picker offers {offered}, got {icons:?}"
        );
    }
    // A phone has a camera, BLE, NFC, audio and an accelerometer; only the
    // USB-gated Cable row may still read as unavailable.
    for row in rows
        .iter()
        .filter(|row| row.icon_token.as_deref() != Some("cable"))
    {
        assert!(
            !row.subtitle
                .as_deref()
                .unwrap_or_default()
                .starts_with("Requires"),
            "{} is gated on hardware a phone has: {:?}",
            row.title,
            row.subtitle
        );
    }
}

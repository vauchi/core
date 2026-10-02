// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Backup screen (#459, design pass #419 item 8; owner decisions
//! 2026-10-01): Export backup and Restore from backup as rows, and when the
//! last backup was made; the full / identity-only choice moves into the
//! export flow.

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    AppEngine, AppScreen, BackupRecoveryEngine, Component, ScreenModel, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;

const DAY: u64 = 24 * 60 * 60;
const NOW: u64 = 1_790_000_000;

fn rows(screen: &ScreenModel) -> Vec<(String, Option<String>)> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == "backup_rows" => Some(
                items
                    .iter()
                    .map(|i| (i.label.clone(), i.detail.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn texts(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Text { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

fn level_toggle(screen: &ScreenModel) -> Option<bool> {
    screen.components.iter().find_map(|c| match c {
        Component::ToggleList { id, items, .. } if id == "backup_level" => {
            items.first().map(|i| i.selected)
        }
        _ => None,
    })
}

fn open(engine: &mut BackupRecoveryEngine, row: &str) {
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "backup_rows".into(),
        item_id: row.into(),
    });
}

fn choose() -> BackupRecoveryEngine {
    BackupRecoveryEngine::new(None, true, Locale::English)
}

// @internal
#[test]
fn backup_is_export_and_restore_with_the_last_backup() {
    let screen = choose().current_screen();
    assert_eq!(
        rows(&screen),
        [
            (
                "Export backup".into(),
                Some("Encrypted file, protected by a password you choose".into())
            ),
            (
                "Restore from backup".into(),
                Some("Replaces the identity on this device".into())
            ),
        ]
    );
    let texts = texts(&screen);
    assert!(
        texts.contains(
            &"An encrypted backup lets you restore Vauchi after reinstalling it, or move to a new \
          device."
                .to_string()
        )
    );
    assert!(texts.contains(&"No backup yet".to_string()));
    assert!(screen.contextual_actions.is_empty());
    assert_eq!(
        level_toggle(&screen),
        None,
        "the level choice is in the export flow"
    );
}

// @internal
#[test]
fn the_last_backup_says_how_long_ago() {
    let screen = choose()
        .with_last_backup(Some(NOW - 3 * DAY), NOW)
        .current_screen();
    assert!(texts(&screen).contains(&"Last backup · 3 days ago".to_string()));
}

// @internal
#[test]
fn export_asks_full_or_identity_only_restore_does_not() {
    let mut export = choose();
    open(&mut export, "create");
    assert_eq!(
        level_toggle(&export.current_screen()),
        Some(true),
        "full by default"
    );
    let _ = export.handle_action(UserAction::ItemToggled {
        component_id: "backup_level".into(),
        item_id: "level_toggle".into(),
    });
    assert_eq!(level_toggle(&export.current_screen()), Some(false));

    let mut restore = choose();
    open(&mut restore, "restore");
    let screen = restore.current_screen();
    assert_eq!(level_toggle(&screen), None);
    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::TextInput { id, .. } if id == "backup_data"))
    );
}

// @internal
#[test]
fn an_export_shows_as_the_last_backup() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let _ = vauchi
        .export_full_backup("correct-horse-battery-staple-42")
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Backup);
    assert!(
        texts(&screen).contains(&"Last backup · Just now".to_string()),
        "{:?}",
        texts(&screen)
    );
}

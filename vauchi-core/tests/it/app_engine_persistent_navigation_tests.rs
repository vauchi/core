// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Command::SetNavigation` tests: the persistent navigation surface
//! (ADR-066 Amendment 2026-07-18 D4) tracks the currently selected
//! destination across navigation, and publishes no items while the app
//! is locked.

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::Command;
use vauchi_core::api::Vauchi;

fn navigation_selection(commands: &[Command]) -> Vec<(&str, bool)> {
    commands
        .iter()
        .find_map(|command| match command {
            Command::SetNavigation { navigation, .. } => Some(
                navigation
                    .items
                    .iter()
                    .map(|item| (item.interaction_id.as_str(), item.selected))
                    .collect(),
            ),
            _ => None,
        })
        .expect("initial commands must publish SetNavigation")
}

// @internal
#[test]
fn navigating_contacts_then_my_card_moves_the_selected_navigation_item() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::Contacts);
    let commands = engine.initial_commands().expect("contacts commands");
    let selection = navigation_selection(&commands);
    assert_eq!(
        selection
            .iter()
            .filter(|(id, selected)| *selected && id.ends_with("contacts"))
            .count(),
        1,
        "contacts item must be the sole selection: {selection:?}"
    );

    engine.navigate_to(AppScreen::MyInfo);
    let commands = engine.initial_commands().expect("my card commands");
    let selection = navigation_selection(&commands);
    assert_eq!(
        selection
            .iter()
            .filter(|(id, selected)| *selected && id.ends_with("my_info"))
            .count(),
        1,
        "my card item must be the sole selection: {selection:?}"
    );
    assert_eq!(
        selection
            .iter()
            .filter(|(id, selected)| *selected && !id.ends_with("my_info"))
            .count(),
        0,
        "only my card may stay selected after navigating away from contacts: {selection:?}"
    );
}

// @internal
#[test]
fn locked_app_publishes_navigation_with_no_items() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::Lock);
    let commands = engine.initial_commands().expect("lock commands");

    assert!(
        navigation_selection(&commands).is_empty(),
        "a locked app must publish no navigation destinations"
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The new phone's recovery screen restores the backup its guardians
//! handed back (vauchi/private#796, step 3b): it offers to once a guardian
//! has, says how many did, asks for the owner's passphrase when the
//! backup is sealed under one, and says how many contacts came back, or
//! that the backup did not open.

use std::sync::Arc;
use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ScreenModel, UserAction, WorkflowEngine,
};

use vauchi_core::api::Vauchi;
use vauchi_core::backup::GuardianGroups;
use vauchi_core::backup::passphrase_layer;
use vauchi_core::clock::{Clock, SystemClock};
use vauchi_core::sync::guardian_piece::{GuardianDelivery, split_into_pieces};
use vauchi_core::{Contact, ContactCard, Identity, SymmetricKey};

const PASSPHRASE: &str = "the owl flies at noon";

fn vauchi_named(name: &str) -> Vauchi {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity(name).unwrap();
    vauchi
}

fn signing_pk(vauchi: &Vauchi) -> [u8; 32] {
    *vauchi.identity().unwrap().signing_public_key()
}

fn exchanged(public_key: [u8; 32], name: &str) -> Contact {
    Contact::from_exchange(
        public_key,
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    )
}

/// Alice has Dave and three guardians, each a group of its own; her backup
/// is sealed under `passphrase` when given. Her new phone holds what the
/// first `handing_back` guardians returned, on its recovery screen.
fn phone(passphrase: Option<&str>, handing_back: usize) -> AppEngine {
    phone_on(Arc::new(SystemClock), passphrase, handing_back)
}

/// [`phone`] with the new phone on `clock`.
fn phone_on(clock: Arc<dyn Clock>, passphrase: Option<&str>, handing_back: usize) -> AppEngine {
    let alice = vauchi_named("Alice");
    alice
        .add_contact(exchanged(
            *Identity::create("Dave", 0).signing_public_key(),
            "Dave",
        ))
        .unwrap();
    let guardians: Vec<Vauchi> = (0..3).map(|i| vauchi_named(&format!("G{i}"))).collect();
    let groups =
        GuardianGroups::new(guardians.iter().map(|g| vec![signing_pk(g)]).collect()).unwrap();
    let (backup_hex, sealed) = alice.export_guardian_backup_for_groups(&groups).unwrap();
    let mut backup = hex::decode(backup_hex).unwrap();
    if let Some(passphrase) = passphrase {
        let salt = [9; 16];
        let key = passphrase_layer::passphrase_key(passphrase, &salt).unwrap();
        backup = passphrase_layer::seal(&key, &salt, &backup).unwrap();
    }

    let mut phone = Vauchi::in_memory_with_clock(clock).unwrap();
    phone.create_identity("Alice, new phone").unwrap();
    let phone_pk = signing_pk(&phone);
    for (index, guardian) in guardians.iter().enumerate().take(handing_back) {
        let at_phone = exchanged(signing_pk(guardian), &format!("G{index}"));
        let guardian_id = at_phone.id().to_string();
        phone.add_contact(at_phone).unwrap();
        let share = guardian
            .respond_to_recovery(&sealed[index][0], &phone_pk)
            .unwrap();
        let delivery = GuardianDelivery::new(1, share, backup.clone()).unwrap();
        for piece in split_into_pieces([index as u8; 16], &delivery.encode()).unwrap() {
            phone
                .storage()
                .returned_backups()
                .keep_piece(&guardian_id, &piece, 1)
                .unwrap();
        }
    }
    let mut engine = AppEngine::new(phone);
    engine.navigate_to(AppScreen::Recovery);
    engine
}

fn screen_of(result: ActionResult) -> ScreenModel {
    match result {
        ActionResult::UpdateScreen(screen) | ActionResult::NavigateTo(screen) => screen,
        other => panic!("expected a screen, got {other:?}"),
    }
}

fn open_restore(engine: &mut AppEngine) -> ScreenModel {
    screen_of(engine.handle_action(UserAction::ListItemSelected {
        component_id: "recovery_actions".into(),
        item_id: "restore_contacts".into(),
    }))
}

fn press(engine: &mut AppEngine, action_id: &str) -> ScreenModel {
    screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    }))
}

fn type_passphrase(engine: &mut AppEngine, value: &str) {
    let screen = screen_of(engine.handle_action(UserAction::TextChanged {
        component_id: "restore_passphrase".into(),
        value: value.into(),
    }));
    let shown = screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::TextInput { id, value, .. } if id == "restore_passphrase" => {
                Some(value.clone())
            }
            _ => None,
        });
    assert_eq!(shown.as_deref(), Some(value));
}

fn has(screen: &ScreenModel, wanted: &str) -> bool {
    screen.components.iter().any(|component| match component {
        Component::Text { id, .. }
        | Component::TextInput { id, .. }
        | Component::StatusIndicator { id, .. } => id == wanted,
        _ => false,
    })
}

fn text(screen: &ScreenModel, wanted: &str) -> String {
    screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::Text { id, content, .. } if id == wanted => Some(content.clone()),
            Component::StatusIndicator { id, title, .. } if id == wanted => Some(title.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {wanted} on {:?}", screen.components))
}

fn action_rows(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::ButtonList { id, items } if id == "recovery_actions" => {
                Some(items.iter().map(|item| item.id.clone()).collect())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn has_dave(engine: &AppEngine) -> bool {
    engine
        .vauchi()
        .list_contacts()
        .unwrap()
        .iter()
        .any(|contact| contact.display_name() == "Dave")
}

// @internal
#[test]
fn the_recovery_screen_offers_restore_once_a_guardian_handed_back() {
    assert!(action_rows(&phone(None, 1).current_screen()).contains(&"restore_contacts".into()));
    assert!(!action_rows(&phone(None, 0).current_screen()).contains(&"restore_contacts".into()));
}

// @internal
#[test]
fn restoring_with_every_group_brings_the_contacts_back_and_says_how_many() {
    let mut engine = phone(None, 3);

    let screen = open_restore(&mut engine);
    assert!(text(&screen, "restore_handed_back").contains('3'));
    assert!(!has(&screen, "restore_passphrase"));

    let done = press(&mut engine, "restore_now");

    assert!(text(&done, "restore_done").contains('1'), "Dave");
    assert!(has_dave(&engine));
}

// @internal
#[test]
fn a_share_of_a_group_missing_says_the_backup_did_not_open_and_restores_nothing() {
    let mut engine = phone(None, 2);
    open_restore(&mut engine);

    let screen = press(&mut engine, "restore_now");

    assert!(has(&screen, "restore_error"));
    assert!(!has(&screen, "restore_done"));
    assert!(!has_dave(&engine));
}

// @internal
#[test]
fn a_sealed_backup_asks_for_the_passphrase_and_opens_only_with_it() {
    let mut engine = phone(Some(PASSPHRASE), 3);
    let screen = open_restore(&mut engine);
    assert!(has(&screen, "restore_passphrase"));

    assert!(has(&press(&mut engine, "restore_now"), "restore_error"));
    type_passphrase(&mut engine, "the owl flies at dusk");
    assert!(has(&press(&mut engine, "restore_now"), "restore_error"));
    assert!(!has_dave(&engine));

    type_passphrase(&mut engine, PASSPHRASE);
    let done = press(&mut engine, "restore_now");

    assert!(has(&done, "restore_done"));
    assert!(has_dave(&engine));
}

// @internal
#[test]
fn cancelling_restore_goes_back_and_restores_nothing() {
    let mut engine = phone(None, 3);
    open_restore(&mut engine);

    let screen = press(&mut engine, "cancel_restore");

    assert!(action_rows(&screen).contains(&"restore_contacts".into()));
    assert!(!has_dave(&engine));
}

/// The waiting list's rows on the recovery screen, as (name, detail).
#[cfg(feature = "testing")]
fn waiting_rows(screen: &ScreenModel) -> Vec<(String, String)> {
    screen
        .components
        .iter()
        .find_map(|component| match component {
            Component::InfoPanel { id, items, .. } if id == "rekey_waiting" => Some(
                items
                    .iter()
                    .map(|item| (item.title.clone(), item.detail.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

// @internal
#[cfg(feature = "testing")]
#[test]
fn after_the_restore_the_recovery_screen_lists_who_has_not_moved_yet() {
    use std::time::{Duration, SystemTime};
    use vauchi_core::clock::FakeClock;
    let clock = Arc::new(FakeClock::new(SystemTime::now()));
    let mut engine = phone_on(clock.clone(), None, 3);
    assert!(waiting_rows(&engine.current_screen()).is_empty());
    open_restore(&mut engine);
    press(&mut engine, "restore_now");
    drop(engine.handle_action(UserAction::ActionPressed {
        action_id: "done".into(),
    }));

    engine.navigate_to(AppScreen::Recovery);
    let asking = waiting_rows(&engine.current_screen());
    let dave = asking
        .iter()
        .find(|(name, _)| name == "Dave")
        .cloned()
        .unwrap();

    clock.advance(Duration::from_secs(14 * 86_400));
    engine.navigate_to(AppScreen::Contacts);
    engine.navigate_to(AppScreen::Recovery);
    let later = waiting_rows(&engine.current_screen());
    let dave_later = later
        .iter()
        .find(|(name, _)| name == "Dave")
        .cloned()
        .unwrap();

    assert_ne!(dave.1, dave_later.1, "asking, then not answered");
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Recovery screen (#459, design pass #419 item 8; owner decisions
//! 2026-10-01): who you trust for recovery, by name, with the real
//! threshold; guardians are never told, and the screen says to reach them
//! personally, one at a time, only when recovery is needed; Start recovery
//! is always reachable, because it is used from a new device that has no
//! contacts yet.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, Item, RecoveryEngine, ScreenModel, UserAction,
    WorkflowEngine,
};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

const NOT_TOLD: &str = "Vauchi does not tell them they are trusted, and it is safest if you do \
not either. Only if you lose every device, reach them yourself, one at a time, by phone or in \
person and never in a shared group, and ask each to vouch for you.";

fn person(id: &str, name: &str) -> Item {
    Item {
        id: id.into(),
        name: name.into(),
        subtitle: None,
        initials: name.chars().take(1).collect(),
        status: None,
        actions: vec![],
        a11y: None,
    }
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

fn trusted_rows(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "recovery_trusted" => {
                Some(items.iter().map(|i| i.name.clone()).collect())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn button(screen: &ScreenModel, id: &str) -> Option<(String, Option<String>)> {
    screen.components.iter().find_map(|c| match c {
        Component::ButtonList { id: list, items } if list == "recovery_actions" => items
            .iter()
            .find(|i| i.id == id)
            .map(|i| (i.label.clone(), i.detail.clone())),
        _ => None,
    })
}

fn select(engine: &mut impl WorkflowEngine, list: &str, id: &str) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: list.into(),
        item_id: id.into(),
    })
}

fn six_trusted() -> RecoveryEngine {
    RecoveryEngine::new(
        ["Amira", "Priya", "Tomás", "Jonas", "Léa", "Kofi"]
            .iter()
            .map(|n| person(&format!("c-{n}"), n))
            .collect(),
        3,
    )
}

// @internal
#[test]
fn it_names_who_you_trust_and_how_many_are_needed() {
    let screen = six_trusted().current_screen();
    let texts = texts(&screen);
    assert!(
        texts.contains(
            &"If you lose every device, the contacts you trust can vouch for you in person. \
          3 of 6 are needed."
                .to_string()
        )
    );
    assert!(texts.contains(&"Trusted for recovery · 6".to_string()));
    assert_eq!(
        trusted_rows(&screen),
        ["Amira", "Priya", "Tomás", "Jonas", "Léa", "Kofi"]
    );
    assert!(screen.contextual_actions.is_empty());
}

// @internal
#[test]
fn guardians_are_never_told_and_the_screen_says_so() {
    for engine in [six_trusted(), RecoveryEngine::new(vec![], 3)] {
        assert!(
            texts(&engine.current_screen()).contains(&NOT_TOLD.to_string()),
            "the guardian text shows whatever the setup"
        );
    }
}

// @internal
#[test]
fn too_few_trusted_says_how_far_along() {
    let screen = RecoveryEngine::new(vec![person("c-1", "Amira")], 3).current_screen();
    assert!(
        texts(&screen).contains(
            &"If you lose every device, the contacts you trust can vouch for you in person. \
          3 are needed; you trust 1 so far."
                .to_string()
        )
    );
}

// @internal
#[test]
fn nobody_trusted_explains_how_to_add_someone() {
    let screen = RecoveryEngine::new(vec![], 3).current_screen();
    assert!(
        texts(&screen).contains(
            &"No one yet. Turn on Trust for recovery on the page of a contact you have met in \
          person."
                .to_string()
        )
    );
    assert!(trusted_rows(&screen).is_empty());
}

// @internal
#[test]
fn start_recovery_works_on_a_new_device_with_no_contacts() {
    let mut engine = RecoveryEngine::new(vec![], 3);
    assert_eq!(
        button(&engine.current_screen(), "start_recovery_process"),
        Some((
            "Start recovery".into(),
            Some("Lost every device? Set up Vauchi again, then start here.".into())
        ))
    );
    let result = select(&mut engine, "recovery_actions", "start_recovery_process");
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("start opens the claim step, got {result:?}");
    };
    assert!(
        screen
            .components
            .iter()
            .any(|c| matches!(c, Component::TextInput { id, .. } if id == "old_public_key"))
    );
}

// @internal
#[test]
fn how_recovery_works_is_one_tap_away() {
    let mut engine = six_trusted();
    match select(&mut engine, "recovery_actions", "how_it_works") {
        ActionResult::ShowInfoOverlay { title, body } => {
            assert_eq!(title, "How recovery works");
            assert!(body.contains("Create New Identity"), "{body}");
        }
        other => panic!("expected the how-it-works overlay, got {other:?}"),
    }
}

// @internal
#[test]
fn a_trusted_row_opens_the_contact() {
    let mut engine = six_trusted();
    assert_eq!(
        select(&mut engine, "recovery_trusted", "c-Priya"),
        ActionResult::OpenContact {
            contact_id: "c-Priya".into()
        }
    );
}

fn contact(vauchi: &Vauchi, name: &str) -> String {
    let identity = Identity::create(name, 0);
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    id
}

// @internal
#[test]
fn the_screen_counts_only_contacts_marked_trusted_against_the_real_threshold() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let amira = contact(&vauchi, "Amira");
    let _bob = contact(&vauchi, "Bob");
    vauchi.toggle_recovery_trust(&amira).unwrap();
    let threshold = vauchi.get_recovery_readiness().unwrap().threshold;

    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Recovery);

    assert_eq!(
        trusted_rows(&screen),
        ["Amira"],
        "Bob is a contact, not trusted"
    );
    assert!(texts(&screen).contains(&format!(
        "If you lose every device, the contacts you trust can vouch for you in person. \
         {threshold} are needed; you trust 1 so far."
    )));
}

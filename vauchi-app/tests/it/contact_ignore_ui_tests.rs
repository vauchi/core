// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ignore in the app layer (ADR-072): the detail screen offers the
//! toggle with Core-resolved labels, the `AppEngine` persists it, and
//! ignored contacts sort below active ones while staying searchable.

use vauchi_app::i18n::{Locale, get_string};
use vauchi_app::ui::{
    ActionResult, ActionStyle, AppEngine, AppScreen, Component, ContactDetailEngine, Field, Item,
    ScreenModel, UserAction, WorkflowEngine,
};
use vauchi_core::Identity;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

fn detail_engine(is_ignored: bool) -> ContactDetailEngine {
    let item = Item {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: None,
        initials: "A".into(),
        status: None,
        actions: vec![],
        a11y: None,
    };
    ContactDetailEngine::new(item, Vec::<Field>::new(), String::new()).with_ignored(is_ignored)
}

fn action_ids(screen: &ScreenModel) -> Vec<String> {
    screen
        .contextual_actions
        .iter()
        .map(|a| a.id.clone())
        .collect()
}

fn vauchi_with_contacts(names: &[&str]) -> (Vauchi, Vec<String>) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Owner").unwrap();
    let ids = names
        .iter()
        .map(|name| {
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
        })
        .collect();
    (vauchi, ids)
}

fn contact_names(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "contacts" => Some(items),
            _ => None,
        })
        .map(|items| items.iter().map(|i| i.name.clone()).collect())
        .unwrap_or_default()
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn detail_offers_ignore_with_core_resolved_label_for_an_active_contact() {
    let screen = detail_engine(false).current_screen();

    let ignore = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "ignore_contact")
        .expect("active contact must offer ignore_contact");
    assert_eq!(
        ignore.label,
        get_string(Locale::English, "contact_detail.ignore_contact_button")
    );
    assert_eq!(ignore.style, ActionStyle::Secondary);
    assert!(
        !action_ids(&screen).contains(&"unignore_contact".to_string()),
        "an active contact must not offer unignore"
    );
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn detail_offers_unignore_for_an_ignored_contact() {
    let screen = detail_engine(true).current_screen();

    let unignore = screen
        .contextual_actions
        .iter()
        .find(|a| a.id == "unignore_contact")
        .expect("ignored contact must offer unignore_contact");
    assert_eq!(
        unignore.label,
        get_string(Locale::English, "contact_detail.unignore_contact_button")
    );
    assert!(
        !action_ids(&screen).contains(&"ignore_contact".to_string()),
        "an ignored contact must not offer ignore again"
    );
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn pressing_ignore_on_detail_persists_and_flips_to_unignore() {
    let (vauchi, ids) = vauchi_with_contacts(&["Alice"]);
    let alice = ids[0].clone();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: alice.clone(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "ignore_contact".into(),
    });

    let ActionResult::UpdateScreen(screen) = result else {
        panic!("ignore must stay on the detail screen, got {result:?}");
    };
    assert!(action_ids(&screen).contains(&"unignore_contact".to_string()));
    assert!(
        engine
            .vauchi()
            .get_contact(&alice)
            .unwrap()
            .unwrap()
            .is_ignored(),
        "ignore must be persisted, not only flipped in memory"
    );
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn pressing_unignore_on_detail_persists_and_flips_back_to_ignore() {
    let (vauchi, ids) = vauchi_with_contacts(&["Alice"]);
    let alice = ids[0].clone();
    vauchi.ignore_contact(&alice).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: alice.clone(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unignore_contact".into(),
    });

    let ActionResult::UpdateScreen(screen) = result else {
        panic!("unignore must stay on the detail screen, got {result:?}");
    };
    assert!(action_ids(&screen).contains(&"ignore_contact".to_string()));
    assert!(
        !engine
            .vauchi()
            .get_contact(&alice)
            .unwrap()
            .unwrap()
            .is_ignored(),
        "unignore must be persisted"
    );
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn ignored_contacts_sort_after_active_ones_keeping_name_order_within_each_group() {
    let (vauchi, ids) = vauchi_with_contacts(&["Alice", "Bob", "Carol"]);
    vauchi.ignore_contact(&ids[0]).unwrap();
    vauchi.ignore_contact(&ids[2]).unwrap();
    let mut engine = AppEngine::new(vauchi);

    let screen = engine.navigate_to(AppScreen::Contacts);

    assert_eq!(contact_names(&screen), ["Bob", "Alice", "Carol"]);
}

// @scenario: release_privacy_multidevice_certification :: Ignoring a contact removes attention but keeps continuity
#[test]
fn ignored_contact_stays_findable_by_search() {
    let (vauchi, ids) = vauchi_with_contacts(&["Alice", "Bob"]);
    vauchi.ignore_contact(&ids[0]).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    let result = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "ali".into(),
    });

    let ActionResult::UpdateScreen(screen) = result else {
        panic!("search must update the list, got {result:?}");
    };
    assert_eq!(contact_names(&screen), ["Alice"]);
}

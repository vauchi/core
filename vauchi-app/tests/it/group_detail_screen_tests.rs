// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The group screen reads like the Groups list (#446, owner 2026-09-30:
//! "the app should be coherent"): Contacts and "Sees N entries" with the
//! list's effective count, and an entry shown to everyone says so instead
//! of reading as hidden from the group.

use vauchi_app::ui::group_detail::{GroupDetailEngine, GroupFieldVisibility};
use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ScreenModel, UserAction, WorkflowEngine,
};
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

fn entry(id: &str, is_visible: bool, shown_to_everyone: bool) -> GroupFieldVisibility {
    GroupFieldVisibility {
        field_id: id.into(),
        label: id.into(),
        value: format!("{id}@example.org"),
        is_visible,
        shown_to_everyone,
        stop_seeing_if_granted: 0,
    }
}

fn info(screen: &ScreenModel) -> Vec<(String, String)> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::InfoPanel { id, items, .. } if id == "group_info" => Some(
                items
                    .iter()
                    .map(|i| (i.title.clone(), i.detail.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

type Toggle = (String, bool, Option<String>, Option<String>);

fn toggles(screen: &ScreenModel) -> (String, Vec<Toggle>) {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ToggleList {
                id, label, items, ..
            } if id == "field_visibility" => Some((
                label.clone(),
                items
                    .iter()
                    .map(|i| {
                        (
                            i.id.clone(),
                            i.selected,
                            i.subtitle.clone(),
                            i.a11y.as_ref().and_then(|a| a.hint.clone()),
                        )
                    })
                    .collect(),
            )),
            _ => None,
        })
        .expect("the entries toggle list")
}

fn engine() -> GroupDetailEngine {
    GroupDetailEngine::new("g1".into(), "Family".into(), vec![]).with_field_visibility(vec![
        entry("granted", true, false),
        entry("public", false, true),
        entry("private", false, false),
    ])
}

// @internal
#[test]
fn the_group_counts_contacts_and_the_entries_it_sees() {
    assert_eq!(
        info(&engine().current_screen()),
        [
            ("Contacts".to_string(), "0".to_string()),
            ("Sees".to_string(), "2 entries".to_string()),
        ],
        "an entry shown to everyone is seen by the group too"
    );
}

// @internal
#[test]
fn an_entry_shown_to_everyone_says_so() {
    let (label, items) = toggles(&engine().current_screen());
    assert_eq!(label, "Entries this group sees");
    assert_eq!(
        items,
        [
            (
                "granted".to_string(),
                true,
                Some("granted@example.org".to_string()),
                Some("Visible to this group".to_string()),
            ),
            (
                "public".to_string(),
                false,
                Some("public@example.org · Shown to everyone".to_string()),
                Some("Shown to everyone, including this group".to_string()),
            ),
            (
                "private".to_string(),
                false,
                Some("private@example.org".to_string()),
                Some("Hidden from this group".to_string()),
            ),
        ]
    );
}

fn toggle(engine: &mut impl WorkflowEngine, entry: &str) -> ActionResult {
    engine.handle_action(UserAction::ItemToggled {
        component_id: "field_visibility".into(),
        item_id: entry.into(),
    })
}

fn press(engine: &mut impl WorkflowEngine, action: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action.into(),
    })
}

fn confirmation(screen: &ScreenModel) -> Option<(String, String)> {
    screen.components.iter().find_map(|c| match c {
        Component::InlineConfirm {
            id,
            warning,
            confirm_text,
            ..
        } if id == "grant_entry" => Some((warning.clone(), confirm_text.clone())),
        _ => None,
    })
}

fn public_entry_losing(others: usize) -> GroupDetailEngine {
    let mut public = entry("public", false, true);
    public.stop_seeing_if_granted = others;
    GroupDetailEngine::new("g1".into(), "Family".into(), vec![])
        .with_field_visibility(vec![public, entry("private", false, false)])
}

// @internal
#[test]
fn giving_a_group_an_entry_everyone_sees_asks_first() {
    let mut engine = public_entry_losing(12);
    let asked = toggle(&mut engine, "public");
    let ActionResult::UpdateScreen(screen) = asked else {
        panic!("nothing is saved before the owner confirms, got {asked:?}");
    };
    assert_eq!(
        confirmation(&screen),
        Some((
            "Only Family will see this entry. 12 other contacts will stop seeing it.".into(),
            "Show only to Family".into()
        ))
    );
    let (_, items) = toggles(&screen);
    assert!(!items[0].1, "the toggle stays off until confirmed");

    assert_eq!(
        press(&mut engine, "confirm_grant_entry"),
        ActionResult::SetGroupFieldVisibility {
            group_id: "g1".into(),
            field_id: "public".into(),
            visible: true,
        }
    );
    assert_eq!(confirmation(&engine.current_screen()), None);
}

// @internal
#[test]
fn cancelling_keeps_the_entry_for_everyone() {
    let mut engine = public_entry_losing(1);
    let _ = toggle(&mut engine, "public");
    assert_eq!(
        confirmation(&engine.current_screen()).map(|c| c.0),
        Some("Only Family will see this entry. 1 other contact will stop seeing it.".into())
    );
    let cancelled = press(&mut engine, "cancel_grant_entry");
    let ActionResult::UpdateScreen(screen) = cancelled else {
        panic!("cancel re-renders, got {cancelled:?}");
    };
    assert_eq!(confirmation(&screen), None);
    assert!(!toggles(&screen).1[0].1);
}

// @internal
#[test]
fn no_confirmation_when_nobody_would_lose_the_entry() {
    let mut nobody_loses = public_entry_losing(0);
    assert!(matches!(
        toggle(&mut nobody_loses, "public"),
        ActionResult::SetGroupFieldVisibility { visible: true, .. }
    ));
    let mut hidden = public_entry_losing(12);
    assert!(matches!(
        toggle(&mut hidden, "private"),
        ActionResult::SetGroupFieldVisibility { visible: true, .. }
    ));
}

fn own_field(vauchi: &Vauchi, label: &str) -> String {
    let field = ContactField::new(FieldType::Email, label, &format!("{label}@example.org"), 0);
    let id = field.id().to_string();
    vauchi.add_own_field(field).unwrap();
    id
}

// @internal
#[test]
fn the_group_screen_and_the_list_agree() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let granted = own_field(&vauchi, "granted");
    let public = own_field(&vauchi, "public");
    let private = own_field(&vauchi, "private");
    vauchi.set_own_field_public(&public).unwrap();
    vauchi.set_own_field_private(&private).unwrap();
    let family = vauchi.create_group("Family").unwrap().id().to_string();
    vauchi
        .set_group_field_visibility(&family, &granted, true)
        .unwrap();
    let identity = Identity::create("Léa", 0);
    let lea = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new("Léa"),
        SymmetricKey::generate(),
        0,
    );
    let lea_id = lea.id().to_string();
    vauchi.add_contact(lea).unwrap();
    vauchi.add_contact_to_group(&family, &lea_id).unwrap();

    let mut engine = AppEngine::new(vauchi);
    let list = engine.navigate_to(AppScreen::Groups);
    let detail = engine.navigate_to(AppScreen::GroupDetail {
        group_id: family.clone(),
    });

    let row = list
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { items, .. } => items.first().and_then(|i| i.detail.clone()),
            _ => None,
        })
        .expect("Family row");
    assert_eq!(row, "1 contact · sees 2 entries");
    assert_eq!(
        info(&detail),
        [
            ("Contacts".to_string(), "1".to_string()),
            ("Sees".to_string(), "2 entries".to_string()),
        ]
    );
    let (_, items) = toggles(&detail);
    let public_row = items.iter().find(|t| t.0 == public).expect("public row");
    assert_eq!(
        public_row.2.as_deref(),
        Some("public@example.org · Shown to everyone")
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
fn the_count_names_who_loses_it_and_confirming_takes_it_from_them() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let public = own_field(&vauchi, "public");
    vauchi.set_own_field_public(&public).unwrap();
    let family = vauchi.create_group("Family").unwrap().id().to_string();
    let work = vauchi.create_group("Work").unwrap().id().to_string();
    let lea = contact(&vauchi, "Léa");
    let bob = contact(&vauchi, "Bob");
    let carol = contact(&vauchi, "Carol");
    let dan = contact(&vauchi, "Dan");
    vauchi.add_contact_to_group(&family, &lea).unwrap();
    vauchi.add_contact_to_group(&work, &carol).unwrap();
    vauchi
        .set_contact_visibility_override(&dan, &public, true)
        .unwrap();

    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::GroupDetail {
        group_id: family.clone(),
    });
    let _ = toggle(&mut engine, &public);
    assert_eq!(
        confirmation(&engine.current_screen()).map(|c| c.0),
        Some("Only Family will see this entry. 2 other contacts will stop seeing it.".into()),
        "Bob and Carol lose it; Léa is in Family and Dan's override keeps it"
    );

    let _ = press(&mut engine, "confirm_grant_entry");
    let sees = |contact: &str| {
        engine
            .vauchi()
            .get_effective_field_visibility(contact, &public)
            .unwrap()
    };
    assert_eq!(
        [sees(&lea), sees(&bob), sees(&carol), sees(&dan)],
        [true, false, false, true]
    );
}

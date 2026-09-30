// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The group screen reads like the Groups list (#446, owner 2026-09-30:
//! "the app should be coherent"): Contacts and "Sees N entries" with the
//! list's effective count, and an entry shown to everyone says so instead
//! of reading as hidden from the group.

use vauchi_app::ui::group_detail::{GroupDetailEngine, GroupFieldVisibility};
use vauchi_app::ui::{AppEngine, AppScreen, Component, ScreenModel, WorkflowEngine};
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

fn entry(id: &str, is_visible: bool, shown_to_everyone: bool) -> GroupFieldVisibility {
    GroupFieldVisibility {
        field_id: id.into(),
        label: id.into(),
        value: format!("{id}@example.org"),
        is_visible,
        shown_to_everyone,
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

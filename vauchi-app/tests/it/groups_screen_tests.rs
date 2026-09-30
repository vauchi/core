// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Groups screen (#446, design pass #419 item 6; owner decisions
//! 2026-09-30): each row says how many contacts a group has and how many
//! of your entries they see — the effective view, entries shown to
//! everyone included — with plain singular/plural counts; "Add group" is a
//! button in the body, not a toolbar action; with no groups, one sentence
//! says what groups do.

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, GroupInfo, GroupsEngine, ScreenModel,
    UserAction, WorkflowEngine,
};
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

fn group(id: &str, name: &str, member_count: usize, entries_seen: usize) -> GroupInfo {
    GroupInfo {
        id: id.into(),
        name: name.into(),
        member_count,
        entries_seen,
    }
}

fn rows(screen: &ScreenModel) -> Vec<(String, Option<String>)> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ActionList { id, items } if id == "groups" => Some(
                items
                    .iter()
                    .map(|i| (i.label.clone(), i.detail.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn add_group_label(screen: &ScreenModel) -> Option<String> {
    screen.components.iter().find_map(|c| match c {
        Component::ButtonList { id, items } if id == "group_actions" => items
            .iter()
            .find(|i| i.id == "add_group")
            .map(|i| i.label.clone()),
        _ => None,
    })
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

// @internal
#[test]
fn each_row_counts_contacts_and_the_entries_they_see() {
    let screen = GroupsEngine::new(vec![
        group("g1", "Family", 3, 5),
        group("g2", "Solo", 1, 1),
        group("g3", "New", 0, 0),
    ])
    .current_screen();

    assert_eq!(
        rows(&screen),
        [
            ("Family".into(), Some("3 contacts · sees 5 entries".into())),
            ("Solo".into(), Some("1 contact · sees 1 entry".into())),
            ("New".into(), Some("0 contacts · sees 0 entries".into())),
        ]
    );
}

// @internal
#[test]
fn rows_are_localized() {
    let screen = GroupsEngine::new(vec![group("g1", "Familie", 3, 1)])
        .with_locale(Locale::German)
        .current_screen();
    assert_eq!(
        rows(&screen),
        [(
            "Familie".into(),
            Some("3 Kontakte · sieht 1 Eintrag".into())
        )]
    );
}

// @internal
#[test]
fn both_counts_on_every_row_leave_no_view_switch() {
    let screen = GroupsEngine::new(vec![group("g1", "Family", 3, 5)]).current_screen();
    assert!(
        !screen
            .components
            .iter()
            .any(|c| matches!(c, Component::Dropdown { .. })),
        "the Members/Visibility switch only swapped the row text: {:?}",
        screen.components
    );
}

// @internal
#[test]
fn add_group_is_a_body_button_not_a_toolbar_action() {
    let mut engine = GroupsEngine::new(vec![group("g1", "Family", 3, 5)]);
    let screen = engine.current_screen();

    assert_eq!(add_group_label(&screen).as_deref(), Some("Add group"));
    assert!(
        screen.contextual_actions.is_empty(),
        "shells fold toolbar actions into menus: {:?}",
        screen.contextual_actions
    );
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "group_actions".into(),
        item_id: "add_group".into(),
    });
    assert_eq!(
        result,
        ActionResult::ShowFormDialog {
            dialog_type: "create_group".into(),
            context_id: None,
        }
    );
}

// @internal
#[test]
fn no_groups_says_what_groups_do_and_offers_add_group() {
    let screen = GroupsEngine::new(vec![]).current_screen();
    assert_eq!(
        texts(&screen),
        ["Groups decide which of your entries each contact sees."]
    );
    assert_eq!(add_group_label(&screen).as_deref(), Some("Add group"));
    assert!(rows(&screen).is_empty());
}

// @internal
#[test]
fn a_row_opens_its_group() {
    let mut engine = GroupsEngine::new(vec![group("g1", "Family", 3, 5)]);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "groups".into(),
        item_id: "g1".into(),
    });
    assert_eq!(
        result,
        ActionResult::OpenContact {
            contact_id: "g1".into()
        }
    );
}

fn own_field(vauchi: &Vauchi, label: &str) -> String {
    let field = ContactField::new(FieldType::Email, label, &format!("{label}@example.org"), 0);
    let id = field.id().to_string();
    vauchi.add_own_field(field).unwrap();
    id
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
fn sees_counts_what_a_member_is_actually_shown() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let granted = own_field(&vauchi, "granted");
    let public = own_field(&vauchi, "public");
    let private = own_field(&vauchi, "private");
    let other_groups = own_field(&vauchi, "work");
    vauchi.set_own_field_public(&public).unwrap();
    vauchi.set_own_field_private(&private).unwrap();
    vauchi.set_own_field_public(&other_groups).unwrap();

    let family = vauchi.create_group("Family").unwrap().id().to_string();
    let work = vauchi.create_group("Work").unwrap().id().to_string();
    vauchi
        .set_group_field_visibility(&family, &granted, true)
        .unwrap();
    vauchi
        .set_group_field_visibility(&work, &other_groups, true)
        .unwrap();
    let lea = contact(&vauchi, "Léa");
    vauchi.add_contact_to_group(&family, &lea).unwrap();

    let shown_to_lea = [&granted, &public, &private, &other_groups]
        .into_iter()
        .filter(|f| vauchi.get_effective_field_visibility(&lea, f).unwrap())
        .count();

    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::Groups);
    let family_row = rows(&screen)
        .into_iter()
        .find(|(name, _)| name == "Family")
        .expect("Family row");

    assert_eq!(
        shown_to_lea, 2,
        "the grant plus the entry shown to everyone"
    );
    assert_eq!(
        family_row.1.as_deref(),
        Some("1 contact · sees 2 entries"),
        "the row must match what Léa is shown"
    );
}

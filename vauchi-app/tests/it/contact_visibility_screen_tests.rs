// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The per-contact "Visibility: <name>" screen controls which of YOUR
//! entries that contact receives (#428). It listed the contact's own card
//! fields with every switch on, and saving wrote overrides for ids that
//! are not yours, so nothing changed. Each switch now shows the real state
//! and the reason, as the ContactVisibility artboard draws it, with an
//! intro that states the real precedence (owner decision 2026-09-28).

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ScreenModel, ToggleItem, UserAction,
    WorkflowEngine,
};
use vauchi_core::Identity;
use vauchi_core::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;

const NOW: u64 = 1_790_000_000;

struct World {
    vauchi: Vauchi,
    grace: String,
    alan: String,
    mobile: String,
    email: String,
    birthday: String,
    home: String,
}

fn exchanged(vauchi: &Vauchi, name: &str) -> String {
    let peer = Identity::create(name, NOW);
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(
        FieldType::Phone,
        "their phone",
        "+1 555 0100",
        NOW,
    ))
    .expect("their field");
    let contact = Contact::from_exchange(
        *peer.signing_public_key(),
        card,
        SymmetricKey::generate(),
        NOW,
    );
    let id = contact.id().to_string();
    vauchi.add_contact(contact).expect("add contact");
    id
}

fn own(vauchi: &Vauchi, kind: FieldType, label: &str, value: &str) -> String {
    let field = ContactField::new(kind, label, value, NOW);
    let id = field.id().to_string();
    vauchi.add_own_field(field).expect("own field");
    id
}

fn world() -> World {
    let mut vauchi = Vauchi::in_memory().expect("in-memory Core");
    vauchi.create_identity("Ada").expect("identity");
    let mobile = own(&vauchi, FieldType::Phone, "mobile", "+41 79 000 00 00");
    let email = own(&vauchi, FieldType::Email, "email", "ada@example.org");
    let birthday = own(&vauchi, FieldType::Birthday, "birthday", "1815-12-10");
    let home = own(&vauchi, FieldType::Address, "home", "12 St James's Square");
    let grace = exchanged(&vauchi, "Grace");
    let alan = exchanged(&vauchi, "Alan");

    let family = vauchi.create_group("Family").expect("family");
    let cycling = vauchi.create_group("Cycling club").expect("cycling");
    vauchi
        .add_contact_to_group(family.id(), &grace)
        .expect("member");
    vauchi
        .set_group_field_visibility(family.id(), &mobile, true)
        .expect("grant mobile");
    vauchi
        .set_group_field_visibility(cycling.id(), &birthday, true)
        .expect("grant birthday");
    vauchi.set_field_shown(&email, true).expect("email to all");
    vauchi.set_field_shown(&home, false).expect("home hidden");
    World {
        vauchi,
        grace,
        alan,
        mobile,
        email,
        birthday,
        home,
    }
}

fn open(vauchi: Vauchi, contact_id: &str) -> AppEngine {
    let mut engine = AppEngine::new(vauchi);
    // The real route: contact detail, then its visibility screen.
    let _ = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.to_string(),
    });
    let _ = engine.navigate_to(AppScreen::ContactVisibility {
        contact_id: contact_id.to_string(),
    });
    engine
}

fn toggles(screen: &ScreenModel) -> Vec<ToggleItem> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ToggleList { id, items, .. } if id == "field_toggles" => Some(items.clone()),
            _ => None,
        })
        .expect("the field toggles")
}

fn toggle<'a>(items: &'a [ToggleItem], id: &str) -> &'a ToggleItem {
    items
        .iter()
        .find(|i| i.id == id)
        .expect("a toggle for the entry")
}

fn texts(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Text { content, .. } => Some(content.clone()),
            Component::Banner { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

// @internal
#[test]
fn the_screen_lists_your_entries_not_the_contacts_fields() {
    let w = world();
    let engine = open(w.vauchi, &w.grace);

    let mut ids: Vec<String> = toggles(&engine.current_screen())
        .into_iter()
        .map(|t| t.id)
        .collect();
    ids.sort();
    let mut expected = vec![w.mobile, w.email, w.birthday, w.home];
    expected.sort();
    assert_eq!(ids, expected);
}

// @internal
#[test]
fn each_switch_shows_what_the_contact_really_receives() {
    let w = world();
    let engine = open(w.vauchi, &w.grace);
    let items = toggles(&engine.current_screen());

    for field in [&w.mobile, &w.email, &w.birthday, &w.home] {
        let sent = engine
            .vauchi()
            .get_effective_field_visibility(&w.grace, field)
            .expect("effective visibility");
        assert_eq!(toggle(&items, field).selected, sent, "field {field}");
    }
}

// @internal
#[test]
fn each_entry_says_why_the_contact_sees_it_or_not() {
    let w = world();
    let engine = open(w.vauchi, &w.grace);
    let items = toggles(&engine.current_screen());
    let reason = |id: &str| toggle(&items, id).subtitle.clone();

    assert_eq!(reason(&w.mobile).as_deref(), Some("Visible via Family"));
    assert_eq!(
        reason(&w.birthday).as_deref(),
        Some("Hidden: only Cycling club can see it")
    );
    assert_eq!(reason(&w.home).as_deref(), Some("Hidden from Grace"));
    let email_reason = reason(&w.email);
    assert!(
        matches!(
            email_reason.as_deref(),
            Some("Visible to all contacts") | Some("Hidden from Grace")
        ),
        "{email_reason:?}"
    );
}

// @internal
#[test]
fn the_intro_states_that_a_switch_here_wins_over_groups() {
    let w = world();
    let grace = open(w.vauchi, &w.grace);

    assert!(
        texts(&grace.current_screen()).iter().any(|t| t
            == "Grace is in Family. A switch you change here applies to Grace only \
                and takes precedence over group rules."),
        "{:?}",
        texts(&grace.current_screen())
    );
}

// @internal
#[test]
fn a_contact_in_no_group_gets_no_group_note() {
    let w = world();
    let alan = open(w.vauchi, &w.alan);

    assert!(
        !texts(&alan.current_screen())
            .iter()
            .any(|t| t.contains("takes precedence over group rules")),
    );
}

// @internal
#[test]
fn the_screen_says_changes_reach_the_contact_on_the_next_sync_with_help() {
    let w = world();
    let mut engine = open(w.vauchi, &w.grace);
    let screen = engine.current_screen();

    let (text, action_id) = screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::Banner {
                text, action_id, ..
            } => Some((text.clone(), action_id.clone())),
            _ => None,
        })
        .expect("the next-sync note");
    assert_eq!(text, "What changes here reaches Grace on the next sync.");

    match engine.handle_action(UserAction::ActionPressed { action_id }) {
        ActionResult::ShowInfoOverlay { title, .. } => {
            assert_eq!(title, "How visibility is decided")
        }
        other => panic!("Help should explain the rule, got {other:?}"),
    }
}

// @internal
#[test]
fn hiding_an_entry_from_one_contact_takes_effect_and_says_so() {
    let w = world();
    let grace = w.grace.clone();
    let mobile = w.mobile.clone();
    let mut engine = open(w.vauchi, &grace);

    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "field_toggles".into(),
        item_id: mobile.clone(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "save".into(),
    });

    assert!(
        !engine
            .vauchi()
            .get_effective_field_visibility(&grace, &mobile)
            .expect("effective visibility"),
        "Grace must no longer receive the mobile entry"
    );
    let _ = engine.navigate_to(AppScreen::ContactVisibility {
        contact_id: grace.clone(),
    });
    let items = toggles(&engine.current_screen());
    assert!(!toggle(&items, &mobile).selected);
    assert_eq!(
        toggle(&items, &mobile).subtitle.as_deref(),
        Some("Set for Grace: hidden")
    );
}

// @internal
#[test]
fn the_reason_reaches_the_shells_as_visible_and_spoken_text() {
    use vauchi_app::ui::PreparedSurface;
    use vauchi_core::{Command, PresentationNode, SurfaceId};

    let w = world();
    let engine = open(w.vauchi, &w.grace);
    let screen = engine.current_screen();
    let prepared =
        PreparedSurface::from_screen(SurfaceId::new("contact.visibility").unwrap(), 1, &screen)
            .expect("projection");
    let Command::ReplaceSurface { surface } = prepared.command() else {
        panic!("expected ReplaceSurface");
    };
    let rows = surface
        .nodes
        .iter()
        .find_map(|n| match n {
            PresentationNode::List { rows, .. } if rows.iter().any(|r| r.title == "mobile") => {
                Some(rows.clone())
            }
            _ => None,
        })
        .expect("the entries project as rows");
    let row = rows
        .iter()
        .find(|r| r.title == "mobile")
        .expect("mobile row");

    assert_eq!(row.subtitle.as_deref(), Some("Visible via Family"));
    assert_eq!(
        row.accessibility.description.as_deref(),
        Some("Visible via Family")
    );
    assert!(
        matches!(
            row.controls.as_slice(),
            [PresentationNode::Toggle { value: true, .. }]
        ),
        "{:?}",
        row.controls
    );
}

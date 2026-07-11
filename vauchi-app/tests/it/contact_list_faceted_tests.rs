// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Faceted contact search (ADR-051, Phase 4b.2).
//!
//! Engine-level: the facet ToggleList + the faceted-id restriction. The
//! query→facet→core round-trip is driven through a real `Vauchi` so it
//! exercises `Vauchi::search_contacts_faceted` (the canonical matcher).

use vauchi_app::ui::{
    AppEngine, AppScreen, Component, ContactListEngine, IndexedItem, Item, UserAction,
    WorkflowEngine,
};
use vauchi_core::api::Vauchi;

fn item(id: &str, name: &str) -> IndexedItem {
    IndexedItem::from(Item {
        id: id.into(),
        name: name.into(),
        subtitle: None,
        initials: name.chars().next().unwrap_or('?').to_string(),
        status: None,
        actions: vec![],
        a11y: None,
    })
}

fn contact_names(screen: &vauchi_app::ui::ScreenModel) -> Vec<String> {
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

fn facet_items(screen: &vauchi_app::ui::ScreenModel) -> Vec<(String, bool)> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ToggleList { id, items, .. } if id == "search_facets" => Some(items),
            _ => None,
        })
        .map(|items| items.iter().map(|i| (i.id.clone(), i.selected)).collect())
        .unwrap_or_default()
}

// ── Engine-level ───────────────────────────────────────────────────────────

// @internal
#[test]
fn renders_search_facets_toggle_default_off() {
    let engine = ContactListEngine::new(vec![item("a", "Alice")]);
    assert_eq!(
        facet_items(&engine.current_screen()),
        vec![
            ("tags".to_string(), false),
            ("comment".to_string(), false),
            ("place".to_string(), false),
        ]
    );
}

// @internal
#[test]
fn set_faceted_ids_restricts_the_list() {
    let mut engine = ContactListEngine::new(vec![
        item("a", "Alice"),
        item("b", "Bob"),
        item("c", "Carol"),
    ]);
    assert_eq!(contact_names(&engine.current_screen()).len(), 3);

    engine.set_faceted_ids(Some(vec!["a".into(), "c".into()]));
    assert_eq!(
        contact_names(&engine.current_screen()),
        vec!["Alice".to_string(), "Carol".to_string()],
        "faceted mode shows exactly the core result set"
    );

    engine.set_faceted_ids(None);
    assert_eq!(
        contact_names(&engine.current_screen()).len(),
        3,
        "clearing faceted ids reverts to the full list"
    );
}

// @internal
#[test]
fn toggle_facet_flips_only_that_flag() {
    let mut engine = ContactListEngine::new(vec![item("a", "Alice")]);
    assert_eq!(engine.facet_flags(), (false, false, false));
    engine.toggle_facet("comment");
    assert_eq!(engine.facet_flags(), (false, true, false));
    assert!(engine.any_facet());
    engine.toggle_facet("comment");
    assert_eq!(engine.facet_flags(), (false, false, false));
    assert!(!engine.any_facet());
}

// ── Intercept / integration ────────────────────────────────────────────────

fn engine_with_tagged_bob() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Me").unwrap();
    vauchi
        .import_contacts_from_vcf(
            b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Alice\r\nEND:VCARD\r\n\
              BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n",
        )
        .unwrap();
    let bob_id = vauchi
        .list_contacts()
        .unwrap()
        .into_iter()
        .find(|c| c.display_name() == "Bob")
        .unwrap()
        .id()
        .to_string();

    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail { contact_id: bob_id });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "add_tag:climbing".into(),
    });
    engine
}

// @internal
#[test]
fn enabling_tags_facet_finds_contact_by_tag_name() {
    let mut engine = engine_with_tagged_bob();
    engine.navigate_to(AppScreen::Contacts);

    // Plain name search for "climb" matches nobody (no name contains it).
    let s = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "climb".into(),
    });
    let names = match s {
        vauchi_app::ui::ActionResult::UpdateScreen(s) => contact_names(&s),
        other => panic!("expected UpdateScreen, got {other:?}"),
    };
    assert!(
        names.is_empty(),
        "name-only search must not match the tag, got {names:?}"
    );

    // Enable the Tags facet → core search matches Bob via his tag.
    let s = engine.handle_action(UserAction::ItemToggled {
        component_id: "search_facets".into(),
        item_id: "tags".into(),
    });
    let names = match s {
        vauchi_app::ui::ActionResult::UpdateScreen(s) => contact_names(&s),
        other => panic!("expected UpdateScreen, got {other:?}"),
    };
    assert_eq!(
        names,
        vec!["Bob".to_string()],
        "tags facet finds the contact by tag name"
    );
}

// @internal
#[test]
fn disabling_all_facets_reverts_to_name_search() {
    let mut engine = engine_with_tagged_bob();
    engine.navigate_to(AppScreen::Contacts);
    let _ = engine.handle_action(UserAction::SearchChanged {
        component_id: "contacts".into(),
        query: "climb".into(),
    });
    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "search_facets".into(),
        item_id: "tags".into(),
    });

    // Turn the tags facet back off → query "climb" reverts to name-only,
    // which matches nobody.
    let s = engine.handle_action(UserAction::ItemToggled {
        component_id: "search_facets".into(),
        item_id: "tags".into(),
    });
    let names = match s {
        vauchi_app::ui::ActionResult::UpdateScreen(s) => contact_names(&s),
        other => panic!("expected UpdateScreen, got {other:?}"),
    };
    assert!(
        names.is_empty(),
        "disabling the facet reverts to name search, got {names:?}"
    );
}

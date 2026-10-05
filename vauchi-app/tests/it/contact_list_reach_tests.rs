// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Every contact is reachable through the presentation protocol alone
//! (vauchi/private#482). Core sends the Contacts list 200 rows at a time;
//! nothing a shell could send reached the next window or the search, so on
//! every shell contact 201 and later could not be opened and no one could
//! search. Core now draws both with primitives every shell already renders:
//! a search input above the list and a row that shows the next contacts.

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::{
    BindingId, Command, Event, InputValue, InteractionId, PresentationNode, SurfaceId,
};

fn engine_with_contacts(count: usize) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut vcf = String::new();
    for i in 0..count {
        vcf.push_str(&format!(
            "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Person {i:03}\r\nEND:VCARD\r\n"
        ));
    }
    vauchi.import_contacts_from_vcf(vcf.as_bytes()).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);
    engine
}

/// The Contacts surface's id and nodes from the last `ReplaceSurface` in
/// the batch that carries the contacts list.
fn contacts_surface(commands: &[Command]) -> (SurfaceId, Vec<PresentationNode>) {
    commands
        .iter()
        .rev()
        .find_map(|command| {
            match command {
            Command::ReplaceSurface { surface }
                if surface.nodes.iter().any(|node| {
                    matches!(node, PresentationNode::List { id, .. } if id.as_str() == "contacts")
                }) =>
            {
                Some((surface.surface_id.clone(), surface.nodes.clone()))
            }
            _ => None,
        }
        })
        .expect("the batch must carry the Contacts surface")
}

fn contact_names(nodes: &[PresentationNode]) -> Vec<String> {
    nodes
        .iter()
        .find_map(|node| match node {
            PresentationNode::List { id, rows, .. } if id.as_str() == "contacts" => {
                Some(rows.iter().map(|row| row.title.clone()).collect())
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// Every activatable row outside the contacts list itself, by title.
fn other_rows(nodes: &[PresentationNode]) -> Vec<(String, InteractionId)> {
    nodes
        .iter()
        .filter_map(|node| match node {
            PresentationNode::List { id, rows, .. } if id.as_str() != "contacts" => Some(rows),
            _ => None,
        })
        .flatten()
        .filter_map(|row| {
            row.activation
                .as_ref()
                .map(|action| (row.title.clone(), action.interaction_id.clone()))
        })
        .collect()
}

fn search_binding(nodes: &[PresentationNode]) -> Option<BindingId> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Input { binding_id, .. } => Some(binding_id.clone()),
        _ => None,
    })
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn a_list_longer_than_its_window_offers_the_next_contacts() {
    let mut engine = engine_with_contacts(250);
    let (surface_id, nodes) = contacts_surface(&engine.initial_commands().unwrap());
    assert_eq!(contact_names(&nodes).len(), 200);

    let (title, interaction_id) = other_rows(&nodes)
        .into_iter()
        .find(|(title, _)| title.starts_with("Show contacts"))
        .expect("a row shows the contacts past the first 200");
    assert_eq!(title, "Show contacts 51–250");

    let commands = engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id,
        })
        .expect("the row activates");
    let (_, nodes) = contacts_surface(&commands);
    let names = contact_names(&nodes);
    assert!(names.contains(&"Person 249".to_owned()), "got {names:?}");

    let back = other_rows(&nodes)
        .into_iter()
        .map(|(title, _)| title)
        .find(|title| title.starts_with("Show contacts"))
        .expect("the second window offers the first contacts again");
    assert_eq!(back, "Show contacts 1–200");
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn typing_in_the_search_finds_a_contact_past_the_first_window() {
    let mut engine = engine_with_contacts(250);
    let (surface_id, nodes) = contacts_surface(&engine.initial_commands().unwrap());
    let binding_id = search_binding(&nodes).expect("Contacts offers a search input");

    let commands = engine
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id,
            value: InputValue::Text("Person 249".into()),
        })
        .expect("the search accepts text");
    let (_, nodes) = contacts_surface(&commands);
    assert_eq!(contact_names(&nodes), vec!["Person 249".to_owned()]);
}

// @internal
#[test]
fn a_list_within_its_window_offers_no_next_row() {
    let mut engine = engine_with_contacts(3);
    let (_, nodes) = contacts_surface(&engine.initial_commands().unwrap());

    assert_eq!(contact_names(&nodes).len(), 3);
    assert!(
        other_rows(&nodes)
            .iter()
            .all(|(title, _)| !title.starts_with("Show contacts")),
        "three contacts fit in one window"
    );
    assert!(
        search_binding(&nodes).is_some(),
        "search is there regardless"
    );
}

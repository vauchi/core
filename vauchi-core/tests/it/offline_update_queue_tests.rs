// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! With no relay reachable, an edit waits on the device and the app says so.

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::linked_peer::link_contacts;
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::{Command, PresentationNode};

const OFFLINE_NOTICE: &str = "You're offline. Changes will sync when you reconnect.";

fn status_titles(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.nodes.clone()),
            _ => None,
        })
        .flatten()
        .filter_map(|node| match node {
            PresentationNode::Status { title, .. } => Some(title),
            _ => None,
        })
        .collect()
}

// @scenario: relay_network :: No relays available
#[test]
fn an_edit_made_offline_is_queued_and_the_app_says_it_will_sync_later() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let linked = link_contacts(&bob, &alice, "Alice");
    let mut engine = AppEngine::new(alice);
    engine.set_network_online(false);

    engine
        .vauchi()
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "Mobile",
            "+41 79 000 00 00",
            0,
        ))
        .unwrap();
    let mobile = engine.vauchi().own_card().unwrap().unwrap().fields()[0]
        .id()
        .to_string();
    engine.vauchi().set_own_field_public(&mobile).unwrap();
    engine.vauchi().run_owed_repropagation().unwrap();
    engine.navigate_to(AppScreen::Contacts);
    let shown = engine.initial_commands().unwrap();

    let queued_for_bob = engine
        .vauchi()
        .storage()
        .pending()
        .get_pending_updates(&linked.host_contact_id)
        .unwrap();
    assert_eq!(queued_for_bob.len(), 1);
    assert!(
        status_titles(&shown).contains(&OFFLINE_NOTICE.to_string()),
        "{shown:#?}"
    );
}

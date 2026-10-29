// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The trust a contact's screen shows and the proposal trust it lets me
//! grant, driven through the events the shells report.

use crate::event_path_support::{activate, row, surfaces};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::{
    BindingId, Command, Contact, ContactCard, Event, Identity, InputValue, PresentationNode,
    SurfaceId, SymmetricKey,
};

fn exchanged(name: &str) -> Contact {
    let peer = Identity::create(name, 0);
    Contact::from_exchange(
        *peer.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    )
}

fn screen_of(contact: Contact) -> (AppEngine, Vec<Command>) {
    let name = contact.display_name().to_string();
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.add_contact(contact).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);
    let list = engine.initial_commands().unwrap();
    let (surface_id, contact) = row(&list, &name);
    let screen = activate(&mut engine, surface_id, &contact);
    (engine, screen)
}

/// The Trust line as (level shown, whether it can be tapped).
fn trust(commands: &[Command]) -> (Option<String>, bool) {
    surfaces(commands)
        .into_iter()
        .flat_map(|(_, nodes)| nodes)
        .find_map(|node| match node {
            PresentationNode::Status {
                title,
                detail,
                activation,
                ..
            } if title == "Trust" => Some((detail, activation.is_some())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no Trust line: {commands:#?}"))
}

fn row_toggle(commands: &[Command], title_wanted: &str) -> (SurfaceId, BindingId) {
    surfaces(commands)
        .into_iter()
        .find_map(|(surface_id, nodes)| {
            nodes.into_iter().find_map(|node| match node {
                PresentationNode::List { rows, .. } => rows
                    .into_iter()
                    .find(|r| r.title == title_wanted)
                    .and_then(|r| {
                        r.controls.into_iter().find_map(|control| match control {
                            PresentationNode::Toggle { binding_id, .. } => Some(binding_id),
                            _ => None,
                        })
                    })
                    .map(|binding_id| (surface_id.clone(), binding_id)),
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("no {title_wanted} toggle: {commands:#?}"))
}

// @claim: S10
// @scenario: navigation :: Contact shows trust level badge
#[test]
fn the_trust_shown_follows_the_exchange_and_cannot_be_tapped() {
    let (_, plain) = screen_of(exchanged("Bob"));
    let mut carol = exchanged("Carol");
    carol.mark_fingerprint_verified().unwrap();
    let (_, verified) = screen_of(carol);

    assert_eq!(trust(&plain), (Some("Standard".to_string()), false));
    assert_eq!(trust(&verified), (Some("Verified".to_string()), false));
}

// @scenario: navigation :: Toggle proposal trust on a contact
#[test]
fn switching_on_can_propose_contacts_trusts_proposals_only() {
    let (mut engine, screen) = screen_of(exchanged("Bob"));
    let (surface_id, binding_id) = row_toggle(&screen, "Can propose contacts");

    engine
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id,
            value: InputValue::Boolean(true),
        })
        .unwrap();

    let bob = &engine.vauchi().list_contacts().unwrap()[0];
    assert_eq!(
        (bob.is_proposal_trusted(), bob.is_recovery_trusted()),
        (true, false)
    );
}

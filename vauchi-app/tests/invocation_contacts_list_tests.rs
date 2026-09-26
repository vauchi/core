// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! `contacts list` as a Core-applied invocation (ADR-066 Amendment
//! 2026-09-26 (b)): Core prepares the text surface or the versioned JSON
//! document and ends the run with an explicit outcome.

use serde_json::{Value, json};
use vauchi_app::i18n::Locale;
use vauchi_app::ui::invocation::{Invocation, InvocationOutput, invoke};
use vauchi_core::{
    Command, InvocationOutcome, PresentationNode, SurfaceSpec, SymmetricKey, api::Vauchi,
    contact::Contact, contact_card::ContactCard, contact_card::ContactField,
    contact_card::FieldType,
};

fn vauchi_with(names: &[&str]) -> (Vauchi, Vec<String>) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    let mut ids = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let mut public_key = [0u8; 32];
        public_key[0] = index as u8 + 1;
        let mut card = ContactCard::new(name);
        if *name == "Bob" {
            card.add_field(ContactField::new(
                FieldType::Email,
                "work",
                "bob@example.test",
                0,
            ))
            .expect("field");
        }
        let contact = Contact::from_exchange(public_key, card, SymmetricKey::generate(), 0);
        ids.push(contact.id().to_string());
        vauchi.add_contact(contact).expect("contact");
    }
    (vauchi, ids)
}

fn list(offset: usize, limit: usize) -> Invocation {
    Invocation::ContactsList { offset, limit }
}

fn document(commands: &[Command]) -> Value {
    match commands {
        [
            Command::EmitDocument { document },
            Command::FinishInvocation { outcome },
        ] => {
            assert_eq!(*outcome, InvocationOutcome::Succeeded);
            assert_eq!(document.media_type, "application/json");
            assert_eq!(document.schema, "vauchi.contacts.v1");
            serde_json::from_slice(&document.data).expect("document is JSON")
        }
        other => panic!("expected document then success, got {other:?}"),
    }
}

fn surface(commands: &[Command]) -> &SurfaceSpec {
    match commands {
        [
            Command::ReplaceSurface { surface },
            Command::FinishInvocation { outcome },
        ] => {
            assert_eq!(*outcome, InvocationOutcome::Succeeded);
            surface
        }
        other => panic!("expected one surface then success, got {other:?}"),
    }
}

// @internal
#[test]
fn document_reproduces_the_contacts_v1_shape() {
    let (vauchi, ids) = vauchi_with(&["Bob", "Carol"]);
    vauchi
        .verify_contact_fingerprint(&ids[0])
        .expect("verify Bob");

    let doc = document(&invoke(
        &vauchi,
        &list(0, 0),
        InvocationOutput::Document,
        Locale::English,
    ));

    let entries = doc.as_array().expect("array");
    assert_eq!(entries.len(), 2);
    let bob = entries
        .iter()
        .find(|entry| entry["id"] == ids[0])
        .expect("Bob by id");
    assert_eq!(
        *bob,
        json!({
            "id": ids[0],
            "display_name": "Bob",
            "fingerprint_verified": true,
            "recovery_trusted": false,
            "card": {
                "display_name": "Bob",
                "fields": [
                    { "field_type": "Email", "label": "work", "value": "bob@example.test" }
                ]
            }
        })
    );
    let carol = entries
        .iter()
        .find(|entry| entry["id"] == ids[1])
        .expect("Carol by id");
    assert_eq!(carol["fingerprint_verified"], false);
    assert_eq!(carol["card"]["fields"], json!([]));
}

// @internal
#[test]
fn document_for_no_contacts_is_an_empty_array() {
    let (vauchi, _) = vauchi_with(&[]);

    let doc = document(&invoke(
        &vauchi,
        &list(0, 0),
        InvocationOutput::Document,
        Locale::English,
    ));

    assert_eq!(doc, json!([]));
}

// @internal
#[test]
fn document_honours_offset_and_limit() {
    let (vauchi, _) = vauchi_with(&["Bob", "Carol", "Dave"]);
    let expected = vauchi.list_contacts_paginated(1, 1).expect("page");

    let doc = document(&invoke(
        &vauchi,
        &list(1, 1),
        InvocationOutput::Document,
        Locale::English,
    ));

    let entries = doc.as_array().expect("array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], expected[0].id());
}

// @internal
#[test]
fn text_surface_lists_contacts_with_prepared_status_copy() {
    let (vauchi, ids) = vauchi_with(&["Bob", "Carol"]);
    vauchi
        .verify_contact_fingerprint(&ids[0])
        .expect("verify Bob");

    let commands = invoke(
        &vauchi,
        &list(0, 0),
        InvocationOutput::Text,
        Locale::English,
    );
    let surface = surface(&commands);

    assert_eq!(surface.title, "Contacts (2):");
    let rows = match surface.nodes.as_slice() {
        [PresentationNode::List { rows, .. }] => rows,
        other => panic!("expected one list, got {other:?}"),
    };
    let row = |name: &str| {
        rows.iter()
            .find(|row| row.title == name)
            .unwrap_or_else(|| panic!("row {name}"))
    };
    assert_eq!(row("Bob").subtitle.as_deref(), Some("Verified"));
    assert_eq!(row("Carol").subtitle.as_deref(), Some("Not verified"));
    assert_eq!(row("Bob").detail.as_deref(), Some(&ids[0][..8]));
    assert!(
        rows.iter().all(|row| row.activation.is_none()),
        "a one-shot listing offers no interactions"
    );
}

// @internal
#[test]
fn text_surface_for_a_page_names_its_range() {
    let (vauchi, _) = vauchi_with(&["Bob", "Carol", "Dave"]);

    let commands = invoke(
        &vauchi,
        &list(1, 1),
        InvocationOutput::Text,
        Locale::English,
    );

    assert_eq!(surface(&commands).title, "Contacts (showing 2-2 of 3):");
}

// @internal
#[test]
fn text_surface_without_contacts_points_at_exchange() {
    let (vauchi, _) = vauchi_with(&[]);

    let commands = invoke(
        &vauchi,
        &list(0, 0),
        InvocationOutput::Text,
        Locale::English,
    );
    assert_eq!(
        surface(&commands).title,
        "Contacts (0):",
        "the hint is body copy; repeating it as the title prints it twice"
    );
    let texts: Vec<&str> = surface(&commands)
        .nodes
        .iter()
        .map(|node| match node {
            PresentationNode::Text { content, .. } => content.as_str(),
            other => panic!("expected text, got {other:?}"),
        })
        .collect();

    assert_eq!(
        texts,
        [
            "No contacts yet. Exchange with someone using:",
            "vauchi exchange start"
        ]
    );
}

// ADR-032: a duress unlock must list only the decoys — the invocation reads
// through the same auth-mode-aware API, never around it.
// @scenario: duress_mode :: Duress credential shows decoy contacts
#[test]
fn duress_unlock_lists_only_decoys_in_both_outputs() {
    let (mut vauchi, _) = vauchi_with(&["Bob"]);
    vauchi
        .setup_app_password("real-password")
        .expect("app password");
    vauchi
        .setup_duress_password("432198")
        .expect("duress password");
    vauchi
        .add_decoy_contact("decoy-1", "Safe Name", &ContactCard::new("Safe Name"))
        .expect("decoy");
    vauchi.authenticate("432198").expect("duress unlock");

    let doc = document(&invoke(
        &vauchi,
        &list(0, 0),
        InvocationOutput::Document,
        Locale::English,
    ));
    let names: Vec<&str> = doc
        .as_array()
        .expect("array")
        .iter()
        .map(|entry| entry["display_name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, ["Safe Name"]);

    let commands = invoke(
        &vauchi,
        &list(0, 0),
        InvocationOutput::Text,
        Locale::English,
    );
    let text = surface(&commands);
    assert_eq!(text.title, "Contacts (1):");
    let rows = match text.nodes.as_slice() {
        [PresentationNode::List { rows, .. }] => rows,
        other => panic!("expected one list, got {other:?}"),
    };
    assert_eq!(
        rows.iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        ["Safe Name"]
    );
}

fn archive(vauchi: &Vauchi, id: &str) {
    vauchi.archive_contact(id).expect("archive");
}

// @internal
#[test]
fn archived_document_lists_only_archived_contacts_in_the_v1_shape() {
    let (vauchi, ids) = vauchi_with(&["Bob", "Carol"]);
    archive(&vauchi, &ids[1]);

    let doc = document(&invoke(
        &vauchi,
        &Invocation::ArchivedContactsList,
        InvocationOutput::Document,
        Locale::English,
    ));

    let entries = doc.as_array().expect("array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["id"], ids[1]);
    assert_eq!(entries[0]["display_name"], "Carol");
    assert_eq!(entries[0]["card"]["fields"], json!([]));
}

// @internal
#[test]
fn archived_text_surface_uses_the_archived_header() {
    let (vauchi, ids) = vauchi_with(&["Bob", "Carol"]);
    archive(&vauchi, &ids[0]);

    let commands = invoke(
        &vauchi,
        &Invocation::ArchivedContactsList,
        InvocationOutput::Text,
        Locale::English,
    );
    let surface = surface(&commands);

    assert_eq!(surface.title, "Archived contacts (1):");
    let rows = match surface.nodes.as_slice() {
        [PresentationNode::List { rows, .. }] => rows,
        other => panic!("expected one list, got {other:?}"),
    };
    assert_eq!(
        rows.iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        ["Bob"]
    );
}

// @internal
#[test]
fn archived_text_surface_without_archive_says_so() {
    let (vauchi, _) = vauchi_with(&["Bob"]);

    let commands = invoke(
        &vauchi,
        &Invocation::ArchivedContactsList,
        InvocationOutput::Text,
        Locale::English,
    );
    let surface = surface(&commands);

    assert_eq!(surface.title, "Archived contacts (0):");
    match surface.nodes.as_slice() {
        [PresentationNode::Text { content, .. }] => {
            assert_eq!(content, "No archived contacts.");
        }
        other => panic!("expected the empty-archive text, got {other:?}"),
    }
}

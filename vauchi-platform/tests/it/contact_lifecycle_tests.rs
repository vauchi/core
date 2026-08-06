// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact-lifecycle behaviour: soft-delete, undo, archive,
//! unarchive, and list_archived.
//!
//! These verify that `PlatformAppEngine`'s `DomainCommand` dispatch
//! handlers correctly delegate to the core `ContactManager` API and
//! map errors to `MobileError`. Slice 32g (2026-05-17) retired the
//! `impl VauchiPlatform` surface that these tests previously
//! exercised; the dispatch path is the only public entry point now.

use std::sync::Arc;

use tempfile::TempDir;

use vauchi_platform::{
    DomainCommand, DomainCommandResult, MobileContact, MobileError, PlatformAppEngine,
    PlatformAppEngineTestHelpers,
};

fn setup() -> (Arc<PlatformAppEngine>, TempDir) {
    let dir = TempDir::new().unwrap();
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
        key.as_bytes().to_vec(),
    )
    .expect("create PlatformAppEngine");
    drive_onboarding(&engine);
    (engine, dir)
}

/// Drive through the full onboarding flow via the canonical envelope.
///
/// Every step reads the Core-minted interaction and binding ids from the
/// current command batch — exactly what a real shell renders — and
/// dispatches generic events back. No retired action/screen seams.
fn drive_onboarding(engine: &PlatformAppEngine) {
    fn primary_interaction(batch: &serde_json::Value) -> (String, String) {
        let bar = batch["commands"]
            .as_array()
            .and_then(|commands| commands.iter().find_map(|c| c.get("SetContextBar")))
            .expect("command batch must carry a context bar");
        (
            bar["surface_id"]
                .as_str()
                .expect("bar surface id")
                .to_owned(),
            bar["bar"]["primary"]["interaction_id"]
                .as_str()
                .expect("primary interaction id")
                .to_owned(),
        )
    }

    fn dispatch_primary(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
    ) -> serde_json::Value {
        let (surface_id, interaction_id) = primary_interaction(batch);
        let event = serde_json::json!({
            "ActionActivated": { "surface_id": surface_id, "interaction_id": interaction_id }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch primary activation"),
        )
        .expect("parse command batch")
    }

    fn find_input(nodes: &[serde_json::Value]) -> Option<&serde_json::Value> {
        nodes.iter().find_map(|node| {
            if let Some(input) = node.get("Input") {
                Some(input)
            } else {
                node["Group"]["children"]
                    .as_array()
                    .and_then(|children| find_input(children))
            }
        })
    }

    fn set_text_input(
        engine: &PlatformAppEngine,
        batch: &serde_json::Value,
        text: &str,
    ) -> serde_json::Value {
        let (surface_id, nodes) = batch["commands"]
            .as_array()
            .and_then(|commands| {
                commands.iter().find_map(|c| {
                    let surface = &c["ReplaceSurface"]["surface"];
                    surface
                        .is_object()
                        .then(|| (surface["surface_id"].clone(), surface["nodes"].clone()))
                })
            })
            .expect("command batch must replace a surface");
        let nodes: Vec<serde_json::Value> =
            serde_json::from_value(nodes).expect("surface nodes array");
        let input = find_input(&nodes).expect("surface must carry a text input");
        let event = serde_json::json!({
            "ValueChanged": {
                "surface_id": surface_id,
                "binding_id": input["binding_id"],
                "value": { "text": text },
            }
        });
        serde_json::from_str(
            &engine
                .dispatch_json(event.to_string())
                .expect("dispatch text input"),
        )
        .expect("parse command batch")
    }

    let mut batch: serde_json::Value = serde_json::from_str(
        &engine
            .initial_commands_json()
            .expect("initial onboarding commands"),
    )
    .expect("parse initial batch");

    batch = dispatch_primary(engine, &batch); // identity_check → default_name
    batch = set_text_input(engine, &batch, "Alice"); // enter display name
    batch = dispatch_primary(engine, &batch); // default_name → groups_setup
    batch = dispatch_primary(engine, &batch); // groups_setup → contact_info
    batch = dispatch_primary(engine, &batch); // contact_info → what_next
    let _ = dispatch_primary(engine, &batch); // what_next → complete → home
}

fn add_imported_contact(engine: &PlatformAppEngine, name: &str) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let contact_id = format!("contact-{name}");
    let contact = vauchi_core::Contact::from_import(
        contact_id,
        card,
        vauchi_core::ImportSource::VcardFile,
        None,
        0,
    );
    let id = contact.id().to_string();
    engine.save_test_contact(&contact).unwrap();
    id
}

fn add_exchanged_contact(engine: &PlatformAppEngine, name: &str) -> String {
    let card = vauchi_core::contact_card::ContactCard::new(name);
    let contact = vauchi_core::Contact::from_exchange(
        [0xAB; 32],
        card,
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    engine.save_test_contact(&contact).unwrap();
    id
}

fn list_contacts(engine: &PlatformAppEngine) -> Vec<MobileContact> {
    match engine
        .dispatch_domain_command(DomainCommand::ListContacts)
        .expect("ListContacts dispatch")
    {
        DomainCommandResult::Contacts { contacts } => contacts,
        other => panic!("expected Contacts, got {other:?}"),
    }
}

fn list_archived_contacts(engine: &PlatformAppEngine) -> Vec<MobileContact> {
    match engine
        .dispatch_domain_command(DomainCommand::ListArchivedContacts)
        .expect("ListArchivedContacts dispatch")
    {
        DomainCommandResult::Contacts { contacts } => contacts,
        other => panic!("expected Contacts, got {other:?}"),
    }
}

fn get_contact(engine: &PlatformAppEngine, id: String) -> Option<MobileContact> {
    match engine
        .dispatch_domain_command(DomainCommand::GetContact { id })
        .expect("GetContact dispatch")
    {
        DomainCommandResult::ContactOpt { contact } => contact,
        other => panic!("expected ContactOpt, got {other:?}"),
    }
}

fn footer_action_id(engine: &PlatformAppEngine, contact_id: String) -> Result<String, MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::ContactDetailFooterActionId { contact_id })
        .map(|r| match r {
            DomainCommandResult::Text { value } => value,
            other => panic!("expected Text, got {other:?}"),
        })
}

fn soft_delete(engine: &PlatformAppEngine, id: String) -> Result<(), MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::SoftDeleteImportedContact { id })
        .map(|_| ())
}

fn undo_delete(engine: &PlatformAppEngine, id: String) -> Result<(), MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::UndoDeleteImportedContact { id })
        .map(|_| ())
}

fn hard_delete(engine: &PlatformAppEngine, id: String) -> Result<(), MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::HardDeleteImportedContact { id })
        .map(|_| ())
}

fn archive(engine: &PlatformAppEngine, id: String) -> Result<(), MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::ArchiveContact { id })
        .map(|_| ())
}

fn unarchive(engine: &PlatformAppEngine, id: String) -> Result<(), MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::UnarchiveContact { id })
        .map(|_| ())
}

// === Soft-Delete (imported contacts only) ===

// @scenario: contacts_management :: Soft-delete imported contact hides from list
#[test]
fn test_soft_delete_imported_contact_hides_from_list() {
    let (engine, _dir) = setup();
    let id = add_imported_contact(&engine, "Bob");

    assert_eq!(list_contacts(&engine).len(), 1);
    soft_delete(&engine, id.clone()).unwrap();
    assert_eq!(
        list_contacts(&engine).len(),
        0,
        "Soft-deleted contact must not appear in list_contacts"
    );
}

// @scenario: contacts_management :: Soft-delete exchanged contact fails
#[test]
fn test_soft_delete_exchanged_contact_returns_error() {
    let (engine, _dir) = setup();
    let id = add_exchanged_contact(&engine, "Carol");

    let result = soft_delete(&engine, id);
    assert!(
        result.is_err(),
        "Soft-deleting an exchanged contact must fail"
    );
}

// @scenario: contacts_management :: Undo soft-delete restores contact
#[test]
fn test_undo_soft_delete_restores_contact_to_list() {
    let (engine, _dir) = setup();
    let id = add_imported_contact(&engine, "Dave");

    soft_delete(&engine, id.clone()).unwrap();
    assert_eq!(list_contacts(&engine).len(), 0);

    undo_delete(&engine, id).unwrap();
    assert_eq!(
        list_contacts(&engine).len(),
        1,
        "Undo must restore contact to visible list"
    );
}

// @scenario: contacts_management :: Hard-delete permanently removes contact
#[test]
fn test_hard_delete_permanently_removes_contact() {
    let (engine, _dir) = setup();
    let id = add_imported_contact(&engine, "Eve");

    soft_delete(&engine, id.clone()).unwrap();
    hard_delete(&engine, id.clone()).unwrap();

    // Even direct lookup should fail
    let contact = get_contact(&engine, id);
    assert!(
        contact.is_none(),
        "Hard-deleted contact must not be findable"
    );
}

// @scenario: contacts_management :: Hard-delete exchanged contact fails
#[test]
fn test_hard_delete_exchanged_contact_returns_error() {
    let (engine, _dir) = setup();
    let id = add_exchanged_contact(&engine, "Frank");

    let result = hard_delete(&engine, id);
    assert!(
        result.is_err(),
        "Hard-deleting an exchanged contact must fail"
    );
}

// === Archive (exchanged contacts only) ===

// @scenario: contacts_management :: Archive exchanged contact hides from list
#[test]
fn test_archive_contact_hides_from_main_list() {
    let (engine, _dir) = setup();
    let id = add_exchanged_contact(&engine, "Grace");

    assert_eq!(list_contacts(&engine).len(), 1);
    archive(&engine, id).unwrap();
    assert_eq!(
        list_contacts(&engine).len(),
        0,
        "Archived contact must not appear in list_contacts"
    );
}

// @scenario: contacts_management :: Archived contacts appear in archive list
#[test]
fn test_list_archived_contacts_returns_archived() {
    let (engine, _dir) = setup();
    let id = add_exchanged_contact(&engine, "Heidi");

    assert_eq!(list_archived_contacts(&engine).len(), 0);
    archive(&engine, id).unwrap();

    let archived = list_archived_contacts(&engine);
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].display_name, "Heidi");
}

// @scenario: contacts_management :: Unarchive restores contact to main list
#[test]
fn test_unarchive_contact_restores_to_main_list() {
    let (engine, _dir) = setup();
    let id = add_exchanged_contact(&engine, "Ivan");

    archive(&engine, id.clone()).unwrap();
    assert_eq!(list_contacts(&engine).len(), 0);

    unarchive(&engine, id).unwrap();
    assert_eq!(
        list_contacts(&engine).len(),
        1,
        "Unarchived contact must return to list_contacts"
    );
    assert_eq!(list_archived_contacts(&engine).len(), 0);
}

// @scenario: contacts_management :: Archive imported contact fails
#[test]
fn test_archive_imported_contact_returns_error() {
    let (engine, _dir) = setup();
    let id = add_imported_contact(&engine, "Judy");

    let result = archive(&engine, id);
    assert!(result.is_err(), "Archiving an imported contact must fail");
}

// @scenario: contacts_management :: Nonexistent contact returns error
#[test]
fn test_lifecycle_operations_on_missing_contact_return_error() {
    let (engine, _dir) = setup();
    let fake_id = "nonexistent-id".to_string();

    assert!(soft_delete(&engine, fake_id.clone()).is_err());
    assert!(undo_delete(&engine, fake_id.clone()).is_err());
    assert!(hard_delete(&engine, fake_id.clone()).is_err());
    assert!(archive(&engine, fake_id.clone()).is_err());
    assert!(unarchive(&engine, fake_id).is_err());
}

// === Contact Detail Footer Action ===
//
// Frontends call `contact_detail_footer_action_id` so the view layer
// stops branching on `MobileContact.is_imported` directly. Verifies the
// id matches what `ContactDetailEngine` would emit at the bottom of the
// detail screen.

// @internal
#[test]
fn test_contact_detail_footer_action_id_imported_returns_delete() {
    let (engine, _dir) = setup();
    let id = add_imported_contact(&engine, "Karen");

    let action_id = footer_action_id(&engine, id).unwrap();

    assert_eq!(action_id, "delete_contact");
}

// @internal
#[test]
fn test_contact_detail_footer_action_id_exchanged_returns_archive() {
    let (engine, _dir) = setup();
    let id = add_exchanged_contact(&engine, "Liam");

    let action_id = footer_action_id(&engine, id).unwrap();

    assert_eq!(action_id, "archive_contact");
}

// @internal
#[test]
fn test_contact_detail_footer_action_id_unknown_returns_invalid_input() {
    let (engine, _dir) = setup();

    let result = footer_action_id(&engine, "nonexistent-id".to_string());

    match result {
        Err(MobileError::InvalidInput { field, .. }) => {
            assert_eq!(
                field, "contact_id",
                "InvalidInput must name the offending field"
            );
        }
        other => panic!(
            "expected MobileError::InvalidInput {{ field: \"contact_id\", .. }}, got {:?}",
            other
        ),
    }
}

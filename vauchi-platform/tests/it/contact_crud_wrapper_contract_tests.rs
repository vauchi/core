// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Wrapper-surface contract tests for the contact-CRUD cluster.
//!
//! These verify FFI projection invariants (`MobileContact*` shape,
//! `MobileContactTrustLevel` enum mapping, `MobileError::Other`
//! not-found format, `MobileFieldNote` sort order) on
//! `PlatformAppEngine`'s `DomainCommand` dispatch path. They were
//! previously exercised against the legacy `impl VauchiPlatform`
//! helpers (`lib.rs:1348-1581`, retired in the 2026-05-18 Phase 2a
//! slice 32g-B cleanup). The dispatch path is the only public entry
//! point now.

use std::sync::Arc;

use tempfile::TempDir;

use vauchi_platform::{
    DomainCommand, DomainCommandResult, MobileContact, MobileContactCard, MobileContactTrustLevel,
    MobileError, MobileFieldNote, MobileFieldType, MobileSocialNetwork, PlatformAppEngine,
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

/// Drive the onboarding flow to create the identity. Mirrors the
/// pattern in `contact_lifecycle_tests.rs`.
fn drive_onboarding(engine: &PlatformAppEngine) {
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "create_new"}}"#.into())
        .expect("create_new");
    engine
        .handle_action_json(
            r#"{"TextChanged": {"component_id": "display_name", "value": "Alice"}}"#.into(),
        )
        .expect("display_name");
    for _ in 0..3 {
        engine
            .handle_action_json(r#"{"ActionPressed": {"action_id": "continue"}}"#.into())
            .expect("continue");
    }
    engine
        .handle_action_json(r#"{"ActionPressed": {"action_id": "start_app"}}"#.into())
        .expect("start_app");
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn make_test_contact() -> vauchi_core::Contact {
    vauchi_core::Contact::from_exchange(
        [0xAB; 32],
        vauchi_core::contact_card::ContactCard::new("Bob"),
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    )
}

// ── Dispatch wrappers ──────────────────────────────────────────────

fn get_own_card(engine: &PlatformAppEngine) -> MobileContactCard {
    match engine
        .dispatch_domain_command(DomainCommand::GetOwnCard)
        .expect("GetOwnCard dispatch")
    {
        DomainCommandResult::ContactCardPayload { card } => card,
        other => panic!("expected ContactCardPayload, got {other:?}"),
    }
}

fn add_field(
    engine: &PlatformAppEngine,
    field_type: MobileFieldType,
    label: String,
    value: String,
) -> Result<(), MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::AddField {
            field_type,
            label,
            value,
        })
        .map(|_| ())
}

fn update_field(
    engine: &PlatformAppEngine,
    label: String,
    new_value: String,
) -> Result<(), MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::UpdateField { label, new_value })
        .map(|_| ())
}

fn remove_field(engine: &PlatformAppEngine, label: String) -> bool {
    match engine
        .dispatch_domain_command(DomainCommand::RemoveField { label })
        .expect("RemoveField dispatch")
    {
        DomainCommandResult::Bool { value } => value,
        other => panic!("expected Bool, got {other:?}"),
    }
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

fn get_contact(engine: &PlatformAppEngine, id: String) -> Option<MobileContact> {
    match engine
        .dispatch_domain_command(DomainCommand::GetContact { id })
        .expect("GetContact dispatch")
    {
        DomainCommandResult::ContactOpt { contact } => contact,
        other => panic!("expected ContactOpt, got {other:?}"),
    }
}

fn set_proposal_trusted(
    engine: &PlatformAppEngine,
    contact_id: String,
    trusted: bool,
) -> Result<(), MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::SetProposalTrusted {
            contact_id,
            trusted,
        })
        .map(|_| ())
}

fn set_contact_field_note(
    engine: &PlatformAppEngine,
    contact_id: String,
    field_id: String,
    note: String,
) -> Result<(), MobileError> {
    engine
        .dispatch_domain_command(DomainCommand::SetContactFieldNote {
            contact_id,
            field_id,
            note,
        })
        .map(|_| ())
}

fn get_contact_field_notes(engine: &PlatformAppEngine, contact_id: String) -> Vec<MobileFieldNote> {
    match engine
        .dispatch_domain_command(DomainCommand::GetContactFieldNotes { contact_id })
        .expect("GetContactFieldNotes dispatch")
    {
        DomainCommandResult::FieldNotes { notes } => notes,
        other => panic!("expected FieldNotes, got {other:?}"),
    }
}

fn list_social_networks(engine: &PlatformAppEngine) -> Vec<MobileSocialNetwork> {
    match engine
        .dispatch_domain_command(DomainCommand::ListSocialNetworks)
        .expect("ListSocialNetworks dispatch")
    {
        DomainCommandResult::SocialNetworks { networks } => networks,
        other => panic!("expected SocialNetworks, got {other:?}"),
    }
}

fn get_profile_url(
    engine: &PlatformAppEngine,
    network_id: String,
    username: String,
) -> Option<String> {
    match engine
        .dispatch_domain_command(DomainCommand::GetProfileUrl {
            network_id,
            username,
        })
        .expect("GetProfileUrl dispatch")
    {
        DomainCommandResult::StringOpt { value } => value,
        other => panic!("expected StringOpt, got {other:?}"),
    }
}

// === Own-card field-CRUD (AddField / UpdateField / RemoveField / GetOwnCard) ===

// @scenario: contact_card_management:User adds an email field to their card
#[test]
fn test_add_field() {
    let (engine, _dir) = setup();
    add_field(
        &engine,
        MobileFieldType::Email,
        "work".to_string(),
        "alice@company.com".to_string(),
    )
    .unwrap();

    let card = get_own_card(&engine);
    assert_eq!(card.fields.len(), 1);
    assert_eq!(card.fields[0].label, "work");
    assert_eq!(card.fields[0].value, "alice@company.com");
}

// @scenario: contact_card_management:User edits a field on their card
#[test]
fn test_update_field() {
    let (engine, _dir) = setup();
    add_field(
        &engine,
        MobileFieldType::Phone,
        "mobile".to_string(),
        "+1234567890".to_string(),
    )
    .unwrap();
    update_field(&engine, "mobile".to_string(), "+0987654321".to_string()).unwrap();

    let card = get_own_card(&engine);
    assert_eq!(card.fields[0].value, "+0987654321");
}

// @scenario: contact_card_management:User removes a field from their card
#[test]
fn test_remove_field() {
    let (engine, _dir) = setup();
    add_field(
        &engine,
        MobileFieldType::Email,
        "work".to_string(),
        "alice@company.com".to_string(),
    )
    .unwrap();

    assert!(remove_field(&engine, "work".to_string()));

    let card = get_own_card(&engine);
    assert!(card.fields.is_empty());
}

// A mobile own-card edit must propagate to exchanged contacts (Option B:
// the handler calls propagate_card_update explicitly). Before the wiring the
// handlers only persisted + refreshed MyInfo, so the edit reached no contact.
// @scenario: contact_card_management:Editing the own card propagates to exchanged contacts
#[test]
fn add_field_propagates_to_a_ratcheted_contact() {
    let (engine, _dir) = setup();

    // An exchanged + ratcheted contact, so propagation has a real target.
    let shared = vauchi_core::crypto::SymmetricKey::generate();
    let contact = vauchi_core::Contact::from_exchange(
        [0xCD; 32],
        vauchi_core::contact_card::ContactCard::new("Bob"),
        shared.clone(),
        0,
    );
    let contact_id = contact.id().to_string();
    engine.save_test_contact(&contact).unwrap();
    let their_dh = vauchi_core::exchange::X3DHKeyPair::generate();
    engine
        .create_test_ratchet_as_initiator(contact_id.clone(), &shared, *their_dh.public_key())
        .unwrap();

    assert_eq!(
        engine
            .test_pending_update_count(contact_id.clone())
            .unwrap(),
        0,
        "no pending update before the edit"
    );

    add_field(
        &engine,
        MobileFieldType::Email,
        "work".to_string(),
        "alice@company.com".to_string(),
    )
    .unwrap();

    assert_eq!(
        engine.test_pending_update_count(contact_id).unwrap(),
        1,
        "editing the own card queues a propagation update for the ratcheted contact"
    );
}

// === Social networks ===

// @scenario: contact_card_management:Social network profile links
#[test]
fn test_social_networks() {
    let (engine, _dir) = setup();

    let networks = list_social_networks(&engine);
    assert!(!networks.is_empty());
    networks
        .iter()
        .find(|n| n.id == "github")
        .expect("expected Some github entry");

    let url = get_profile_url(&engine, "github".to_string(), "octocat".to_string());
    assert_eq!(url, Some("https://github.com/octocat".to_string()));
}

// === Fingerprint projection ===

// @scenario: security:Verify contact fingerprint manually
#[test]
fn test_mobile_contact_has_fingerprint_field() {
    let (engine, _dir) = setup();
    let contact = vauchi_core::Contact::from_exchange(
        [0xAB; 32],
        vauchi_core::contact_card::ContactCard::new("Bob"),
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    engine.save_test_contact(&contact).unwrap();

    let contacts = list_contacts(&engine);
    assert_eq!(contacts.len(), 1);

    let mc = &contacts[0];
    assert!(
        !mc.fingerprint.is_empty(),
        "MobileContact should have a fingerprint field"
    );

    // Must match Contact::fingerprint() format: 16 groups of 4 uppercase hex
    let groups: Vec<&str> = mc.fingerprint.split(' ').collect();
    assert_eq!(groups.len(), 16);
}

// === Trust level enum projection ===

// @scenario: contact_trust:Standard trust contact has correct fields in MobileContact
#[test]
fn test_mobile_contact_trust_level_standard() {
    let (engine, _dir) = setup();
    engine.save_test_contact(&make_test_contact()).unwrap();

    let contacts = list_contacts(&engine);
    assert_eq!(contacts.len(), 1);
    let mc = &contacts[0];

    // Default exchange has no proximity verification — must be Standard
    assert_eq!(mc.trust_level, MobileContactTrustLevel::Standard);
    assert_eq!(mc.exchange_transport, "qr");
    assert_eq!(mc.proximity_confidence, "unknown");
    assert!(!mc.proposal_trusted);
}

// @scenario: contact_trust:Fingerprint-verified contact maps to Verified trust level
#[test]
fn test_mobile_contact_trust_level_verified() {
    let (engine, _dir) = setup();
    let mut contact = make_test_contact();
    contact.mark_fingerprint_verified().unwrap();
    engine.save_test_contact(&contact).unwrap();

    let contacts = list_contacts(&engine);
    let mc = &contacts[0];
    assert_eq!(mc.trust_level, MobileContactTrustLevel::Verified);
    assert!(mc.is_verified);
}

// === Proposal-trusted flag ===

// @scenario: contact_trust:proposal_trusted flag round-trips via set_proposal_trusted
#[test]
fn test_set_proposal_trusted_round_trip() {
    let (engine, _dir) = setup();
    let contact = make_test_contact();
    let contact_id = contact.id().to_string();
    engine.save_test_contact(&contact).unwrap();

    // Initially false
    let mc = get_contact(&engine, contact_id.clone()).unwrap();
    assert!(!mc.proposal_trusted);

    // Set trusted
    set_proposal_trusted(&engine, contact_id.clone(), true).unwrap();
    let mc = get_contact(&engine, contact_id.clone()).unwrap();
    assert!(mc.proposal_trusted);

    // Unset trusted
    set_proposal_trusted(&engine, contact_id.clone(), false).unwrap();
    let mc = get_contact(&engine, contact_id).unwrap();
    assert!(!mc.proposal_trusted);
}

// @scenario: contact_trust:set_proposal_trusted returns ContactNotFound for unknown ID
#[test]
fn test_set_proposal_trusted_contact_not_found() {
    let (engine, _dir) = setup();
    let err = set_proposal_trusted(&engine, "nonexistent_id".to_string(), true).unwrap_err();
    assert!(
        matches!(
            &err,
            MobileError::Other { detail } if detail.starts_with("Contact not found:")
        ),
        "expected Other(Contact not found: …), got {err:?}"
    );
}

// === Field-note sort order ===

// @scenario: contact_notes:Multiple field notes are returned sorted by field_id
#[test]
fn test_contact_field_notes_multiple_sorted() {
    let (engine, _dir) = setup();
    let contact = make_test_contact();
    let contact_id = contact.id().to_string();
    engine.save_test_contact(&contact).unwrap();

    set_contact_field_note(
        &engine,
        contact_id.clone(),
        "zzz_field".to_string(),
        "last note".to_string(),
    )
    .unwrap();
    set_contact_field_note(
        &engine,
        contact_id.clone(),
        "aaa_field".to_string(),
        "first note".to_string(),
    )
    .unwrap();

    let notes = get_contact_field_notes(&engine, contact_id);
    assert_eq!(notes.len(), 2);
    // Sorted by field_id
    assert_eq!(notes[0].field_id, "aaa_field");
    assert_eq!(notes[1].field_id, "zzz_field");
}

// === MobileContactField.note projection ===

// @scenario: contact_notes:ContactField note is exposed in MobileContactField
#[test]
fn test_mobile_contact_field_note_exposed() {
    let (engine, _dir) = setup();

    // Build a contact whose card has a field with a private note
    let email_field = vauchi_core::ContactField::new(
        vauchi_core::FieldType::Email,
        "work",
        "bob@example.com",
        now_secs(),
    )
    .with_note("Bob's work email".to_string());

    let mut card = vauchi_core::contact_card::ContactCard::new("Bob");
    card.add_field(email_field).unwrap();

    let contact = vauchi_core::Contact::from_exchange(
        [0xBC; 32],
        card,
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    engine.save_test_contact(&contact).unwrap();

    let contacts = list_contacts(&engine);
    assert_eq!(contacts.len(), 1);
    let field = &contacts[0].card.fields[0];
    assert_eq!(field.label, "work");
    assert_eq!(field.note.as_deref(), Some("Bob's work email"));
}

// @scenario: contact_notes:ContactField without note has None in MobileContactField
#[test]
fn test_mobile_contact_field_note_none_when_absent() {
    let (engine, _dir) = setup();

    engine.save_test_contact(&make_test_contact()).unwrap();

    let contacts = list_contacts(&engine);
    let mc = &contacts[0];
    assert!(mc.card.fields.is_empty());

    // Add a contact with a field that has no note
    let email_field = vauchi_core::ContactField::new(
        vauchi_core::FieldType::Email,
        "personal",
        "bob@personal.com",
        now_secs(),
    );
    let mut card = vauchi_core::contact_card::ContactCard::new("Bob");
    card.add_field(email_field).unwrap();

    let contact_with_field = vauchi_core::Contact::from_exchange(
        [0xCD; 32],
        card,
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    engine.save_test_contact(&contact_with_field).unwrap();

    let all = list_contacts(&engine);
    let with_field = all
        .iter()
        .find(|c| c.exchange_transport == "qr" && !c.card.fields.is_empty())
        .expect("should find contact with field");
    assert!(
        with_field.card.fields[0].note.is_none(),
        "field without note should have note = None"
    );
}

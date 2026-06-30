// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A mobile field-visibility edit must propagate on the **effective** model:
//! the write lands on the layer the resolver reads, the change is
//! repropagated to the affected contact, and the readout reflects the
//! effective verdict.
//!
//! Concern area 1 of
//! `_private/docs/problems/2026-06-14-visibility-changes-not-fully-propagated`:
//! before the fix the mobile PAE handlers wrote Layer A (by-label) and/or
//! never repropagated, so a phone user hiding a field from a grouped contact
//! did not revoke it on the wire.

use std::sync::Arc;

use tempfile::TempDir;
use vauchi_core::{Contact, ContactCard, Identity, SymmetricKey, exchange::X3DHKeyPair};
use vauchi_platform::{
    DomainCommand, DomainCommandResult, MobileFieldType, MobileVisibilityLabel, PlatformAppEngine,
    PlatformAppEngineTestHelpers,
};

fn setup() -> (Arc<PlatformAppEngine>, TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = SymmetricKey::generate();
    let engine = PlatformAppEngine::new(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    drive_onboarding(&engine);
    (engine, dir)
}

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

fn add_own_field(
    engine: &PlatformAppEngine,
    field_type: MobileFieldType,
    label: &str,
    value: &str,
) {
    engine
        .dispatch_domain_command(DomainCommand::AddField {
            field_type,
            label: label.into(),
            value: value.into(),
        })
        .expect("AddField");
}

fn own_field_id(engine: &PlatformAppEngine, label: &str) -> String {
    match engine
        .dispatch_domain_command(DomainCommand::GetOwnCard)
        .expect("GetOwnCard")
    {
        DomainCommandResult::ContactCardPayload { card } => card
            .fields
            .iter()
            .find(|f| f.label == label)
            .unwrap_or_else(|| panic!("own field not found: {label}"))
            .id
            .clone(),
        other => panic!("expected ContactCardPayload, got {other:?}"),
    }
}

/// A contact with an initiator ratchet, so the repropagation path has a real
/// delivery target (mirrors the core `add_contact_with_ratchet` helper).
fn add_ratcheted_contact(engine: &PlatformAppEngine, name: &str) -> String {
    let identity = Identity::create(name, 0);
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        shared.clone(),
        0,
    );
    let contact_id = contact.id().to_string();
    engine.save_test_contact(&contact).expect("save contact");
    let their_dh = X3DHKeyPair::generate();
    engine
        .create_test_ratchet_as_initiator(contact_id.clone(), &shared, *their_dh.public_key())
        .expect("create ratchet");
    contact_id
}

fn create_label(engine: &PlatformAppEngine, name: &str) -> MobileVisibilityLabel {
    match engine
        .dispatch_domain_command(DomainCommand::CreateLabel { name: name.into() })
        .expect("CreateLabel")
    {
        DomainCommandResult::Label { label } => label,
        other => panic!("expected Label, got {other:?}"),
    }
}

fn add_contact_to_group(engine: &PlatformAppEngine, label_id: &str, contact_id: &str) {
    engine
        .dispatch_domain_command(DomainCommand::AddContactToGroup {
            label_id: label_id.into(),
            contact_id: contact_id.into(),
        })
        .expect("AddContactToGroup");
}

fn pending(engine: &PlatformAppEngine, contact_id: &str) -> usize {
    engine
        .test_pending_update_count(contact_id.into())
        .expect("pending count")
}

fn effective(engine: &PlatformAppEngine, contact_id: &str, field_id: &str) -> bool {
    engine
        .test_effective_field_visibility(contact_id.into(), field_id.into())
        .expect("effective visibility")
}

// @internal
#[test]
fn hide_field_from_contact_queues_repropagation() {
    let (engine, _dir) = setup();
    add_own_field(
        &engine,
        MobileFieldType::Email,
        "Work",
        "alice@work.example",
    );
    add_own_field(&engine, MobileFieldType::Phone, "Mobile", "+15550100");
    let bob = add_ratcheted_contact(&engine, "Bob");
    assert_eq!(
        pending(&engine, &bob),
        0,
        "no pending update before the edit"
    );

    engine
        .dispatch_domain_command(DomainCommand::HideFieldFromContact {
            contact_id: bob.clone(),
            field_label: "Work".into(),
        })
        .expect("HideFieldFromContact");

    assert!(
        pending(&engine, &bob) > 0,
        "hiding a field from a contact must repropagate the revoked card to that contact"
    );
}

// @internal
#[test]
fn show_field_to_contact_queues_repropagation() {
    let (engine, _dir) = setup();
    add_own_field(
        &engine,
        MobileFieldType::Email,
        "Work",
        "alice@work.example",
    );
    let bob = add_ratcheted_contact(&engine, "Bob");
    assert_eq!(pending(&engine, &bob), 0);

    engine
        .dispatch_domain_command(DomainCommand::ShowFieldToContact {
            contact_id: bob.clone(),
            field_label: "Work".into(),
        })
        .expect("ShowFieldToContact");

    assert!(
        pending(&engine, &bob) > 0,
        "showing a field to a contact must repropagate the updated card"
    );
}

// @internal
#[test]
fn set_contact_field_override_queues_repropagation() {
    let (engine, _dir) = setup();
    add_own_field(
        &engine,
        MobileFieldType::Email,
        "Work",
        "alice@work.example",
    );
    add_own_field(&engine, MobileFieldType::Phone, "Mobile", "+15550100");
    let bob = add_ratcheted_contact(&engine, "Bob");
    assert_eq!(pending(&engine, &bob), 0);

    engine
        .dispatch_domain_command(DomainCommand::SetContactFieldOverride {
            contact_id: bob.clone(),
            field_label: "Work".into(),
            is_visible: false,
        })
        .expect("SetContactFieldOverride");

    assert!(
        pending(&engine, &bob) > 0,
        "a per-contact override must repropagate the updated card"
    );
}

// @internal
#[test]
fn remove_contact_field_override_queues_repropagation() {
    let (engine, _dir) = setup();
    add_own_field(
        &engine,
        MobileFieldType::Email,
        "Work",
        "alice@work.example",
    );
    add_own_field(&engine, MobileFieldType::Phone, "Mobile", "+15550100");
    let bob = add_ratcheted_contact(&engine, "Bob");

    engine
        .dispatch_domain_command(DomainCommand::SetContactFieldOverride {
            contact_id: bob.clone(),
            field_label: "Work".into(),
            is_visible: false,
        })
        .expect("SetContactFieldOverride");
    let after_set = pending(&engine, &bob);

    engine
        .dispatch_domain_command(DomainCommand::RemoveContactFieldOverride {
            contact_id: bob.clone(),
            field_label: "Work".into(),
        })
        .expect("RemoveContactFieldOverride");

    assert!(
        pending(&engine, &bob) > after_set,
        "removing a per-contact override must itself repropagate the restored card"
    );
}

// @internal
#[test]
fn set_group_field_visibility_queues_repropagation() {
    let (engine, _dir) = setup();
    add_own_field(
        &engine,
        MobileFieldType::Email,
        "Work",
        "alice@work.example",
    );
    let bob = add_ratcheted_contact(&engine, "Bob");
    let label = create_label(&engine, "Colleagues");
    add_contact_to_group(&engine, &label.id, &bob);
    assert_eq!(pending(&engine, &bob), 0);

    engine
        .dispatch_domain_command(DomainCommand::SetGroupFieldVisibility {
            label_id: label.id.clone(),
            field_label: "Work".into(),
            is_visible: true,
        })
        .expect("SetGroupFieldVisibility");

    assert!(
        pending(&engine, &bob) > 0,
        "a group field-visibility change must repropagate to each contact in the group"
    );
}

/// The discriminating test for the layer bug: a grouped contact sees a field
/// via the group (Layer B), and hiding it must flip the **effective** verdict
/// — the one the wire uses — to hidden. A Layer-A by-label write leaves the
/// resolver answering "visible", so the field keeps leaking.
// @internal
#[test]
fn hide_field_from_grouped_contact_revokes_effective_visibility() {
    let (engine, _dir) = setup();
    add_own_field(
        &engine,
        MobileFieldType::Email,
        "Work",
        "alice@work.example",
    );
    let work_id = own_field_id(&engine, "Work");
    let bob = add_ratcheted_contact(&engine, "Bob");
    let label = create_label(&engine, "Colleagues");
    add_contact_to_group(&engine, &label.id, &bob);
    engine
        .dispatch_domain_command(DomainCommand::SetGroupFieldVisibility {
            label_id: label.id.clone(),
            field_label: "Work".into(),
            is_visible: true,
        })
        .expect("SetGroupFieldVisibility");
    assert!(
        effective(&engine, &bob, &work_id),
        "precondition: the group grant makes Work effectively visible to Bob"
    );

    engine
        .dispatch_domain_command(DomainCommand::HideFieldFromContact {
            contact_id: bob.clone(),
            field_label: "Work".into(),
        })
        .expect("HideFieldFromContact");

    assert!(
        !effective(&engine, &bob, &work_id),
        "hiding a field from a grouped contact must revoke its effective visibility"
    );
}

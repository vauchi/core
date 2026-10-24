// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Own-entry addressing for the label-taking visibility APIs (private#651).
//! Two entries may share a label (private#649), so a label naming several is
//! refused with the entries' ids instead of silently picking the first, and
//! an entry id is accepted wherever a label is.

use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi, VauchiError,
};

/// Alice has two "Phone" entries and one "Email", all visible; Bob is an
/// exchanged contact.
fn two_phones() -> (Vauchi, String, [String; 2]) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    for (field_type, label, value) in [
        (FieldType::Phone, "Phone", "5550101"),
        (FieldType::Phone, "Phone", "5550202"),
        (FieldType::Email, "Email", "alice@example.com"),
    ] {
        wb.add_own_field(ContactField::new(field_type, label, value, 0))
            .unwrap();
    }
    let card = wb.own_card().unwrap().unwrap();
    for field in card.fields() {
        wb.set_own_field_public(field.id()).unwrap();
    }
    let phones: Vec<String> = card
        .fields()
        .iter()
        .filter(|f| f.label() == "Phone")
        .map(|f| f.id().to_string())
        .collect();
    let contact = Contact::from_exchange(
        [1; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    (wb, contact_id, [phones[0].clone(), phones[1].clone()])
}

fn visible(wb: &Vauchi, contact_id: &str, field_id: &str) -> bool {
    wb.get_effective_field_visibility(contact_id, field_id)
        .unwrap()
}

// @scenario: visibility :: a label shared by two entries is refused, naming their ids
// @internal
#[test]
fn a_label_shared_by_two_entries_is_refused_with_their_ids() {
    let (wb, contact_id, [first, second]) = two_phones();

    let result = wb.toggle_field_visibility(&contact_id, "Phone");

    let Err(VauchiError::InvalidState(message)) = result else {
        panic!("an ambiguous label must be refused, got {result:?}");
    };
    assert!(
        message.contains(&first) && message.contains(&second),
        "the refusal names both entries' ids: {message}"
    );
    assert!(visible(&wb, &contact_id, &first), "nothing changed");
    assert!(visible(&wb, &contact_id, &second), "nothing changed");
    assert!(matches!(
        wb.is_field_visible_by_label(&contact_id, "Phone"),
        Err(VauchiError::InvalidState(_))
    ));
}

// @scenario: visibility :: an entry id addresses one of two same-label entries
// @internal
#[test]
fn an_entry_id_addresses_one_of_two_same_label_entries() {
    let (wb, contact_id, [first, second]) = two_phones();

    let now_visible = wb.toggle_field_visibility(&contact_id, &second).unwrap();

    assert!(!now_visible);
    assert!(
        !visible(&wb, &contact_id, &second),
        "the addressed entry is hidden"
    );
    assert!(
        visible(&wb, &contact_id, &first),
        "its namesake is untouched"
    );
    assert!(!wb.is_field_visible_by_label(&contact_id, &second).unwrap());
}

// @internal
#[test]
fn a_unique_label_still_resolves() {
    let (wb, contact_id, _) = two_phones();

    assert!(!wb.toggle_field_visibility(&contact_id, "Email").unwrap());
    assert!(!wb.is_field_visible_by_label(&contact_id, "Email").unwrap());
}

// CC-14: the input is untrusted text from a CLI or a binding.
// @internal
#[test]
fn an_unknown_empty_or_odd_label_is_not_found() {
    let (wb, contact_id, _) = two_phones();

    for input in ["", "phone", "Phone ", "Ph\0one", "📞", &"x".repeat(10_000)] {
        assert!(
            matches!(
                wb.is_field_visible_by_label(&contact_id, input),
                Err(VauchiError::NotFound(_))
            ),
            "{input:?} names no entry"
        );
    }
}

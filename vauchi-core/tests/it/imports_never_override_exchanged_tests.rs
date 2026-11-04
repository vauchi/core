// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! adr-constraints "Imported contacts never override exchanged ones"
//! (ADR-059): a vCard that matches an exchanged contact, by name and phone
//! or by the Vauchi id an export wrote, leaves that contact as it was
//! (private#972, SC-64). Data crosses only by explicit adoption
//! (cross_kind_merge_adoption_tests) and a direct merge is refused
//! (imported_contact_api_tests).

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::{SymmetricKey, Vauchi};

const NAME: &str = "Zara Okonkwo-Lind";
const PHONE: &str = "+41 79 555 04 56";

fn with_exchanged_contact() -> (Vauchi, String) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut card = ContactCard::new(NAME);
    card.add_field(ContactField::new(FieldType::Phone, "Mobile", PHONE, 0))
        .unwrap();
    let contact = Contact::from_exchange([0x42; 32], card, SymmetricKey::generate(), 0);
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    (vauchi, id)
}

fn exchanged_as_stored(vauchi: &Vauchi, id: &str) -> (bool, String, serde_json::Value) {
    let contact = vauchi.get_contact(id).unwrap().unwrap();
    (
        contact.is_exchanged(),
        contact.display_name().to_string(),
        serde_json::to_value(contact.card()).unwrap(),
    )
}

// @internal
#[test]
fn a_vcard_with_the_same_name_and_phone_leaves_the_exchanged_contact_alone() {
    let (vauchi, id) = with_exchanged_contact();
    let before = exchanged_as_stored(&vauchi, &id);
    let vcf = format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:{NAME}\r\nTEL:{PHONE}\r\n\
         EMAIL:zara.override@example.invalid\r\nNICKNAME:Zed\r\nEND:VCARD\r\n"
    );

    vauchi.import_contacts_from_vcf(vcf.as_bytes()).unwrap();

    assert_eq!(exchanged_as_stored(&vauchi, &id), before);
}

// @internal
#[test]
fn a_vauchi_export_naming_the_exchanged_contact_is_skipped() {
    let (vauchi, id) = with_exchanged_contact();
    let before = exchanged_as_stored(&vauchi, &id);
    let vcf = format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Someone Else\r\nTEL:+41 79 555 99 99\r\n\
         X-VAUCHI-ID:{id}\r\nX-VAUCHI-KIND:exchanged\r\nEND:VCARD\r\n"
    );

    let result = vauchi.import_contacts_from_vcf(vcf.as_bytes()).unwrap();

    assert_eq!((result.imported, result.skipped), (0, 1));
    assert_eq!(exchanged_as_stored(&vauchi, &id), before);
}

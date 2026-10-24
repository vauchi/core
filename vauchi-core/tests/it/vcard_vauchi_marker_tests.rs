// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The vCard export names each contact's Vauchi id and whether it was
//! exchanged or imported (RFC 6350 §6.10 private `X-` properties), so a
//! re-import into Vauchi matches the contact instead of duplicating it
//! (#712; owner decision 2026-10-20). Other apps ignore the markers.

use vauchi_core::ImportSource;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::contact_card::vcard::export_contact_vcard;
use vauchi_core::contact_card::vcard_import::import_vcf;
use vauchi_core::crypto::SymmetricKey;

fn exchanged(name: &str) -> Contact {
    Contact::from_exchange(
        [7u8; 32],
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    )
}

fn imported(id: &str, name: &str) -> Contact {
    Contact::from_import(
        id.into(),
        ContactCard::new(name),
        ImportSource::VcardFile,
        None,
        0,
    )
}

// @scenario: privacy_compliance :: A vCard export carries Vauchi origin markers
#[test]
fn an_exported_contact_carries_its_vauchi_id_and_kind() {
    let bob = exchanged("Bob");
    let carol = imported("contact-carol", "Carol");

    let export: fn(&Contact) -> String = export_contact_vcard;
    let bob_vcf = export(&bob);
    let carol_vcf = export_contact_vcard(&carol);

    assert!(
        bob_vcf.contains(&format!("\r\nX-VAUCHI-ID:{}\r\n", bob.id())),
        "{bob_vcf}"
    );
    assert!(
        bob_vcf.contains("\r\nX-VAUCHI-KIND:exchanged\r\n"),
        "{bob_vcf}"
    );
    assert!(bob_vcf.ends_with("END:VCARD"), "{bob_vcf}");
    assert!(
        carol_vcf.contains("\r\nX-VAUCHI-ID:contact-carol\r\n"),
        "{carol_vcf}"
    );
    assert!(
        carol_vcf.contains("\r\nX-VAUCHI-KIND:imported\r\n"),
        "{carol_vcf}"
    );
}

// @internal — preserve the dereference coercions accepted by the concrete Contact API.
#[test]
fn contact_export_accepts_existing_reference_and_smart_pointer_callers() {
    let contact = imported("contact-carol", "Carol");
    let expected = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Carol\r\nX-VAUCHI-ID:contact-carol\r\nX-VAUCHI-KIND:imported\r\nEND:VCARD";
    let boxed = Box::new(contact.clone());
    let shared = std::sync::Arc::new(contact.clone());
    let borrowed = &contact;

    assert_eq!(export_contact_vcard(&contact), expected);
    assert_eq!(export_contact_vcard(&boxed), expected);
    assert_eq!(export_contact_vcard(&shared), expected);
    assert_eq!(export_contact_vcard(&borrowed), expected);

    let export = export_contact_vcard;
    assert_eq!(export(&boxed), expected);
    assert_eq!(export(&contact), expected);
}

// @scenario: privacy_compliance :: A vCard export carries Vauchi origin markers
#[test]
fn reimporting_the_export_matches_the_contacts_instead_of_duplicating_them() {
    let vauchi = Vauchi::in_memory().unwrap();
    let bob = exchanged("Bob");
    let carol = imported("contact-carol", "Carol");
    let vcf = [export_contact_vcard(&bob), export_contact_vcard(&carol)].join("\r\n");
    vauchi.add_contact(bob).unwrap();
    vauchi.add_contact(carol).unwrap();

    let result = vauchi.import_contacts_from_vcf(vcf.as_bytes()).unwrap();

    assert_eq!(result.imported, 0);
    assert_eq!(result.skipped, 2);
    assert_eq!(vauchi.list_contacts().unwrap().len(), 2);
}

// @internal
#[test]
fn a_vauchi_id_this_install_does_not_know_imports_as_a_new_contact() {
    let vauchi = Vauchi::in_memory().unwrap();
    let vcf = export_contact_vcard(&exchanged("Bob"));

    let result = vauchi.import_contacts_from_vcf(vcf.as_bytes()).unwrap();

    assert_eq!(result.imported, 1);
    let contacts = vauchi.list_contacts().unwrap();
    assert_eq!(contacts.len(), 1);
    assert!(
        contacts[0].is_imported(),
        "an exchange cannot be recreated from a file"
    );
}

// The marker is untrusted input from a file: only an id-shaped value is
// kept, so a crafted card cannot smuggle text into a lookup.
// @internal
#[test]
fn a_malformed_vauchi_id_is_ignored() {
    for value in [
        "",
        "has space",
        "semi;colon",
        "new\\nline",
        "ü-not-ascii",
        &"a".repeat(129),
    ] {
        let vcf =
            format!("BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Bob\r\nX-VAUCHI-ID:{value}\r\nEND:VCARD\r\n");

        let parsed = import_vcf(vcf.as_bytes(), 0).unwrap();

        assert_eq!(parsed[0].vauchi_id, None, "{value:?} must not be kept");
    }
    let vcf = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Bob\r\nX-VAUCHI-ID:contact-bob-1\r\nEND:VCARD\r\n";
    assert_eq!(
        import_vcf(vcf.as_bytes(), 0).unwrap()[0]
            .vauchi_id
            .as_deref(),
        Some("contact-bob-1")
    );
}

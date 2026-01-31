// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact_card::vcard (vCard 4.0 export/import)

use vauchi_core::contact_card::vcard::{export_vcard, import_vcard};
use vauchi_core::{ContactCard, ContactField, FieldType};

#[test]
fn test_export_basic_card() {
    let mut card = ContactCard::new("Alice Smith");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+15551234567",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@example.com",
    ))
    .unwrap();

    let vcard = export_vcard(&card);
    assert!(vcard.starts_with("BEGIN:VCARD"));
    assert!(vcard.contains("VERSION:4.0"));
    assert!(vcard.contains("FN:Alice Smith"));
    assert!(vcard.contains("TEL;TYPE=mobile:+15551234567"));
    assert!(vcard.contains("EMAIL;TYPE=work:alice@example.com"));
    assert!(vcard.ends_with("END:VCARD"));
}

#[test]
fn test_export_all_field_types() {
    let mut card = ContactCard::new("Bob");
    card.add_field(ContactField::new(FieldType::Phone, "home", "+15559876543"))
        .unwrap();
    card.add_field(ContactField::new(
        FieldType::Email,
        "personal",
        "bob@mail.com",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Website,
        "blog",
        "https://bob.dev",
    ))
    .unwrap();
    card.add_field(ContactField::new(FieldType::Address, "home", "123 Main St"))
        .unwrap();
    card.add_field(ContactField::new(FieldType::Social, "twitter", "@bob"))
        .unwrap();
    card.add_field(ContactField::new(FieldType::Custom, "notes", "Some note"))
        .unwrap();

    let vcard = export_vcard(&card);
    assert!(vcard.contains("TEL;TYPE=home:+15559876543"));
    assert!(vcard.contains("EMAIL;TYPE=personal:bob@mail.com"));
    assert!(vcard.contains("URL:https://bob.dev"));
    assert!(vcard.contains("ADR;TYPE=home:;;123 Main St;;;;"));
    assert!(vcard.contains("X-SOCIALPROFILE;TYPE=twitter:@bob"));
    assert!(vcard.contains("NOTE;TYPE=notes:Some note"));
}

#[test]
fn test_export_escaping() {
    let mut card = ContactCard::new("O'Brien, John");
    card.add_field(ContactField::new(FieldType::Custom, "note", "line1\nline2"))
        .unwrap();

    let vcard = export_vcard(&card);
    assert!(vcard.contains("FN:O'Brien\\, John"));
    assert!(vcard.contains("line1\\nline2"));
}

#[test]
fn test_import_basic_vcard() {
    let vcard = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Alice Smith\r\nTEL;TYPE=mobile:+15551234567\r\nEMAIL;TYPE=work:alice@example.com\r\nEND:VCARD";

    let card = import_vcard(vcard).unwrap();
    assert_eq!(card.display_name(), "Alice Smith");
    assert_eq!(card.fields().len(), 2);

    let phone = card
        .fields()
        .iter()
        .find(|f| f.field_type() == FieldType::Phone)
        .unwrap();
    assert_eq!(phone.value(), "+15551234567");
    assert_eq!(phone.label(), "mobile");

    let email = card
        .fields()
        .iter()
        .find(|f| f.field_type() == FieldType::Email)
        .unwrap();
    assert_eq!(email.value(), "alice@example.com");
    assert_eq!(email.label(), "work");
}

#[test]
fn test_import_url_field() {
    let vcard = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Bob\r\nURL:https://bob.dev\r\nEND:VCARD";

    let card = import_vcard(vcard).unwrap();
    assert_eq!(card.fields().len(), 1);
    let url_field = &card.fields()[0];
    assert_eq!(url_field.field_type(), FieldType::Website);
    assert_eq!(url_field.value(), "https://bob.dev");
}

#[test]
fn test_import_address_field() {
    let vcard =
        "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Bob\r\nADR;TYPE=home:;;123 Main St;;;;\r\nEND:VCARD";

    let card = import_vcard(vcard).unwrap();
    assert_eq!(card.fields().len(), 1);
    let addr = &card.fields()[0];
    assert_eq!(addr.field_type(), FieldType::Address);
    assert_eq!(addr.label(), "home");
}

#[test]
fn test_import_missing_begin() {
    let vcard = "VERSION:4.0\r\nFN:Alice\r\nEND:VCARD";
    let result = import_vcard(vcard);
    assert!(result.is_err());
}

#[test]
fn test_import_missing_fn() {
    let vcard = "BEGIN:VCARD\r\nVERSION:4.0\r\nTEL:+1234\r\nEND:VCARD";
    let result = import_vcard(vcard);
    assert!(result.is_err());
}

#[test]
fn test_import_empty_string() {
    let result = import_vcard("");
    assert!(result.is_err());
}

#[test]
fn test_roundtrip_export_import() {
    let mut card = ContactCard::new("Roundtrip Test");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+15551234567",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "test@example.com",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Website,
        "blog",
        "https://test.dev",
    ))
    .unwrap();

    let vcard = export_vcard(&card);
    let reimported = import_vcard(&vcard).unwrap();

    assert_eq!(reimported.display_name(), "Roundtrip Test");
    assert_eq!(reimported.fields().len(), 3);
}

#[test]
fn test_import_tel_without_type() {
    let vcard = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:NoType\r\nTEL:+15551234567\r\nEND:VCARD";
    let card = import_vcard(vcard).unwrap();
    assert_eq!(card.fields().len(), 1);
    let phone = &card.fields()[0];
    assert_eq!(phone.field_type(), FieldType::Phone);
}

#[test]
fn test_export_empty_card() {
    let card = ContactCard::new("Empty");
    let vcard = export_vcard(&card);
    assert!(vcard.contains("BEGIN:VCARD"));
    assert!(vcard.contains("FN:Empty"));
    assert!(vcard.contains("END:VCARD"));
}

#[test]
fn test_import_with_escaped_chars() {
    let vcard = "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Smith\\, John\r\nEND:VCARD";
    let card = import_vcard(vcard).unwrap();
    assert_eq!(card.display_name(), "Smith, John");
}

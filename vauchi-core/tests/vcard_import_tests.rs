// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the multi-version vCard import parser.

use vauchi_core::contact_card::vcard_import::{VCardImportError, import_vcf};
use vauchi_core::{ContactCard, FieldType};

/// Helper to get fields of a specific type from a card.
fn fields_of_type(card: &ContactCard, ft: FieldType) -> Vec<(&str, &str)> {
    card.fields()
        .iter()
        .filter(|f| f.field_type() == ft)
        .map(|f| (f.label(), f.value()))
        .collect()
}

// ── Basic parsing ───────────────────────────────────────────────────

#[test]
fn test_split_multi_contact_vcf() {
    let vcf = b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Alice\r\nEND:VCARD\r\n\
                 BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Bob\r\nEND:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0.display_name(), "Alice");
    assert_eq!(results[1].0.display_name(), "Bob");
}

#[test]
fn test_import_vcard_40_basic() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:John Doe\r\n\
                 TEL;TYPE=cell:+1-555-0100\r\n\
                 EMAIL;TYPE=work:john@example.com\r\n\
                 UID:urn:uuid:12345\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    assert_eq!(results.len(), 1);

    let (card, uid) = &results[0];
    assert_eq!(card.display_name(), "John Doe");
    assert_eq!(uid.as_deref(), Some("urn:uuid:12345"));

    let phones = fields_of_type(card, FieldType::Phone);
    assert_eq!(phones.len(), 1);
    assert_eq!(phones[0].1, "+1-555-0100");

    let emails = fields_of_type(card, FieldType::Email);
    assert_eq!(emails.len(), 1);
    assert_eq!(emails[0].0, "Work");
    assert_eq!(emails[0].1, "john@example.com");
}

#[test]
fn test_import_vcard_30_google() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:3.0\r\n\
                 FN:Jane Smith\r\n\
                 N:Smith;Jane;;;\r\n\
                 TEL;TYPE=CELL:+49-170-1234567\r\n\
                 EMAIL;TYPE=INTERNET:jane@gmail.com\r\n\
                 ORG:Acme Corp\r\n\
                 TITLE:Engineer\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    assert_eq!(results.len(), 1);

    let (card, _) = &results[0];
    assert_eq!(card.display_name(), "Jane Smith");

    let phones = fields_of_type(card, FieldType::Phone);
    assert_eq!(phones.len(), 1);
    assert_eq!(phones[0].0, "Mobile");

    let custom = fields_of_type(card, FieldType::Custom);
    let org = custom.iter().find(|(l, _)| *l == "Organization");
    assert!(org.is_some());
    assert_eq!(org.unwrap().1, "Acme Corp");

    let title = custom.iter().find(|(l, _)| *l == "Title");
    assert!(title.is_some());
    assert_eq!(title.unwrap().1, "Engineer");
}

#[test]
fn test_import_vcard_21_outlook() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:2.1\r\n\
                 N:Mueller;Hans;;;\r\n\
                 FN:Hans Mueller\r\n\
                 TEL;CELL:+49-171-9999999\r\n\
                 TEL;WORK;VOICE:+49-30-12345\r\n\
                 EMAIL;INTERNET:hans@outlook.com\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    assert_eq!(results.len(), 1);

    let (card, _) = &results[0];
    assert_eq!(card.display_name(), "Hans Mueller");

    let phones = fields_of_type(card, FieldType::Phone);
    assert_eq!(phones.len(), 2);
    // First phone should be Mobile (CELL)
    assert_eq!(phones[0].0, "Mobile");
    // Second should be Work
    assert_eq!(phones[1].0, "Work");
}

// ── QUOTED-PRINTABLE ────────────────────────────────────────────────

#[test]
fn test_vcard_21_quoted_printable_utf8() {
    // Simulate a v2.1 card with QP-encoded UTF-8 name
    let vcf = "BEGIN:VCARD\r\n\
                VERSION:2.1\r\n\
                FN;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:M=C3=BCller\r\n\
                N;CHARSET=UTF-8;ENCODING=QUOTED-PRINTABLE:M=C3=BCller;Hans;;;\r\n\
                END:VCARD\r\n";

    let results = import_vcf(vcf.as_bytes()).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0.display_name(), "Müller");
}

// ── Properties ──────────────────────────────────────────────────────

#[test]
fn test_parse_tel_with_types() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:Test\r\n\
                 TEL;TYPE=home:+1-555-0001\r\n\
                 TEL;TYPE=work:+1-555-0002\r\n\
                 TEL;TYPE=cell:+1-555-0003\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let phones = fields_of_type(&results[0].0, FieldType::Phone);
    assert_eq!(phones.len(), 3);
    assert_eq!(phones[0].0, "Home");
    assert_eq!(phones[1].0, "Work");
    assert_eq!(phones[2].0, "Mobile");
}

#[test]
fn test_parse_email_with_types() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:Test\r\n\
                 EMAIL;TYPE=home:home@test.com\r\n\
                 EMAIL;TYPE=work:work@test.com\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let emails = fields_of_type(&results[0].0, FieldType::Email);
    assert_eq!(emails.len(), 2);
    assert_eq!(emails[0].0, "Home");
    assert_eq!(emails[1].0, "Work");
}

#[test]
fn test_parse_adr_structured() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:Test\r\n\
                 ADR;TYPE=home:;;123 Main St;Springfield;IL;62701;USA\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let addrs = fields_of_type(&results[0].0, FieldType::Address);
    assert_eq!(addrs.len(), 1);
    assert_eq!(addrs[0].0, "Home");
    assert!(addrs[0].1.contains("123 Main St"));
    assert!(addrs[0].1.contains("Springfield"));
    assert!(addrs[0].1.contains("USA"));
}

#[test]
fn test_parse_org_title_nickname() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:Test User\r\n\
                 ORG:Big Corp;Engineering\r\n\
                 TITLE:Senior Dev\r\n\
                 NICKNAME:Testy\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let (card, _) = &results[0];

    assert_eq!(card.nickname(), Some("Testy"));

    let custom = fields_of_type(card, FieldType::Custom);
    let org = custom.iter().find(|(l, _)| *l == "Organization");
    assert!(org.is_some());
    assert!(org.unwrap().1.contains("Big Corp"));
    assert!(org.unwrap().1.contains("Engineering"));

    let title = custom.iter().find(|(l, _)| *l == "Title");
    assert_eq!(title.unwrap().1, "Senior Dev");
}

#[test]
fn test_parse_bday() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:Test\r\n\
                 BDAY:1990-05-15\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let bdays = fields_of_type(&results[0].0, FieldType::Birthday);
    assert_eq!(bdays.len(), 1);
    assert_eq!(bdays[0].1, "1990-05-15");
}

#[test]
fn test_parse_uid_returned() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:Test\r\n\
                 UID:abc-123-def\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    assert_eq!(results[0].1.as_deref(), Some("abc-123-def"));
}

// ── N fallback ──────────────────────────────────────────────────────

#[test]
fn test_fn_fallback_to_n() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:3.0\r\n\
                 N:Doe;John;;;\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0.display_name(), "John Doe");
}

// ── Group prefixes ──────────────────────────────────────────────────

#[test]
fn test_apple_group_prefix_with_ablabel() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:3.0\r\n\
                 FN:Alice\r\n\
                 item1.TEL:+1-555-0199\r\n\
                 item1.X-ABLabel:_$!<Main>!$_\r\n\
                 item2.URL:https://example.com\r\n\
                 item2.X-ABLabel:Homepage\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let (card, _) = &results[0];

    let phones = fields_of_type(card, FieldType::Phone);
    assert_eq!(phones.len(), 1);
    assert_eq!(phones[0].0, "Main");

    let urls = fields_of_type(card, FieldType::Website);
    assert_eq!(urls.len(), 1);
    assert_eq!(urls[0].0, "Homepage");
}

// ── Limits ──────────────────────────────────────────────────────────

#[test]
fn test_oversized_file_rejected() {
    let big = vec![b'x'; 10 * 1024 * 1024 + 1];
    let err = import_vcf(&big).unwrap_err();
    let VCardImportError::FileTooLarge { size, max } = err;
    assert_eq!(size, 10 * 1024 * 1024 + 1);
    assert_eq!(max, 10 * 1024 * 1024);
}

#[test]
fn test_oversized_field_truncated() {
    let long_name = "A".repeat(200);
    let vcf = format!("BEGIN:VCARD\r\nVERSION:4.0\r\nFN:{long_name}\r\nEND:VCARD\r\n");

    let results = import_vcf(vcf.as_bytes()).unwrap();
    assert_eq!(results.len(), 1);
    // Display name truncated to 100 chars
    assert_eq!(results[0].0.display_name().len(), 100);
}

#[test]
fn test_malformed_contact_skipped() {
    // First contact has no FN or N → skipped. Second is valid.
    let vcf = b"BEGIN:VCARD\r\nVERSION:4.0\r\nTEL:+1234\r\nEND:VCARD\r\n\
                 BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Valid\r\nEND:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0.display_name(), "Valid");
}

// ── Edge cases ──────────────────────────────────────────────────────

#[test]
fn test_empty_file_returns_empty() {
    let results = import_vcf(b"").unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_missing_version_defaults_to_30() {
    let vcf = b"BEGIN:VCARD\r\n\
                 FN:No Version\r\n\
                 TEL;TYPE=CELL:+1234567890\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0.display_name(), "No Version");
}

#[test]
fn test_bare_params_vcard_21() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:2.1\r\n\
                 FN:Test\r\n\
                 TEL;CELL:+1-555-0001\r\n\
                 TEL;HOME:+1-555-0002\r\n\
                 TEL;WORK;VOICE:+1-555-0003\r\n\
                 TEL;FAX:+1-555-0004\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let phones = fields_of_type(&results[0].0, FieldType::Phone);
    assert_eq!(phones.len(), 4);
    assert_eq!(phones[0].0, "Mobile");
    assert_eq!(phones[1].0, "Home");
    assert_eq!(phones[2].0, "Work");
    assert_eq!(phones[3].0, "Fax");
}

// ── Line unfolding ──────────────────────────────────────────────────

#[test]
fn test_line_unfolding() {
    // NOTE value split across two lines with space continuation
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:3.0\r\n\
                 FN:Test\r\n\
                 NOTE:This is a long\r\n  note value\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let notes = fields_of_type(&results[0].0, FieldType::Custom);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].1.contains("long"));
    assert!(notes[0].1.contains("note value"));
}

// ── Social profiles ─────────────────────────────────────────────────

#[test]
fn test_social_profile() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:Test\r\n\
                 X-SOCIALPROFILE;TYPE=twitter:https://twitter.com/testuser\r\n\
                 IMPP;TYPE=xmpp:xmpp:user@example.com\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let social = fields_of_type(&results[0].0, FieldType::Social);
    assert_eq!(social.len(), 2);
}

// ── Unescape / date / address via public API ───────────────────────

#[test]
fn test_unescape_backslash_sequences_via_note() {
    // Verify unescaping of \n, \,, \;, \:, \\ through a NOTE field
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:Test\r\n\
                 NOTE:hello\\nworld\\,and\\;more\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let notes = fields_of_type(&results[0].0, FieldType::Custom);
    assert_eq!(notes.len(), 1);
    assert!(notes[0].1.contains('\n'), "newline not unescaped");
    assert!(notes[0].1.contains(','), "comma not unescaped");
    assert!(notes[0].1.contains(';'), "semicolon not unescaped");
}

#[test]
fn test_compact_bday_format() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:Test\r\n\
                 BDAY:19900515\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let bdays = fields_of_type(&results[0].0, FieldType::Birthday);
    assert_eq!(bdays.len(), 1);
    assert_eq!(bdays[0].1, "1990-05-15");
}

#[test]
fn test_n_only_family_name() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:3.0\r\n\
                 N:Smith;;;;\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    assert_eq!(results[0].0.display_name(), "Smith");
}

#[test]
fn test_n_only_given_name() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:3.0\r\n\
                 N:;Jane;;;\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    assert_eq!(results[0].0.display_name(), "Jane");
}

#[test]
fn test_adr_full_structured() {
    let vcf = b"BEGIN:VCARD\r\n\
                 VERSION:4.0\r\n\
                 FN:Test\r\n\
                 ADR;TYPE=home:;;123 Main St;Springfield;IL;62701;USA\r\n\
                 END:VCARD\r\n";

    let results = import_vcf(vcf).unwrap();
    let addrs = fields_of_type(&results[0].0, FieldType::Address);
    assert_eq!(addrs[0].1, "123 Main St, Springfield, IL, 62701, USA");
}

// ── Proptest ────────────────────────────────────────────────────────

mod proptests {
    use proptest::prelude::*;
    use vauchi_core::contact_card::vcard_import::import_vcf;

    proptest! {
        #[test]
        fn vcard_parser_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
            // The parser must never panic, regardless of input.
            // It may return Ok or Err, but must not panic.
            let _ = import_vcf(&data);
        }

        #[test]
        fn vcard_parser_respects_field_limits(
            name in "[a-zA-Z ]{1,200}",
            value in "[a-zA-Z0-9 ]{0,2000}",
        ) {
            let vcf = format!(
                "BEGIN:VCARD\r\nVERSION:4.0\r\nFN:{name}\r\nNOTE:{value}\r\nEND:VCARD\r\n"
            );
            let results = import_vcf(vcf.as_bytes()).unwrap();
            if let Some((card, _)) = results.first() {
                // Display name max 100 chars
                assert!(card.display_name().chars().count() <= 100);
                // Field values max 1000 chars
                for field in card.fields() {
                    assert!(field.value().chars().count() <= 1000);
                }
            }
        }
    }
}

// ── Non-UTF-8 encoding support ──────────────────────────────────────

#[test]
fn import_latin1_contact() {
    // "José" in Latin-1: J(4A) o(6F) s(73) é(E9)
    let mut raw = b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:".to_vec();
    raw.extend_from_slice(&[0x4A, 0x6F, 0x73, 0xE9]); // "José" in Latin-1
    raw.extend_from_slice(b"\r\nEND:VCARD\r\n");

    let cards = import_vcf(&raw).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].0.display_name(), "Jos\u{e9}"); // "José"
}

#[test]
fn import_utf8_bom_stripped() {
    let mut raw = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
    raw.extend_from_slice(b"BEGIN:VCARD\r\nVERSION:4.0\r\nFN:Alice\r\nEND:VCARD\r\n");
    let cards = import_vcf(&raw).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].0.display_name(), "Alice");
}

#[test]
fn import_plain_utf8_still_works() {
    // "José" in UTF-8: J(4A) o(6F) s(73) é(C3 A9)
    let vcf = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:José\r\nEND:VCARD\r\n";
    let cards = import_vcf(vcf.as_bytes()).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].0.display_name(), "José");
}

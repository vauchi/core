// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Paper heirloom export (private#363): a printable, human-readable
//! record of the contact book. Traces to features/paper_heirloom.feature.

use proptest::prelude::*;
use vauchi_core::api::{
    HeirloomBook, HeirloomContact, HeirloomField, HeirloomText, render_heirloom_html,
};
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::{AuthMode, Contact, ContactCard, SymmetricKey, Vauchi};

const EXCHANGED_2026_10_08: u64 = 1_791_417_600;
const BERLIN_LAT: f64 = 52.5200;
const BERLIN_LON: f64 = 13.4050;

fn setup() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

fn add_contact(wb: &Vauchi, name: &str, fields: &[(&str, &str)], exchanged_at: u64) -> Contact {
    let mut pk = [0u8; 32];
    for (i, b) in name.bytes().enumerate().take(32) {
        pk[i] = b;
    }
    let mut card = ContactCard::new(name);
    for (label, value) in fields {
        card.add_field(ContactField::new(
            FieldType::Email,
            label,
            value,
            exchanged_at,
        ))
        .unwrap();
    }
    let contact = Contact::from_exchange(pk, card, SymmetricKey::generate(), exchanged_at);
    wb.add_contact(contact.clone()).unwrap();
    contact
}

fn text() -> HeirloomText {
    HeirloomText {
        document_title: "Contacts of".into(),
        exchanged_on: "Exchanged on".into(),
        met_at: "at".into(),
    }
}

fn contact(name: &str) -> HeirloomContact {
    HeirloomContact {
        name: name.into(),
        fields: vec![],
        exchanged_at: None,
        place: None,
        avatar_webp: None,
    }
}

fn book(contacts: Vec<HeirloomContact>) -> HeirloomBook {
    HeirloomBook {
        owner_name: "Alice".into(),
        contacts,
    }
}

// @scenario: paper_heirloom :: Export my contacts as a printable document
#[test]
fn book_lists_contacts_by_name_with_fields_and_exchange_date() {
    let wb = setup();
    add_contact(
        &wb,
        "Bob",
        &[("Work", "bob@example.com")],
        EXCHANGED_2026_10_08,
    );
    add_contact(&wb, "Ann", &[], 0);

    let book = wb.heirloom_book().unwrap();

    assert_eq!(book.owner_name, "Alice");
    let names: Vec<&str> = book.contacts.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Ann", "Bob"]);
    let bob = &book.contacts[1];
    assert_eq!(
        bob.fields,
        vec![HeirloomField {
            label: "Work".into(),
            value: "bob@example.com".into()
        }]
    );
    assert_eq!(bob.exchanged_at, Some(EXCHANGED_2026_10_08));
}

// @scenario: paper_heirloom :: Export my contacts as a printable document
#[test]
fn rendered_document_shows_names_fields_dates_and_place() {
    let mut bob = contact("Bob");
    bob.fields = vec![HeirloomField {
        label: "Work".into(),
        value: "bob@example.com".into(),
    }];
    bob.exchanged_at = Some(EXCHANGED_2026_10_08);
    bob.place = Some("Café Einstein".into());

    let html = render_heirloom_html(&book(vec![bob]), &text());

    assert!(html.starts_with("<!DOCTYPE html>"), "{html}");
    assert!(html.contains("<h1>Contacts of Alice</h1>"), "{html}");
    assert!(html.contains("<h2>Bob</h2>"), "{html}");
    assert!(
        html.contains("<dt>Work</dt><dd>bob@example.com</dd>"),
        "{html}"
    );
    assert!(
        html.contains("Exchanged on 2026-10-08 at Café Einstein"),
        "{html}"
    );
}

// @scenario: paper_heirloom :: Export my contacts as a printable document
#[test]
fn dates_render_as_calendar_days_in_utc() {
    for (secs, day) in [
        (0, "1970-01-01"),
        (951_782_400, "2000-02-29"),
        (1_735_603_200, "2024-12-31"),
        (1_791_503_999, "2026-10-08"),
    ] {
        let mut c = contact("Bob");
        c.exchanged_at = Some(secs);
        let html = render_heirloom_html(&book(vec![c]), &text());
        assert!(
            html.contains(&format!("Exchanged on {day}")),
            "{secs}: {html}"
        );
    }
}

// @scenario: paper_heirloom :: Export my contacts as a printable document
#[test]
fn book_carries_the_named_place() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob", &[], EXCHANGED_2026_10_08);
    wb.set_exchange_location(bob.id(), BERLIN_LAT, BERLIN_LON)
        .unwrap();
    wb.name_exchange_place(bob.id(), "Café Einstein").unwrap();

    let book = wb.heirloom_book().unwrap();

    assert_eq!(book.contacts[0].place.as_deref(), Some("Café Einstein"));
}

// @scenario: paper_heirloom :: The document carries no app-internal identifiers
#[test]
fn document_has_no_keys_ids_or_coordinates() {
    let wb = setup();
    let bob = add_contact(&wb, "Bob", &[], EXCHANGED_2026_10_08);
    wb.set_exchange_location(bob.id(), BERLIN_LAT, BERLIN_LON)
        .unwrap();
    wb.name_exchange_place(bob.id(), "Office").unwrap();
    let place_id = wb
        .exchange_location(bob.id())
        .unwrap()
        .unwrap()
        .place_id
        .unwrap();

    let html = render_heirloom_html(&wb.heirloom_book().unwrap(), &text());

    for secret in [
        bob.id().to_string(),
        bob.fingerprint(),
        place_id,
        "52.52".to_string(),
        "13.405".to_string(),
    ] {
        assert!(!html.contains(&secret), "leaked {secret}: {html}");
    }
}

// @scenario: paper_heirloom :: Contact-provided text cannot inject markup
#[test]
fn contact_text_is_escaped() {
    let cases = [
        (
            "<script>alert(1)</script>",
            "&lt;script&gt;alert(1)&lt;/script&gt;",
        ),
        (
            "\"><img src=x onerror=alert(1)>",
            "&quot;&gt;&lt;img src=x onerror=alert(1)&gt;",
        ),
        ("Tom & Jerry", "Tom &amp; Jerry"),
        ("O'Brien", "O&#39;Brien"),
        ("Zoë 🌱", "Zoë 🌱"),
    ];
    for (raw, escaped) in cases {
        let mut c = contact(raw);
        c.fields = vec![HeirloomField {
            label: raw.into(),
            value: raw.into(),
        }];
        c.place = Some(raw.into());
        c.exchanged_at = Some(0);
        let html = render_heirloom_html(&book(vec![c]), &text());
        assert!(
            html.contains(&format!("<h2>{escaped}</h2>")),
            "{raw}: {html}"
        );
        assert!(
            html.contains(&format!("<dt>{escaped}</dt><dd>{escaped}</dd>")),
            "{raw}: {html}"
        );
        assert!(!html.contains("<script>"), "{raw}: {html}");
        assert!(!html.contains("<img src=x"), "{raw}: {html}");
    }
}

// @scenario: paper_heirloom :: Contact-provided text cannot inject markup
#[test]
fn owner_name_and_labels_are_escaped_too() {
    let mut b = book(vec![]);
    b.owner_name = "<b>Alice</b>".into();
    let mut t = text();
    t.document_title = "<i>Contacts</i>".into();

    let html = render_heirloom_html(&b, &t);

    assert!(
        html.contains("<h1>&lt;i&gt;Contacts&lt;/i&gt; &lt;b&gt;Alice&lt;/b&gt;</h1>"),
        "{html}"
    );
}

// @scenario: paper_heirloom :: Export my contacts as a printable document
#[test]
fn webp_avatar_is_embedded_and_other_bytes_are_dropped() {
    let webp = b"RIFF\x0c\x00\x00\x00WEBPVP8 ".to_vec();
    let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    let mut with_webp = contact("Bob");
    with_webp.avatar_webp = Some(webp);
    let mut with_png = contact("Eve");
    with_png.avatar_webp = Some(png);

    let html = render_heirloom_html(&book(vec![with_webp, with_png]), &text());

    assert_eq!(html.matches("data:image/webp;base64,").count(), 1, "{html}");
    assert!(
        html.contains("data:image/webp;base64,UklGRgwAAABXRUJQVlA4IA=="),
        "{html}"
    );
    assert!(!html.contains("iVBORw0KGgo"), "PNG bytes embedded: {html}");
}

// @scenario: paper_heirloom :: Duress mode exports only decoy contacts
#[test]
fn duress_mode_book_holds_only_decoys() {
    let mut wb = setup();
    add_contact(&wb, "Bob", &[], EXCHANGED_2026_10_08);
    wb.setup_app_password("normal-pin").unwrap();
    wb.setup_duress_password("112233").unwrap();
    wb.add_decoy_contact("decoy-dana", "Decoy Dana", &ContactCard::new("Decoy Dana"))
        .unwrap();
    assert_eq!(wb.authenticate("112233").unwrap(), AuthMode::Duress);

    let book = wb.heirloom_book().unwrap();

    let names: Vec<&str> = book.contacts.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Decoy Dana"]);
}

// @scenario: paper_heirloom :: The same contacts always produce the same document
#[test]
fn same_book_renders_identically() {
    let wb = setup();
    add_contact(
        &wb,
        "Bob",
        &[("Work", "bob@example.com")],
        EXCHANGED_2026_10_08,
    );
    add_contact(&wb, "Ann", &[], 0);

    let first = render_heirloom_html(&wb.heirloom_book().unwrap(), &text());
    let second = render_heirloom_html(&wb.heirloom_book().unwrap(), &text());

    assert_eq!(first, second);
}

proptest! {
    // @scenario: paper_heirloom :: Contact-provided text cannot inject markup
    #[test]
    fn any_name_renders_without_raw_markup_and_deterministically(name in "\\PC{0,40}") {
        let b = book(vec![contact(&name)]);
        let html = render_heirloom_html(&b, &text());
        prop_assert_eq!(&html, &render_heirloom_html(&b, &text()));
        let body = html.split("<h2>").nth(1).unwrap().split("</h2>").next().unwrap();
        prop_assert!(!body.contains('<') && !body.contains('>') && !body.contains('"'));
    }
}

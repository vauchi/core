// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Seeded in-memory Core for the screen catalog: one identity, a small
//! realistic address book, two groups, and own-card entries with mixed
//! visibility. Names are fixed so titles and node structure are stable
//! across regenerations even though keys and ids are random per run.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{Identity, Vauchi};

/// Fixed exchange timestamp so relative-time labels never drift.
const SEED_NOW: u64 = 1_700_000_000;

pub const OWNER_NAME: &str = "Ada Lovelace";

pub struct SeededContact {
    pub name: &'static str,
    pub id: String,
    pub field_labels: Vec<String>,
}

pub struct SeededWorld {
    pub vauchi: Vauchi,
    pub contacts: Vec<SeededContact>,
    pub family_group_id: String,
    pub own_phone_field_id: String,
}

impl SeededWorld {
    pub fn verified(&self) -> &SeededContact {
        &self.contacts[0]
    }

    pub fn unverified(&self) -> &SeededContact {
        &self.contacts[1]
    }
}

/// `(name, phone, website)` per contact; the archived one comes last.
const CONTACTS: [(&str, &str, &str); 7] = [
    ("Grace Hopper", "+1 555 0100", "https://grace.example.org"),
    (
        "Alan Turing",
        "+44 20 7946 0100",
        "https://turing.example.org",
    ),
    (
        "Charles Babbage",
        "+44 20 7946 0101",
        "https://babbage.example.org",
    ),
    (
        "Mary Somerville",
        "+44 131 496 0100",
        "https://somerville.example.org",
    ),
    (
        "Annabella Byron",
        "+44 20 7946 0102",
        "https://byron.example.org",
    ),
    (
        "Emmy Noether",
        "+49 551 390100",
        "https://noether.example.org",
    ),
    (
        "Katherine Johnson",
        "+1 757 555 0100",
        "https://johnson.example.org",
    ),
];

fn field(field_type: FieldType, label: &str, value: &str) -> ContactField {
    ContactField::new(field_type, label, value, SEED_NOW)
}

fn contact_card(name: &str, phone: &str, website: &str) -> ContactCard {
    let handle = name.to_lowercase().replace(' ', ".");
    let mut card = ContactCard::new(name);
    for entry in [
        field(FieldType::Phone, "mobile", phone),
        field(FieldType::Email, "work", &format!("{handle}@example.org")),
        field(FieldType::Website, "homepage", website),
    ] {
        card.add_field(entry).expect("seed card field");
    }
    card
}

fn exchanged_contact(name: &str, phone: &str, website: &str) -> Contact {
    let peer = Identity::create(name, SEED_NOW);
    Contact::from_exchange(
        *peer.signing_public_key(),
        contact_card(name, phone, website),
        SymmetricKey::generate(),
        SEED_NOW,
    )
}

fn seed_own_card(vauchi: &Vauchi) -> String {
    for entry in [
        field(FieldType::Phone, "mobile", "+44 20 7946 0958"),
        field(FieldType::Email, "work", "ada@analytical.example.org"),
        field(FieldType::Website, "homepage", "https://ada.example.org"),
        field(FieldType::Address, "home", "12 St James's Square, London"),
        field(FieldType::Birthday, "birthday", "1815-12-10"),
    ] {
        vauchi.add_own_field(entry).expect("seed own field");
    }
    let card = vauchi
        .own_card()
        .expect("own card")
        .expect("identity has a card");
    let field_id = |label: &str| {
        card.fields()
            .iter()
            .find(|entry| entry.label() == label)
            .map(|entry| entry.id().to_owned())
            .expect("seeded own field")
    };
    vauchi
        .set_own_field_private(&field_id("home"))
        .expect("private address");
    vauchi
        .set_own_field_private(&field_id("birthday"))
        .expect("private birthday");
    field_id("mobile")
}

/// Identity plus own-card entries only — the "filled my info, empty
/// contacts" starting point every seeded world builds on.
pub fn identity_only() -> Vauchi {
    let mut vauchi = Vauchi::in_memory().expect("in-memory Core");
    vauchi.create_identity(OWNER_NAME).expect("seed identity");
    vauchi
}

pub fn seeded_world() -> SeededWorld {
    let vauchi = identity_only();
    let own_phone_field_id = seed_own_card(&vauchi);

    let contacts: Vec<SeededContact> = CONTACTS
        .iter()
        .map(|(name, phone, website)| {
            let contact = exchanged_contact(name, phone, website);
            let field_labels = contact
                .card()
                .fields()
                .iter()
                .map(|entry| entry.label().to_owned())
                .collect();
            let id = contact.id().to_owned();
            vauchi.add_contact(contact).expect("seed contact");
            SeededContact {
                name,
                id,
                field_labels,
            }
        })
        .collect();

    let family = vauchi.create_group("Family").expect("family group");
    let cycling = vauchi.create_group("Cycling club").expect("cycling group");
    for index in [2, 4] {
        vauchi
            .add_contact_to_group(family.id(), &contacts[index].id)
            .expect("family member");
    }
    for index in [0, 1, 5] {
        vauchi
            .add_contact_to_group(cycling.id(), &contacts[index].id)
            .expect("cycling member");
    }

    vauchi
        .verify_contact_fingerprint(&contacts[0].id)
        .expect("verified contact");
    vauchi
        .archive_contact(&contacts[6].id)
        .expect("archived contact");

    SeededWorld {
        vauchi,
        contacts,
        family_group_id: family.id().to_owned(),
        own_phone_field_id,
    }
}

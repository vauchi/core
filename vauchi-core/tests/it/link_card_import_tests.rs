// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Vauchi::import_received_link_card` — slice
//! `2026-05-24-core-exchange-completion-contact-save`.
//!
//! A link-mode received card becomes an **Imported** contact
//! (`ImportSource::LinkExchange`): link mode establishes no persistent
//! update channel, so per HR-1 it carries no `ExchangedData`.

use vauchi_core::contact_card::ContactCard;
use vauchi_core::{ImportSource, Vauchi};

fn create_test_vauchi() -> Vauchi {
    Vauchi::in_memory().unwrap()
}

// @internal
#[test]
fn import_received_link_card_creates_imported_link_exchange_contact() {
    let vauchi = create_test_vauchi();
    let card = ContactCard::new("Alice");
    let expected_name = card.display_name().to_string();

    let id = vauchi
        .import_received_link_card(card)
        .expect("import should succeed");

    let contact = vauchi
        .get_contact(&id)
        .expect("get_contact ok")
        .expect("contact must exist after import");
    assert_eq!(contact.display_name(), expected_name);
    let imported = contact
        .kind()
        .imported_data()
        .expect("a link-mode contact must be Imported (no update channel), not Exchanged");
    assert_eq!(imported.source, ImportSource::LinkExchange);
}

// @internal
#[test]
fn import_received_link_card_is_idempotent_by_card_id() {
    let vauchi = create_test_vauchi();
    let card = ContactCard::new("Bob");

    let id1 = vauchi
        .import_received_link_card(card.clone())
        .expect("first import");
    let id2 = vauchi
        .import_received_link_card(card)
        .expect("second import");

    assert_eq!(id1, id2, "re-receiving the same card returns the same id");
    assert_eq!(
        vauchi.list_contacts().expect("list").len(),
        1,
        "idempotent — the same card must not duplicate",
    );
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A personal note saved in the app reaches linked devices (private#452):
//! the app's save went through the low-level store, which journals nothing.

use crate::common;

use common::device_sync::{journal_for_tablet, link_tablet};
use vauchi_app::ui::*;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::sync::SyncItem;

// @scenario: contacts_management :: A personal note saved in the app reaches linked devices
// @scenario: navigation :: Add private note to a contact
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn a_note_saved_in_the_app_is_journaled_for_linked_devices() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let contact = Contact::from_exchange(
        [4; 32],
        ContactCard::new("Dave"),
        SymmetricKey::generate(),
        0,
    );
    let dave = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    let (registry, tablet_id) = link_tablet(&vauchi, [31; 32]);
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: dave.clone(),
    });

    for action in [
        UserAction::ActionPressed {
            action_id: "edit_personal_note".into(),
        },
        UserAction::TextChanged {
            component_id: "personal_note".into(),
            value: "Colleague at Acme".into(),
        },
        UserAction::ActionPressed {
            action_id: "save_personal_note".into(),
        },
    ] {
        let _ = engine.handle_action(action);
    }

    let journal = journal_for_tablet(engine.vauchi(), registry, &tablet_id);
    assert!(
        journal.iter().any(|item| matches!(
            item,
            SyncItem::PersonalNoteChanged { contact_id, note, .. }
                if *contact_id == dave && note == "Colleague at Acme"
        )),
        "the saved note is journaled for the tablet, got {journal:?}"
    );
}

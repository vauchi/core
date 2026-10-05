// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The full-link payload carries every per-contact owner flag, so a device
//! linked later matches its siblings (#295, owner decision 2026-10-05):
//! blocked, archived, ignored, favorite and hidden.

use proptest::prelude::*;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::sync::ContactSyncData;
use vauchi_core::{Contact, SymmetricKey};

#[derive(Debug, Clone, PartialEq)]
struct OwnerFlags {
    blocked: bool,
    hidden: bool,
    favorite: bool,
    archived_at: Option<u64>,
    ignored_at: Option<u64>,
}

fn contact_with(flags: &OwnerFlags) -> Contact {
    let mut contact = Contact::from_exchange(
        [0x42u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::from_bytes([0x55u8; 32]),
        0,
    );
    if flags.blocked {
        contact.block();
    }
    contact.set_hidden(flags.hidden);
    contact.set_favorite(flags.favorite);
    if let Some(at) = flags.archived_at {
        contact.archive(at);
    }
    if let Some(at) = flags.ignored_at {
        contact.ignore(at);
    }
    contact
}

fn flags_of(contact: &Contact) -> OwnerFlags {
    OwnerFlags {
        blocked: contact.is_blocked(),
        hidden: contact.is_hidden(),
        favorite: contact.is_favorite(),
        archived_at: contact.archived_at(),
        ignored_at: contact.ignored_at(),
    }
}

fn owner_flags() -> impl Strategy<Value = OwnerFlags> {
    (
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        proptest::option::of(1u64..=u64::from(u32::MAX)),
        proptest::option::of(1u64..=u64::from(u32::MAX)),
    )
        .prop_map(
            |(blocked, hidden, favorite, archived_at, ignored_at)| OwnerFlags {
                blocked,
                hidden,
                favorite,
                archived_at,
                ignored_at,
            },
        )
}

proptest! {
    // @scenario: device_management :: New device receives full state
    #[test]
    fn full_link_payload_preserves_every_owner_flag(flags in owner_flags()) {
        let json = serde_json::to_string(&ContactSyncData::from_contact(&contact_with(&flags))).unwrap();
        let restored: ContactSyncData = serde_json::from_str(&json).unwrap();

        prop_assert_eq!(flags_of(&restored.to_contact().unwrap()), flags);
    }
}

/// A payload from a build that predates the owner flags still decodes, with
/// every flag at its default.
// @scenario: device_management :: New device receives full state
#[test]
fn payload_without_owner_flags_decodes_with_defaults() {
    let mut value =
        serde_json::to_value(ContactSyncData::from_contact(&contact_with(&OwnerFlags {
            blocked: true,
            hidden: true,
            favorite: true,
            archived_at: Some(7),
            ignored_at: Some(9),
        })))
        .unwrap();
    let object = value.as_object_mut().unwrap();
    for field in ["blocked", "hidden", "favorite", "archived_at", "ignored_at"] {
        object.remove(field);
    }

    let restored: ContactSyncData = serde_json::from_value(value).unwrap();

    assert_eq!(
        flags_of(&restored.to_contact().unwrap()),
        OwnerFlags {
            blocked: false,
            hidden: false,
            favorite: false,
            archived_at: None,
            ignored_at: None,
        }
    );
}

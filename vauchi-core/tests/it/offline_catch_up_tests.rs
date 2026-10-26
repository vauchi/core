// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A client that was offline for a long time still receives what the relay
//! kept for it (vauchi/private#691): the relay retains an envelope 120
//! days, and the receiver must route the whole retained interval.

use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use vauchi_core::Vauchi;
use vauchi_core::api::vauchi::process_received_blobs;
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};

use crate::common::card_update::seal_update_default;
use crate::common::helpers::create_vauchi_with_identity;
use crate::common::linked_peer::link_contacts;

/// The record's offline-routing probe, promoted: red on main c37f74d3 at
/// day 100 (`token_resolved: false`), since receive routing resolved only
/// today's and yesterday's mailbox tokens.
// @scenario: message_delivery :: Receive pending updates when coming online
#[test]
fn a_retained_update_still_routes_after_100_days_offline() {
    for days_offline in [0, 100] {
        let clock = Arc::new(FakeClock::new(
            UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        ));
        let mut alice = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
        alice.create_identity("Alice").unwrap();
        let bob = create_vauchi_with_identity("Bob");
        let linked = link_contacts(&alice, &bob, "Bob");
        let alice_pk = *alice.identity().unwrap().signing_public_key();
        let old_card = ContactCard::new("Bob");
        let mut new_card = old_card.clone();
        new_card
            .add_field(ContactField::new(
                FieldType::Email,
                "Email",
                "bob@example.invalid",
                0,
            ))
            .unwrap();
        let ciphertext = seal_update_default(&bob, &linked.host_contact_id, &old_card, &new_card);
        let token = token_hex(&compute_mailbox_token(
            linked.shared_secret.as_bytes(),
            &alice_pk,
            current_day_epoch(clock.unix_seconds()),
        ));
        clock.advance(Duration::from_secs(days_offline * 86_400));

        let outcomes = process_received_blobs(
            alice.identity().unwrap(),
            alice.storage(),
            &alice.storage().contacts().list_contacts().unwrap(),
            vec![("retained-update".into(), token, ciphertext, None)],
        );

        assert_eq!(outcomes.len(), 1);
        assert!(
            outcomes[0].token_resolved,
            "retained mailbox must resolve after {days_offline} days: {outcomes:?}"
        );
        assert!(
            outcomes[0].decrypted,
            "retained update must decrypt after {days_offline} days: {outcomes:?}"
        );
        let saved = alice
            .storage()
            .contacts()
            .load_contact(&linked.peer_contact_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            saved
                .card()
                .fields()
                .iter()
                .map(|field| field.value())
                .collect::<Vec<_>>(),
            vec!["bob@example.invalid"]
        );
    }
}

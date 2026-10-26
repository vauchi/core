// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::card::ContactCard;
use crate::crypto::SymmetricKey;
use crate::network::mailbox_token::{
    compute_device_mailbox_token, compute_device_sync_token, compute_mailbox_token, token_hex,
};
use proptest::prelude::*;

// @internal: Captured windows cannot cross identity, device, application-relay or OHTTP-relay scope.
#[test]
fn mismatched_scope_does_not_authorize_any_route() {
    let identity = Identity::create("Alice", 0);
    let relay = RelayConfig::default();
    let context = registered_mailboxes(&identity, &[], &relay, &[41, 42]);
    let token = token_hex(&compute_device_sync_token(
        identity.master_seed(),
        identity.device_id(),
        42,
    ));
    for changed in 0..4 {
        let mut stale = context.clone();
        match changed {
            0 => stale.identity[0] ^= 1,
            1 => stale.device[0] ^= 1,
            2 => stale.application_relay.push_str("/changed"),
            3 => stale.ohttp_relay.push_str("/changed"),
            _ => unreachable!(),
        }
        let mut routes = ReceiveContext::new(&identity, &[], &relay);
        assert!(!routes.include(&stale, &token));
        assert!(routes.contact_tokens.is_empty());
        assert!(routes.self_tokens.is_empty());
    }
}

// @internal: Pending replies use current relationship authorization, not stale keys or removed contacts.
#[test]
fn removed_blocked_and_rekeyed_contacts_do_not_resolve_the_old_mailbox() {
    let identity = Identity::create("Alice", 0);
    let relay = RelayConfig::default();
    let original = Contact::from_exchange(
        [7; 32],
        ContactCard::new("Bob"),
        SymmetricKey::from_bytes([4; 32]),
        0,
    );
    let mut blocked = original.clone();
    blocked.set_blocked(true);
    let rekeyed = Contact::from_exchange(
        [7; 32],
        ContactCard::new("Bob"),
        SymmetricKey::from_bytes([5; 32]),
        0,
    );
    let context = registered_mailboxes(&identity, &[original], &relay, &[99, 100]);
    for contacts in [vec![], vec![blocked], vec![rekeyed]] {
        let mut routes = ReceiveContext::new(&identity, &contacts, &relay);
        for day in [99, 100] {
            for token in [
                compute_mailbox_token(&[4; 32], identity.signing_public_key(), day),
                compute_device_mailbox_token(
                    &[4; 32],
                    identity.signing_public_key(),
                    identity.device_id(),
                    day,
                ),
            ] {
                assert!(!routes.include(&context, &token_hex(&token)));
                assert_eq!(routes.contact_tokens.get(&token_hex(&token)), None);
            }
        }
    }
}

// @internal: Collecting distinct epochs attributes only observed tokens, not every contact/day combination.
#[test]
fn collecting_multiple_days_keeps_only_the_selected_routes() {
    let identity = Identity::create("Alice", 0);
    let relay = RelayConfig::default();
    let contacts = [Contact::from_exchange(
        [7; 32],
        ContactCard::new("Bob"),
        SymmetricKey::from_bytes([4; 32]),
        0,
    )];
    let mut received = ReceiveContext::new(&identity, &contacts, &relay);
    for day in [0, 1, 100, 1000, u64::MAX] {
        let full =
            registered_mailboxes(&identity, &contacts, &relay, &[day.saturating_sub(1), day]);
        let token = token_hex(&compute_mailbox_token(
            &[4; 32],
            identity.signing_public_key(),
            day,
        ));
        assert!(received.include(&full, &token));
        assert_eq!(
            received.contact_tokens.get(&token).map(String::as_str),
            Some(contacts[0].id())
        );
    }
    assert_eq!(received.contact_tokens.len(), 5);
    assert!(received.self_tokens.is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // @internal: Exact locally captured epochs, including early/extreme clocks, bound both contact token families and self routes.
    #[test]
    fn captured_epochs_route_current_keys_without_an_age_search(day in prop_oneof![Just(0), Just(1), Just(u64::MAX), any::<u64>()]) {
        let identity = Identity::create("Alice", 0);
        let relay = RelayConfig::default();
        let contact = Contact::from_exchange(
            [7; 32], ContactCard::new("Bob"), SymmetricKey::from_bytes([4; 32]), 0,
        );
        let contacts = [contact.clone()];
        let mut routes = ReceiveContext::new(&identity, &contacts, &relay);
        let context = registered_mailboxes(&identity, &contacts, &relay, &[day.saturating_sub(1), day]);
        for token_day in [day.saturating_sub(1), day] {
            for token in [
                compute_mailbox_token(&[4; 32], identity.signing_public_key(), token_day),
                compute_device_mailbox_token(&[4; 32], identity.signing_public_key(), identity.device_id(), token_day),
            ] {
                prop_assert!(routes.include(&context, &token_hex(&token)));
                prop_assert_eq!(routes.contact_tokens.get(&token_hex(&token)).map(String::as_str), Some(contact.id()));
            }
            let self_token = compute_device_sync_token(identity.master_seed(), identity.device_id(), token_day);
            prop_assert!(routes.include(&context, &token_hex(&self_token)));
            prop_assert!(routes.self_tokens.contains(&token_hex(&self_token)));
        }
        let foreign = compute_device_mailbox_token(&[4; 32], identity.signing_public_key(), &[99; 32], day);
        prop_assert!(!routes.include(&context, &token_hex(&foreign)));
        prop_assert_eq!(routes.contact_tokens.get(&token_hex(&foreign)), None);
        let wrong_direction = compute_mailbox_token(&[4; 32], &[7; 32], day);
        prop_assert!(!routes.include(&context, &token_hex(&wrong_direction)));
        prop_assert_eq!(routes.contact_tokens.get(&token_hex(&wrong_direction)), None);
        if day > 1 {
            let outside = compute_mailbox_token(&[4; 32], identity.signing_public_key(), day - 2);
            prop_assert!(!routes.include(&context, &token_hex(&outside)));
            prop_assert_eq!(routes.contact_tokens.get(&token_hex(&outside)), None);
        }
    }
}

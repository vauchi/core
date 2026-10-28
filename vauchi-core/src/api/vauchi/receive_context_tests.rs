// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::card::ContactCard;
use crate::crypto::SymmetricKey;
use crate::network::mailbox_token::{
    compute_device_mailbox_token, compute_device_sync_token, compute_mailbox_token, token_hex,
};
use proptest::prelude::*;

// @internal: a registration batch carries one day's routes, so no fetch request
// spans two days and the relay cannot chain a fetcher across days (private#890).
#[test]
fn registration_batches_carry_one_day_of_routes_each() {
    let identity = Identity::create("Alice", 0);
    let relay = RelayConfig::default();
    let contacts: Vec<Contact> = (1..=3u8)
        .map(|i| {
            Contact::from_exchange(
                [i; 32],
                ContactCard::new("Peer"),
                SymmetricKey::from_bytes([i + 10; 32]),
                0,
            )
        })
        .collect();
    let days = [41u64, 42, 43];
    let context = registered_mailboxes(&identity, &contacts, &relay, &days);
    let batches = registration_batches(&crate::rng::OsSecureRng::new(), &context.routes);

    let day_of = |token: &String| match context.routes.get(token) {
        Some(MailboxRoute::Contact { day, .. }) | Some(MailboxRoute::DeviceSync { day }) => {
            Some(*day)
        }
        None => None,
    };
    let mut days_seen = std::collections::BTreeSet::new();
    let mut routed = 0;
    for batch in &batches {
        assert_eq!(batch.len(), 256, "every batch is padded");
        let batch_days: std::collections::BTreeSet<u64> = batch.iter().filter_map(day_of).collect();
        assert_eq!(batch_days.len(), 1, "a batch mixes days: {batch_days:?}");
        routed += batch.iter().filter(|t| day_of(t).is_some()).count();
        days_seen.extend(batch_days);
    }
    assert_eq!(
        routed,
        context.routes.len(),
        "every route is registered once"
    );
    assert_eq!(days_seen.into_iter().collect::<Vec<_>>(), days.to_vec());
}

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

// @internal: Every day a receive plan covers has all three route families — contact identity, contact device-scoped, self device-sync — so a chunk of historical days routes like the live days (private#691).
#[test]
fn every_planned_day_routes_all_three_mailbox_families() {
    use crate::network::mailbox_token::{CATCH_UP_DAYS_PER_CYCLE, plan_receive};
    let identity = Identity::create("Alice", 0);
    let relay = RelayConfig::default();
    let contact = Contact::from_exchange(
        [7; 32],
        ContactCard::new("Bob"),
        SymmetricKey::from_bytes([4; 32]),
        0,
    );
    let contacts = [contact.clone()];
    let today = 20_000;
    let plan = plan_receive(today, Some(today - 60));
    assert_eq!(plan.days.len() as u64, CATCH_UP_DAYS_PER_CYCLE + 2);
    let context = registered_mailboxes(&identity, &contacts, &relay, &plan.days);
    let mut routes = ReceiveContext::new(&identity, &contacts, &relay);
    for &day in &plan.days {
        for token in [
            compute_mailbox_token(&[4; 32], identity.signing_public_key(), day),
            compute_device_mailbox_token(
                &[4; 32],
                identity.signing_public_key(),
                identity.device_id(),
                day,
            ),
        ] {
            assert!(routes.include(&context, &token_hex(&token)), "day {day}");
        }
        let self_token =
            compute_device_sync_token(identity.master_seed(), identity.device_id(), day);
        assert!(
            routes.include(&context, &token_hex(&self_token)),
            "day {day}"
        );
    }
    assert_eq!(routes.contact_tokens.len(), 2 * plan.days.len());
    assert_eq!(routes.self_tokens.len(), plan.days.len());
    let unplanned = compute_mailbox_token(&[4; 32], identity.signing_public_key(), today - 30);
    assert!(!routes.include(&context, &token_hex(&unplanned)));
}

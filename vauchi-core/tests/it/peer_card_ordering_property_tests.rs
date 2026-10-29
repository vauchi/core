// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // @claim: S2
    // @scenario: sync_updates :: Genesis card versions are floored per origin device
    #[test]
    fn signed_peer_deltas_preserve_final_removal_under_delivery_permutations(
        priorities in any::<[u8; 4]>(),
        first_timestamp in 1u64..100_000,
    ) {
        let world = cold_start_world();
        let mut old = world.base_card.clone();
        old.update_field_value(world.base_card.fields()[0].id(), "old@example.com", first_timestamp).unwrap();
        old.set_display_name("Old").unwrap();
        let mut sibling = old.clone();
        sibling.update_field_value(world.base_card.fields()[0].id(), "sibling@example.com", first_timestamp + 1).unwrap();
        sibling.set_display_name("Sibling").unwrap();
        let mut removed = world.base_card.clone();
        removed.remove_field(world.base_card.fields()[0].id()).unwrap();
        removed.set_display_name("Removed").unwrap();
        let mut latest = removed.clone();
        latest.set_display_name("Latest").unwrap();

        let specifications = [
            (&world.alice_a1, &old, 1, first_timestamp),
            (&world.alice_a2, &sibling, 1, first_timestamp + 1),
            (&world.alice_a1, &removed, 2, first_timestamp + 2),
            (&world.alice_a1, &latest, 3, first_timestamp + 2),
        ];
        let messages: Vec<_> = specifications.iter().map(|(sender, card, version, timestamp)| {
            let mut delta = CardDelta::compute(&world.base_card, card, *timestamp);
            delta.set_version(*version);
            seal_delta_for_world(&world, sender, delta)
        }).collect();
        let mut order = [0, 1, 2, 3];
        order.sort_by_key(|index| (priorities[*index], *index));
        for index in order {
            let result = receive(&world, specifications[index].0, &messages[index], false);
            prop_assert!(matches!(result, Ok(ReceiveOutcome::CardDelta) | Err(CardUpdateError::StaleVersion { .. })), "only applied or older-version messages are expected");
        }
        let saved = world.bob.get_contact(&world.alice_contact_id).unwrap().unwrap();
        prop_assert!(saved.card().fields().is_empty(), "held signed additions must not undo removal");
        prop_assert_eq!(saved.display_name(), "Latest");
    }
}

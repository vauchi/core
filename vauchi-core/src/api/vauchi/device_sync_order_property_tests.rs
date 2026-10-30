// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn own_device_order_random_delivery_and_replay_preserves_latest_per_key(priorities in prop::collection::vec(any::<u64>(), 8)) {
        let pair = Pair::new();
        let blobs: Vec<Vec<u8>> = (0..8).map(|index| {
            pair.batch(if index % 2 == 0 { "Email" } else { "Phone" }, &format!("value-{index}"))
        }).collect();
        let mut order: Vec<usize> = (0..8).collect();
        order.sort_by_key(|index| priorities[*index]);
        for index in order {
            pair.apply(&blobs[index]);
            prop_assert_eq!(pair.apply(&blobs[index]), 0);
        }
        prop_assert_eq!(pair.value("Email"), "value-6");
        prop_assert_eq!(pair.value("Phone"), "value-7");
        for blob in &blobs { prop_assert_eq!(pair.apply(blob), 0); }
    }
}

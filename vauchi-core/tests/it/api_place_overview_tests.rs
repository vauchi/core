// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Places overview (#467, design pass #419 item 9): how many contacts
//! were met at each named place, and the exchange locations nobody has
//! named yet, grouped within `PLACE_MATCH_RADIUS_M` so one spot is offered
//! once.

use proptest::prelude::*;
use vauchi_core::contact::place::{PLACE_MATCH_RADIUS_M, haversine_m};
use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

const LAT: f64 = 47.3769;
const LON: f64 = 8.5417;
/// About 11 m of latitude.
const STEP: f64 = 0.0001;

fn setup() -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    wb
}

fn met_at(wb: &Vauchi, name: &str, lat: f64, lon: f64) -> String {
    let mut pk = [0u8; 32];
    for (i, b) in name.bytes().enumerate().take(32) {
        pk[i] = b;
    }
    let contact = Contact::from_exchange(pk, ContactCard::new(name), SymmetricKey::generate(), 0);
    let id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    wb.set_exchange_location(&id, lat, lon).unwrap();
    id
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids
}

// @internal
#[test]
fn a_named_place_counts_the_contacts_met_there() {
    let wb = setup();
    let bob = met_at(&wb, "Bob", LAT, LON);
    let carol = met_at(&wb, "Carol", LAT + STEP, LON);
    let _dan = met_at(&wb, "Dan", LAT + 0.05, LON);
    let zurich = wb.name_exchange_place(&bob, "Zürich").unwrap();
    wb.name_exchange_place(&carol, "zürich").unwrap();
    let empty = wb.create_named_place("Bern", LAT - 1.0, LON).unwrap();

    let counts = wb.place_contact_counts().unwrap();
    assert_eq!(counts.get(&zurich.id), Some(&2));
    assert_eq!(counts.get(&empty.id), None, "nobody met in Bern");
}

// @internal
#[test]
fn unnamed_locations_are_grouped_by_spot() {
    let wb = setup();
    let named = met_at(&wb, "Bob", LAT, LON);
    wb.name_exchange_place(&named, "Zürich").unwrap();
    let dan = met_at(&wb, "Dan", LAT + 0.01, LON);
    let eve = met_at(&wb, "Eve", LAT + 0.01 + 4.0 * STEP, LON);
    let fay = met_at(&wb, "Fay", LAT + 0.05, LON);

    let spots: Vec<Vec<String>> = wb
        .unnamed_exchange_spots()
        .unwrap()
        .into_iter()
        .map(sorted)
        .collect();
    assert_eq!(spots.len(), 2, "{spots:?}");
    assert!(spots.contains(&sorted(vec![dan, eve])));
    assert!(spots.contains(&vec![fay]));
}

// @internal
#[test]
fn naming_a_spot_moves_it_to_the_place() {
    let wb = setup();
    let dan = met_at(&wb, "Dan", LAT, LON);
    let eve = met_at(&wb, "Eve", LAT + STEP, LON);
    for id in [&dan, &eve] {
        wb.name_exchange_place(id, "Bundesplatz").unwrap();
    }
    assert!(wb.unnamed_exchange_spots().unwrap().is_empty());
    let place = wb.find_place_by_name("Bundesplatz").unwrap().unwrap();
    assert_eq!(wb.place_contact_counts().unwrap().get(&place.id), Some(&2));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Every unnamed location is offered in exactly one spot, and within the
    /// match radius of that spot's first location.
    // @internal
    #[test]
    fn every_unnamed_location_is_in_one_spot_near_its_anchor(
        offsets in prop::collection::vec((0u32..40, 0u32..40), 1..12),
    ) {
        let wb = setup();
        let mut placed = Vec::new();
        for (i, (dy, dx)) in offsets.iter().enumerate() {
            let lat = LAT + f64::from(*dy) * STEP;
            let lon = LON + f64::from(*dx) * STEP;
            let id = met_at(&wb, &format!("Contact {i}"), lat, lon);
            placed.push((id, lat, lon));
        }

        let spots = wb.unnamed_exchange_spots().unwrap();
        let mut seen: Vec<String> = spots.iter().flatten().cloned().collect();
        seen.sort();
        let mut expected: Vec<String> = placed.iter().map(|(id, _, _)| id.clone()).collect();
        expected.sort();
        prop_assert_eq!(seen, expected);

        for spot in &spots {
            let anchor = placed.iter().find(|(id, _, _)| id == &spot[0]).unwrap();
            for member in spot {
                let m = placed.iter().find(|(id, _, _)| id == member).unwrap();
                prop_assert!(haversine_m(anchor.1, anchor.2, m.1, m.2) <= PLACE_MATCH_RADIUS_M);
            }
        }
    }
}

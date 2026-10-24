// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Links a tapped contact field opens (vauchi/private#709).
//! Reference: features/contact_actions.feature

use proptest::prelude::*;
use vauchi_core::contact_card::{ContactField, FieldType, MapProvider, is_safe_url, map_uri};
use vauchi_core::social::SocialNetworkRegistry;

fn registry() -> SocialNetworkRegistry {
    SocialNetworkRegistry::with_defaults()
}

const ADDRESS: &str = "Bahnhofstrasse 1, 8001 Zürich";
const ENCODED: &str = "Bahnhofstrasse+1%2C+8001+Z%C3%BCrich";

// @scenario: contact_actions :: Phone number with international format
#[test]
fn a_phone_link_carries_no_whitespace() {
    let field = ContactField::new(FieldType::Phone, "International", "+44 20 7946 0958", 0);
    assert_eq!(
        field.to_uri(&registry()).as_deref(),
        Some("tel:+442079460958")
    );
}

// @scenario: contact_actions :: Various phone number formats are normalized for dialer
#[test]
fn a_phone_link_keeps_the_punctuation_dialers_accept() {
    let field = ContactField::new(FieldType::Phone, "Home", "(555) 123-4567", 0);
    assert_eq!(
        field.to_uri(&registry()).as_deref(),
        Some("tel:(555)123-4567")
    );
}

// @internal
#[test]
fn each_map_provider_searches_for_the_address() {
    let expected = [
        (MapProvider::System, format!("geo:0,0?q={ENCODED}")),
        (
            MapProvider::AppleMaps,
            format!("https://maps.apple.com/?q={ENCODED}"),
        ),
        (
            MapProvider::GoogleMaps,
            format!("https://www.google.com/maps/search/?api=1&query={ENCODED}"),
        ),
        (
            MapProvider::OpenStreetMap,
            format!("https://www.openstreetmap.org/search?query={ENCODED}"),
        ),
        (
            MapProvider::OrganicMaps,
            format!("om://search?query={ENCODED}"),
        ),
    ];
    for (provider, url) in expected {
        assert_eq!(map_uri(ADDRESS, provider), Some(url), "{provider:?}");
    }
}

// @internal
#[test]
fn a_blank_address_has_no_map_link() {
    for provider in [MapProvider::System, MapProvider::OrganicMaps] {
        assert_eq!(map_uri("  \t ", provider), None, "{provider:?}");
    }
}

// @internal
#[test]
fn an_address_cannot_break_out_of_its_query() {
    let url = map_uri(
        "x&api=evil#frag?q=1 javascript:alert(1)",
        MapProvider::GoogleMaps,
    )
    .expect("a non-blank address has a link");
    assert_eq!(
        url,
        "https://www.google.com/maps/search/?api=1&query=x%26api%3Devil%23frag%3Fq%3D1+javascript%3Aalert%281%29"
    );
}

proptest! {
    // @internal
    #[test]
    fn every_map_link_is_a_safe_url_without_whitespace(address in "\\PC{1,80}") {
        for provider in [
            MapProvider::System,
            MapProvider::AppleMaps,
            MapProvider::GoogleMaps,
            MapProvider::OpenStreetMap,
            MapProvider::OrganicMaps,
        ] {
            if let Some(url) = map_uri(&address, provider) {
                prop_assert!(is_safe_url(&url), "{provider:?}: {url}");
                prop_assert!(!url.chars().any(char::is_whitespace), "{provider:?}: {url}");
            }
        }
    }
}

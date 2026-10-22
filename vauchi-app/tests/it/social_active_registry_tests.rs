// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The active social-network registry is the CDN copy once one loads,
//! else the bundled set, and a rejected file keeps what was active
//! (vauchi/private#734 D2).

use vauchi_app::social::{active_registry, load_networks_from_bytes};
use vauchi_core::social::{NetworkDataError, SocialNetworkRegistry};

fn renamed_twitter() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!([
        {"id": "twitter", "name": "X", "url": "https://x.com/{username}", "aliases": ["x"]}
    ]))
    .unwrap()
}

/// One test, so the three states run in order against the one store.
// @internal
#[test]
fn the_active_registry_is_bundled_then_loaded_and_survives_a_bad_load() {
    let bundled = SocialNetworkRegistry::with_defaults();
    assert_eq!(active_registry().len(), bundled.len());
    assert_eq!(
        active_registry()
            .get("twitter")
            .map(|n| n.display_name().to_string()),
        bundled.get("twitter").map(|n| n.display_name().to_string())
    );

    assert_eq!(load_networks_from_bytes(&renamed_twitter()), Ok(()));
    assert_eq!(active_registry().len(), 1);
    assert_eq!(
        active_registry()
            .get("twitter")
            .map(|n| n.display_name().to_string()),
        Some("X".to_string())
    );

    assert_eq!(
        load_networks_from_bytes(b"[]"),
        Err(NetworkDataError::TooManyEntries)
    );
    assert_eq!(
        active_registry()
            .get("twitter")
            .map(|n| n.profile_url_template().to_string()),
        Some("https://x.com/{username}".to_string()),
        "a rejected file keeps the registry that was active"
    );
}

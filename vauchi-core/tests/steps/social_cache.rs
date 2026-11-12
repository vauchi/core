// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Offline social-network list (contact_card_management.feature): a networks
//! file a past content cycle left in the cache is what the field options use
//! when the next cycle cannot reach the network.

use std::sync::Arc;

use cucumber::{given, then, when};
use vauchi_app::content::{
    ContentCache, ContentConfig, ContentManager, ContentSource, ContentSourceHandle, ContentType,
    UpdateStatus, compute_checksum,
};
use vauchi_core::{ContactField, FieldType};

use crate::VauchiWorld;

/// A network only the cached file carries, so finding it proves the list
/// came from the cache and not from the copy bundled with the app.
const CACHED_ONLY_NETWORK: &str = "Cachedverse";

const CACHED_NETWORKS: &str = r#"[{
    "id": "cachedverse",
    "name": "Cachedverse",
    "url": "https://cachedverse.example/{username}",
    "username": {"max_len": 30, "charset": "alnum_underscore", "strip_at": true}
}]"#;

/// Every request fails, as it does on a device with no connectivity.
struct NoNetwork;

impl ContentSource for NoNetwork {
    fn fetch(&self, _path: &str) -> Result<Vec<u8>, String> {
        Err("network unreachable".into())
    }
}

#[given("I have previously loaded the social network config")]
fn previously_loaded_social_config(world: &mut VauchiWorld) {
    let dir = tempfile::tempdir().expect("content dir");
    ContentCache::new(dir.path())
        .expect("content cache")
        .save_content(
            ContentType::Networks,
            "networks.json",
            CACHED_NETWORKS.as_bytes(),
            &compute_checksum(CACHED_NETWORKS.as_bytes()),
        )
        .expect("cache the networks file");
    world.content_dir = Some(dir);
}

// A Given, though it acts: the feature writes it as `And` after a Given.
#[given("the app has no network connectivity")]
fn no_network_connectivity(world: &mut VauchiWorld) {
    let dir = world.content_dir.as_ref().expect("a content cache exists");
    let config = ContentConfig {
        storage_path: dir.path().to_path_buf(),
        remote_updates_enabled: true,
        source: Some(ContentSourceHandle(Arc::new(NoNetwork))),
        ..ContentConfig::default()
    };
    let manager =
        ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).expect("manager");
    let (status, applied) = manager.run_update_cycle_blocking();
    assert!(
        matches!(status, UpdateStatus::CheckFailed(_)),
        "an update cycle with no network did not fail: {status:?}"
    );
    assert!(applied.is_none(), "an offline cycle applied content");
    world.content_manager = Some(manager);
}

#[when("I open the social field options")]
fn open_social_field_options(world: &mut VauchiWorld) {
    let manager = world
        .content_manager
        .as_ref()
        .expect("the app has a content manager");
    world.social_networks = manager.networks();
}

#[then("I should see the cached list of social networks")]
fn cached_social_networks(world: &mut VauchiWorld) {
    let shown = world
        .social_networks
        .find_by_display_name(CACHED_ONLY_NETWORK)
        .map(|network| network.id().to_string());
    assert_eq!(shown.as_deref(), Some("cachedverse"), "the cached network");
    assert!(
        world.social_networks.get("tumblr").is_none(),
        "the bundled list was shown instead of the cached one"
    );
}

#[then("I should be able to add social fields normally")]
fn add_social_field_offline(world: &mut VauchiWorld) {
    let username = "alice_99";
    vauchi_core::contact_card::validate_social_username(
        &world.social_networks,
        CACHED_ONLY_NETWORK,
        username,
    )
    .expect("a cached network's username rule accepts a valid name");
    world
        .vauchi
        .add_own_field(ContactField::new(
            FieldType::Social,
            CACHED_ONLY_NETWORK,
            username,
            0,
        ))
        .expect("add the social field");
    let card = world.vauchi.own_card().unwrap().expect("own card");
    let added = card
        .fields()
        .iter()
        .find(|field| field.label() == CACHED_ONLY_NETWORK)
        .map(|field| field.value().to_string());
    assert_eq!(added.as_deref(), Some(username));
}

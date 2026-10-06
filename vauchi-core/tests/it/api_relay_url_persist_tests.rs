// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for durable relay-URL persistence.
//!
//! The relay URL is editable from settings on every frontend (shared
//! `EditRelayUrl` form), but on mobile it persisted nowhere — config is
//! rebuilt to defaults each launch. These tests pin the core contract:
//! `set_relay_url` survives a restart and self-seeds config, without
//! clobbering an explicit `with_relay_url` override (TUI `--relay-url`).
//!
//! @scenario: settings.feature - Persist relay URL across restart

use vauchi_core::{SymmetricKey, Vauchi, VauchiConfig};

const DEFAULT_RELAY: &str = "https://relay.vauchi.app";

// @scenario: settings.feature :: Persist relay URL across restart
#[test]
fn relay_url_persists_and_seeds_config_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let mut vauchi = Vauchi::new(config).unwrap();
        assert_eq!(
            vauchi.config().relay.server_url,
            DEFAULT_RELAY,
            "first launch uses the default relay"
        );

        vauchi.set_relay_url("https://my.relay.example").unwrap();
        assert_eq!(
            vauchi.config().relay.server_url,
            "https://my.relay.example",
            "set_relay_url updates config in-session"
        );
    }

    {
        let config = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
        let vauchi = Vauchi::new(config).unwrap();
        assert_eq!(
            vauchi.config().relay.server_url,
            "https://my.relay.example",
            "persisted relay URL seeds config on reopen"
        );
    }
}

// @scenario: settings.feature :: Persist relay URL across restart
#[test]
fn relay_url_seed_does_not_clobber_explicit_override() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let mut vauchi = Vauchi::new(config).unwrap();
        vauchi
            .set_relay_url("https://persisted.relay.example")
            .unwrap();
    }

    {
        // Explicit override (TUI `--relay-url` / resolve_relay_url file) must win.
        let config = VauchiConfig::with_storage_path(&db_path)
            .with_storage_key(storage_key)
            .with_relay_url("https://explicit.relay.example");
        let vauchi = Vauchi::new(config).unwrap();
        assert_eq!(
            vauchi.config().relay.server_url,
            "https://explicit.relay.example",
            "explicit with_relay_url is not clobbered by the persisted seed"
        );
    }
}

// @internal
#[test]
fn set_relay_url_rejects_empty_after_trim() {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(&dir.path().join("v.db"))
        .with_storage_key(SymmetricKey::generate());
    let mut vauchi = Vauchi::new(config).unwrap();
    assert!(
        vauchi.set_relay_url("   ").is_err(),
        "whitespace-only relay URL must be rejected"
    );
}

// @internal
#[test]
fn set_relay_url_overwrites_previous() {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(&dir.path().join("v.db"))
        .with_storage_key(SymmetricKey::generate());
    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.set_relay_url("https://one.example").unwrap();
    vauchi.set_relay_url("https://two.example").unwrap();
    assert_eq!(vauchi.config().relay.server_url, "https://two.example");
}

// ── OHTTP trust anchor follows the relay (#288, decision 0.9) ───────

const ANCHOR: [u8; 32] = [0x5a; 32];

/// Editing the relay URL in settings must not keep the previous relay's
/// anchor bound to the new one.
// @internal
#[test]
fn set_relay_url_drops_the_previous_relays_anchor() {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(&dir.path().join("v.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay("https://one.example", ANCHOR);
    let mut vauchi = Vauchi::new(config).unwrap();
    assert_eq!(vauchi.config().relay.ohttp_trust_anchor(), Some(ANCHOR));

    vauchi.set_relay_url("https://two.example").unwrap();

    assert_eq!(vauchi.config().relay.ohttp_trust_anchor(), None);
}

/// A caller that set the default URL together with an anchor chose it
/// explicitly; a persisted URL must not replace it and leave the anchor on
/// a different relay.
// @internal
#[test]
fn a_persisted_url_does_not_replace_a_relay_set_with_its_anchor() {
    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");
    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let mut vauchi = Vauchi::new(config).unwrap();
        vauchi.set_relay_url("https://my.relay.example").unwrap();
    }

    let config = VauchiConfig::with_storage_path(&db_path)
        .with_storage_key(storage_key)
        .with_relay(DEFAULT_RELAY, ANCHOR);
    let vauchi = Vauchi::new(config).unwrap();

    assert_eq!(vauchi.config().relay.server_url, DEFAULT_RELAY);
    assert_eq!(vauchi.config().relay.ohttp_trust_anchor(), Some(ANCHOR));
}

// @internal
#[test]
fn the_builders_relay_url_drops_the_configured_anchor() {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(&dir.path().join("v.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay("https://one.example", ANCHOR);

    let vauchi = vauchi_core::VauchiBuilder::new()
        .config(config)
        .relay_url("https://two.example")
        .build()
        .unwrap();

    assert_eq!(vauchi.config().relay.server_url, "https://two.example");
    assert_eq!(vauchi.config().relay.ohttp_trust_anchor(), None);
}

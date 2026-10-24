// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Network data from cdn.vauchi.app is validated whole or not at all
//! (vauchi/private#734 D1, D3; DC-01).

use proptest::prelude::*;
use vauchi_core::social::{NetworkDataError, SocialNetworkRegistry, UsernameCharset};

fn entry(id: &str) -> serde_json::Value {
    serde_json::json!({"id": id, "name": "Example", "url": "https://example.com/{username}"})
}

fn parse(entries: Vec<serde_json::Value>) -> Result<SocialNetworkRegistry, NetworkDataError> {
    SocialNetworkRegistry::from_data_bytes(&serde_json::to_vec(&entries).unwrap())
}

fn with(field: &str, value: serde_json::Value) -> serde_json::Value {
    let mut e = entry("example");
    e[field] = value;
    e
}

// @internal
#[test]
fn an_entry_with_aliases_and_username_rules_parses() {
    let registry = parse(vec![serde_json::json!({
        "id": "twitter",
        "name": "X",
        "url": "https://x.com/{username}",
        "aliases": ["x", "twitter-x"],
        "username": {"max_len": 15, "charset": "alnum_underscore", "strip_at": true}
    })])
    .expect("valid file parses");

    let twitter = registry.get("twitter").expect("twitter present");
    assert_eq!(twitter.display_name(), "X");
    assert_eq!(twitter.profile_url_template(), "https://x.com/{username}");
    assert_eq!(twitter.aliases(), ["x", "twitter-x"]);
    let rule = twitter.username_rule().expect("rule present");
    assert_eq!(rule.max_len(), 15);
    assert_eq!(rule.charset(), UsernameCharset::AlnumUnderscore);
    assert!(rule.strip_at());
}

// @internal
#[test]
fn an_entry_without_aliases_or_rules_parses_with_none() {
    let registry = parse(vec![entry("example")]).expect("minimal entry parses");
    let example = registry.get("example").expect("example present");
    assert!(example.aliases().is_empty());
    assert_eq!(example.username_rule(), None);
}

// @internal
#[test]
fn a_username_in_the_leftmost_host_label_is_accepted() {
    let registry = parse(vec![with("url", "https://{username}.substack.com".into())])
        .expect("subdomain template parses");
    assert_eq!(
        registry.profile_url("example", "alice").as_deref(),
        Some("https://alice.substack.com")
    );
}

// @internal
#[test]
fn the_bundled_networks_pass_the_same_validation() {
    let registry =
        SocialNetworkRegistry::from_data_bytes(include_bytes!("../../src/social/networks.json"))
            .expect("bundled networks.json is valid CDN data");
    assert_eq!(registry.len(), SocialNetworkRegistry::with_defaults().len());
}

// @internal
#[test]
fn each_limit_exceeded_by_one_rejects_the_whole_file() {
    let id_33 = "a".repeat(33);
    let name_65 = "n".repeat(65);
    let url_257 = format!("https://example.com/{}{{username}}", "p".repeat(257 - 30));
    let cases: Vec<(&str, Vec<serde_json::Value>, NetworkDataError)> = vec![
        (
            "201 entries",
            (0..201).map(|i| entry(&format!("n{i}"))).collect(),
            NetworkDataError::TooManyEntries,
        ),
        (
            "33-char id",
            vec![entry(&id_33)],
            NetworkDataError::InvalidId,
        ),
        (
            "9 aliases",
            vec![with(
                "aliases",
                (0..9).map(|i| format!("a{i}")).collect::<Vec<_>>().into(),
            )],
            NetworkDataError::InvalidAlias,
        ),
        (
            "33-char alias",
            vec![with("aliases", vec![id_33.clone()].into())],
            NetworkDataError::InvalidAlias,
        ),
        (
            "65-char name",
            vec![with("name", name_65.into())],
            NetworkDataError::InvalidName,
        ),
        (
            "257-byte url",
            vec![with("url", url_257.into())],
            NetworkDataError::InvalidUrl,
        ),
        (
            "max_len 257",
            vec![with(
                "username",
                serde_json::json!({"max_len": 257, "charset": "any", "strip_at": false}),
            )],
            NetworkDataError::InvalidUsernameRule,
        ),
    ];
    for (case, entries, expected) in cases {
        assert_eq!(parse(entries).err(), Some(expected), "{case}");
    }
}

// @internal
#[test]
fn each_limit_at_its_bound_is_accepted() {
    let mut entries: Vec<_> = (0..199).map(|i| entry(&format!("n{i}"))).collect();
    let mut edge = entry(&"a".repeat(32));
    edge["name"] = "n".repeat(64).into();
    edge["aliases"] = (0..8).map(|i| format!("a{i}")).collect::<Vec<_>>().into();
    edge["url"] = format!("https://example.com/{}{{username}}", "p".repeat(256 - 30)).into();
    edge["username"] = serde_json::json!({"max_len": 256, "charset": "any", "strip_at": false});
    entries.push(edge);
    assert_eq!(parse(entries).map(|r| r.len()).ok(), Some(200));
}

// @internal
#[test]
fn adversarial_files_are_rejected() {
    let mut huge = b"[".to_vec();
    huge.extend(std::iter::repeat_n(b' ', 1024 * 1024));
    huge.push(b']');
    assert_eq!(
        SocialNetworkRegistry::from_data_bytes(&huge).err(),
        Some(NetworkDataError::TooLarge)
    );
    assert_eq!(
        SocialNetworkRegistry::from_data_bytes(b"{not json").err(),
        Some(NetworkDataError::Malformed)
    );
    assert_eq!(parse(vec![]).err(), Some(NetworkDataError::TooManyEntries));

    let bad_urls = [
        "https://example.com/{username}/{username}",
        "https://example.com/profile",
        "http://example.com/{username}",
        "javascript:alert(1)//{username}",
        "https://x.com@evil.test/{username}",
        "https://example.com:8443/{username}",
        "https://exämple.com/{username}",
        "https://ex ample.com/{username}",
        "https:///{username}",
        "https://evil.test{username}/",
    ];
    for url in bad_urls {
        assert_eq!(
            parse(vec![with("url", url.into())]).err(),
            Some(NetworkDataError::InvalidUrl),
            "{url}"
        );
    }

    for name in ["", "Ex\0ample", "Ex\nample", "\u{7f}"] {
        assert_eq!(
            parse(vec![with("name", name.into())]).err(),
            Some(NetworkDataError::InvalidName),
            "{name:?}"
        );
    }
    for id in ["", "Twitter", "x_y", "a b", "ü"] {
        assert_eq!(
            parse(vec![entry(id)]).err(),
            Some(NetworkDataError::InvalidId),
            "{id:?}"
        );
    }
    assert_eq!(
        parse(vec![with(
            "username",
            serde_json::json!({"max_len": 0, "charset": "any", "strip_at": false}),
        )])
        .err(),
        Some(NetworkDataError::InvalidUsernameRule)
    );
    assert_eq!(
        parse(vec![with(
            "username",
            serde_json::json!({"max_len": 9, "charset": "^(a+)+$", "strip_at": false}),
        )])
        .err(),
        Some(NetworkDataError::Malformed),
        "a charset is a name Core implements, never a pattern"
    );
}

// @internal
#[test]
fn an_id_or_alias_claimed_twice_rejects_the_file() {
    let mut first = entry("twitter");
    first["aliases"] = serde_json::json!(["x"]);
    let mut second = entry("xcom");
    second["aliases"] = serde_json::json!(["x"]);
    assert_eq!(
        parse(vec![first.clone(), second]).err(),
        Some(NetworkDataError::DuplicateKey)
    );
    assert_eq!(
        parse(vec![first.clone(), entry("x")]).err(),
        Some(NetworkDataError::DuplicateKey)
    );
    assert_eq!(
        parse(vec![entry("github"), entry("github")]).err(),
        Some(NetworkDataError::DuplicateKey)
    );
}

proptest! {
    // @internal
    #[test]
    fn an_accepted_registry_has_unique_ids_and_aliases(
        entries in prop::collection::vec(
            ("[ab]{1,2}", prop::collection::vec("[ab]{1,2}", 0..3)),
            1..6,
        )
    ) {
        let entries = entries
            .into_iter()
            .map(|(id, aliases)| {
                let mut e = entry(&id);
                e["aliases"] = aliases.into();
                e
            })
            .collect();
        if let Ok(registry) = parse(entries) {
            let mut keys: Vec<&str> = registry
                .all()
                .into_iter()
                .flat_map(|n| std::iter::once(n.id()).chain(n.aliases().iter().map(String::as_str)))
                .collect();
            let total = keys.len();
            keys.sort_unstable();
            keys.dedup();
            prop_assert_eq!(keys.len(), total);
        }
    }
}

// @internal
#[test]
fn a_network_resolves_from_its_id_alias_or_name_in_any_case() {
    let mut twitter = entry("twitter");
    twitter["name"] = "Twitter / X".into();
    twitter["aliases"] = serde_json::json!(["x"]);
    let registry = parse(vec![twitter, entry("github")]).expect("valid file");

    for name in ["X", "x", "twitter", "TWITTER", "Twitter / X", " twitter "] {
        assert_eq!(
            registry.resolve(name).map(|n| n.id()),
            Some("twitter"),
            "{name:?}"
        );
    }
    for name in ["nope", "", "twit", "Twitter /"] {
        assert_eq!(registry.resolve(name).map(|n| n.id()), None, "{name:?}");
    }
}

// Profile links come from the data (vauchi/private#734 T1.5, D7).

fn social_field(label: &str, value: &str) -> vauchi_core::contact_card::ContactField {
    vauchi_core::contact_card::ContactField::new(
        vauchi_core::contact_card::FieldType::Social,
        label,
        value,
        0,
    )
}

// @internal
#[test]
fn every_bundled_network_links_a_username_through_its_template() {
    let registry = SocialNetworkRegistry::with_defaults();
    for network in registry.all() {
        let expected = network
            .profile_url_template()
            .replace("{username}", "alice");
        for label in [network.id(), network.display_name()] {
            assert_eq!(
                social_field(label, "alice").to_uri(&registry).as_deref(),
                Some(expected.as_str()),
                "{label}"
            );
        }
    }
}

// @internal
#[test]
fn a_field_labelled_with_any_alias_opens_the_registry_url() {
    let mut twitter = entry("twitter");
    twitter["url"] = "https://x.com/{username}".into();
    twitter["aliases"] = serde_json::json!(["x", "tw"]);
    let registry = parse(vec![twitter]).expect("valid file");

    for label in ["X", "Tw", "twitter"] {
        assert_eq!(
            social_field(label, "@alice").to_uri(&registry).as_deref(),
            Some("https://x.com/alice"),
            "{label}"
        );
    }
}

// @internal
#[test]
fn a_leading_at_is_dropped_for_every_network() {
    let registry = SocialNetworkRegistry::with_defaults();
    assert_eq!(
        registry.profile_url("github", "@octocat").as_deref(),
        Some("https://github.com/octocat")
    );
    assert_eq!(
        registry.profile_url("x", "@alice").as_deref(),
        Some("https://twitter.com/alice")
    );
}

/// The networks file the CDN publishes passes Core's validation. Used by
/// CI's validate-content-contracts job: `VAUCHI_NETWORKS_PATH`, else the
/// sibling website checkout; skipped when neither exists.
// @internal
#[test]
fn the_cdn_networks_file_passes_core_validation() {
    let path = std::env::var("VAUCHI_NETWORKS_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../website/app-files-src/networks.json")
        });
    let Ok(data) = std::fs::read(&path) else {
        return;
    };
    let registry = SocialNetworkRegistry::from_data_bytes(&data)
        .unwrap_or_else(|e| panic!("{} fails Core's network validation: {e}", path.display()));
    assert!(registry.get("twitter").is_some());
}

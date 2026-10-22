// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for social handle platform rules (Twitter, Instagram, GitHub, etc.).
//!
//! Split from field_validation_tests.rs (structural tidy, no behavior change).

// =============================================================================
// Social Handle Platform Rules Tests
// Traces to: _private/features/field_validation.feature @validate @social
// =============================================================================

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn test_social_handle_platform_rules_twitter() {
    use vauchi_core::social::SocialNetworkRegistry;

    let registry = SocialNetworkRegistry::with_defaults();

    // Twitter usernames: 4-15 chars, alphanumeric + underscore
    let twitter = registry.get("twitter").expect("Twitter should exist");

    let valid_handles = vec!["alice", "bob_smith", "user1234", "@alice", "A1B2"];

    for handle in valid_handles {
        let url = twitter.profile_url(handle);
        assert!(
            url.starts_with("https://twitter.com/"),
            "Twitter URL should be generated for '{}'",
            handle
        );
        assert!(
            !url.contains("@@"),
            "Should not have double @ for '{}'",
            handle
        );
    }
}

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn test_social_handle_platform_rules_instagram() {
    use vauchi_core::social::SocialNetworkRegistry;

    let registry = SocialNetworkRegistry::with_defaults();

    // Instagram: 1-30 chars, alphanumeric, underscores, periods (no consecutive)
    let instagram = registry.get("instagram").expect("Instagram should exist");

    let handles = vec!["alice", "bob.smith", "user_name", "@alice"];

    for handle in handles {
        let url = instagram.profile_url(handle);
        assert!(
            url.starts_with("https://instagram.com/"),
            "Instagram URL should be generated for '{}'",
            handle
        );
    }
}

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn test_social_handle_platform_rules_github() {
    use vauchi_core::social::SocialNetworkRegistry;

    let registry = SocialNetworkRegistry::with_defaults();

    // GitHub: 1-39 chars, alphanumeric + hyphen, no consecutive hyphens
    let github = registry.get("github").expect("GitHub should exist");

    let handles = vec!["octocat", "test-user", "a1b2c3"];

    for handle in handles {
        let url = github.profile_url(handle);
        assert!(
            url.starts_with("https://github.com/"),
            "GitHub URL should be generated for '{}'",
            handle
        );
    }
}

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn test_social_handle_platform_rules_mastodon() {
    use vauchi_core::social::SocialNetworkRegistry;

    let registry = SocialNetworkRegistry::with_defaults();

    // Mastodon: @user@instance.social format
    let mastodon = registry.get("mastodon").expect("Mastodon should exist");

    let federated = "user@fosstodon.org";
    let url = mastodon.profile_url(federated);
    assert!(
        url.contains("user@fosstodon.org") || url.contains("mastodon.social"),
        "Mastodon should handle federated handle"
    );

    let simple = "@alice";
    let url = mastodon.profile_url(simple);
    assert!(
        url.contains("alice"),
        "Mastodon should handle simple handle"
    );
}

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn test_social_handle_platform_rules_linkedin() {
    use vauchi_core::social::SocialNetworkRegistry;

    let registry = SocialNetworkRegistry::with_defaults();

    // LinkedIn: vanity URLs are 3-100 chars
    let linkedin = registry.get("linkedin").expect("LinkedIn should exist");

    let handles = vec!["john-doe", "janedoe123", "professional-person"];

    for handle in handles {
        let url = linkedin.profile_url(handle);
        assert!(
            url.starts_with("https://linkedin.com/in/"),
            "LinkedIn URL should use /in/ path for '{}'",
            handle
        );
    }
}

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn test_social_handle_preserves_full_urls() {
    use vauchi_core::social::SocialNetworkRegistry;

    let registry = SocialNetworkRegistry::with_defaults();
    let twitter = registry.get("twitter").expect("Twitter should exist");

    // If user provides full URL, it should be preserved
    let full_url = "https://twitter.com/already_full";
    let result = twitter.profile_url(full_url);
    assert_eq!(result, full_url, "Full URLs should be returned as-is");
}

// Username rules are data in networks.json, checked where the user types
// (vauchi/private#734 T1.4); received cards are judged by type and length.

fn username_check(
    network: &str,
    value: &str,
) -> Result<(), vauchi_core::contact_card::ValidationError> {
    vauchi_core::contact_card::validate_social_username(
        &vauchi_core::social::SocialNetworkRegistry::with_defaults(),
        network,
        value,
    )
}

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn twitter_usernames_follow_the_bundled_rule() {
    use vauchi_core::contact_card::ValidationError::InvalidSocialUsername;
    for ok in ["alice", "bob_smith", "@alice", &"a".repeat(15)] {
        assert_eq!(username_check("Twitter", ok), Ok(()), "{ok}");
    }
    for bad in [&"a".repeat(16), "bad-name", "", "@"] {
        assert_eq!(
            username_check("Twitter", bad),
            Err(InvalidSocialUsername),
            "{bad:?}"
        );
    }
    assert_eq!(
        username_check("X", &"a".repeat(16)),
        Err(InvalidSocialUsername),
        "an alias reaches the same rule"
    );
}

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn github_usernames_follow_the_bundled_rule() {
    use vauchi_core::contact_card::ValidationError::InvalidSocialUsername;
    for ok in ["octocat", "test-user", "@octocat", &"a".repeat(39)] {
        assert_eq!(username_check("GitHub", ok), Ok(()), "{ok}");
    }
    for bad in ["-a", "a-", "a--b", "a_b", &"a".repeat(40)] {
        assert_eq!(
            username_check("github", bad),
            Err(InvalidSocialUsername),
            "{bad}"
        );
    }
}

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn a_network_without_a_rule_or_a_full_url_accepts_any_value() {
    assert_eq!(username_check("LinkedIn", "anything goes, really"), Ok(()));
    assert_eq!(username_check("My blog", "!!"), Ok(()));
    assert_eq!(
        username_check(
            "Twitter",
            "https://twitter.com/a_name_far_too_long_for_a_handle"
        ),
        Ok(())
    );
}

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn the_rules_live_in_the_data_not_the_code() {
    let without_rules = vauchi_core::social::SocialNetworkRegistry::from_data_bytes(
        br#"[{"id": "twitter", "name": "Twitter", "url": "https://twitter.com/{username}"}]"#,
    )
    .expect("valid data");
    assert_eq!(
        vauchi_core::contact_card::validate_social_username(
            &without_rules,
            "Twitter",
            &"a".repeat(16)
        ),
        Ok(())
    );
}

// A peer's card is judged by type and length only, so network data can
// never make a phone reject a card it receives.
// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn a_received_social_field_is_not_judged_by_username_rules() {
    use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
    let field = ContactField::new(FieldType::Social, "Twitter", &"a".repeat(16), 0);
    assert_eq!(field.validate(), Ok(()));
    let mut card = ContactCard::new("Bob");
    assert!(card.add_field(field).is_ok());
    assert_eq!(card.fields().len(), 1);
}

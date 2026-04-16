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

    // Valid Twitter handles
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

    // Federated handle
    let federated = "user@fosstodon.org";
    let url = mastodon.profile_url(federated);
    // Should preserve the federation handle
    assert!(
        url.contains("user@fosstodon.org") || url.contains("mastodon.social"),
        "Mastodon should handle federated handle"
    );

    // Simple handle
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

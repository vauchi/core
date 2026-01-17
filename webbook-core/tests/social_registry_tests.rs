//! Tests for social::registry
//! Extracted from registry.rs

use webbook_core::*;
use webbook_core::social::*;

    #[test]
    fn test_social_network_creation() {
        let network = SocialNetwork::new("twitter", "Twitter", "https://twitter.com/{username}");

        assert_eq!(network.id(), "twitter");
        assert_eq!(network.display_name(), "Twitter");
        assert_eq!(
            network.profile_url_template(),
            "https://twitter.com/{username}"
        );
        assert!(network.icon().is_none());
    }

    #[test]
    fn test_social_network_with_icon() {
        let network = SocialNetwork::new("github", "GitHub", "https://github.com/{username}")
            .with_icon("github-mark");

        assert_eq!(network.icon(), Some("github-mark"));
    }

    #[test]
    fn test_profile_url_generation() {
        let twitter = SocialNetwork::new("twitter", "Twitter", "https://twitter.com/{username}");

        assert_eq!(twitter.profile_url("alice"), "https://twitter.com/alice");
    }

    #[test]
    fn test_profile_url_strips_at_symbol() {
        let twitter = SocialNetwork::new("twitter", "Twitter", "https://twitter.com/{username}");

        assert_eq!(twitter.profile_url("@alice"), "https://twitter.com/alice");
    }

    #[test]
    fn test_profile_url_preserves_full_url() {
        let twitter = SocialNetwork::new("twitter", "Twitter", "https://twitter.com/{username}");

        let full_url = "https://twitter.com/custom_path";
        assert_eq!(twitter.profile_url(full_url), full_url);
    }

    #[test]
    fn test_profile_url_trims_whitespace() {
        let github = SocialNetwork::new("github", "GitHub", "https://github.com/{username}");

        assert_eq!(github.profile_url("  alice  "), "https://github.com/alice");
    }

    #[test]
    fn test_registry_with_defaults() {
        let registry = SocialNetworkRegistry::with_defaults();

        assert!(registry.len() > 20);
        assert!(registry.get("twitter").is_some());
        assert!(registry.get("github").is_some());
        assert!(registry.get("linkedin").is_some());
    }

    #[test]
    fn test_registry_get_case_insensitive() {
        let registry = SocialNetworkRegistry::with_defaults();

        assert!(registry.get("Twitter").is_some());
        assert!(registry.get("GITHUB").is_some());
        assert!(registry.get("LinkedIn").is_some());
    }

    #[test]
    fn test_registry_profile_url() {
        let registry = SocialNetworkRegistry::with_defaults();

        assert_eq!(
            registry.profile_url("github", "octocat"),
            Some("https://github.com/octocat".to_string())
        );

        assert_eq!(registry.profile_url("nonexistent", "user"), None);
    }

    #[test]
    fn test_registry_search() {
        let registry = SocialNetworkRegistry::with_defaults();

        let results = registry.search("git");
        assert!(results.iter().any(|n| n.id() == "github"));
        assert!(results.iter().any(|n| n.id() == "gitlab"));
    }

    #[test]
    fn test_registry_add_and_remove() {
        let mut registry = SocialNetworkRegistry::new();

        registry.add(SocialNetwork::new(
            "custom",
            "Custom Network",
            "https://custom.com/{username}",
        ));
        assert!(registry.get("custom").is_some());

        registry.remove("custom");
        assert!(registry.get("custom").is_none());
    }

    #[test]
    fn test_registry_json_serialization() {
        let registry = SocialNetworkRegistry::with_defaults();

        let json = registry.to_json().unwrap();
        let restored = SocialNetworkRegistry::from_json(&json).unwrap();

        assert_eq!(registry.len(), restored.len());
        assert_eq!(registry.version(), restored.version());
    }

    #[test]
    fn test_registry_all_sorted() {
        let registry = SocialNetworkRegistry::with_defaults();
        let all = registry.all();

        // Verify sorted by display name
        for i in 1..all.len() {
            assert!(all[i - 1].display_name() <= all[i].display_name());
        }
    }

    #[test]
    fn test_specific_url_formats() {
        let registry = SocialNetworkRegistry::with_defaults();

        // LinkedIn uses /in/ path
        assert_eq!(
            registry.profile_url("linkedin", "johndoe"),
            Some("https://linkedin.com/in/johndoe".to_string())
        );

        // YouTube uses @ prefix
        assert_eq!(
            registry.profile_url("youtube", "creator"),
            Some("https://youtube.com/@creator".to_string())
        );

        // Substack uses subdomain
        assert_eq!(
            registry.profile_url("substack", "writer"),
            Some("https://writer.substack.com".to_string())
        );
    }

    #[test]
    fn test_mastodon_handles() {
        let mastodon = SocialNetwork::new(
            "mastodon",
            "Mastodon",
            "https://mastodon.social/@{username}",
        );

        // Simple username
        assert_eq!(
            mastodon.profile_url("alice"),
            "https://mastodon.social/@alice"
        );

        // Full federation handle - preserved
        assert_eq!(
            mastodon.profile_url("alice@fosstodon.org"),
            "https://mastodon.social/@alice@fosstodon.org"
        );
    }

//! Social Network Registry
//!
//! Provides a registry of known social networks with profile URL templates.
//! This enables generating clickable profile links from usernames.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A social network definition with profile URL template.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SocialNetwork {
    /// Unique identifier (lowercase, no spaces, e.g., "twitter", "github").
    id: String,
    /// Human-readable display name (e.g., "Twitter", "GitHub").
    display_name: String,
    /// URL template with `{username}` placeholder.
    /// Example: "https://twitter.com/{username}"
    profile_url_template: String,
    /// Optional icon identifier for UI rendering.
    icon: Option<String>,
}

impl SocialNetwork {
    /// Creates a new social network definition.
    pub fn new(id: &str, display_name: &str, profile_url_template: &str) -> Self {
        Self {
            id: id.to_lowercase(),
            display_name: display_name.to_string(),
            profile_url_template: profile_url_template.to_string(),
            icon: None,
        }
    }

    /// Creates a new social network with an icon identifier.
    pub fn with_icon(mut self, icon: &str) -> Self {
        self.icon = Some(icon.to_string());
        self
    }

    /// Returns the network's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the display name.
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Returns the profile URL template.
    pub fn profile_url_template(&self) -> &str {
        &self.profile_url_template
    }

    /// Returns the icon identifier if set.
    pub fn icon(&self) -> Option<&str> {
        self.icon.as_deref()
    }

    /// Generates a profile URL from a username.
    ///
    /// Handles common username formats:
    /// - Removes leading @ for Twitter-style handles
    /// - Preserves full URLs if already provided
    ///
    /// # Examples
    ///
    /// ```
    /// use webbook_core::social::SocialNetwork;
    ///
    /// let twitter = SocialNetwork::new("twitter", "Twitter", "https://twitter.com/{username}");
    /// assert_eq!(twitter.profile_url("alice"), "https://twitter.com/alice");
    /// assert_eq!(twitter.profile_url("@alice"), "https://twitter.com/alice");
    /// ```
    pub fn profile_url(&self, username: &str) -> String {
        // Clean up the username
        let clean_username = Self::normalize_username(username, &self.id);

        // If already a full URL, return as-is
        if clean_username.starts_with("http://") || clean_username.starts_with("https://") {
            return clean_username;
        }

        self.profile_url_template.replace("{username}", &clean_username)
    }

    /// Normalizes a username for URL generation.
    fn normalize_username(username: &str, network_id: &str) -> String {
        let username = username.trim();

        // Remove leading @ for applicable networks
        let username = if username.starts_with('@')
            && matches!(network_id, "twitter" | "instagram" | "threads" | "mastodon")
        {
            &username[1..]
        } else {
            username
        };

        // For Mastodon, handle full handles like @user@instance.social
        if network_id == "mastodon" && username.contains('@') {
            // Keep as-is for federation handles
            return username.to_string();
        }

        username.to_string()
    }
}

/// Registry of known social networks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialNetworkRegistry {
    /// Map of network ID to network definition.
    networks: HashMap<String, SocialNetwork>,
    /// Version of the registry (for cache invalidation).
    version: u32,
}

impl Default for SocialNetworkRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl SocialNetworkRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            networks: HashMap::new(),
            version: 1,
        }
    }

    /// Creates a registry with default social networks.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        // Major social platforms
        registry.add(SocialNetwork::new(
            "twitter",
            "Twitter / X",
            "https://twitter.com/{username}",
        ).with_icon("twitter"));

        registry.add(SocialNetwork::new(
            "instagram",
            "Instagram",
            "https://instagram.com/{username}",
        ).with_icon("instagram"));

        registry.add(SocialNetwork::new(
            "facebook",
            "Facebook",
            "https://facebook.com/{username}",
        ).with_icon("facebook"));

        registry.add(SocialNetwork::new(
            "linkedin",
            "LinkedIn",
            "https://linkedin.com/in/{username}",
        ).with_icon("linkedin"));

        registry.add(SocialNetwork::new(
            "tiktok",
            "TikTok",
            "https://tiktok.com/@{username}",
        ).with_icon("tiktok"));

        registry.add(SocialNetwork::new(
            "youtube",
            "YouTube",
            "https://youtube.com/@{username}",
        ).with_icon("youtube"));

        registry.add(SocialNetwork::new(
            "twitch",
            "Twitch",
            "https://twitch.tv/{username}",
        ).with_icon("twitch"));

        // Developer platforms
        registry.add(SocialNetwork::new(
            "github",
            "GitHub",
            "https://github.com/{username}",
        ).with_icon("github"));

        registry.add(SocialNetwork::new(
            "gitlab",
            "GitLab",
            "https://gitlab.com/{username}",
        ).with_icon("gitlab"));

        registry.add(SocialNetwork::new(
            "bitbucket",
            "Bitbucket",
            "https://bitbucket.org/{username}",
        ).with_icon("bitbucket"));

        registry.add(SocialNetwork::new(
            "stackoverflow",
            "Stack Overflow",
            "https://stackoverflow.com/users/{username}",
        ).with_icon("stackoverflow"));

        registry.add(SocialNetwork::new(
            "dev",
            "DEV Community",
            "https://dev.to/{username}",
        ).with_icon("dev"));

        registry.add(SocialNetwork::new(
            "codepen",
            "CodePen",
            "https://codepen.io/{username}",
        ).with_icon("codepen"));

        // Professional/Business
        registry.add(SocialNetwork::new(
            "dribbble",
            "Dribbble",
            "https://dribbble.com/{username}",
        ).with_icon("dribbble"));

        registry.add(SocialNetwork::new(
            "behance",
            "Behance",
            "https://behance.net/{username}",
        ).with_icon("behance"));

        registry.add(SocialNetwork::new(
            "medium",
            "Medium",
            "https://medium.com/@{username}",
        ).with_icon("medium"));

        registry.add(SocialNetwork::new(
            "substack",
            "Substack",
            "https://{username}.substack.com",
        ).with_icon("substack"));

        // Messaging (profile pages)
        registry.add(SocialNetwork::new(
            "telegram",
            "Telegram",
            "https://t.me/{username}",
        ).with_icon("telegram"));

        registry.add(SocialNetwork::new(
            "discord",
            "Discord",
            "https://discord.com/users/{username}",
        ).with_icon("discord"));

        registry.add(SocialNetwork::new(
            "snapchat",
            "Snapchat",
            "https://snapchat.com/add/{username}",
        ).with_icon("snapchat"));

        // Decentralized/Fediverse
        registry.add(SocialNetwork::new(
            "mastodon",
            "Mastodon",
            "https://mastodon.social/@{username}",
        ).with_icon("mastodon"));

        registry.add(SocialNetwork::new(
            "threads",
            "Threads",
            "https://threads.net/@{username}",
        ).with_icon("threads"));

        registry.add(SocialNetwork::new(
            "bluesky",
            "Bluesky",
            "https://bsky.app/profile/{username}",
        ).with_icon("bluesky"));

        // Music
        registry.add(SocialNetwork::new(
            "spotify",
            "Spotify",
            "https://open.spotify.com/user/{username}",
        ).with_icon("spotify"));

        registry.add(SocialNetwork::new(
            "soundcloud",
            "SoundCloud",
            "https://soundcloud.com/{username}",
        ).with_icon("soundcloud"));

        registry.add(SocialNetwork::new(
            "bandcamp",
            "Bandcamp",
            "https://{username}.bandcamp.com",
        ).with_icon("bandcamp"));

        // Gaming
        registry.add(SocialNetwork::new(
            "steam",
            "Steam",
            "https://steamcommunity.com/id/{username}",
        ).with_icon("steam"));

        registry.add(SocialNetwork::new(
            "xbox",
            "Xbox",
            "https://account.xbox.com/profile?gamertag={username}",
        ).with_icon("xbox"));

        registry.add(SocialNetwork::new(
            "playstation",
            "PlayStation",
            "https://psnprofiles.com/{username}",
        ).with_icon("playstation"));

        // Other
        registry.add(SocialNetwork::new(
            "reddit",
            "Reddit",
            "https://reddit.com/user/{username}",
        ).with_icon("reddit"));

        registry.add(SocialNetwork::new(
            "pinterest",
            "Pinterest",
            "https://pinterest.com/{username}",
        ).with_icon("pinterest"));

        registry.add(SocialNetwork::new(
            "tumblr",
            "Tumblr",
            "https://{username}.tumblr.com",
        ).with_icon("tumblr"));

        registry.add(SocialNetwork::new(
            "flickr",
            "Flickr",
            "https://flickr.com/people/{username}",
        ).with_icon("flickr"));

        registry.add(SocialNetwork::new(
            "vimeo",
            "Vimeo",
            "https://vimeo.com/{username}",
        ).with_icon("vimeo"));

        registry.add(SocialNetwork::new(
            "patreon",
            "Patreon",
            "https://patreon.com/{username}",
        ).with_icon("patreon"));

        registry.add(SocialNetwork::new(
            "kofi",
            "Ko-fi",
            "https://ko-fi.com/{username}",
        ).with_icon("kofi"));

        registry.add(SocialNetwork::new(
            "buymeacoffee",
            "Buy Me a Coffee",
            "https://buymeacoffee.com/{username}",
        ).with_icon("buymeacoffee"));

        registry.add(SocialNetwork::new(
            "linktree",
            "Linktree",
            "https://linktr.ee/{username}",
        ).with_icon("linktree"));

        registry
    }

    /// Adds a social network to the registry.
    pub fn add(&mut self, network: SocialNetwork) {
        self.networks.insert(network.id.clone(), network);
    }

    /// Removes a social network from the registry.
    pub fn remove(&mut self, id: &str) -> Option<SocialNetwork> {
        self.networks.remove(id)
    }

    /// Gets a social network by ID.
    pub fn get(&self, id: &str) -> Option<&SocialNetwork> {
        self.networks.get(&id.to_lowercase())
    }

    /// Returns all social networks in the registry.
    pub fn all(&self) -> Vec<&SocialNetwork> {
        let mut networks: Vec<_> = self.networks.values().collect();
        networks.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        networks
    }

    /// Returns the number of networks in the registry.
    pub fn len(&self) -> usize {
        self.networks.len()
    }

    /// Returns true if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.networks.is_empty()
    }

    /// Returns the registry version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Generates a profile URL for a given network and username.
    ///
    /// Returns None if the network is not found.
    pub fn profile_url(&self, network_id: &str, username: &str) -> Option<String> {
        self.get(network_id).map(|n| n.profile_url(username))
    }

    /// Searches for networks by name (case-insensitive partial match).
    pub fn search(&self, query: &str) -> Vec<&SocialNetwork> {
        let query_lower = query.to_lowercase();
        self.networks
            .values()
            .filter(|n| {
                n.id.contains(&query_lower) ||
                n.display_name.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// Merges another registry into this one.
    ///
    /// Networks from the other registry will overwrite existing ones.
    pub fn merge(&mut self, other: &SocialNetworkRegistry) {
        for (id, network) in &other.networks {
            self.networks.insert(id.clone(), network.clone());
        }
        self.version = self.version.max(other.version);
    }

    /// Serializes the registry to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserializes a registry from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_social_network_creation() {
        let network = SocialNetwork::new("twitter", "Twitter", "https://twitter.com/{username}");

        assert_eq!(network.id(), "twitter");
        assert_eq!(network.display_name(), "Twitter");
        assert_eq!(network.profile_url_template(), "https://twitter.com/{username}");
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

        registry.add(SocialNetwork::new("custom", "Custom Network", "https://custom.com/{username}"));
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
            assert!(all[i-1].display_name() <= all[i].display_name());
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
        let mastodon = SocialNetwork::new("mastodon", "Mastodon", "https://mastodon.social/@{username}");

        // Simple username
        assert_eq!(mastodon.profile_url("alice"), "https://mastodon.social/@alice");

        // Full federation handle - preserved
        assert_eq!(mastodon.profile_url("alice@fosstodon.org"), "https://mastodon.social/@alice@fosstodon.org");
    }
}

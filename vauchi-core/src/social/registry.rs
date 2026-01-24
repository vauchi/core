//! Social Network Registry
//!
//! Provides a registry of known social networks with profile URL templates.
//! This enables generating clickable profile links from usernames.

use crate::content::ContentManager;
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
    /// use vauchi_core::social::SocialNetwork;
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

        self.profile_url_template
            .replace("{username}", &clean_username)
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

/// Compact format for loading networks from JSON.
#[derive(Deserialize)]
struct NetworkData {
    id: String,
    name: String,
    url: String,
}

/// Registry of known social networks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialNetworkRegistry {
    networks: HashMap<String, SocialNetwork>,
    version: u32,
}

impl Default for SocialNetworkRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Embedded social network data (loaded at compile time).
const NETWORKS_JSON: &str = include_str!("networks.json");

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
        let networks: Vec<NetworkData> =
            serde_json::from_str(NETWORKS_JSON).expect("Invalid embedded networks.json");
        for n in networks {
            registry.add(SocialNetwork::new(&n.id, &n.name, &n.url).with_icon(&n.id));
        }
        registry
    }

    /// Creates a registry from ContentManager (cached → bundled fallback).
    ///
    /// This allows networks to be updated remotely without app updates.
    pub fn from_content_manager(content: &ContentManager) -> Self {
        let mut registry = Self::new();
        let networks = content.networks();
        for n in networks {
            registry.add(SocialNetwork::new(&n.id, &n.name, &n.url).with_icon(&n.id));
        }
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
                n.id.contains(&query_lower) || n.display_name.to_lowercase().contains(&query_lower)
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

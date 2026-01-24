//! Content Manager - orchestrates content updates
//!
//! The ContentManager is the main entry point for the content system.
//! It coordinates between:
//! - Bundled content (fallback)
//! - Cached content (preferred when available)
//! - Remote content (fetched on demand)

use std::collections::HashMap;
use std::time::SystemTime;
use thiserror::Error;

use super::cache::{CacheError, ContentCache};
use super::config::ContentConfig;
use super::types::{ContentType, UpdateStatus};

/// Simplified network entry for content system
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkEntry {
    /// Unique identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// URL template with {handle} placeholder
    pub url_template: String,
    /// Optional icon identifier
    #[serde(default)]
    pub icon: Option<String>,
}

/// Simplified locale strings
pub type LocaleStrings = HashMap<String, String>;

/// Manages content loading with cache → bundled fallback
pub struct ContentManager {
    config: ContentConfig,
    cache: ContentCache,
}

impl ContentManager {
    /// Create a new ContentManager
    pub fn new(config: ContentConfig) -> Result<Self, ContentError> {
        let cache = ContentCache::new(&config.storage_path)?;
        Ok(Self { config, cache })
    }

    /// Get social networks (cached → bundled)
    pub fn networks(&self) -> Vec<NetworkEntry> {
        // Try cached first
        if let Some(data) = self
            .cache
            .get_content(ContentType::Networks, "networks.json")
        {
            if let Ok(networks) = serde_json::from_slice(&data) {
                return networks;
            }
        }

        // Fall back to bundled
        bundled_networks()
    }

    /// Get locale strings for language (cached → bundled → None)
    pub fn locale(&self, lang: &str) -> Option<LocaleStrings> {
        let filename = format!("{}.json", lang);

        // Try cached first
        if let Some(data) = self.cache.get_content(ContentType::Locales, &filename) {
            if let Ok(strings) = serde_json::from_slice(&data) {
                return Some(strings);
            }
        }

        // Fall back to bundled
        bundled_locale(lang)
    }

    /// Check if an update check should be performed now
    pub fn should_check_now(&self) -> bool {
        if !self.config.remote_updates_enabled {
            return false;
        }

        let Some(last_check) = self.cache.get_last_check_time() else {
            // Never checked before
            return true;
        };

        let elapsed = SystemTime::now()
            .duration_since(last_check)
            .unwrap_or_default();

        elapsed >= self.config.check_interval
    }

    /// Record that an update check was performed
    pub fn record_check_time(&self) -> Result<(), ContentError> {
        self.cache.set_last_check_time(SystemTime::now())?;
        Ok(())
    }

    /// Check for updates (synchronous, for when remote is disabled)
    pub fn check_for_updates_sync(&self) -> UpdateStatus {
        if !self.config.remote_updates_enabled {
            return UpdateStatus::Disabled;
        }

        if !self.should_check_now() {
            return UpdateStatus::UpToDate;
        }

        // Without the content-updates feature, we can't actually check
        #[cfg(not(feature = "content-updates"))]
        {
            UpdateStatus::Disabled
        }

        #[cfg(feature = "content-updates")]
        {
            // This would need async runtime - return disabled for sync version
            UpdateStatus::Disabled
        }
    }

    /// Get access to the cache (for advanced operations)
    pub fn cache(&self) -> &ContentCache {
        &self.cache
    }

    /// Get the configuration
    pub fn config(&self) -> &ContentConfig {
        &self.config
    }
}

/// Bundled networks - compiled into the binary
fn bundled_networks() -> Vec<NetworkEntry> {
    vec![
        NetworkEntry {
            id: "twitter".to_string(),
            name: "Twitter / X".to_string(),
            url_template: "https://x.com/{handle}".to_string(),
            icon: Some("twitter".to_string()),
        },
        NetworkEntry {
            id: "github".to_string(),
            name: "GitHub".to_string(),
            url_template: "https://github.com/{handle}".to_string(),
            icon: Some("github".to_string()),
        },
        NetworkEntry {
            id: "linkedin".to_string(),
            name: "LinkedIn".to_string(),
            url_template: "https://linkedin.com/in/{handle}".to_string(),
            icon: Some("linkedin".to_string()),
        },
        NetworkEntry {
            id: "instagram".to_string(),
            name: "Instagram".to_string(),
            url_template: "https://instagram.com/{handle}".to_string(),
            icon: Some("instagram".to_string()),
        },
        NetworkEntry {
            id: "mastodon".to_string(),
            name: "Mastodon".to_string(),
            url_template: "https://{handle}".to_string(), // Full URL required for federated
            icon: Some("mastodon".to_string()),
        },
        NetworkEntry {
            id: "bluesky".to_string(),
            name: "Bluesky".to_string(),
            url_template: "https://bsky.app/profile/{handle}".to_string(),
            icon: Some("bluesky".to_string()),
        },
    ]
}

/// Bundled locale - currently only English
fn bundled_locale(lang: &str) -> Option<LocaleStrings> {
    match lang {
        "en" => Some(bundled_english()),
        _ => None,
    }
}

fn bundled_english() -> LocaleStrings {
    let mut strings = HashMap::new();
    strings.insert("app.name".to_string(), "Vauchi".to_string());
    strings.insert("settings.title".to_string(), "Settings".to_string());
    strings.insert(
        "settings.remote_updates".to_string(),
        "Remote Content Updates".to_string(),
    );
    strings.insert(
        "settings.remote_updates.enabled".to_string(),
        "Enable automatic content updates".to_string(),
    );
    strings.insert("contacts.title".to_string(), "Contacts".to_string());
    strings.insert("contacts.empty".to_string(), "No contacts yet".to_string());
    strings.insert("card.title".to_string(), "Your Card".to_string());
    strings
}

/// Errors that can occur with the content manager
#[derive(Debug, Error)]
pub enum ContentError {
    /// Cache error
    #[error("Cache error: {0}")]
    Cache(#[from] CacheError),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundled_networks_not_empty() {
        let networks = bundled_networks();
        assert!(!networks.is_empty());
        assert!(networks.iter().any(|n| n.id == "twitter"));
    }

    #[test]
    fn test_bundled_english_locale() {
        let locale = bundled_locale("en");
        assert!(locale.is_some());
        let strings = locale.unwrap();
        assert!(strings.contains_key("app.name"));
    }

    #[test]
    fn test_bundled_unknown_locale() {
        let locale = bundled_locale("zz");
        assert!(locale.is_none());
    }
}

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
#[cfg(feature = "content-updates")]
use super::fetcher::ContentFetcher;
#[cfg(feature = "content-updates")]
use super::types::ContentManifest;
use super::types::{ContentType, UpdateStatus};

/// Network entry for content system (matches networks.json format)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NetworkEntry {
    /// Unique identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// URL template with {username} placeholder
    pub url: String,
}

/// Simplified locale strings
pub type LocaleStrings = HashMap<String, String>;

/// Result of applying content updates
#[derive(Debug, Clone)]
pub enum ApplyResult {
    /// No updates were available
    NoUpdates,
    /// Updates were applied successfully
    Applied {
        /// Content types that were successfully updated
        applied: Vec<ContentType>,
        /// Content types that failed to update with error messages
        failed: Vec<(ContentType, String)>,
    },
    /// Remote updates are disabled
    Disabled,
}

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

// Async methods (require content-updates feature)
#[cfg(feature = "content-updates")]
impl ContentManager {
    /// Check for content updates asynchronously
    ///
    /// Returns which content types have updates available.
    pub async fn check_for_updates(&self) -> UpdateStatus {
        if !self.config.remote_updates_enabled {
            return UpdateStatus::Disabled;
        }

        if !self.should_check_now() {
            return UpdateStatus::UpToDate;
        }

        let fetcher = match ContentFetcher::new(&self.config) {
            Ok(f) => f,
            Err(e) => return UpdateStatus::CheckFailed(e.to_string()),
        };

        match fetcher.fetch_manifest().await {
            Ok(remote) => {
                // Record check time
                let _ = self.record_check_time();
                self.compare_versions(&remote)
            }
            Err(e) => UpdateStatus::CheckFailed(e.to_string()),
        }
    }

    /// Apply available content updates
    ///
    /// Downloads and caches any available updates.
    pub async fn apply_updates(&self) -> Result<ApplyResult, ContentError> {
        if !self.config.remote_updates_enabled {
            return Ok(ApplyResult::Disabled);
        }

        let fetcher = match ContentFetcher::new(&self.config) {
            Ok(f) => f,
            Err(e) => return Err(ContentError::Fetch(e.to_string())),
        };

        let remote = match fetcher.fetch_manifest().await {
            Ok(m) => m,
            Err(e) => return Err(ContentError::Fetch(e.to_string())),
        };

        let updates = self.find_updates(&remote);
        if updates.is_empty() {
            return Ok(ApplyResult::NoUpdates);
        }

        let mut applied = Vec::new();
        let mut failed = Vec::new();

        for content_type in updates {
            match self
                .apply_single_update(&fetcher, &remote, content_type)
                .await
            {
                Ok(()) => applied.push(content_type),
                Err(e) => failed.push((content_type, e.to_string())),
            }
        }

        // Save updated manifest
        self.cache.save_manifest(&remote)?;
        let _ = self.record_check_time();

        Ok(ApplyResult::Applied { applied, failed })
    }

    /// Compare local versions with remote manifest
    fn compare_versions(&self, remote: &ContentManifest) -> UpdateStatus {
        let updates = self.find_updates(remote);

        if updates.is_empty() {
            UpdateStatus::UpToDate
        } else {
            UpdateStatus::UpdatesAvailable(updates)
        }
    }

    /// Find which content types have updates available
    fn find_updates(&self, remote: &ContentManifest) -> Vec<ContentType> {
        let cached = self.cache.get_manifest();
        let mut updates = Vec::new();

        // Check networks
        if let Some(remote_entry) = &remote.content.networks {
            let needs_update = cached
                .as_ref()
                .and_then(|c| c.content.networks.as_ref())
                .map(|local| local.version != remote_entry.version)
                .unwrap_or(true);

            if needs_update && self.is_compatible(&remote_entry.min_app_version) {
                updates.push(ContentType::Networks);
            }
        }

        // Check locales
        if let Some(remote_entry) = &remote.content.locales {
            let needs_update = cached
                .as_ref()
                .and_then(|c| c.content.locales.as_ref())
                .map(|local| local.version != remote_entry.version)
                .unwrap_or(true);

            if needs_update && self.is_compatible(&remote_entry.min_app_version) {
                updates.push(ContentType::Locales);
            }
        }

        // Check themes
        if let Some(remote_entry) = &remote.content.themes {
            let needs_update = cached
                .as_ref()
                .and_then(|c| c.content.themes.as_ref())
                .map(|local| local.version != remote_entry.version)
                .unwrap_or(true);

            if needs_update && self.is_compatible(&remote_entry.min_app_version) {
                updates.push(ContentType::Themes);
            }
        }

        updates
    }

    /// Check if content is compatible with current app version
    fn is_compatible(&self, min_version: &str) -> bool {
        let app_version = option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0");
        version_compare(app_version, min_version).is_ge()
    }

    /// Apply a single content update
    async fn apply_single_update(
        &self,
        fetcher: &ContentFetcher,
        manifest: &ContentManifest,
        content_type: ContentType,
    ) -> Result<(), ContentError> {
        match content_type {
            ContentType::Networks => {
                let entry =
                    manifest.content.networks.as_ref().ok_or_else(|| {
                        ContentError::Fetch("No networks entry in manifest".into())
                    })?;

                let data = fetcher
                    .fetch_content(&entry.path, &entry.checksum)
                    .await
                    .map_err(|e| ContentError::Fetch(e.to_string()))?;

                self.cache.save_content(
                    ContentType::Networks,
                    "networks.json",
                    &data,
                    &entry.checksum,
                )?;
            }
            ContentType::Locales => {
                // For locales, we only download the user's language
                // This is a simplified implementation
                let entry =
                    manifest.content.locales.as_ref().ok_or_else(|| {
                        ContentError::Fetch("No locales entry in manifest".into())
                    })?;

                // Download English as default
                if let Some(en_file) = entry.files.get("en") {
                    let path = format!("{}{}", entry.path, en_file.path);
                    let data = fetcher
                        .fetch_content(&path, &en_file.checksum)
                        .await
                        .map_err(|e| ContentError::Fetch(e.to_string()))?;

                    self.cache.save_content(
                        ContentType::Locales,
                        "en.json",
                        &data,
                        &en_file.checksum,
                    )?;
                }
            }
            ContentType::Themes => {
                let entry = manifest
                    .content
                    .themes
                    .as_ref()
                    .ok_or_else(|| ContentError::Fetch("No themes entry in manifest".into()))?;

                let data = fetcher
                    .fetch_content(&entry.path, &entry.checksum)
                    .await
                    .map_err(|e| ContentError::Fetch(e.to_string()))?;

                self.cache.save_content(
                    ContentType::Themes,
                    "themes.json",
                    &data,
                    &entry.checksum,
                )?;
            }
            ContentType::Help => {
                // TODO: Implement help content caching when help system is defined
            }
        }

        Ok(())
    }
}

/// Simple version comparison (semver-like)
#[cfg(feature = "content-updates")]
fn version_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| -> Vec<u32> { s.split('.').filter_map(|p| p.parse().ok()).collect() };

    let a_parts = parse(a);
    let b_parts = parse(b);

    for (av, bv) in a_parts.iter().zip(b_parts.iter()) {
        match av.cmp(bv) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }

    a_parts.len().cmp(&b_parts.len())
}

/// Bundled networks - compiled into the binary (matches networks.json format)
fn bundled_networks() -> Vec<NetworkEntry> {
    vec![
        NetworkEntry {
            id: "twitter".to_string(),
            name: "Twitter / X".to_string(),
            url: "https://twitter.com/{username}".to_string(),
        },
        NetworkEntry {
            id: "instagram".to_string(),
            name: "Instagram".to_string(),
            url: "https://instagram.com/{username}".to_string(),
        },
        NetworkEntry {
            id: "github".to_string(),
            name: "GitHub".to_string(),
            url: "https://github.com/{username}".to_string(),
        },
        NetworkEntry {
            id: "linkedin".to_string(),
            name: "LinkedIn".to_string(),
            url: "https://linkedin.com/in/{username}".to_string(),
        },
        NetworkEntry {
            id: "mastodon".to_string(),
            name: "Mastodon".to_string(),
            url: "https://mastodon.social/@{username}".to_string(),
        },
        NetworkEntry {
            id: "bluesky".to_string(),
            name: "Bluesky".to_string(),
            url: "https://bsky.app/profile/{username}".to_string(),
        },
        NetworkEntry {
            id: "threads".to_string(),
            name: "Threads".to_string(),
            url: "https://threads.net/@{username}".to_string(),
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

    /// Fetch error (network/remote)
    #[error("Fetch error: {0}")]
    Fetch(String),
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

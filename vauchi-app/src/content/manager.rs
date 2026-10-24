// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Content Manager - orchestrates content updates
//!
//! The ContentManager is the main entry point for the content system.
//! It coordinates between:
//! - Bundled content (fallback)
//! - Cached content (preferred when available)
//! - Remote content (fetched on demand)

use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use vauchi_core::clock::Clock;
use vauchi_core::social::SocialNetworkRegistry;

use super::cache::{CacheError, ContentCache};
use super::config::ContentConfig;
#[cfg(feature = "content-updates")]
use super::fetcher::ContentFetcher;
#[cfg(feature = "content-updates")]
use super::types::ContentManifest;
use super::types::{ContentType, UpdateStatus};

/// Simplified locale strings
pub type LocaleStrings = HashMap<String, String>;

/// Result of applying content updates
#[derive(Debug, Clone)]
#[non_exhaustive]
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
    clock: Arc<dyn Clock>,
}

impl ContentManager {
    /// Create a new ContentManager
    pub fn new(config: ContentConfig, clock: Arc<dyn Clock>) -> Result<Self, ContentError> {
        let cache =
            ContentCache::new(&config.storage_path)?.with_max_content_size(config.max_content_size);
        Ok(Self {
            config,
            cache,
            clock,
        })
    }

    /// Social networks: the cached file when it passes validation,
    /// else the bundled `networks.json` (vauchi/private#734 D3).
    pub fn networks(&self) -> SocialNetworkRegistry {
        self.cache
            .get_content(ContentType::Networks, "networks.json")
            .and_then(|data| SocialNetworkRegistry::from_data_bytes(&data).ok())
            .unwrap_or_else(SocialNetworkRegistry::with_defaults)
    }

    /// Get themes (cached → single bundled default).
    ///
    /// Returns all themes from the cached themes.json if available,
    /// otherwise returns a vec containing only the bundled default theme.
    pub fn themes(&self) -> Vec<crate::Theme> {
        if let Some(data) = self.cache.get_content(ContentType::Themes, "themes.json")
            && let Ok(themes) = crate::load_themes_from_json(&data)
            && !themes.is_empty()
        {
            return themes;
        }

        // Fallback: single bundled default
        vec![crate::default_theme()]
    }

    /// Get help content for language (cached only, no bundled fallback)
    pub fn help(&self, lang: &str) -> Option<LocaleStrings> {
        let filename = format!("{}.json", lang);

        if let Some(data) = self.cache.get_content(ContentType::Help, &filename)
            && let Ok(strings) = serde_json::from_slice(&data)
        {
            return Some(strings);
        }

        None
    }

    /// Get locale strings for language (cached → bundled → None)
    pub fn locale(&self, lang: &str) -> Option<LocaleStrings> {
        let filename = format!("{}.json", lang);

        // Try cached first
        if let Some(data) = self.cache.get_content(ContentType::Locales, &filename)
            && let Ok(strings) = serde_json::from_slice(&data)
        {
            return Some(strings);
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

        let elapsed = self
            .clock
            .now()
            .duration_since(last_check)
            .unwrap_or_default();

        elapsed >= self.config.check_interval
    }

    /// Record that an update check was performed
    pub fn record_check_time(&self) -> Result<(), ContentError> {
        self.cache.set_last_check_time(self.clock.now())?;
        Ok(())
    }

    /// Check for updates (synchronous, for when remote is disabled)
    pub fn check_for_updates_sync(&self) -> UpdateStatus {
        if !self.config.remote_updates_enabled {
            return UpdateStatus::Disabled;
        }

        if !self.should_check_now() && !self.cache_misses_a_wanted_locale() {
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

    /// Load cached locale files into the i18n system.
    ///
    /// Reads all `*.json` files from the content cache locales directory and
    /// loads each into the i18n store via `i18n::load_locale_from_bytes()`.
    /// Called at startup AFTER `i18n::init()` to overlay CDN-cached versions.
    pub fn load_cached_locales_into_i18n(&self) {
        self.publish_listed_locales();
        for filename in self.cache.content_files(ContentType::Locales) {
            if let Some(code) = filename.strip_suffix(".json")
                && let Some(data) = self.cache.get_content(ContentType::Locales, &filename)
                && crate::i18n::load_locale_from_bytes(code, &data).is_err()
            {
                // The bundled locale stays in effect; the other locales
                // still load.
                self.drop_unloadable(ContentType::Locales, &filename);
            }
        }
    }

    /// Overlay the CDN-cached `themes.json` into the theme store at
    /// startup. Mirrors [`load_cached_locales_into_i18n`]; call AFTER the
    /// cache is readable but BEFORE the first render so screens use the
    /// cached catalog rather than the bundled one.
    pub fn load_cached_themes_into_theme_store(&self) {
        if let Some(data) = self.cache.get_content(ContentType::Themes, "themes.json")
            && crate::theme::load_themes_from_bytes(&data).is_err()
        {
            // The bundled catalog stays in effect.
            self.drop_unloadable(ContentType::Themes, "themes.json");
        }
    }

    /// A cached file that does not load would fail again at every start
    /// while the cached manifest calls it current, so it goes with the
    /// manifest and the next update cycle downloads it again (#747).
    fn drop_unloadable(&self, content_type: ContentType, filename: &str) {
        let dropped = self
            .cache
            .remove_content(content_type, filename)
            .and_then(|()| self.cache.clear_manifest());
        if let Err(e) = dropped {
            tracing::warn!("[Content] Failed to drop an unloadable cache entry: {e}");
        }
    }

    /// Loads the cached networks file into the active social-network
    /// registry. A file that fails validation leaves the registry as it was
    /// (vauchi/private#734 D3).
    pub fn load_cached_networks_into_registry(&self) {
        if let Some(data) = self
            .cache
            .get_content(ContentType::Networks, "networks.json")
        {
            #[allow(clippy::let_underscore_must_use)]
            let _ = crate::social::load_networks_from_bytes(&data);
        }
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

        if !self.should_check_now() && !self.cache_misses_a_wanted_locale() {
            return UpdateStatus::UpToDate;
        }

        let fetcher = match ContentFetcher::new(&self.config) {
            Ok(f) => f,
            Err(e) => return UpdateStatus::CheckFailed(e.to_string()),
        };

        match fetcher.fetch_manifest().await {
            Ok(remote) => {
                // Reject unknown schema versions (#241)
                if remote.schema_version > super::types::MAX_SUPPORTED_SCHEMA_VERSION {
                    return UpdateStatus::CheckFailed(format!(
                        "Unsupported manifest schema version {} (max: {})",
                        remote.schema_version,
                        super::types::MAX_SUPPORTED_SCHEMA_VERSION,
                    ));
                }
                // Record check time — best-effort bookkeeping; on
                // failure the next check_for_updates fires sooner
                #[allow(clippy::let_underscore_must_use)]
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

        // Reject unknown schema versions (#241)
        if remote.schema_version > super::types::MAX_SUPPORTED_SCHEMA_VERSION {
            return Err(ContentError::Fetch(format!(
                "Unsupported manifest schema version {} (max: {})",
                remote.schema_version,
                super::types::MAX_SUPPORTED_SCHEMA_VERSION,
            )));
        }

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

        if self.can_cache_manifest(&remote, &applied) {
            self.cache.save_manifest(&remote)?;
            self.publish_listed_locales();
        }
        // best-effort: see check_for_updates rationale
        #[allow(clippy::let_underscore_must_use)]
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
        let cached = self.trusted_cached_manifest();
        let mut updates = Vec::new();

        if let Some(remote_entry) = &remote.content.networks {
            let needs_update = needs_update(
                cached.as_ref(),
                ContentType::Networks,
                &remote_entry.version,
            );

            if needs_update && self.is_compatible(&remote_entry.min_app_version) {
                updates.push(ContentType::Networks);
            }
        }

        // Check locales
        if let Some(remote_entry) = &remote.content.locales {
            let needs_update =
                needs_update(cached.as_ref(), ContentType::Locales, &remote_entry.version)
                    || super::chosen_locales::misses_a_wanted_file(
                        &self.cache,
                        self.config.wanted_locales.as_deref(),
                        remote_entry,
                    );

            if needs_update && self.is_compatible(&remote_entry.min_app_version) {
                updates.push(ContentType::Locales);
            }
        }

        // Check help
        if let Some(remote_entry) = &remote.content.help {
            let needs_update =
                needs_update(cached.as_ref(), ContentType::Help, &remote_entry.version);

            if needs_update && self.is_compatible(&remote_entry.min_app_version) {
                updates.push(ContentType::Help);
            }
        }

        // Check themes
        if let Some(remote_entry) = &remote.content.themes {
            let needs_update =
                needs_update(cached.as_ref(), ContentType::Themes, &remote_entry.version);

            if needs_update && self.is_compatible(&remote_entry.min_app_version) {
                updates.push(ContentType::Themes);
            }
        }

        updates
    }

    /// Check if content is compatible with current app version
    fn is_compatible(&self, min_version: &str) -> bool {
        let app_version = option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0");
        matches!(
            version_compare(app_version, min_version),
            Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)
        )
    }

    /// The language codes the trusted cached manifest lists in its locales
    /// entry; Settings offers them before their files arrive
    /// (vauchi/private#733 D3).
    pub fn listed_locale_codes(&self) -> Vec<String> {
        self.trusted_cached_manifest()
            .and_then(|manifest| manifest.content.locales)
            .map(|locales| locales.files.into_keys().collect())
            .unwrap_or_default()
    }

    fn publish_listed_locales(&self) {
        let listed = self
            .trusted_cached_manifest()
            .and_then(|manifest| manifest.content.locales)
            .map(|locales| locales.files)
            .unwrap_or_default()
            .into_iter()
            .map(|(code, file)| crate::i18n::ListedLocale {
                code,
                name: file.name,
                english_name: file.english_name,
                is_rtl: file.is_rtl.unwrap_or(false),
            });
        crate::i18n::set_listed_locales(listed);
    }

    fn trusted_cached_manifest(&self) -> Option<ContentManifest> {
        self.cache.get_manifest().filter(|manifest| {
            super::integrity::verify_manifest_signature(manifest, &self.config.publisher_public_key)
                .is_ok()
        })
    }

    fn can_cache_manifest(&self, remote: &ContentManifest, applied: &[ContentType]) -> bool {
        let cached = self.trusted_cached_manifest();
        [
            ContentType::Networks,
            ContentType::Locales,
            ContentType::Help,
            ContentType::Themes,
        ]
        .into_iter()
        .all(|content_type| {
            let remote_version = manifest_version(remote, content_type);
            let cached_version = cached
                .as_ref()
                .and_then(|manifest| manifest_version(manifest, content_type));

            match (remote_version, cached_version) {
                (None, None) => true,
                (None, Some(_)) => false,
                (Some(remote), None) => {
                    parse_exact_version(remote).is_some() && applied.contains(&content_type)
                }
                (Some(remote), Some(local)) => match version_compare(remote, local) {
                    Some(std::cmp::Ordering::Equal) => true,
                    Some(std::cmp::Ordering::Greater) => applied.contains(&content_type),
                    Some(std::cmp::Ordering::Less) | None => false,
                },
            }
        })
    }

    /// One whole cycle on the calling thread: check, then apply only when
    /// updates are available. The binding layers and Core's content
    /// scheduler run it (vauchi/private#733 D10). The apply result is
    /// `None` when the check found nothing to apply.
    pub fn run_update_cycle_blocking(
        &self,
    ) -> (UpdateStatus, Option<Result<ApplyResult, ContentError>>) {
        let runtime = match tokio::runtime::Builder::new_current_thread().build() {
            Ok(runtime) => runtime,
            Err(e) => return (UpdateStatus::CheckFailed(e.to_string()), None),
        };
        runtime.block_on(async {
            let status = self.check_for_updates().await;
            let apply = if matches!(status, UpdateStatus::UpdatesAvailable(_)) {
                Some(self.apply_updates().await)
            } else {
                None
            };
            (status, apply)
        })
    }

    fn cache_misses_a_wanted_locale(&self) -> bool {
        self.trusted_cached_manifest()
            .and_then(|manifest| manifest.content.locales)
            .is_some_and(|entry| {
                super::chosen_locales::misses_a_wanted_file(
                    &self.cache,
                    self.config.wanted_locales.as_deref(),
                    &entry,
                )
            })
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
                // Only the wanted languages (vauchi/private#733 D7); fetching
                // through OHTTP keeps the choice from being tied to an IP (D8).
                let entry =
                    manifest.content.locales.as_ref().ok_or_else(|| {
                        ContentError::Fetch("No locales entry in manifest".into())
                    })?;
                let wanted = self.config.wanted_locales.as_deref();
                let version_changed = needs_update(
                    self.trusted_cached_manifest().as_ref(),
                    ContentType::Locales,
                    &entry.version,
                );

                for (lang_code, file_entry) in super::chosen_locales::files_to_fetch(
                    &self.cache,
                    wanted,
                    entry,
                    version_changed,
                ) {
                    let path = format!("{}{}", entry.path, file_entry.path);
                    let data = fetcher
                        .fetch_content(&path, &file_entry.checksum)
                        .await
                        .map_err(|e| ContentError::Fetch(e.to_string()))?;

                    // Loaded before it is cached: a file that does not load
                    // would fail again at every start, and a cached manifest
                    // would never download it again (#747).
                    crate::i18n::load_locale_from_bytes(lang_code, &data)
                        .map_err(|e| ContentError::Load(format!("locale {lang_code}: {e}")))?;

                    let filename = format!("{}.json", lang_code);
                    self.cache.save_content(
                        ContentType::Locales,
                        &filename,
                        &data,
                        &file_entry.checksum,
                    )?;
                }
                super::chosen_locales::prune_unwanted(&self.cache, wanted);
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

                // Loaded before it is cached, as in the Locales branch.
                // Tokens are global (tokens.json) and reload via the
                // explicit reload API, not per-theme.
                crate::theme::load_themes_from_bytes(&data)
                    .map_err(|e| ContentError::Load(format!("themes: {e}")))?;

                self.cache.save_content(
                    ContentType::Themes,
                    "themes.json",
                    &data,
                    &entry.checksum,
                )?;
            }
            ContentType::Help => {
                // Download ALL help files (privacy: bundle everything so CDN
                // cannot learn the user's language from request patterns)
                let entry = manifest
                    .content
                    .help
                    .as_ref()
                    .ok_or_else(|| ContentError::Fetch("No help entry in manifest".into()))?;

                for (lang_code, file_entry) in &entry.files {
                    let path = format!("{}{}", entry.path, file_entry.path);
                    let data = fetcher
                        .fetch_content(&path, &file_entry.checksum)
                        .await
                        .map_err(|e| ContentError::Fetch(e.to_string()))?;

                    let filename = format!("{}.json", lang_code);
                    self.cache.save_content(
                        ContentType::Help,
                        &filename,
                        &data,
                        &file_entry.checksum,
                    )?;
                }
                // Cache-only, no hot-reload (unlike locales)
            }
        }

        Ok(())
    }
}

#[cfg(feature = "content-updates")]
fn manifest_version(manifest: &ContentManifest, content_type: ContentType) -> Option<&str> {
    match content_type {
        ContentType::Networks => manifest
            .content
            .networks
            .as_ref()
            .map(|entry| entry.version.as_str()),
        ContentType::Locales => manifest
            .content
            .locales
            .as_ref()
            .map(|entry| entry.version.as_str()),
        ContentType::Help => manifest
            .content
            .help
            .as_ref()
            .map(|entry| entry.version.as_str()),
        ContentType::Themes => manifest
            .content
            .themes
            .as_ref()
            .map(|entry| entry.version.as_str()),
    }
}

#[cfg(feature = "content-updates")]
fn needs_update(
    cached: Option<&ContentManifest>,
    content_type: ContentType,
    remote_version: &str,
) -> bool {
    let Some(remote) = parse_exact_version(remote_version) else {
        return false;
    };
    let Some(local_version) = cached.and_then(|manifest| manifest_version(manifest, content_type))
    else {
        return true;
    };
    let Some(local) = parse_exact_version(local_version) else {
        return false;
    };

    remote > local
}

#[cfg(feature = "content-updates")]
fn parse_exact_version(version: &str) -> Option<[u32; 3]> {
    let mut parts = version.split('.');
    let parsed = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    if parts.next().is_some() {
        return None;
    }
    Some(parsed)
}

/// Compare exact three-component numeric versions.
#[cfg(feature = "content-updates")]
fn version_compare(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    Some(parse_exact_version(a)?.cmp(&parse_exact_version(b)?))
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
#[non_exhaustive]
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

    /// Downloaded content that matches its checksum but does not load
    #[error("Content does not load: {0}")]
    Load(String),
}

// INLINE_TEST_REQUIRED: tests private internals
#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;

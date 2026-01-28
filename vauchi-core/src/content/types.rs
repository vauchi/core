// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content type definitions for remote content updates
//!
//! These types represent the manifest and content entries used for
//! remote content updates.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Content manifest from remote server
///
/// The manifest describes all available content and their versions,
/// checksums, and compatibility requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentManifest {
    /// Schema version for manifest format compatibility
    pub schema_version: u32,
    /// ISO 8601 timestamp when manifest was generated
    pub generated_at: String,
    /// Base URL for content files
    pub base_url: String,
    /// Index of available content
    pub content: ContentIndex,
}

/// Index of all available content types
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ContentIndex {
    /// Social network definitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub networks: Option<ContentEntry>,
    /// Localization files
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locales: Option<LocalesEntry>,
    /// Help content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<LocalesEntry>,
    /// Theme definitions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub themes: Option<ContentEntry>,
}

/// A single content entry (for networks, themes, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentEntry {
    /// Semantic version of this content
    pub version: String,
    /// Path relative to base_url
    pub path: String,
    /// SHA-256 checksum in format "sha256:hexstring"
    pub checksum: String,
    /// File size in bytes
    pub size_bytes: u64,
    /// Minimum app version required
    pub min_app_version: String,
    /// Maximum app version supported (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_app_version: Option<String>,
}

/// Localized content entry with per-language files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalesEntry {
    /// Semantic version of this content set
    pub version: String,
    /// Base path relative to base_url
    pub path: String,
    /// Per-language file entries (key: language code, e.g., "en", "de")
    pub files: HashMap<String, FileEntry>,
    /// Minimum app version required
    pub min_app_version: String,
}

/// Individual file entry within a localized content set
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    /// Filename relative to parent path
    pub path: String,
    /// SHA-256 checksum in format "sha256:hexstring"
    pub checksum: String,
    /// File size in bytes
    pub size_bytes: u64,
}

/// Update check result
#[derive(Debug, Clone)]
pub enum UpdateStatus {
    /// All content is up to date
    UpToDate,
    /// Updates are available for the listed content types
    UpdatesAvailable(Vec<ContentType>),
    /// Update check failed with the given error message
    CheckFailed(String),
    /// Remote updates are disabled by user settings
    Disabled,
}

/// Types of remotely updatable content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContentType {
    /// Social network definitions
    Networks,
    /// Localization strings
    Locales,
    /// Help content (FAQ, hints, etc.)
    Help,
    /// Theme definitions
    Themes,
}

impl ContentType {
    /// Get the directory name for this content type
    pub fn dir_name(&self) -> &'static str {
        match self {
            ContentType::Networks => "networks",
            ContentType::Locales => "locales",
            ContentType::Help => "help",
            ContentType::Themes => "themes",
        }
    }
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.dir_name())
    }
}

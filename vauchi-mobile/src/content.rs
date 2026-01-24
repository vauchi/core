//! Mobile bindings for content update system.
//!
//! Provides UniFFI-compatible types and methods for checking and applying
//! remote content updates (networks, locales, themes).

#[cfg(feature = "content-updates")]
use std::path::PathBuf;

#[cfg(feature = "content-updates")]
use vauchi_core::content::{ContentConfig, UpdateStatus};

/// Content type for mobile platforms.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileContentType {
    /// Social network definitions
    Networks,
    /// Localization strings
    Locales,
    /// UI themes
    Themes,
    /// Help content
    Help,
}

impl From<MobileContentType> for vauchi_core::content::ContentType {
    fn from(ct: MobileContentType) -> Self {
        match ct {
            MobileContentType::Networks => vauchi_core::content::ContentType::Networks,
            MobileContentType::Locales => vauchi_core::content::ContentType::Locales,
            MobileContentType::Themes => vauchi_core::content::ContentType::Themes,
            MobileContentType::Help => vauchi_core::content::ContentType::Help,
        }
    }
}

impl From<vauchi_core::content::ContentType> for MobileContentType {
    fn from(ct: vauchi_core::content::ContentType) -> Self {
        match ct {
            vauchi_core::content::ContentType::Networks => MobileContentType::Networks,
            vauchi_core::content::ContentType::Locales => MobileContentType::Locales,
            vauchi_core::content::ContentType::Themes => MobileContentType::Themes,
            vauchi_core::content::ContentType::Help => MobileContentType::Help,
        }
    }
}

/// Result of checking for content updates.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileUpdateStatus {
    /// Content is up to date
    UpToDate,
    /// Updates are available for the specified content types
    UpdatesAvailable { types: Vec<MobileContentType> },
    /// Update check failed
    CheckFailed { error: String },
    /// Remote updates are disabled
    Disabled,
}

#[cfg(feature = "content-updates")]
impl From<UpdateStatus> for MobileUpdateStatus {
    fn from(status: UpdateStatus) -> Self {
        match status {
            UpdateStatus::UpToDate => MobileUpdateStatus::UpToDate,
            UpdateStatus::UpdatesAvailable(types) => MobileUpdateStatus::UpdatesAvailable {
                types: types.into_iter().map(MobileContentType::from).collect(),
            },
            UpdateStatus::CheckFailed(err) => MobileUpdateStatus::CheckFailed { error: err },
            UpdateStatus::Disabled => MobileUpdateStatus::Disabled,
        }
    }
}

/// Result of applying content updates.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileApplyResult {
    /// No updates were available
    NoUpdates,
    /// Updates were applied (some may have failed)
    Applied {
        /// Content types that were successfully updated
        applied: Vec<MobileContentType>,
        /// Content types that failed with error messages
        failed: Vec<MobileApplyFailure>,
    },
    /// Remote updates are disabled
    Disabled,
    /// Apply failed completely
    Error { error: String },
}

/// A failed content update.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileApplyFailure {
    /// The content type that failed
    pub content_type: MobileContentType,
    /// The error message
    pub error: String,
}

/// Configuration for content updates.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileContentConfig {
    /// Whether remote updates are enabled
    pub remote_updates_enabled: bool,
    /// Content server URL
    pub content_url: String,
    /// Optional SOCKS5 proxy URL (e.g., for Tor)
    pub proxy_url: Option<String>,
}

impl Default for MobileContentConfig {
    fn default() -> Self {
        Self {
            remote_updates_enabled: true,
            content_url: "https://vauchi.app/app-files".to_string(),
            proxy_url: None,
        }
    }
}

#[cfg(feature = "content-updates")]
impl MobileContentConfig {
    pub fn to_core_config(&self, storage_path: PathBuf) -> ContentConfig {
        let mut config = ContentConfig {
            storage_path,
            content_url: self.content_url.clone(),
            remote_updates_enabled: self.remote_updates_enabled,
            proxy_url: self.proxy_url.clone(),
            ..Default::default()
        };

        // Increase timeout for Tor
        if self.proxy_url.is_some() {
            config.timeout = std::time::Duration::from_secs(60);
        }

        config
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content updates operations for mobile.

#[cfg(feature = "content-updates")]
use vauchi_app::content::{ContentConfig, ContentManager};

use super::VauchiPlatform;
use super::content;
use super::types::MobileSocialNetwork;

#[uniffi::export]
impl VauchiPlatform {
    // === Content Updates ===

    /// Check if remote content updates are supported.
    ///
    /// Returns true if the content-updates feature is enabled at compile time.
    pub fn is_content_updates_supported(&self) -> bool {
        cfg!(feature = "content-updates")
    }

    /// Check for available content updates.
    ///
    /// This is a blocking call that checks the remote server for updates.
    /// Returns the update status indicating what updates are available.
    ///
    /// Note: Returns Disabled if the `content-updates` feature is not enabled.
    pub fn check_content_updates(&self) -> content::MobileUpdateStatus {
        #[cfg(feature = "content-updates")]
        {
            self.check_content_updates_impl()
        }

        #[cfg(not(feature = "content-updates"))]
        {
            content::MobileUpdateStatus::Disabled
        }
    }

    /// Apply available content updates.
    ///
    /// Downloads and caches any available updates. After applying,
    /// the new content will be used for subsequent operations.
    ///
    /// Note: Returns Disabled if the `content-updates` feature is not enabled.
    pub fn apply_content_updates(&self) -> content::MobileApplyResult {
        #[cfg(feature = "content-updates")]
        {
            self.apply_content_updates_impl()
        }

        #[cfg(not(feature = "content-updates"))]
        {
            content::MobileApplyResult::Disabled
        }
    }

    /// Reload social networks from content cache.
    ///
    /// Call this after applying content updates to refresh the
    /// list of social networks available in the app.
    pub fn reload_social_networks(&self) -> Vec<MobileSocialNetwork> {
        self.list_social_networks()
    }
}

// Internal implementation methods for content updates (feature-gated)
#[cfg(feature = "content-updates")]
impl VauchiPlatform {
    fn check_content_updates_impl(&self) -> content::MobileUpdateStatus {
        use content::MobileUpdateStatus;

        let config = ContentConfig {
            storage_path: self
                .storage_path
                .parent()
                .unwrap_or(&self.storage_path)
                .to_path_buf(),
            remote_updates_enabled: true,
            ..Default::default()
        };

        let manager = match ContentManager::new(config) {
            Ok(m) => m,
            Err(e) => {
                return MobileUpdateStatus::CheckFailed {
                    error: e.to_string(),
                };
            }
        };

        let rt: tokio::runtime::Runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                return MobileUpdateStatus::CheckFailed {
                    error: e.to_string(),
                };
            }
        };

        rt.block_on(async { manager.check_for_updates().await.into() })
    }

    fn apply_content_updates_impl(&self) -> content::MobileApplyResult {
        use content::{MobileApplyFailure, MobileApplyResult, MobileContentType};

        let config = ContentConfig {
            storage_path: self
                .storage_path
                .parent()
                .unwrap_or(&self.storage_path)
                .to_path_buf(),
            remote_updates_enabled: true,
            ..Default::default()
        };

        let manager = match ContentManager::new(config) {
            Ok(m) => m,
            Err(e) => {
                return MobileApplyResult::Error {
                    error: e.to_string(),
                };
            }
        };

        let rt: tokio::runtime::Runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                return MobileApplyResult::Error {
                    error: e.to_string(),
                };
            }
        };

        rt.block_on(async {
            match manager.apply_updates().await {
                Ok(result) => match result {
                    vauchi_app::content::ApplyResult::NoUpdates => MobileApplyResult::NoUpdates,
                    vauchi_app::content::ApplyResult::Disabled => MobileApplyResult::Disabled,
                    vauchi_app::content::ApplyResult::Applied { applied, failed } => {
                        MobileApplyResult::Applied {
                            applied: applied.into_iter().map(MobileContentType::from).collect(),
                            failed: failed
                                .into_iter()
                                .map(|(ct, err)| MobileApplyFailure {
                                    content_type: MobileContentType::from(ct),
                                    error: err,
                                })
                                .collect(),
                        }
                    }
                },
                Err(e) => MobileApplyResult::Error {
                    error: e.to_string(),
                },
            }
        })
    }
}

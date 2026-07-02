// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mobile bindings for content update system.
//!
//! Provides UniFFI-compatible types and methods for checking and applying
//! remote content updates (networks, locales, themes).

#[cfg(feature = "content-updates")]
use std::path::PathBuf;

#[cfg(feature = "content-updates")]
use vauchi_app::content::{ContentConfig, UpdateStatus};

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

impl From<MobileContentType> for vauchi_app::content::ContentType {
    fn from(ct: MobileContentType) -> Self {
        match ct {
            MobileContentType::Networks => vauchi_app::content::ContentType::Networks,
            MobileContentType::Locales => vauchi_app::content::ContentType::Locales,
            MobileContentType::Themes => vauchi_app::content::ContentType::Themes,
            MobileContentType::Help => vauchi_app::content::ContentType::Help,
        }
    }
}

impl From<vauchi_app::content::ContentType> for MobileContentType {
    fn from(ct: vauchi_app::content::ContentType) -> Self {
        match ct {
            vauchi_app::content::ContentType::Networks => MobileContentType::Networks,
            vauchi_app::content::ContentType::Locales => MobileContentType::Locales,
            vauchi_app::content::ContentType::Themes => MobileContentType::Themes,
            vauchi_app::content::ContentType::Help => MobileContentType::Help,
            _ => MobileContentType::Help,
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
            _ => MobileUpdateStatus::CheckFailed {
                error: "unknown update status".to_string(),
            },
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

/// Presentation-only outcome of `DomainCommand::RunContentUpdateCycle`.
///
/// Frontends schedule the cycle (WorkManager / BGTask / app launch) and
/// read only scheduler/appearance signals; the check→apply→refresh
/// domain sequencing lives core-side (ADR-021/ADR-043 — F-3 of the
/// pure-functional-core program record, Findings 2026-07-02).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MobileContentCycleOutcome {
    /// At least one content type was updated on disk.
    pub applied: bool,
    /// The cycle failed in a way a scheduler may retry (check or
    /// per-type download failure). Disabled/up-to-date is not a failure.
    pub retryable_failure: bool,
    /// Themes changed — re-apply the selected theme through the
    /// native appearance API.
    pub refresh_appearance: bool,
}

/// Pure mapping from (check status, optional apply result) to the
/// cycle outcome. `apply` is `None` when the check did not surface
/// `UpdatesAvailable` (the driver skips apply) — if it is `None`
/// *despite* `UpdatesAvailable`, the cycle did not complete and the
/// outcome is retryable.
pub(crate) fn content_cycle_outcome(
    status: &MobileUpdateStatus,
    apply: Option<&MobileApplyResult>,
) -> MobileContentCycleOutcome {
    let noop = MobileContentCycleOutcome {
        applied: false,
        retryable_failure: false,
        refresh_appearance: false,
    };
    match status {
        MobileUpdateStatus::UpToDate | MobileUpdateStatus::Disabled => noop,
        MobileUpdateStatus::CheckFailed { .. } => MobileContentCycleOutcome {
            retryable_failure: true,
            ..noop
        },
        MobileUpdateStatus::UpdatesAvailable { .. } => match apply {
            Some(MobileApplyResult::Applied { applied, failed }) => MobileContentCycleOutcome {
                applied: !applied.is_empty(),
                retryable_failure: !failed.is_empty(),
                refresh_appearance: applied
                    .iter()
                    .any(|t| matches!(t, MobileContentType::Themes)),
            },
            Some(MobileApplyResult::NoUpdates) | Some(MobileApplyResult::Disabled) => noop,
            Some(MobileApplyResult::Error { .. }) | None => MobileContentCycleOutcome {
                retryable_failure: true,
                ..noop
            },
        },
    }
}

/// Configuration for content updates.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileContentConfig {
    /// Whether remote updates are enabled
    pub remote_updates_enabled: bool,
    /// Content server URL
    pub content_url: String,
    /// Optional SOCKS5 proxy URL (e.g., for SOCKS5 proxy)
    pub proxy_url: Option<String>,
}

impl Default for MobileContentConfig {
    fn default() -> Self {
        Self {
            remote_updates_enabled: true,
            content_url: "https://cdn.vauchi.app/v1".to_string(),
            proxy_url: None,
        }
    }
}

#[cfg(feature = "content-updates")]
impl MobileContentConfig {
    /// Converts this mobile content configuration into a core `ContentConfig` for the content update system.
    pub fn to_core_config(&self, storage_path: PathBuf) -> ContentConfig {
        let mut config = ContentConfig {
            storage_path,
            content_url: self.content_url.clone(),
            remote_updates_enabled: self.remote_updates_enabled,
            proxy_url: self.proxy_url.clone(),
            ..Default::default()
        };

        // Increase timeout for proxy
        if self.proxy_url.is_some() {
            config.timeout = std::time::Duration::from_secs(60);
        }

        config
    }
}

// INLINE_TEST_REQUIRED: content_cycle_outcome is pub(crate), cannot be tested from external tests/
#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(applied: bool, retryable: bool, refresh: bool) -> MobileContentCycleOutcome {
        MobileContentCycleOutcome {
            applied,
            retryable_failure: retryable,
            refresh_appearance: refresh,
        }
    }

    // @internal
    #[test]
    fn cycle_outcome_up_to_date_and_disabled_are_noop() {
        assert_eq!(
            content_cycle_outcome(&MobileUpdateStatus::UpToDate, None),
            outcome(false, false, false)
        );
        assert_eq!(
            content_cycle_outcome(&MobileUpdateStatus::Disabled, None),
            outcome(false, false, false)
        );
    }

    // @internal
    #[test]
    fn cycle_outcome_check_failure_is_retryable() {
        assert_eq!(
            content_cycle_outcome(
                &MobileUpdateStatus::CheckFailed {
                    error: "timeout".into()
                },
                None
            ),
            outcome(false, true, false)
        );
    }

    // @internal
    #[test]
    fn cycle_outcome_applied_themes_requests_appearance_refresh() {
        let status = MobileUpdateStatus::UpdatesAvailable {
            types: vec![MobileContentType::Themes, MobileContentType::Networks],
        };
        let apply = MobileApplyResult::Applied {
            applied: vec![MobileContentType::Themes, MobileContentType::Networks],
            failed: vec![],
        };
        assert_eq!(
            content_cycle_outcome(&status, Some(&apply)),
            outcome(true, false, true)
        );
    }

    // @internal
    #[test]
    fn cycle_outcome_applied_without_themes_skips_appearance_refresh() {
        let status = MobileUpdateStatus::UpdatesAvailable {
            types: vec![MobileContentType::Locales],
        };
        let apply = MobileApplyResult::Applied {
            applied: vec![MobileContentType::Locales],
            failed: vec![],
        };
        assert_eq!(
            content_cycle_outcome(&status, Some(&apply)),
            outcome(true, false, false)
        );
    }

    // @internal
    #[test]
    fn cycle_outcome_partial_failure_is_applied_and_retryable() {
        let status = MobileUpdateStatus::UpdatesAvailable {
            types: vec![MobileContentType::Locales, MobileContentType::Help],
        };
        let apply = MobileApplyResult::Applied {
            applied: vec![MobileContentType::Locales],
            failed: vec![MobileApplyFailure {
                content_type: MobileContentType::Help,
                error: "404".into(),
            }],
        };
        assert_eq!(
            content_cycle_outcome(&status, Some(&apply)),
            outcome(true, true, false)
        );
    }

    // @internal
    #[test]
    fn cycle_outcome_apply_error_and_missing_apply_are_retryable() {
        let status = MobileUpdateStatus::UpdatesAvailable {
            types: vec![MobileContentType::Networks],
        };
        assert_eq!(
            content_cycle_outcome(
                &status,
                Some(&MobileApplyResult::Error {
                    error: "disk full".into()
                })
            ),
            outcome(false, true, false)
        );
        assert_eq!(
            content_cycle_outcome(&status, None),
            outcome(false, true, false)
        );
    }

    // @internal
    #[test]
    fn cycle_outcome_apply_noop_variants_are_noop() {
        let status = MobileUpdateStatus::UpdatesAvailable {
            types: vec![MobileContentType::Networks],
        };
        assert_eq!(
            content_cycle_outcome(&status, Some(&MobileApplyResult::NoUpdates)),
            outcome(false, false, false)
        );
        assert_eq!(
            content_cycle_outcome(&status, Some(&MobileApplyResult::Disabled)),
            outcome(false, false, false)
        );
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content-update cycle internals.
//!
//! Only `MobileContentCycleOutcome` is exported over UniFFI; it is the
//! presentation-only result of `DomainCommand::RunContentUpdateCycle`.
//! The intermediate status/result mirrors in this module are internal
//! helpers that add an error variant to the core apply result so the
//! cycle can distinguish "apply errored" from "apply skipped".

use vauchi_app::content::{ApplyResult, ContentType, UpdateStatus};

/// Internal check-status mirror.
#[derive(Debug, Clone)]
pub(crate) enum MobileUpdateStatus {
    UpToDate,
    UpdatesAvailable { types: Vec<ContentType> },
    CheckFailed { error: String },
    Disabled,
}

impl From<UpdateStatus> for MobileUpdateStatus {
    fn from(status: UpdateStatus) -> Self {
        match status {
            UpdateStatus::UpToDate => MobileUpdateStatus::UpToDate,
            UpdateStatus::UpdatesAvailable(types) => MobileUpdateStatus::UpdatesAvailable { types },
            UpdateStatus::CheckFailed(err) => MobileUpdateStatus::CheckFailed { error: err },
            UpdateStatus::Disabled => MobileUpdateStatus::Disabled,
            // Non-exhaustive guard: unknown status is treated as a check failure.
            _ => MobileUpdateStatus::CheckFailed {
                error: "unknown update status".to_string(),
            },
        }
    }
}

/// Internal apply-result mirror. Adds an `Error` variant so that a
/// failure to even start applying (e.g. runtime creation error) can be
/// mapped to a retryable cycle outcome.
#[derive(Debug, Clone)]
pub(crate) enum MobileApplyResult {
    NoUpdates,
    Applied {
        applied: Vec<ContentType>,
        failed: Vec<(ContentType, String)>,
    },
    Disabled,
    Error {
        error: String,
    },
}

impl From<ApplyResult> for MobileApplyResult {
    fn from(result: ApplyResult) -> Self {
        match result {
            ApplyResult::NoUpdates => MobileApplyResult::NoUpdates,
            ApplyResult::Disabled => MobileApplyResult::Disabled,
            ApplyResult::Applied { applied, failed } => {
                MobileApplyResult::Applied { applied, failed }
            }
            // Non-exhaustive guard: unknown result is treated as an error.
            _ => MobileApplyResult::Error {
                error: "unknown apply result".to_string(),
            },
        }
    }
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
                refresh_appearance: applied.iter().any(|t| matches!(t, ContentType::Themes)),
            },
            // NoUpdates: benign check→apply race, data already current.
            // Disabled: ContentManager deactivated internally even with
            // the compile-time feature on — retrying won't help.
            Some(MobileApplyResult::NoUpdates) | Some(MobileApplyResult::Disabled) => noop,
            Some(MobileApplyResult::Error { .. }) | None => MobileContentCycleOutcome {
                retryable_failure: true,
                ..noop
            },
        },
    }
}

// INLINE_TEST_REQUIRED: content_cycle_outcome is pub(crate); its mapping
// logic is exercised here against the internal status/result helpers.
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
            types: vec![ContentType::Themes, ContentType::Networks],
        };
        let apply = MobileApplyResult::Applied {
            applied: vec![ContentType::Themes, ContentType::Networks],
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
            types: vec![ContentType::Locales],
        };
        let apply = MobileApplyResult::Applied {
            applied: vec![ContentType::Locales],
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
            types: vec![ContentType::Locales, ContentType::Help],
        };
        let apply = MobileApplyResult::Applied {
            applied: vec![ContentType::Locales],
            failed: vec![(ContentType::Help, "404".into())],
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
            types: vec![ContentType::Networks],
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
            types: vec![ContentType::Networks],
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

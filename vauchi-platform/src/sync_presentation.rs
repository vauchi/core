// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Prepared presentation for a sync cycle: the localized summary line,
//! the refresh hint, and the first-update celebration. Core decides all
//! three so no shell assembles copy from counts or owns a milestone
//! tracker (ADR-066, ADR-069; RG-11 `TODO(HUMBLE)` sites).

use vauchi_app::i18n::{Locale, get_string, get_string_with_args};
use vauchi_core::AhaMomentType;
use vauchi_core::api::VauchiSyncOutcome;
use vauchi_core::platform::{AnimationToken, HapticPattern, SoundToken};

use crate::error::MobileError;
use crate::types::MobileSyncResult;

/// Haptic intent, mirroring [`HapticPattern`] for `Command::Celebrate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileHapticPattern {
    /// The platform's "success" notification haptic.
    Success,
    /// A single light tap.
    Light,
    /// No haptic.
    None,
}

impl From<HapticPattern> for MobileHapticPattern {
    fn from(pattern: HapticPattern) -> Self {
        match pattern {
            HapticPattern::Success => Self::Success,
            HapticPattern::Light => Self::Light,
            _ => Self::None,
        }
    }
}

/// Sound intent, mirroring [`SoundToken`] for `Command::Celebrate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileSoundToken {
    /// The short exchange chime.
    ExchangeChime,
    /// No sound.
    None,
}

impl From<SoundToken> for MobileSoundToken {
    fn from(token: SoundToken) -> Self {
        match token {
            SoundToken::ExchangeChime => Self::ExchangeChime,
            _ => Self::None,
        }
    }
}

/// Animation intent, mirroring [`AnimationToken`] for `Command::Celebrate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileAnimationToken {
    /// The two cards meet — the exchange-success beat.
    CardsMeet,
    /// No animation.
    None,
}

impl From<AnimationToken> for MobileAnimationToken {
    fn from(token: AnimationToken) -> Self {
        match token {
            AnimationToken::CardsMeet => Self::CardsMeet,
            _ => Self::None,
        }
    }
}

/// A milestone celebration the shell performs verbatim: localized copy
/// plus the same closed intent tokens as `Command::Celebrate`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MobileCelebration {
    pub title: String,
    pub message: String,
    pub haptic: MobileHapticPattern,
    pub sound: MobileSoundToken,
    pub animation: MobileAnimationToken,
}

impl MobileSyncResult {
    /// Maps a core sync outcome to the mobile result shape, presentation
    /// included.
    ///
    /// Throttle decision (engine-resident-sync-orchestration design §4):
    /// `TooSoon` is a benign no-change result, *not* an error — a
    /// user-initiated sync inside the C1/C2 privacy window reports
    /// "up to date" rather than failing. `NotConnected` / `NoIdentity`
    /// stay errors so the caller distinguishes them from a successful
    /// empty sync. The `Ok` field mapping mirrors the retired
    /// `VauchiPlatform::sync()` exactly (`received → cards_updated`,
    /// `sent → updates_sent`; `contacts_added` / `updated_contact_names`
    /// have no source in the outcome).
    pub fn from_outcome(outcome: VauchiSyncOutcome, locale: Locale) -> Result<Self, MobileError> {
        match outcome {
            VauchiSyncOutcome::Ok {
                received,
                fetched,
                rejected,
                unresolved,
                reject_reasons,
                sent,
                errors,
                aha_moments,
                ..
            } => {
                let has_changes = received > 0 || sent > 0;
                let celebrate = aha_moments
                    .iter()
                    .any(|moment| moment.moment_type == AhaMomentType::FirstUpdateReceived)
                    .then(|| first_update_celebration(locale));
                Ok(MobileSyncResult {
                    contacts_added: 0,
                    cards_updated: received as u32,
                    updates_sent: sent as u32,
                    total: (received + sent) as u32,
                    has_changes,
                    updated_contact_names: vec![],
                    blobs_fetched: fetched as u32,
                    rejected: rejected as u32,
                    unresolved: unresolved as u32,
                    reject_reasons,
                    summary: summary(locale, received, sent, &errors),
                    should_refresh_presentation: has_changes,
                    celebrate,
                })
            }
            VauchiSyncOutcome::TooSoon => Ok(MobileSyncResult {
                contacts_added: 0,
                cards_updated: 0,
                updates_sent: 0,
                total: 0,
                has_changes: false,
                updated_contact_names: vec![],
                blobs_fetched: 0,
                rejected: 0,
                unresolved: 0,
                reject_reasons: String::new(),
                summary: get_string(locale, "sync.no_changes"),
                should_refresh_presentation: false,
                celebrate: None,
            }),
            VauchiSyncOutcome::NotConnected => Err(MobileError::Other {
                detail: "Not connected".into(),
            }),
            VauchiSyncOutcome::NoIdentity => Err(MobileError::Other {
                detail: "No identity".into(),
            }),
        }
    }
}

fn summary(locale: Locale, received: usize, sent: usize, errors: &[String]) -> String {
    if received > 0 || sent > 0 {
        return get_string_with_args(
            locale,
            "sync.message_format",
            &[
                ("cards_updated", received.to_string().as_str()),
                ("updates_sent", sent.to_string().as_str()),
            ],
        );
    }
    if errors.is_empty() {
        get_string(locale, "sync.no_changes")
    } else {
        get_string(locale, "sync.error_failed")
    }
}

/// Only the haptic plays: the cards-meet beat and chime belong to the
/// exchange ceremony, so reusing them here would blur that moment.
fn first_update_celebration(locale: Locale) -> MobileCelebration {
    MobileCelebration {
        title: get_string(locale, "aha.first_update_received.title"),
        message: get_string(locale, "aha.first_update_received.message"),
        haptic: HapticPattern::Success.into(),
        sound: SoundToken::None.into(),
        animation: AnimationToken::None.into(),
    }
}

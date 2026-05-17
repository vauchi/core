// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! G4 Humble-UI surface for the contact-detail screen.
//!
//! Frontends (iOS `Vauchi/Views/ContactDetailView.swift`, Android
//! `app/.../ui/ContactDetailScreen.kt`) historically branched on raw
//! `MobileContact` flags (`isVerified`, `reciprocity`, `isRecoveryTrusted`,
//! `isHidden`, `trustLevel`) to decide which badges, banners, and actions
//! to render. The two frontends used divergent predicates for the same
//! "Verify Contact" affordance — closing that divergence is the point of
//! G4 (audit V4, ADR-021/043).
//!
//! `VauchiPlatform::contact_detail_view_state(contact_id)` returns a
//! pre-computed `MobileContactDetailViewState` whose `badges`, `banners`,
//! and `actions` lists fully specify what the frontend renders.
//! Frontends iterate; they never branch on contact properties.
//!
//! Closes G4 of the four-phase ScreenModel API gap workstream tracked in
//! `_private/docs/problems/2026-04-27-screenmodel-api-gaps-symmetric-frontend-violations`.

use vauchi_app::i18n::Locale;
use vauchi_app::relative_time::format_relative_time;

/// A user-actionable affordance on the contact-detail screen.
///
/// Frontends render one button per variant in the order returned by
/// `VauchiPlatform::contact_detail_view_state` — the engine emits only
/// those affordances valid for the contact's state. Stateful variants
/// carry the current value so the frontend's button label flips
/// (Trust / Untrust, Hide / Unhide) without re-deriving.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum MobileContactDetailAction {
    /// Mark the fingerprint as manually verified (one-shot).
    Verify,
    /// Toggle the recovery-trust flag. `currently_trusted` lets the
    /// frontend label the button "Trust" or "Untrust" without
    /// re-querying.
    ToggleRecoveryTrust { currently_trusted: bool },
    /// Toggle the hidden flag.
    ToggleHidden { currently_hidden: bool },
    /// Open the contact editor.
    Edit,
    /// Open the fingerprint-verification flow.
    VerifyFingerprint,
    /// Switch to "what they see" preview perspective.
    PreviewAs { contact_id: String },
    /// Soft-delete (exchanged contacts) — recoverable via Archive list.
    Archive,
    /// Hard-delete (imported contacts) — not recoverable.
    Delete,
    /// Navigate back.
    Back,
}

/// A status badge to render next to the contact name.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum MobileContactDetailBadge {
    /// Fingerprint manually verified out-of-band.
    Verified,
    /// Trusted as a recovery helper.
    RecoveryTrusted,
}

/// A banner to render above the field list.
///
/// `label` is plain English today; localization is a follow-up
/// (G4b — see plan §3 / T4.0.3). The label string is core-owned; the
/// frontend never composes it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum MobileContactDetailBanner {
    /// Awaiting async confirmation from the other side.
    ReciprocityPending { label: String },
    /// Confirmation window expired without reciprocation.
    ReciprocityUnreciprocated { label: String },
}

/// Pre-computed render state for the contact-detail screen.
///
/// Closes audit V4 by replacing the iOS `if contact.isVerified` /
/// `contact.reciprocity == .pending` / `contact.isRecoveryTrusted` /
/// `contact.isHidden` and Android
/// `if (c.trustLevel == STANDARD || HIGH)` branches with a typed list
/// for the frontend to render.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileContactDetailViewState {
    /// Status badges to render. Order is canonical for the platform.
    pub badges: Vec<MobileContactDetailBadge>,
    /// Banners to render above the fields.
    pub banners: Vec<MobileContactDetailBanner>,
    /// Action buttons to render in the screen footer/action bar.
    pub actions: Vec<MobileContactDetailAction>,
    /// Localized "Added X ago" display string, or `None` for imported
    /// contacts (no exchange timestamp). Closes G6 (a) — frontends
    /// stop calling Swift's `RelativeDateTimeFormatter` /
    /// Android's `DateUtils.getRelativeTimeSpanString` directly and
    /// instead render this string verbatim. The locale used today is
    /// English; per-locale plumbing is the follow-up
    /// (`contact_detail_view_state_localized` overload).
    pub added_time_display: Option<String>,
}

/// Compute the "Added X ago" display string from the contact's exchange
/// timestamp.
///
/// Returns `None` for imported contacts (no exchange timestamp) and for
/// contacts with `exchange_timestamp == 0` (legacy / migration sentinel).
/// Factored out for tests so the bucket boundaries can be exercised
/// without mocking `SystemTime`.
pub(crate) fn compute_added_time_display(
    contact: &vauchi_core::Contact,
    now: u64,
    locale: Locale,
) -> Option<String> {
    let then = contact.exchange_timestamp()?;
    if then == 0 {
        return None;
    }
    Some(format_relative_time(now, then, locale))
}

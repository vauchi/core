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

use super::VauchiPlatform;
use super::error::MobileError;

use vauchi_app::i18n::Locale;
use vauchi_app::relative_time::format_relative_time;
use vauchi_app::ui::{
    ReciprocityBannerKind, reciprocity_banner, show_recovery_trusted_indicator,
    show_verified_badge, verify_button_visible,
};

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

#[uniffi::export]
impl VauchiPlatform {
    /// Compute the typed render state for a contact-detail screen.
    ///
    /// Frontends call this with a contact id, then iterate the returned
    /// `badges`, `banners`, and `actions` — they never branch on raw
    /// `MobileContact` flags. Closes ADR-021/043 audit V4.
    ///
    /// Returns `MobileError::InvalidInput` if no contact with the given
    /// id exists.
    pub fn contact_detail_view_state(
        &self,
        contact_id: String,
    ) -> Result<MobileContactDetailViewState, MobileError> {
        let storage = self.open_storage()?;
        let contact =
            storage
                .load_contact(&contact_id)?
                .ok_or_else(|| MobileError::InvalidInput {
                    field: "contact_id".to_string(),
                    detail: format!("contact not found: {contact_id}"),
                })?;

        let mut badges = Vec::new();
        if show_verified_badge(contact.is_fingerprint_verified()) {
            badges.push(MobileContactDetailBadge::Verified);
        }
        if show_recovery_trusted_indicator(contact.is_recovery_trusted()) {
            badges.push(MobileContactDetailBadge::RecoveryTrusted);
        }

        let mut banners = Vec::new();
        if let Some(kind) = reciprocity_banner(contact.reciprocity(0)) {
            banners.push(match kind {
                // Plain English today; G4b is the i18n follow-up.
                // See plan §3 / T4.0.3.
                ReciprocityBannerKind::Pending => MobileContactDetailBanner::ReciprocityPending {
                    label: "Waiting for them to share their info".to_string(),
                },
                ReciprocityBannerKind::Unreciprocated => {
                    MobileContactDetailBanner::ReciprocityUnreciprocated {
                        label: "They haven't shared their info".to_string(),
                    }
                }
            });
        }

        let mut actions = Vec::new();
        if verify_button_visible(contact.is_fingerprint_verified(), contact.trust_level()) {
            actions.push(MobileContactDetailAction::Verify);
        }
        actions.push(MobileContactDetailAction::ToggleRecoveryTrust {
            currently_trusted: contact.is_recovery_trusted(),
        });
        actions.push(MobileContactDetailAction::ToggleHidden {
            currently_hidden: contact.is_hidden(),
        });
        actions.push(MobileContactDetailAction::Edit);
        actions.push(MobileContactDetailAction::VerifyFingerprint);
        actions.push(MobileContactDetailAction::PreviewAs {
            contact_id: contact_id.clone(),
        });
        if contact.is_imported() {
            actions.push(MobileContactDetailAction::Delete);
        } else {
            actions.push(MobileContactDetailAction::Archive);
        }
        actions.push(MobileContactDetailAction::Back);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let added_time_display = compute_added_time_display(&contact, now, Locale::English);

        Ok(MobileContactDetailViewState {
            badges,
            banners,
            actions,
            added_time_display,
        })
    }
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

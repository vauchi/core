// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolved per-recipient presentation (ADR-054 D1).

/// How the user is presented to one recipient: the display name, optional bio,
/// and optional avatar that recipient sees, after per-group override resolution
/// (ADR-054 D1).
///
/// This is **not** the cryptographic [`crate::identity::Identity`], which is
/// singular and fixed. Presentation is what the user is *known as* by a given
/// contact and may differ per recipient; it is drawn from one winning group (or
/// the default card), not unioned across groups. See
/// `GroupManager::resolve_presentation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPresentation {
    /// Display name shown to the recipient.
    pub display_name: String,
    /// Bio shown to the recipient, if any.
    pub bio: Option<String>,
    /// Avatar bytes (WebP, ADR-042) shown to the recipient, if any.
    pub avatar: Option<Vec<u8>>,
}

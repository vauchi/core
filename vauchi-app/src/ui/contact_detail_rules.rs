// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pure presentation predicates for the contact-detail screen.
//!
//! These were lifted out of [`crate::ui::contact_detail`] (the engine
//! module) so the engine file stays under its size baseline. They are
//! pure functions over primitives / domain enums — no engine state — and
//! are re-exported from [`crate::ui`] for both the engine and the
//! `vauchi-platform` mobile bridge (which builds native ContactDetail
//! views from `MobileContact` via the same canonical predicates).

use crate::i18n::Locale;
use crate::ui::*;
use vauchi_core::contact::trust::TrustLevel;
use vauchi_core::exchange::reciprocity::Reciprocity;

/// A contact's owner-private tag, reduced to what the renderer needs
/// (ADR-051 contact annotations). The wire shape is UI-only — `id` +
/// `label`-style `name` — so it carries no domain branching.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContactTag {
    /// Stable tag id, used to build the per-row `remove_tag:<id>` action.
    pub id: String,
    /// Display name of the tag.
    pub name: String,
}

/// Build the contact-detail tag affordances: the current-tags list (each
/// row removable via `remove_tag:<id>`), the add-tag text input, and —
/// only while a query is being typed — the autocomplete-or-create
/// suggestion list (each row commits via `add_tag:<name>`, which
/// `Vauchi::add_tag_to_contact` resolves to reuse-or-create).
///
/// Pure presentation: the engine owns the data, the AppEngine intercept
/// owns persistence. Component/field names stay UI-shaped (Wire Humble).
pub fn tag_components(
    tags: &[ContactTag],
    query: &str,
    suggestions: &[String],
    locale: Locale,
) -> Vec<Component> {
    let t = |key: &str| crate::i18n::get_string(locale, key);
    let mut out = Vec::with_capacity(3);

    out.push(Component::ActionList {
        id: "contact_tags".into(),
        items: tags
            .iter()
            .map(|tag| ActionListItem {
                id: format!("remove_tag:{}", tag.id),
                label: tag.name.clone(),
                icon: None,
                detail: None,
                a11y: Some(A11y {
                    label: Some(format!("Remove tag {}", tag.name)),
                    hint: Some(t("contact_detail.tag_remove_a11y_hint")),
                    role: None,
                }),
                info_key: None,
            })
            .collect(),
    });

    out.push(Component::TextInput {
        id: "add_tag".into(),
        label: t("contact_detail.add_tag_label"),
        value: query.to_string(),
        placeholder: Some(t("contact_detail.tag_placeholder")),
        max_length: None,
        validation_error: None,
        input_type: InputType::Text,
        a11y: Some(A11y {
            label: Some(t("contact_detail.add_tag_a11y_label")),
            hint: None,
            role: Some(AccessibilityRole::TextField),
        }),
        info_key: None,
    });

    if !query.is_empty() {
        out.push(Component::ActionList {
            id: "tag_suggestions".into(),
            items: suggestions
                .iter()
                .map(|name| ActionListItem {
                    id: format!("add_tag:{name}"),
                    label: name.clone(),
                    icon: None,
                    detail: None,
                    a11y: Some(A11y {
                        label: Some(format!("Add tag {name}")),
                        hint: None,
                        role: None,
                    }),
                    info_key: None,
                })
                .collect(),
        });
    }

    out
}

/// Action id the contact-detail footer button carries, derived from
/// whether the contact is imported.
///
/// `ContactDetailEngine::build_screen` emits this for a given
/// imported-vs-exchanged contact.
///
/// Imported contacts get `"delete_contact"` (Destructive); exchanged
/// contacts get `"archive_contact"` (Secondary). Frontends should
/// dispatch on the returned id rather than re-deriving the choice from
/// `MobileContact.is_imported`, per the §1A pure-renderer rule —
/// `_private/docs/problems/2026-04-25-isimported-frontend-cleanup/`.
pub fn footer_action_id(is_imported: bool) -> &'static str {
    if is_imported {
        "delete_contact"
    } else {
        "archive_contact"
    }
}

// ── G4: Pure helpers for ContactDetail visibility ─────────────────────────
//
// Lifted out of `build_screen` so that mobile frontends (which do not
// consume `ContactDetailEngine`'s `ScreenModel` directly — they have
// native ContactDetail views consuming `MobileContact`) can call into the
// same canonical predicates via the typed action/badge/banner enums on
// `vauchi-platform`. Closes the iOS/Android divergence on which trust
// transitions are user-actionable (audit V4 — both frontends used
// different predicates for the same Verify button).

/// Whether the "Verify Contact" affordance should be offered.
///
/// Canonical predicate (chosen during the G4 design spike — see plan §3
/// and Risk R5): the contact must not yet be manually verified AND the
/// trust level must be one where verification adds meaningful information
/// (`Standard | High`).
///
/// - `Cautious` (recovered identity) is excluded — re-exchange is the
///   correct response, not verification.
/// - `Verified` is excluded — already verified.
///
/// iOS today gates on `!isVerified` alone; Android on
/// `trustLevel ∈ {Standard, High}` alone. The intersection here resolves
/// the divergence: a contact must satisfy both for Verify to appear.
/// One frontend's behavior changes when this lands (see Risk R5).
pub fn verify_button_visible(is_verified: bool, trust_level: TrustLevel) -> bool {
    !is_verified && matches!(trust_level, TrustLevel::Standard | TrustLevel::High)
}

/// Whether the "Verified" badge should be shown next to the contact name.
pub fn show_verified_badge(is_verified: bool) -> bool {
    is_verified
}

/// Whether the "Recovery Trusted" indicator should be shown.
pub fn show_recovery_trusted_indicator(is_recovery_trusted: bool) -> bool {
    is_recovery_trusted
}

/// Banner state for non-confirmed reciprocity.
///
/// Returns `None` for `Confirmed` and `Unknown` — those states do not
/// surface a banner. Pre-feature contacts (`Unknown`) intentionally
/// render no chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReciprocityBannerKind {
    /// Awaiting async confirmation from the other side.
    Pending,
    /// Confirmation window expired without reciprocation.
    Unreciprocated,
}

/// Returns the reciprocity banner kind to display, or `None` if no
/// banner should appear for this state.
pub fn reciprocity_banner(reciprocity: Reciprocity) -> Option<ReciprocityBannerKind> {
    match reciprocity {
        Reciprocity::Pending => Some(ReciprocityBannerKind::Pending),
        Reciprocity::Unreciprocated => Some(ReciprocityBannerKind::Unreciprocated),
        _ => None,
    }
}

/// A contact's recorded exchange place (ADR-051), reduced to what the
/// renderer needs. `name` is the linked named place, or `None` when the
/// location is recorded but unnamed. The engine holds `Option<ContactPlace>`;
/// `None` there means no location was recorded at all.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContactPlace {
    pub name: Option<String>,
}

/// Build the contact-detail exchange-place affordances: a label for where
/// the exchange happened, a name input (autocomplete-or-create against the
/// named-place vocabulary), the suggestion list while typing, and a clear
/// action. Returns empty when no location was recorded.
///
/// Pure presentation (Wire Humble): the engine owns the data, the AppEngine
/// intercept owns persistence. Rows commit via `name_place:<name>` (→
/// `Vauchi::name_exchange_place`) and `clear_exchange_place` (→
/// `Vauchi::clear_exchange_location`).
pub fn place_components(
    place: &Option<ContactPlace>,
    query: &str,
    suggestions: &[String],
    locale: Locale,
) -> Vec<Component> {
    let t = |key: &str| crate::i18n::get_string(locale, key);
    let Some(place) = place else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(4);

    let label = match &place.name {
        Some(name) => format!("Met at {name}"),
        None => "Exchange location recorded".to_string(),
    };
    out.push(Component::Text {
        a11y: None,
        id: "exchange_place_label".into(),
        content: label,
        style: TextStyle::Subtitle,
    });

    out.push(Component::TextInput {
        id: "name_place".into(),
        label: t("contact_detail.place_name_label"),
        value: query.to_string(),
        placeholder: Some(t("contact_detail.place_name_placeholder")),
        max_length: None,
        validation_error: None,
        input_type: InputType::Text,
        a11y: Some(A11y {
            label: Some(t("contact_detail.place_name_a11y_label")),
            hint: None,
            role: Some(AccessibilityRole::TextField),
        }),
        info_key: None,
    });

    if !query.is_empty() {
        out.push(Component::ActionList {
            id: "place_suggestions".into(),
            items: suggestions
                .iter()
                .map(|name| ActionListItem {
                    id: format!("name_place:{name}"),
                    label: name.clone(),
                    icon: None,
                    detail: None,
                    a11y: Some(A11y {
                        label: Some(format!("Use place {name}")),
                        hint: None,
                        role: None,
                    }),
                    info_key: None,
                })
                .collect(),
        });
    }

    out.push(Component::ActionList {
        id: "place_actions".into(),
        items: vec![ActionListItem {
            id: "clear_exchange_place".into(),
            label: t("contact_detail.remove_location_label"),
            icon: None,
            detail: None,
            a11y: Some(A11y {
                label: Some(t("contact_detail.remove_location_a11y_label")),
                hint: None,
                role: None,
            }),
            info_key: None,
        }],
    });

    out
}

// INLINE_TEST_REQUIRED: G4 visibility-flag helpers are pure functions whose
// invariants must stay co-located with the helpers — future TrustLevel /
// Reciprocity variants must surface here first via exhaustive-match drift.
#[cfg(test)]
mod g4_visibility_helpers_tests {
    use super::*;

    // @internal
    #[test]
    fn verify_button_visible_unverified_standard_yes() {
        assert!(verify_button_visible(false, TrustLevel::Standard));
    }

    // @internal
    #[test]
    fn verify_button_visible_unverified_high_yes() {
        assert!(verify_button_visible(false, TrustLevel::High));
    }

    // @internal
    #[test]
    fn verify_button_visible_already_verified_no() {
        assert!(!verify_button_visible(true, TrustLevel::Standard));
        assert!(!verify_button_visible(true, TrustLevel::High));
    }

    // @internal
    #[test]
    fn verify_button_visible_cautious_excluded_re_exchange_instead() {
        assert!(
            !verify_button_visible(false, TrustLevel::Cautious),
            "Cautious-trust contacts should not offer Verify — re-exchange instead"
        );
    }

    // @internal
    #[test]
    fn verify_button_visible_verified_trust_level_redundant() {
        assert!(
            !verify_button_visible(false, TrustLevel::Verified),
            "TrustLevel::Verified already implies is_verified would be true; defensive check"
        );
    }

    // @internal
    #[test]
    fn verify_button_xor_show_verified_badge_when_consistent() {
        // When is_verified is consistent with trust_level (the normal case),
        // show_verified_badge XOR verify_button_visible holds for the
        // levels where verify is offered (Standard, High).
        for trust in [TrustLevel::Standard, TrustLevel::High] {
            for is_verified in [false, true] {
                let badge = show_verified_badge(is_verified);
                let button = verify_button_visible(is_verified, trust);
                assert!(
                    badge ^ button,
                    "expected XOR for is_verified={is_verified}, trust={trust:?}"
                );
            }
        }
    }

    // @internal
    #[test]
    fn show_recovery_trusted_indicator_passes_through() {
        assert!(show_recovery_trusted_indicator(true));
        assert!(!show_recovery_trusted_indicator(false));
    }

    // @internal
    #[test]
    fn reciprocity_banner_pending_and_unreciprocated_only() {
        assert_eq!(
            reciprocity_banner(Reciprocity::Pending),
            Some(ReciprocityBannerKind::Pending)
        );
        assert_eq!(
            reciprocity_banner(Reciprocity::Unreciprocated),
            Some(ReciprocityBannerKind::Unreciprocated)
        );
        assert_eq!(reciprocity_banner(Reciprocity::Confirmed), None);
        assert_eq!(reciprocity_banner(Reciprocity::Unknown), None);
    }
}

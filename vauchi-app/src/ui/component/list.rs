// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! List-shape wire types: items shown in `Component::ContactList`.
//!
//! Phase-0 prep for the Wire Humble Tier 0 rename
//! (`2026-05-03-coreui-wire-humble-types`). The types in this file
//! are scheduled to become UI-shaped at the wire boundary —
//! `ContactItem → Item`, `Component::ContactList → Component::List`.
//! Engine-side typing stays Rust-internal; `T` does not cross serde.

use serde::{Deserialize, Serialize};

use super::A11y;

/// A lightweight contact summary for list display.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactItem {
    pub id: String,
    pub name: String,
    pub subtitle: Option<String>,
    pub avatar_initials: String,
    pub status: Option<String>,
    /// Field values available for search (phone numbers, emails, etc.).
    /// Not displayed directly — used by ContactListEngine for full-text search.
    #[serde(default)]
    pub searchable_fields: Vec<String>,
    /// Declarative per-row actions (swipe/long-press/context-menu on mobile,
    /// overflow menu on desktop). Empty = no per-row actions. The engine
    /// that produced this item chooses which actions make sense given the
    /// contact's state (e.g. `Unhide` only on hidden contacts).
    #[serde(default)]
    pub actions: Vec<ListItemAction>,
    #[serde(default)]
    pub a11y: Option<A11y>,
}

/// A per-row action on a list component. Rendered as swipe on iOS/Android,
/// as overflow menu on desktop. Sent back via
/// [`crate::ui::UserAction::ListItemAction`] when invoked.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItemAction {
    /// Stable identifier echoed back in [`crate::ui::UserAction::ListItemAction`].
    pub id: String,
    /// Localized label. Frontends may prefer the `kind`-implied icon +
    /// localized string keyed on `kind` for swipe UX.
    pub label: String,
    /// Semantic hint driving icon choice and confirmation affordances.
    pub kind: ListItemActionKind,
    /// True for permanently-destructive ops that must route through an
    /// `InlineConfirm` per ADR-022. Reversible ops (Archive, Hide,
    /// soft-Delete) should leave this `false` and rely on toast+undo.
    #[serde(default)]
    pub destructive: bool,
}

/// Semantic classification of a [`ListItemAction`]. Frontends map this to
/// the appropriate icon and optional confirmation flow.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ListItemActionKind {
    /// Reversible — move contact to the archive (exchanged contacts).
    Archive,
    Unarchive,
    /// Reversible — hide contact from the main list.
    Hide,
    Unhide,
    /// Reversible via soft-delete (imported contacts only).
    Delete,
    Undelete,
    /// Escape hatch for new kinds before they get a dedicated variant.
    Custom,
}

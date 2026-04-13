// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Display preference types for contact nickname and avatar resolution.
//!
//! Controls how a contact's name and avatar are displayed: from the default
//! card, a specific group variant, or a local custom nickname/avatar.

use serde::{Deserialize, Serialize};

/// How the user wants a contact's name or avatar displayed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayPreference {
    /// Use the contact's default card name/avatar — follows updates.
    #[default]
    CardDefault,
    /// Use the name/avatar from a specific group variant — follows updates.
    CardVariant { source_label: String },
    /// Use the local nickname / custom avatar — sticky.
    Custom,
}

/// A name+avatar variant from a specific visibility group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameVariant {
    pub source_label: String,
    pub name: String,
    pub has_avatar: bool,
    pub updated_at: u64,
}

/// All display options for a contact, returned by `get_contact_display_options`.
#[derive(Debug, Clone)]
pub struct ContactDisplayOptions {
    pub names: Vec<NameOption>,
    pub avatars: Vec<AvatarOption>,
    pub active_name_preference: DisplayPreference,
    pub active_avatar_preference: DisplayPreference,
}

/// One name choice in the display options list.
#[derive(Debug, Clone)]
pub struct NameOption {
    pub source: DisplayPreference,
    pub name: String,
    pub label: String,
}

/// One avatar choice in the display options list.
#[derive(Debug, Clone)]
pub struct AvatarOption {
    pub source: DisplayPreference,
    pub has_data: bool,
    pub label: String,
}

/// Resolves the display name given preferences, variants, and nickname.
pub fn resolve_display_name(
    default_name: &str,
    preference: &DisplayPreference,
    variants: &[NameVariant],
    nickname: Option<&str>,
) -> String {
    match preference {
        DisplayPreference::CardDefault => default_name.to_string(),
        DisplayPreference::CardVariant { source_label } => variants
            .iter()
            .find(|v| v.source_label == *source_label)
            .map(|v| v.name.clone())
            .unwrap_or_else(|| default_name.to_string()),
        DisplayPreference::Custom => nickname
            .map(|n| n.to_string())
            .unwrap_or_else(|| default_name.to_string()),
    }
}

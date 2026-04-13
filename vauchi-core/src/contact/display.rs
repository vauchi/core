// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Display preference types for contact nickname and avatar resolution.
//!
//! Names and avatars arrive as flat sets (no group metadata). The user
//! picks one shared name/avatar or a local custom nickname/avatar.

use serde::{Deserialize, Serialize};

/// How the user wants a contact's display name resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayNamePreference {
    /// Use the contact's primary shared name — follows updates.
    #[default]
    Primary,
    /// Use a specific shared name — follows updates, falls back to Primary if removed.
    SharedName { name: String },
    /// Use the local nickname — sticky.
    Custom,
}

/// How the user wants a contact's avatar resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvatarPreference {
    /// Use the contact's primary shared avatar — follows updates.
    #[default]
    Primary,
    /// Use a specific shared avatar by hash — follows updates, falls back to Primary if removed.
    SharedAvatar { hash: String },
    /// Use the local custom avatar — sticky.
    Custom,
}

/// A name from the flat set shared by a contact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedName {
    pub name: String,
    pub is_primary: bool,
    pub updated_at: u64,
}

/// A shared avatar reference (data fetched separately).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedAvatar {
    pub avatar_hash: String,
    pub is_primary: bool,
    pub updated_at: u64,
}

/// All display options for a contact, returned by `get_contact_display_options`.
#[derive(Debug, Clone)]
pub struct ContactDisplayOptions {
    pub names: Vec<NameOption>,
    pub avatars: Vec<AvatarOption>,
    pub active_name_preference: DisplayNamePreference,
    pub active_avatar_preference: AvatarPreference,
}

/// One name choice in the display options list.
#[derive(Debug, Clone)]
pub struct NameOption {
    pub source: DisplayNamePreference,
    pub name: String,
    pub is_primary: bool,
}

/// One avatar choice in the display options list.
#[derive(Debug, Clone)]
pub struct AvatarOption {
    pub source: AvatarPreference,
    pub has_data: bool,
    pub is_primary: bool,
}

/// Resolves the display name given preferences, shared names, and nickname.
///
/// Fallback chain: selected → primary shared name → contact.display_name.
pub fn resolve_display_name(
    default_name: &str,
    preference: &DisplayNamePreference,
    shared_names: &[SharedName],
    nickname: Option<&str>,
) -> String {
    let primary = || {
        shared_names
            .iter()
            .find(|n| n.is_primary)
            .map(|n| n.name.clone())
            .unwrap_or_else(|| default_name.to_string())
    };

    match preference {
        DisplayNamePreference::Primary => primary(),
        DisplayNamePreference::SharedName { name } => {
            if shared_names.iter().any(|n| n.name == *name) {
                name.clone()
            } else {
                primary()
            }
        }
        DisplayNamePreference::Custom => nickname.map(|n| n.to_string()).unwrap_or_else(primary),
    }
}

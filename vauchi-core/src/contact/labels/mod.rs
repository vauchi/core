// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact Groups
//!
//! Groups allow organizing contacts for easier visibility management.
//! Groups are local-only - they are never transmitted to contacts, only synced
//! across your own devices.

mod group;
mod manager;

pub use group::Group;
pub use manager::{GroupManager, resolve_visible_fields};

/// Maximum number of labels allowed per user.
pub const MAX_LABELS: usize = 50;

/// Suggested default labels for new users.
pub const SUGGESTED_LABELS: &[&str] = &["Family", "Friends", "Coworkers", "Business"];

/// Error type for group operations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GroupError {
    /// Group with this name already exists.
    DuplicateName(String),
    /// Group not found.
    NotFound(String),
    /// Maximum number of groups reached.
    MaxLabelsReached,
    /// Invalid group name.
    InvalidName(String),
}

impl std::fmt::Display for GroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GroupError::DuplicateName(name) => write!(f, "Group already exists: {}", name),
            GroupError::NotFound(name) => write!(f, "Group not found: {}", name),
            GroupError::MaxLabelsReached => {
                write!(f, "Maximum number of groups reached ({})", MAX_LABELS)
            }
            GroupError::InvalidName(msg) => write!(f, "Invalid group name: {}", msg),
        }
    }
}

impl std::error::Error for GroupError {}

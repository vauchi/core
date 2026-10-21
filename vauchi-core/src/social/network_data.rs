// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Network data as cdn.vauchi.app publishes it, validated whole or not at
//! all before it reaches the registry (vauchi/private#734 D1, D3; DC-01).

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::registry::SocialNetwork;

const MAX_FILE_BYTES: usize = 64 * 1024;
const MAX_ENTRIES: usize = 200;
const MAX_KEY_CHARS: usize = 32;
const MAX_ALIASES: usize = 8;
const MAX_NAME_CHARS: usize = 64;
const MAX_URL_BYTES: usize = 256;
const MAX_USERNAME_LEN: u16 = 256;
const USERNAME_PLACEHOLDER: &str = "{username}";

/// The characters a network accepts in a username. A fixed set Core
/// implements: the CDN names one, it never sends a pattern, so a hostile
/// file cannot ship a catastrophic regex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsernameCharset {
    Any,
    AlnumUnderscore,
    /// Letters, digits and hyphens, with no leading, trailing or doubled
    /// hyphen (GitHub's rule).
    AlnumHyphenInner,
}

/// A network's username rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsernameRule {
    max_len: u16,
    charset: UsernameCharset,
    strip_at: bool,
}

impl UsernameRule {
    pub fn max_len(&self) -> u16 {
        self.max_len
    }

    pub fn charset(&self) -> UsernameCharset {
        self.charset
    }

    /// Whether a leading `@` is the user's habit rather than part of the
    /// name, so it is dropped before checking and linking.
    pub fn strip_at(&self) -> bool {
        self.strip_at
    }

    pub fn accepts(&self, username: &str) -> bool {
        let username = match username.strip_prefix('@') {
            Some(rest) if self.strip_at => rest,
            _ => username,
        };
        let len = username.chars().count();
        len > 0 && len <= usize::from(self.max_len) && self.charset.accepts(username)
    }
}

impl UsernameCharset {
    fn accepts(self, username: &str) -> bool {
        match self {
            Self::Any => true,
            Self::AlnumUnderscore => username
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_'),
            Self::AlnumHyphenInner => {
                username
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
                    && !username.starts_with('-')
                    && !username.ends_with('-')
                    && !username.contains("--")
            }
        }
    }
}

/// Why network data was rejected. Carries no input, so it is safe to log
/// (DC-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NetworkDataError {
    #[error("network data exceeds the size limit")]
    TooLarge,
    #[error("network data is not a list of network entries")]
    Malformed,
    #[error("network data has no entries or too many")]
    TooManyEntries,
    #[error("a network id is invalid")]
    InvalidId,
    #[error("a network alias is invalid")]
    InvalidAlias,
    #[error("a network id or alias is claimed twice")]
    DuplicateKey,
    #[error("a network name is invalid")]
    InvalidName,
    #[error("a network profile URL template is invalid")]
    InvalidUrl,
    #[error("a network username rule is invalid")]
    InvalidUsernameRule,
}

#[derive(Deserialize)]
struct NetworkEntry {
    id: String,
    name: String,
    url: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    username: Option<UsernameRule>,
}

pub(super) fn parse_networks(bytes: &[u8]) -> Result<Vec<SocialNetwork>, NetworkDataError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(NetworkDataError::TooLarge);
    }
    let entries: Vec<NetworkEntry> =
        serde_json::from_slice(bytes).map_err(|_| NetworkDataError::Malformed)?;
    if entries.is_empty() || entries.len() > MAX_ENTRIES {
        return Err(NetworkDataError::TooManyEntries);
    }
    let mut claimed = HashSet::new();
    entries
        .into_iter()
        .map(|entry| {
            validate_entry(&entry)?;
            for key in std::iter::once(&entry.id).chain(&entry.aliases) {
                if !claimed.insert(key.clone()) {
                    return Err(NetworkDataError::DuplicateKey);
                }
            }
            Ok(SocialNetwork::from_data(
                entry.id,
                entry.name,
                entry.url,
                entry.aliases,
                entry.username,
            ))
        })
        .collect()
}

fn validate_entry(entry: &NetworkEntry) -> Result<(), NetworkDataError> {
    if !is_valid_key(&entry.id) {
        return Err(NetworkDataError::InvalidId);
    }
    if entry.aliases.len() > MAX_ALIASES || !entry.aliases.iter().all(|a| is_valid_key(a)) {
        return Err(NetworkDataError::InvalidAlias);
    }
    let name_chars = entry.name.chars().count();
    if name_chars == 0 || name_chars > MAX_NAME_CHARS || entry.name.chars().any(char::is_control) {
        return Err(NetworkDataError::InvalidName);
    }
    if !is_valid_url_template(&entry.url) {
        return Err(NetworkDataError::InvalidUrl);
    }
    if entry
        .username
        .is_some_and(|rule| rule.max_len == 0 || rule.max_len > MAX_USERNAME_LEN)
    {
        return Err(NetworkDataError::InvalidUsernameRule);
    }
    Ok(())
}

fn is_valid_key(key: &str) -> bool {
    (1..=MAX_KEY_CHARS).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `https://`, one `{username}`, and a plain ASCII host: no userinfo, no
/// port, so the host a link opens is the host it shows (#734 D9).
/// `{username}` may stand as the host's leftmost label
/// (`https://{username}.substack.com`).
fn is_valid_url_template(url: &str) -> bool {
    if url.len() > MAX_URL_BYTES || url.matches(USERNAME_PLACEHOLDER).count() != 1 {
        return false;
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..host_end];
    let fixed_host = host
        .strip_prefix(USERNAME_PLACEHOLDER)
        .map_or(Some(host), |suffix| suffix.strip_prefix('.'));
    fixed_host.is_some_and(is_valid_host)
}

fn is_valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.split('.').all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

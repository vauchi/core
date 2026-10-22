// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Which locale files a cycle fetches and keeps: only the languages a person
//! uses — English, the picked one, the system one (vauchi/private#733 D7).
//! `None` for the wanted set means every listed file, as before D7.

use super::cache::ContentCache;
use super::types::{ContentType, FileEntry, LocalesEntry};

fn is_wanted(wanted: Option<&[String]>, code: &str) -> bool {
    wanted.is_none_or(|wanted| wanted.iter().any(|w| w == code))
}

fn filename(code: &str) -> String {
    format!("{code}.json")
}

/// A wanted, listed language has no cached file, so a cycle must fetch it
/// even inside the check interval. Decided locally, without a request.
pub(super) fn misses_a_wanted_file(
    cache: &ContentCache,
    wanted: Option<&[String]>,
    entry: &LocalesEntry,
) -> bool {
    let Some(wanted) = wanted else {
        return false;
    };
    let cached = cache.content_files(ContentType::Locales);
    entry
        .files
        .keys()
        .filter(|code| is_wanted(Some(wanted), code))
        .any(|code| !cached.contains(&filename(code)))
}

/// The files to fetch: every wanted one when the locales version changed,
/// otherwise only the wanted ones not cached yet.
pub(super) fn files_to_fetch<'a>(
    cache: &ContentCache,
    wanted: Option<&[String]>,
    entry: &'a LocalesEntry,
    version_changed: bool,
) -> Vec<(&'a String, &'a FileEntry)> {
    let cached = cache.content_files(ContentType::Locales);
    entry
        .files
        .iter()
        .filter(|(code, _)| is_wanted(wanted, code))
        .filter(|(code, _)| version_changed || !cached.contains(&filename(code)))
        .collect()
}

/// Deletes cached locale files of languages no longer wanted.
pub(super) fn prune_unwanted(cache: &ContentCache, wanted: Option<&[String]>) {
    if wanted.is_none() {
        return;
    }
    for name in cache.content_files(ContentType::Locales) {
        let wanted_file = name
            .strip_suffix(".json")
            .is_some_and(|code| is_wanted(wanted, code));
        if !wanted_file && let Err(e) = cache.remove_content(ContentType::Locales, &name) {
            tracing::warn!("[Content] Failed to remove an unwanted locale: {e}");
        }
    }
}

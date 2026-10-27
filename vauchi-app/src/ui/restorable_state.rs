// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The language and theme pick a shell keeps for Core outside the vault,
//! as an opaque value (vauchi/private#814). The shell hands it back from
//! its own storage, so it is untrusted on the way in: anything over the
//! size bound, of another version, with unknown fields or with a value
//! Core would not offer is ignored whole.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vauchi_core::settings::AppearancePick;

const VERSION: u8 = 1;
const MAX_STATE_BYTES: usize = 512;
const MAX_THEME_ID_CHARS: usize = 64;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestorableState {
    v: u8,
    locale: Option<String>,
    theme_id: Option<String>,
}

/// Gated with its callers in `ui::app_engine`, which the
/// no-default-features build compiles out.
#[cfg(feature = "network-rustls")]
pub(crate) fn encode(pick: &AppearancePick) -> String {
    serde_json::to_string(&RestorableState {
        v: VERSION,
        locale: pick.locale.clone(),
        theme_id: pick.theme_id.clone(),
    })
    .expect("a struct of strings serializes")
}

pub(crate) fn decode(state: &str) -> Option<AppearancePick> {
    if state.len() > MAX_STATE_BYTES {
        return None;
    }
    let state: RestorableState = serde_json::from_str(state).ok()?;
    let locale_ok = state
        .locale
        .as_deref()
        .is_none_or(|code| crate::i18n::translated_locale(code).is_some());
    let theme_ok = state.theme_id.as_deref().is_none_or(is_theme_id);
    (state.v == VERSION && locale_ok && theme_ok).then_some(AppearancePick {
        locale: state.locale,
        theme_id: state.theme_id,
    })
}

fn is_theme_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_THEME_ID_CHARS
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

const FILE_NAME: &str = "restorable_state";
/// Far above `MAX_STATE_BYTES`; it only stops a damaged or planted file
/// from being read whole. `decode` applies the real bound.
const MAX_FILE_BYTES: u64 = 4096;

/// The pick kept in a data directory, outside the vault, so it applies
/// before storage opens and no shell stores it (vauchi/private#814).
#[derive(Debug, Clone)]
pub(crate) struct KeptStateFile(PathBuf);

impl KeptStateFile {
    pub(crate) fn in_dir(data_dir: &Path) -> Self {
        Self(data_dir.join(FILE_NAME))
    }

    pub(crate) fn read(&self) -> Option<String> {
        let file = std::fs::File::open(&self.0).ok()?;
        if file.metadata().ok()?.len() > MAX_FILE_BYTES {
            return None;
        }
        let mut state = String::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_string(&mut state)
            .ok()?;
        Some(state)
    }

    /// Gated like `encode`: its caller in `ui::app_engine` is compiled out
    /// of the no-default-features build.
    #[cfg(feature = "network-rustls")]
    pub(crate) fn write(&self, state: &str) -> std::io::Result<()> {
        let partial = self.0.with_extension("partial");
        std::fs::write(&partial, state)?;
        // A rename replaces the kept value whole, so a crash mid-write leaves
        // the previous one.
        std::fs::rename(partial, &self.0)
    }
}

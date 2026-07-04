// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Frontend-pushed render context — Category-1 settings per
//! [ADR-047](../../../../../_private/docs/decisions/adr-047-settings-storage-by-sensitivity.md).
//!
//! Frontends own the canonical copy of theme + locale preferences
//! (in OS-native sandboxed storage: `SharedPreferences`,
//! `UserDefaults`, etc.) and push the active values to core via the
//! humble allowlist member `set_render_context_json`. Core uses the
//! pushed values when rendering `ScreenModel` components whose
//! presentation depends on them (Settings dropdown `selected`
//! values today; future locale-keyed string rendering after S3 of
//! the implementation plan).
//!
//! Wire shape: the JSON the PAE shim accepts has the same field
//! names as this struct (snake_case `locale`, `theme_id`). Serde
//! derives live here so the PAE method can deserialize directly,
//! matching the `DeviceCapabilities` pattern (`set_device_capabilities_json`
//! deserialises straight into the core type). Field names are
//! UI-shaped — no domain words — preserving the humble-allowlist
//! invariant (`locale`, `theme_id` are not retired wire keys per
//! `wire_humble_keys_tests.rs`).

/// Active render context pushed from the frontend.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RenderContext {
    /// Active locale code (e.g. `"de"`, `"fr"`). `None` means
    /// "frontend has not pushed a value yet" — fall back to the
    /// platform default during the migration window (S3); after S6
    /// this becomes a hard error (the frontend must push at boot).
    pub locale: Option<String>,
    /// Active theme id (e.g. `"cyber"`, `"classic"`). Same `None`
    /// semantics as `locale`.
    pub theme_id: Option<String>,
}

impl RenderContext {
    /// The pushed locale as a [`crate::i18n::Locale`], English when the
    /// frontend has not pushed one (or pushed an unknown code) — the
    /// resolution every locale-threaded engine factory arm uses.
    pub fn resolved_locale(&self) -> crate::i18n::Locale {
        self.locale
            .as_deref()
            .and_then(crate::i18n::Locale::from_code)
            .unwrap_or_default()
    }
}

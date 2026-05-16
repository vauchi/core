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
//! No serde here — the wire shape is JSON deserialized by the PAE
//! shim into this struct's fields, but the struct itself is
//! crate-internal (`pub` only so the test layer can construct it).

/// Active render context pushed from the frontend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

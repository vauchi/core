// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
    /// The language the user picked (e.g. `"de"`, `"fr"`). `None` is
    /// "Follow system" (ADR-047 absence-is-follow-system): the Settings
    /// dropdown selects `follow_system` from it.
    pub locale: Option<String>,
    /// The OS language tag as the shell reads it (`"fr-CH"`,
    /// `"de_DE.UTF-8"`), unparsed; Core decides what it maps to
    /// (vauchi/private#721).
    #[serde(default)]
    pub system_locale: Option<String>,
    /// Active theme id (e.g. `"cyber"`, `"classic"`). Same `None`
    /// semantics as `locale`.
    pub theme_id: Option<String>,
    /// The OS light or dark setting. Skipped on the wire: shells report it
    /// as a presentation environment observation, not in this push
    /// (vauchi/private#814).
    #[serde(skip)]
    pub system_color_scheme: Option<vauchi_core::ColorScheme>,
    /// The value Core asked the shell to keep with
    /// `Command::SaveRestorableState`, handed back unchanged. Its pick
    /// replaces `locale` and `theme_id` (vauchi/private#814).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restorable_state: Option<String>,
}

impl RenderContext {
    /// A language or theme picked in Core's Settings and kept in the vault
    /// wins over what an app pushes (vauchi/private#790); the pick applied,
    /// if any. A shell that keeps the pick itself makes this copy redundant
    /// (vauchi/private#814). Before the first pick the app's values stand.
    #[cfg(feature = "network-rustls")]
    pub(crate) fn apply_saved_appearance(
        &mut self,
        vauchi: &vauchi_core::api::Vauchi,
    ) -> Option<vauchi_core::settings::AppearancePick> {
        let pick = vauchi
            .load_settings_flags()
            .ok()
            .and_then(|flags| flags.appearance)?;
        self.locale = pick.locale.clone();
        self.theme_id = pick.theme_id.clone();
        Some(pick)
    }

    /// Replaces the shell's own choice with the pick from the value it
    /// keeps for Core. Whether the shell handed back a value Core could
    /// read, which proves it keeps them; a damaged one changes nothing.
    pub fn apply_restorable_state(&mut self) -> bool {
        let Some(pick) = self
            .restorable_state
            .as_deref()
            .and_then(super::restorable_state::decode)
        else {
            return false;
        };
        self.locale = pick.locale;
        self.theme_id = pick.theme_id;
        true
    }

    /// The theme to paint: the pick, else the default for the OS light or
    /// dark setting.
    pub fn theme(&self) -> crate::theme::Theme {
        crate::theme::resolve_theme(self.theme_id.as_deref(), self.system_color_scheme)
    }

    /// The picked language if Core has its strings, else the system
    /// language if Core has its strings, else English — the resolution
    /// every locale-threaded engine factory arm uses.
    pub fn resolved_locale(&self) -> crate::i18n::Locale {
        [self.locale.as_deref(), self.system_locale.as_deref()]
            .into_iter()
            .flatten()
            .find_map(crate::i18n::translated_locale)
            .unwrap_or_default()
    }

    /// The languages a content cycle fetches and keeps: English, the picked
    /// language and the system language, by code (vauchi/private#733 D7).
    pub fn wanted_locale_codes(&self) -> Vec<String> {
        let mut codes = vec![crate::i18n::Locale::ENGLISH.code().to_string()];
        for tag in [self.locale.as_deref(), self.system_locale.as_deref()]
            .into_iter()
            .flatten()
        {
            if let Some(locale) = crate::i18n::Locale::from_code(tag)
                && !codes.iter().any(|code| code == locale.code())
            {
                codes.push(locale.code().to_string());
            }
        }
        codes
    }
}

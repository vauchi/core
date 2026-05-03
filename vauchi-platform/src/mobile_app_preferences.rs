// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI surface for the singleton `app_preferences` row (theme + language).
//!
//! The Settings screen Theme + Language `Component::Dropdown`s persist
//! through `AppEngine::persist_settings_toggle`, which calls
//! `Vauchi::set_app_preferences`. Frontends use these wrappers to
//! source the same row when applying a theme or selecting a locale —
//! making the inline Settings dropdown the single source of truth and
//! retiring the bespoke `ThemeSettingsScreen` / `LanguageSettingsScreen`
//! pickers (Phase 2a/A3a of
//! `2026-05-01-android-humble-ui-deep-retirement`).

use super::VauchiPlatform;
use super::error::MobileError;
use super::types::MobileAppPreferences;

#[uniffi::export]
impl VauchiPlatform {
    /// Loads app preferences (theme + language). Returns the default
    /// (`follow_system_*` both `true`, both ids `None`) if no row has
    /// been written yet. Storage-only — no identity required.
    pub fn app_preferences(&self) -> Result<MobileAppPreferences, MobileError> {
        let storage = self.open_storage()?;
        let prefs = storage
            .load_app_preferences()
            .map_err(|e| MobileError::StorageError {
                detail: e.to_string(),
            })?;
        Ok(prefs.into())
    }

    /// Saves app preferences (theme + language) to the singleton row.
    /// Storage-only — no identity required (the Settings screen is
    /// reachable from More before onboarding).
    pub fn set_app_preferences(&self, prefs: MobileAppPreferences) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage
            .save_app_preferences(&prefs.into())
            .map_err(|e| MobileError::StorageError {
                detail: e.to_string(),
            })
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! App preferences storage (theme + language).
//!
//! CRUD operations for the `app_preferences` table (migration V46).
//! Singleton (id = 1) row holding the user's theme + language picks.
//! Device-local (preferences do not sync across devices).

use rusqlite::params;

use super::{Storage, StorageError};
use crate::types::AppPreferences;

impl Storage {
    /// Saves app preferences (theme + language).
    ///
    /// Idempotent — uses INSERT OR REPLACE on the singleton row.
    pub fn save_app_preferences(&self, prefs: &AppPreferences) -> Result<(), StorageError> {
        let now = super::now_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO app_preferences \
                 (id, theme_id, language_code, follow_system_theme, follow_system_language, updated_at) \
                 VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            params![
                prefs.theme_id,
                prefs.language_code,
                prefs.follow_system_theme as i32,
                prefs.follow_system_language as i32,
                now,
            ],
        )?;

        Ok(())
    }

    /// Loads app preferences. Returns the default (`follow_system_*`
    /// both `true`, both ids `None`) if no row has been written yet.
    pub fn load_app_preferences(&self) -> Result<AppPreferences, StorageError> {
        let result = self.conn.query_row(
            "SELECT theme_id, language_code, follow_system_theme, follow_system_language \
                 FROM app_preferences WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, i32>(3)?,
                ))
            },
        );

        match result {
            Ok((theme_id, language_code, follow_system_theme, follow_system_language)) => {
                Ok(AppPreferences {
                    theme_id,
                    language_code,
                    follow_system_theme: follow_system_theme != 0,
                    follow_system_language: follow_system_language != 0,
                })
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(AppPreferences::default()),
            Err(e) => Err(StorageError::Database(e)),
        }
    }
}

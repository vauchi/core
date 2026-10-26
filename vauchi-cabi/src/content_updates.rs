// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! CDN content for C ABI shells, which schedule nothing themselves
//! (vauchi/private#733 D10): content cached by an earlier run loads when an
//! app opens on a data directory, and once a shell registers its callback
//! Core runs the update cycle within the person's setting. An in-memory app
//! has no data directory and gets neither.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use vauchi_app::content::{
    ApplyResult, ContentConfig, ContentManager, ContentScheduler, ContentType,
};
use vauchi_app::ui::AppEngine;

use crate::VauchiApp;
use crate::app::EventCallbackHandler;

/// The first cycle waits for the app to settle.
const SETTLE: std::time::Duration = std::time::Duration::from_secs(30);

/// Loads content cached by an earlier run; call before the engine is built,
/// which reads the active social networks.
pub(crate) fn load_cached_content(data_dir: &Path) {
    if let Some(manager) = manager_over(data_dir) {
        manager.load_cached_locales_into_i18n();
        manager.load_cached_themes_into_theme_store();
        manager.load_cached_networks_into_registry();
    }
}

fn manager_over(data_dir: &Path) -> Option<ContentManager> {
    manager_wanting(data_dir, None, None)
}

fn manager_wanting(
    data_dir: &Path,
    wanted_locales: Option<Vec<String>>,
    source: Option<vauchi_app::content::ContentSourceHandle>,
) -> Option<ContentManager> {
    let config = ContentConfig {
        storage_path: data_dir.to_path_buf(),
        wanted_locales,
        source,
        ..Default::default()
    };
    ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).ok()
}

/// One cycle as the scheduler runs it, holding only what it shares with
/// the app, so a cycle in progress never reaches into a destroyed handle.
pub(crate) struct ScheduledCycle {
    engine: Arc<Mutex<Option<AppEngine>>>,
    callback: Arc<Mutex<Option<EventCallbackHandler>>>,
    data_dir: PathBuf,
}

impl ScheduledCycle {
    /// Whether the cycle applied content, or `None` when the setting kept
    /// it from running. Applied content rebuilds the screens and calls the
    /// shell's callback, outside the engine lock.
    pub(crate) fn run(&self) -> Option<bool> {
        let (wanted, source) = self.engine.lock().ok().and_then(|slot| {
            slot.as_ref().and_then(|engine| {
                engine.content_updates_allowed().then(|| {
                    (
                        engine.render_context().wanted_locale_codes(),
                        vauchi_app::content::relay_content_source(engine.vauchi()),
                    )
                })
            })
        })?;
        let manager = manager_wanting(&self.data_dir, Some(wanted), Some(source))?;
        let (_, apply) = manager.run_update_cycle_blocking();
        let (applied, themes_applied) = match &apply {
            Some(Ok(ApplyResult::Applied { applied, .. })) => {
                (!applied.is_empty(), applied.contains(&ContentType::Themes))
            }
            _ => (false, false),
        };
        if applied {
            manager.load_cached_networks_into_registry();
            if let Ok(mut slot) = self.engine.lock()
                && let Some(engine) = slot.as_mut()
            {
                engine.reload_social_networks();
                engine.reload_locales();
                if themes_applied {
                    engine.themes_updated();
                }
            }
            let callback = self.callback.lock().ok().and_then(|c| *c);
            if let Some(callback) = callback {
                callback.dispatch(&[]);
            }
        }
        Some(applied)
    }
}

impl VauchiApp {
    pub(crate) fn with_content_dir(mut self, data_dir: &Path) -> Self {
        self.content_dir = Some(data_dir.to_path_buf());
        // Core keeps the language and theme pick beside its content cache
        // (vauchi/private#814).
        if let Ok(mut engine) = self.engine.lock()
            && let Some(engine) = engine.as_mut()
        {
            engine.keep_restorable_state_in(data_dir);
        }
        self
    }

    /// The cycle Core runs for this app, if it has a data directory.
    pub(crate) fn scheduled_content_cycle(&self) -> Option<ScheduledCycle> {
        Some(ScheduledCycle {
            engine: Arc::clone(&self.engine),
            callback: Arc::clone(&self.event_callback),
            data_dir: self.content_dir.clone()?,
        })
    }

    /// Asks Core's content scheduler for a cycle now, when it runs.
    pub(crate) fn request_content_cycle(&self) {
        if let Ok(scheduler) = self.content_scheduler.lock()
            && let Some(scheduler) = scheduler.as_ref()
        {
            scheduler.request_cycle();
        }
    }

    /// Starts Core's own content cycle, once.
    pub(crate) fn start_content_scheduler(&self) {
        let Ok(mut slot) = self.content_scheduler.lock() else {
            return;
        };
        if slot.is_some() {
            return;
        }
        let Some(cycle) = self.scheduled_content_cycle() else {
            return;
        };
        *slot =
            ContentScheduler::spawn(SETTLE, ContentConfig::default().check_interval, move || {
                let _ = cycle.run();
            })
            .ok();
    }
}

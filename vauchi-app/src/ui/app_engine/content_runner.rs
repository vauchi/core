// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CDN content for shells that drive AppEngine directly (tui, linux-gtk;
//! vauchi/private#733 D10). One call loads the cached content and starts
//! Core's update cycle on a thread of its own. The thread reads a snapshot of
//! what the engine allows (setting, network, wanted languages) and never the
//! engine itself; applied content is taken up at the engine's next entry
//! point and announced as `VauchiEvent::ContentUpdated`, which shells already
//! re-render on.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use vauchi_core::api::VauchiEvent;
use vauchi_core::crypto::signing::PublicKey;

use super::AppEngine;
use crate::content::{
    ApplyResult, ContentConfig, ContentManager, ContentScheduler, ContentSourceHandle, ContentType,
};

/// How a direct-engine shell's content updates run.
pub struct ContentRunOptions {
    /// The first cycle waits for the app to settle.
    pub first_run_after: Duration,
    pub interval: Duration,
    /// Where content comes from; `None` is the relay over OHTTP (D8).
    pub source: Option<ContentSourceHandle>,
    /// The manifest signer; `None` is the production publisher key.
    pub publisher_public_key: Option<PublicKey>,
}

impl Default for ContentRunOptions {
    fn default() -> Self {
        Self {
            first_run_after: Duration::from_secs(30),
            interval: ContentConfig::default().check_interval,
            source: None,
            publisher_public_key: None,
        }
    }
}

#[derive(Clone)]
struct Snapshot {
    allowed: bool,
    wanted: Vec<String>,
}

pub(super) struct ContentRunner {
    scheduler: ContentScheduler,
    snapshot: Arc<Mutex<Snapshot>>,
    applied: Arc<AtomicBool>,
    themes_applied: Arc<AtomicBool>,
}

fn manager(
    data_dir: &Path,
    wanted: Option<Vec<String>>,
    source: Option<ContentSourceHandle>,
    publisher_public_key: Option<PublicKey>,
) -> Option<ContentManager> {
    let mut config = ContentConfig {
        storage_path: data_dir.to_path_buf(),
        wanted_locales: wanted,
        source,
        ..Default::default()
    };
    if let Some(key) = publisher_public_key {
        config.publisher_public_key = key;
    }
    ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).ok()
}

impl AppEngine {
    /// Loads content cached under `data_dir` by an earlier run and starts
    /// Core's content cycle for this engine (vauchi/private#733 D10).
    pub fn start_content_updates(&mut self, data_dir: &Path, options: ContentRunOptions) {
        if let Some(cached) = manager(data_dir, None, None, None) {
            cached.load_cached_locales_into_i18n();
            cached.load_cached_themes_into_theme_store();
            cached.load_cached_networks_into_registry();
        }
        self.reload_social_networks();

        let source = options.source.or_else(|| self.relay_content_source());
        let snapshot = Arc::new(Mutex::new(self.content_snapshot()));
        let applied = Arc::new(AtomicBool::new(false));
        let themes_applied = Arc::new(AtomicBool::new(false));
        let cycle = ScheduledCycle {
            data_dir: data_dir.to_path_buf(),
            source,
            publisher_public_key: options.publisher_public_key,
            snapshot: Arc::clone(&snapshot),
            applied: Arc::clone(&applied),
            themes_applied: Arc::clone(&themes_applied),
            events: Arc::clone(self.vauchi.events()),
        };
        let Ok(scheduler) =
            ContentScheduler::spawn(options.first_run_after, options.interval, move || {
                cycle.run()
            })
        else {
            return;
        };
        self.content_runner = Some(ContentRunner {
            scheduler,
            snapshot,
            applied,
            themes_applied,
        });
    }

    #[cfg(feature = "network-http")]
    fn relay_content_source(&self) -> Option<ContentSourceHandle> {
        Some(crate::content::relay_content_source(&self.vauchi))
    }

    #[cfg(not(feature = "network-http"))]
    fn relay_content_source(&self) -> Option<ContentSourceHandle> {
        None
    }

    fn content_snapshot(&self) -> Snapshot {
        Snapshot {
            allowed: self.content_updates_allowed(),
            wanted: self.render_context().wanted_locale_codes(),
        }
    }

    /// Keeps the runner's view of the setting, network and wanted
    /// languages current; a newly wanted language asks for a cycle now.
    pub(super) fn refresh_content_snapshot(&mut self) {
        let next = self.content_snapshot();
        let Some(runner) = &self.content_runner else {
            return;
        };
        let Ok(mut snapshot) = runner.snapshot.lock() else {
            return;
        };
        let wanted_changed = snapshot.wanted != next.wanted;
        let became_allowed = !snapshot.allowed && next.allowed;
        *snapshot = next;
        drop(snapshot);
        if wanted_changed || became_allowed {
            runner.scheduler.request_cycle();
        }
    }

    /// Takes up content a cycle applied since the last entry point.
    pub(super) fn absorb_applied_content(&mut self) {
        let Some(runner) = &self.content_runner else {
            return;
        };
        let applied = runner.applied.swap(false, Ordering::SeqCst);
        let themes_applied = runner.themes_applied.swap(false, Ordering::SeqCst);
        if applied {
            self.reload_social_networks();
            self.reload_locales();
        }
        if themes_applied {
            self.themes_updated();
        }
    }
}

struct ScheduledCycle {
    data_dir: PathBuf,
    source: Option<ContentSourceHandle>,
    publisher_public_key: Option<PublicKey>,
    snapshot: Arc<Mutex<Snapshot>>,
    applied: Arc<AtomicBool>,
    themes_applied: Arc<AtomicBool>,
    events: Arc<vauchi_core::api::EventDispatcher>,
}

impl ScheduledCycle {
    fn run(&self) {
        let Some(snapshot) = self.snapshot.lock().ok().map(|s| s.clone()) else {
            return;
        };
        if !snapshot.allowed {
            return;
        }
        let Some(manager) = manager(
            &self.data_dir,
            Some(snapshot.wanted),
            self.source.clone(),
            self.publisher_public_key.clone(),
        ) else {
            return;
        };
        let (_, apply) = manager.run_update_cycle_blocking();
        if let Some(Ok(ApplyResult::Applied { applied, .. })) = &apply
            && !applied.is_empty()
        {
            manager.load_cached_networks_into_registry();
            if applied.contains(&ContentType::Themes) {
                self.themes_applied.store(true, Ordering::SeqCst);
            }
            self.applied.store(true, Ordering::SeqCst);
            self.events.dispatch(VauchiEvent::ContentUpdated);
        }
    }
}

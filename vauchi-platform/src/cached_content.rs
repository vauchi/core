// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CDN content cached by an earlier run, put back in effect when the engine
//! starts (vauchi/private#736). The update cycle downloads nothing while the
//! cached manifest is current, so without this a CDN string or theme lasts
//! only until the process that downloaded it exits.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use vauchi_app::content::{ContentConfig, ContentManager};

static CONTENT_ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

pub(crate) fn load_at_start(data_dir: &Path) {
    if let Ok(mut root) = CONTENT_ROOT.write() {
        *root = Some(data_dir.to_path_buf());
    }
    if let Some(manager) = manager_over(data_dir) {
        manager.load_cached_locales_into_i18n();
        manager.load_cached_themes_into_theme_store();
        manager.load_cached_networks_into_registry();
    }
}

/// After a content update applied, the networks it downloaded take
/// effect without a restart (vauchi/private#734).
pub(crate) fn reload_cached_networks() {
    let root = CONTENT_ROOT.read().ok().and_then(|root| root.clone());
    if let Some(manager) = root.as_deref().and_then(manager_over) {
        manager.load_cached_networks_into_registry();
    }
}

/// `init_locales` replaces the whole string store, and shells call it before
/// or after starting the engine, so the cached overlay is applied again.
pub(crate) fn reapply_cached_locales() {
    let root = CONTENT_ROOT.read().ok().and_then(|root| root.clone());
    if let Some(manager) = root.as_deref().and_then(manager_over) {
        manager.load_cached_locales_into_i18n();
    }
}

fn manager_over(data_dir: &Path) -> Option<ContentManager> {
    let config = ContentConfig {
        storage_path: data_dir.to_path_buf(),
        ..Default::default()
    };
    ContentManager::new(config, vauchi_core::clock::SystemClock::shared()).ok()
}

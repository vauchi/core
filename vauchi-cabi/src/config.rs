// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Config builder for C ABI consumers.

use std::path::PathBuf;

use vauchi_core::api::VauchiConfig;
use vauchi_core::crypto::SymmetricKey;

/// Opaque config builder for C ABI consumers.
///
/// Built via `vauchi_config_new`, configured with `vauchi_config_set_*`
/// functions, consumed by `vauchi_app_create_from_config`, and freed
/// with `vauchi_config_free`.
pub struct CabiConfig {
    pub(crate) data_dir: PathBuf,
    pub(crate) relay_url: String,
    pub(crate) storage_key: Option<SymmetricKey>,
}

impl CabiConfig {
    pub fn new(data_dir: PathBuf, relay_url: String) -> Self {
        CabiConfig {
            data_dir,
            relay_url,
            storage_key: None,
        }
    }

    pub fn into_vauchi_config(self) -> VauchiConfig {
        // The CABI contract (per `vauchi_config_new` doc) is that
        // `data_dir` is a *directory*. `VauchiConfig::with_storage_path`
        // expects a SQLite database *file* path, so join `vauchi.db`
        // here. Matches the older `vauchi_app_create_with_config`
        // (`app.rs`) pattern and the linux-qt persistence test
        // (`tests/app_engine_test.cpp` asserts `dir / "vauchi.db"`).
        // `create_dir_all` is idempotent — the C caller may have
        // already created the dir for its own logging.
        let _ = std::fs::create_dir_all(&self.data_dir);
        let storage_path = self.data_dir.join("vauchi.db");
        let mut config =
            VauchiConfig::with_storage_path(&storage_path).with_relay_url(&self.relay_url);
        if let Some(key) = self.storage_key {
            config = config.with_storage_key(key);
        }
        config
    }
}

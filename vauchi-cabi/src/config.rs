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
    #[allow(dead_code)] // consumed by config_enable_ble (Task 3)
    pub(crate) ble_enabled: bool,
    #[allow(dead_code)] // consumed by config_enable_audio (Task 3)
    pub(crate) audio_enabled: bool,
}

impl CabiConfig {
    pub fn new(data_dir: PathBuf, relay_url: String) -> Self {
        CabiConfig {
            data_dir,
            relay_url,
            storage_key: None,
            ble_enabled: true,
            audio_enabled: true,
        }
    }

    pub fn into_vauchi_config(self) -> VauchiConfig {
        let mut config =
            VauchiConfig::with_storage_path(&self.data_dir).with_relay_url(&self.relay_url);
        if let Some(key) = self.storage_key {
            config = config.with_storage_key(key);
        }
        config
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The engine's locked start (vauchi/private#580, ADR-043 Amendment 7 §3).
//!
//! When the keychain needs the person to authenticate, or no stored key
//! opens the data, `open_with_keychain` still returns the engine. Until
//! storage opens, `initial_commands_json` and `dispatch_json` answer from
//! Core's storage-lock screens; a retry or a confirmed start-over opens
//! storage and swaps the real engine in.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{AppEngine, StorageLockPresentation, StorageLockReason, StorageLockStep};
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{Command, Event, StorageError, VauchiError};

use crate::error::MobileError;
use crate::{KeychainBridge, MobilePlatformKeychain, PlatformAppEngine};

/// Every keychain name that holds a key opening the database.
const STORAGE_KEY_NAMES: [&str; 2] = ["smk", "storage_bootstrap"];

pub(crate) struct OpenParams {
    pub(crate) storage_path: PathBuf,
    pub(crate) relay_url: String,
    pub(crate) shell_storage_key: Option<SymmetricKey>,
    pub(crate) keychain: Arc<dyn MobilePlatformKeychain>,
}

pub(crate) enum Opened {
    Engine(Box<Vauchi>),
    Locked(StorageLockReason),
}

impl OpenParams {
    pub(crate) fn open(&self) -> Result<Opened, MobileError> {
        let mut config =
            VauchiConfig::with_storage_path(&self.storage_path).with_relay_url(&self.relay_url);
        if let Some(key) = &self.shell_storage_key {
            config = config.with_storage_key(key.clone());
        }
        let bridge = Arc::new(KeychainBridge {
            callback: self.keychain.clone(),
        });
        match Vauchi::with_secure_storage(config, bridge) {
            Ok(vauchi) => Ok(Opened::Engine(Box::new(vauchi))),
            Err(VauchiError::Storage(StorageError::SecureStorageLocked)) => {
                Ok(Opened::Locked(StorageLockReason::Locked))
            }
            Err(VauchiError::Storage(
                StorageError::SecureStorageKeyInvalidated | StorageError::WrongKey,
            )) => Ok(Opened::Locked(StorageLockReason::Unreadable)),
            Err(other) => Err(other.into()),
        }
    }

    /// Deletes the database and every key that opened it. Called only after
    /// the person confirmed on Core's start-over screen.
    fn delete_unreadable_data(&self) -> Result<(), MobileError> {
        for path in database_files(&self.storage_path) {
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| MobileError::StorageError {
                    detail: e.to_string(),
                })?;
            }
        }
        for name in STORAGE_KEY_NAMES {
            self.keychain
                .delete_key(name.to_string())
                .map_err(|e| MobileError::StorageError {
                    detail: e.to_string(),
                })?;
        }
        Ok(())
    }
}

/// The database, its WAL and shared-memory files, and pre-migration backups.
fn database_files(storage_path: &Path) -> Vec<PathBuf> {
    let mut files = vec![storage_path.to_path_buf()];
    let Some(name) = storage_path.file_name().and_then(|n| n.to_str()) else {
        return files;
    };
    let Some(dir) = storage_path.parent() else {
        return files;
    };
    files.push(dir.join(format!("{name}-wal")));
    files.push(dir.join(format!("{name}-shm")));
    if let Ok(entries) = std::fs::read_dir(dir) {
        let backup_prefix = format!("{name}.pre-migration-");
        files.extend(entries.flatten().map(|entry| entry.path()).filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&backup_prefix))
        }));
    }
    files
}

pub(crate) struct LockedStart {
    presentation: StorageLockPresentation,
    params: OpenParams,
}

impl LockedStart {
    pub(crate) fn new(reason: StorageLockReason, params: OpenParams) -> Self {
        Self {
            presentation: StorageLockPresentation::new(reason, Locale::English),
            params,
        }
    }
}

impl PlatformAppEngine {
    /// The locked start's first batch, or `None` once storage is open.
    pub(crate) fn locked_initial_commands(&self) -> Result<Option<Vec<Command>>, MobileError> {
        let mut locked = self.lock_locked_start()?;
        Ok(locked
            .as_mut()
            .map(|start| start.presentation.initial_commands()))
    }

    /// Core's answer to undecodable event JSON while locked, or `None`.
    pub(crate) fn locked_reject_event_json(
        &self,
        error: &vauchi_core::EventJsonError,
    ) -> Result<Option<Vec<Command>>, MobileError> {
        let locked = self.lock_locked_start()?;
        Ok(locked
            .as_ref()
            .map(|start| start.presentation.reject_event_json(error)))
    }

    /// Reduces `event` while locked, or `None` once storage is open.
    pub(crate) fn locked_dispatch(
        &self,
        event: Event,
    ) -> Result<Option<Vec<Command>>, MobileError> {
        let mut locked = self.lock_locked_start()?;
        let Some(start) = locked.as_mut() else {
            return Ok(None);
        };
        match start.presentation.dispatch(event) {
            StorageLockStep::Commands(commands) => Ok(Some(commands)),
            StorageLockStep::RetryOpen => self.retry_open(&mut locked).map(Some),
            StorageLockStep::StartOverConfirmed => {
                start.params.delete_unreadable_data()?;
                self.retry_open(&mut locked).map(Some)
            }
        }
    }

    fn retry_open(&self, locked: &mut Option<LockedStart>) -> Result<Vec<Command>, MobileError> {
        let Some(start) = locked.as_mut() else {
            return Ok(Vec::new());
        };
        match start.params.open()? {
            Opened::Locked(reason) => Ok(start.presentation.show(reason)),
            Opened::Engine(vauchi) => {
                let mut engine = AppEngine::new(*vauchi);
                let commands = engine.initial_commands().map_err(|e| MobileError::Other {
                    detail: format!("Failed to compose initial presentation: {e}"),
                })?;
                *self.lock_engine()? = Some(engine);
                *locked = None;
                Ok(commands)
            }
        }
    }
}

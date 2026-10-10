// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! After a shred, the engine reopens on a fresh install (vauchi/private#599):
//! the shred deleted the database the old engine held, so anything created
//! through it would be written into a deleted file.

use std::sync::Arc;

use vauchi_app::ui::AppEngine;

use crate::error::MobileError;
use crate::platform_app_engine_locked::{LockedStart, OpenParams, Opened};
use crate::{PlatformAppEngine, PlatformEventListener};

/// The registered listener, shared with the direct-call slot, handed to a
/// locked start that registers it once storage opens.
struct SharedListener(Arc<Box<dyn PlatformEventListener>>);

impl PlatformEventListener for SharedListener {
    fn on_presentation_invalidated(&self) {
        self.0.on_presentation_invalidated();
    }
}

impl PlatformAppEngine {
    /// Replaces the shredded `previous` engine with one opened on a fresh
    /// install, keeping what the shell set on it. Without a keychain (the
    /// legacy constructor) there is nothing to reopen with, and `previous`
    /// stays.
    pub(crate) fn reopen_after_shred(&self, previous: AppEngine) -> Result<(), MobileError> {
        let keychain = self
            .platform_keychain
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .clone();
        let Some(keychain) = keychain else {
            *self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })? = Some(previous);
            return Ok(());
        };
        let render_context = previous.render_context().clone();
        let capabilities = previous.device_capabilities().clone();
        let online = previous.is_network_online();
        drop(previous);

        if let Some(data_dir) = self.storage_path.parent() {
            std::fs::create_dir_all(data_dir).map_err(|e| MobileError::StorageError {
                detail: e.to_string(),
            })?;
        }
        let params = OpenParams {
            storage_path: self.storage_path.clone(),
            relay_url: self.relay_url.clone(),
            shell_storage_key: None,
            release_handle: None,
            keychain,
        };
        let listener = self
            .direct_listener
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .clone();
        match params.open()? {
            Opened::Engine(vauchi) => {
                let mut engine = AppEngine::new(*vauchi);
                engine.set_render_context(render_context);
                engine.set_device_capabilities(capabilities);
                engine.set_network_online(online);
                *self.engine.lock().map_err(|e| MobileError::Other {
                    detail: format!("Lock failed: {e}"),
                })? = Some(engine);
                if let Some(listener) = listener {
                    self.attach_event_listener(listener)?;
                }
            }
            Opened::Locked(reason) => {
                let mut start = LockedStart::new(reason, params);
                start.keep_render_context(render_context);
                start.pending.capabilities = Some(capabilities);
                start.pending.network_online = Some(online);
                start.pending.listener =
                    listener.map(|l| Box::new(SharedListener(l)) as Box<dyn PlatformEventListener>);
                *self.lock_locked_start()? = Some(start);
            }
        }
        Ok(())
    }
}

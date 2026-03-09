// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builder pattern for constructing Vauchi instances.

use crate::identity::Identity;
use crate::network::Transport;

use super::super::config::VauchiConfig;
use super::super::error::VauchiResult;
use super::Vauchi;

/// Converts a decoy contact ID string into a fake 32-byte "public key".
///
/// This is a deterministic mapping used only for display purposes — decoy
/// contacts don't have real cryptographic keys. The resulting bytes are
/// derived by hashing the ID with ring's SHA-256, ensuring consistent
/// IDs across sessions.
pub(super) fn decoy_id_to_fake_pk(id: &str) -> [u8; 32] {
    use aws_lc_rs::digest;
    let hash = digest::digest(&digest::SHA256, id.as_bytes());
    let mut pk = [0u8; 32];
    pk.copy_from_slice(hash.as_ref());
    pk
}

/// Builder for creating Vauchi instances.
pub struct VauchiBuilder<T: Transport> {
    config: VauchiConfig,
    identity: Option<Identity>,
    transport_factory: Option<Box<dyn FnOnce() -> T>>,
}

impl<T: Transport> VauchiBuilder<T> {
    /// Creates a new builder with default configuration.
    pub fn new() -> Self {
        VauchiBuilder {
            config: VauchiConfig::default(),
            identity: None,
            transport_factory: None,
        }
    }

    /// Sets the configuration.
    pub fn config(mut self, config: VauchiConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the storage path.
    pub fn storage_path(mut self, path: impl Into<std::path::PathBuf>) -> Self {
        self.config.storage_path = path.into();
        self
    }

    /// Sets the relay URL.
    pub fn relay_url(mut self, url: impl Into<String>) -> Self {
        self.config.relay.server_url = url.into();
        self
    }

    /// Sets an existing identity.
    pub fn identity(mut self, identity: Identity) -> Self {
        self.identity = Some(identity);
        self
    }

    /// Sets the transport factory.
    pub fn transport<F>(mut self, factory: F) -> Self
    where
        F: FnOnce() -> T + 'static,
    {
        self.transport_factory = Some(Box::new(factory));
        self
    }

    /// Builds the Vauchi instance.
    pub fn build(self) -> VauchiResult<Vauchi<T>>
    where
        T: Default,
    {
        let factory = self
            .transport_factory
            .unwrap_or_else(|| Box::new(T::default));
        let mut wb = Vauchi::with_transport_factory(self.config, factory)?;

        if let Some(identity) = self.identity {
            wb.set_identity(identity)?;
        }

        Ok(wb)
    }
}

impl<T: Transport + Default> Default for VauchiBuilder<T> {
    fn default() -> Self {
        Self::new()
    }
}

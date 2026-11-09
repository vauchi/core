// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Builder pattern for constructing Vauchi instances.

use crate::identity::Identity;

use super::super::config::VauchiConfig;
use super::super::error::VauchiResult;
use super::Vauchi;

/// Builder for creating Vauchi instances (ADR-030: transport type-erased).
pub struct VauchiBuilder {
    config: VauchiConfig,
    identity: Option<Identity>,
}

impl VauchiBuilder {
    /// Creates a new builder with default configuration.
    pub fn new() -> Self {
        VauchiBuilder {
            config: VauchiConfig::default(),
            identity: None,
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

    /// Sets the relay URL, clearing any OHTTP trust anchor (see
    /// [`VauchiConfig::with_relay_url`]).
    pub fn relay_url(mut self, url: impl Into<String>) -> Self {
        self.config = self.config.with_relay_url(url);
        self
    }

    /// Sets an existing identity.
    pub fn identity(mut self, identity: Identity) -> Self {
        self.identity = Some(identity);
        self
    }

    /// Builds the Vauchi instance.
    pub fn build(self) -> VauchiResult<Vauchi> {
        let mut wb = Vauchi::new(self.config)?;

        if let Some(identity) = self.identity {
            wb.set_identity(identity)?;
        }

        Ok(wb)
    }
}

impl Default for VauchiBuilder {
    fn default() -> Self {
        Self::new()
    }
}

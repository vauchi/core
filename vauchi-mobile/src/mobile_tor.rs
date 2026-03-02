// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tor privacy mode operations for mobile.

use super::error::MobileError;
use super::types::MobileTorConfig;
use super::types::MobileTorStatus;
use super::VauchiMobile;

#[uniffi::export]
impl VauchiMobile {
    // === Tor Privacy Mode ===

    /// Enables Tor with the current configuration.
    ///
    /// Persists the enabled state to storage.
    /// Note: Actual Tor bootstrapping requires the `tor` feature in core.
    pub fn enable_tor(&self) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        vauchi.enable_tor().map_err(MobileError::from)
    }

    /// Disables Tor.
    ///
    /// Persists the disabled state to storage.
    pub fn disable_tor(&self) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        vauchi.disable_tor().map_err(MobileError::from)
    }

    /// Returns the current Tor connection status.
    pub fn tor_status(&self) -> Result<MobileTorStatus, MobileError> {
        let vauchi = self.open_vauchi()?;
        Ok(MobileTorStatus::from(vauchi.tor_status()))
    }

    /// Configures Tor bridge addresses.
    ///
    /// Bridges are used when direct Tor connections are blocked.
    pub fn configure_tor_bridges(&self, bridges: Vec<String>) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        vauchi
            .configure_tor_bridges(bridges)
            .map_err(MobileError::from)
    }

    /// Requests a new Tor circuit rotation.
    ///
    /// Without the `tor` feature, this is a no-op that returns Ok.
    pub fn request_new_tor_circuit(&self) -> Result<(), MobileError> {
        let vauchi = self.open_vauchi()?;
        vauchi.request_new_tor_circuit().map_err(MobileError::from)
    }

    /// Loads the persisted Tor configuration from storage and returns it.
    pub fn load_tor_config(&self) -> Result<MobileTorConfig, MobileError> {
        let mut vauchi = self.open_vauchi()?;
        vauchi.load_tor_config().map_err(MobileError::from)?;
        Ok(MobileTorConfig::from(vauchi.tor_config()))
    }
}

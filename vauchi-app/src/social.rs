// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The active social-network registry: the CDN copy once one loads, else
//! the bundled `networks.json` (vauchi/private#734 D2). Held here, beside
//! the theme and string stores, so vauchi-core keeps no global (C12 of
//! the 2026-07-06 core audit) and takes the registry as a parameter.

use std::sync::{Arc, LazyLock, RwLock};

use vauchi_core::social::{NetworkDataError, SocialNetworkRegistry};

static BUNDLED_REGISTRY: LazyLock<Arc<SocialNetworkRegistry>> =
    LazyLock::new(|| Arc::new(SocialNetworkRegistry::with_defaults()));
static NETWORK_STORE: RwLock<Option<Arc<SocialNetworkRegistry>>> = RwLock::new(None);

/// Replaces the active registry with network data from the content
/// system. A rejected file leaves the active registry as it was.
pub fn load_networks_from_bytes(data: &[u8]) -> Result<(), NetworkDataError> {
    let registry = Arc::new(SocialNetworkRegistry::from_data_bytes(data)?);
    // A poisoned lock still holds a whole registry: only this assignment
    // writes it.
    *NETWORK_STORE.write().unwrap_or_else(|e| e.into_inner()) = Some(registry);
    Ok(())
}

pub fn active_registry() -> Arc<SocialNetworkRegistry> {
    NETWORK_STORE
        .read()
        .ok()
        .and_then(|lock| lock.clone())
        .unwrap_or_else(|| Arc::clone(&BUNDLED_REGISTRY))
}

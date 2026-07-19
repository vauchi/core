// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync Application Services
//!
//! Storage-coupled application services for synchronization: the
//! `SyncManager` offline queue, incoming card-update processing, and the
//! device-sync orchestrator. These depend on `crate::storage`; the pure
//! sync domain (deltas, merkle, replay detection, device-sync types) lives
//! in `crate::sync`.

#[cfg(feature = "testing")]
pub mod card_update;
#[cfg(not(feature = "testing"))]
mod card_update;

#[cfg(feature = "testing")]
pub mod device_orchestrator;
#[cfg(not(feature = "testing"))]
mod device_orchestrator;

#[cfg(feature = "testing")]
pub mod manager;
#[cfg(not(feature = "testing"))]
mod manager;

pub use card_update::{
    CardUpdateError, CardUpdateResult, ReceiveOutcome, ReceivedAlert, process_card_updates,
    process_single_card_update, process_single_card_update_for_device,
};
pub use device_orchestrator::{DeviceSyncOrchestrator, build_device_sync_envelopes};
pub use manager::{SyncError, SyncManager};

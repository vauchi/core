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

#[cfg(feature = "testing")]
pub mod registry_handshake;
#[cfg(not(feature = "testing"))]
mod registry_handshake;

pub use registry_handshake::RegistryReplyNeeded;
pub(crate) use registry_handshake::journal_handshake_state_for_siblings;

pub use card_update::{
    CardUpdateError, CardUpdateResult, ReceiveOutcome, ReceivedAlert, process_card_updates,
    process_single_card_update, process_single_card_update_for_device,
};
pub(crate) use card_update::{alert_event, process_single_card_update_for_authenticated_device};
pub use device_orchestrator::{DeviceSyncOrchestrator, build_device_sync_envelopes};
pub use manager::{SyncError, SyncManager};

/// `PendingUpdate.update_type` tag for genesis registry-handshake messages
/// (push and ack). The send phase keys the *legacy* sender token off this so
/// a peer that does not yet know the sending device can still resolve the
/// contact (F4 lost-primary bootstrap — see
/// `_private/docs/investigations/2026-07-26-f4-lost-primary-sender-token-root-cause.md`).
pub const REGISTRY_HANDSHAKE_UPDATE_TYPE: &str = "registry_handshake";

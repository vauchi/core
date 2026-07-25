// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync Protocol Module
//!
//! Manages synchronization of contact card updates between users.
//! Handles offline queuing, retry logic, and state tracking.

pub mod delta;
pub mod device_sync;
pub mod merkle;
pub mod registry_activation;
pub mod safety_alert;
pub mod state;

pub use delta::{CardDelta, DeltaError, FieldChange, ValidationSummary};
pub use device_sync::{
    ContactDeviceRegistrySyncData, ContactSyncData, DecodedSyncItems, DeviceLinkIntent,
    DeviceSyncError, DeviceSyncPayload, FieldStamp, GroupSyncData, ImportedContactSyncData,
    InterDeviceSyncState, SyncItem, VersionVector, decode_sync_items_tolerantly,
    validate_timestamp,
};
pub use merkle::MerkleTree;
pub use state::{ReplayDetector, SyncState};

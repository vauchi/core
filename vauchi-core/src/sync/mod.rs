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
pub mod state;

pub use delta::{CardDelta, DeltaError, FieldChange, ValidationSummary};
pub use device_sync::{
    ContactSyncData, DeviceSyncError, DeviceSyncPayload, FieldStamp, ImportedContactSyncData,
    InterDeviceSyncState, SyncItem, VersionVector, validate_timestamp,
};
pub use merkle::MerkleTree;
pub use state::{ReplayDetector, SyncState};

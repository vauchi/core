// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync Protocol Module
//!
//! Manages synchronization of contact card updates between users.
//! Handles offline queuing, retry logic, and state tracking.

pub mod delta;
pub mod device_orchestrator;
pub mod device_sync;
pub mod merkle;
pub mod state;

pub use delta::{CardDelta, DeltaError, FieldChange};
pub use device_orchestrator::DeviceSyncOrchestrator;
pub use device_sync::{
    validate_timestamp, ContactSyncData, DeviceSyncError, DeviceSyncPayload, InterDeviceSyncState,
    SyncItem, VersionVector,
};
pub use merkle::MerkleTree;
pub use state::{ReplayDetector, SyncError, SyncManager, SyncState};

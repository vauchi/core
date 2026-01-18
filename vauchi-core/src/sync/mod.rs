//! Sync Protocol Module
//!
//! Manages synchronization of contact card updates between users.
//! Handles offline queuing, retry logic, and state tracking.

pub mod delta;
pub mod device_orchestrator;
pub mod device_sync;
pub mod state;

pub use delta::{CardDelta, DeltaError, FieldChange};
pub use device_orchestrator::DeviceSyncOrchestrator;
pub use device_sync::{
    ContactSyncData, DeviceSyncError, DeviceSyncPayload, InterDeviceSyncState, SyncItem,
    VersionVector,
};
pub use state::{SyncError, SyncManager, SyncState};

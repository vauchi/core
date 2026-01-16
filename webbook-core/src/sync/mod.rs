//! Sync Protocol Module
//!
//! Manages synchronization of contact card updates between users.
//! Handles offline queuing, retry logic, and state tracking.

pub mod state;
pub mod delta;
pub mod device_sync;

pub use state::{SyncState, SyncManager, SyncError};
pub use delta::{CardDelta, FieldChange, DeltaError};
pub use device_sync::{ContactSyncData, DeviceSyncPayload, DeviceSyncError};

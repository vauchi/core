// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync Protocol Module
//!
//! Manages synchronization of contact card updates between users.
//! Handles offline queuing, retry logic, and state tracking.

pub mod card_update;
pub mod delta;
pub mod device_orchestrator;
pub mod device_sync;
pub mod merkle;
pub mod state;

pub use card_update::{process_card_updates, process_single_card_update, CardUpdateResult};
pub use delta::{CardDelta, DeltaError, FieldChange};
pub use device_orchestrator::{build_device_sync_envelopes, DeviceSyncOrchestrator};
pub use device_sync::{
    validate_timestamp, ContactSyncData, DeviceSyncError, DeviceSyncPayload, InterDeviceSyncState,
    SyncItem, VersionVector,
};
pub use merkle::MerkleTree;
pub use state::{ReplayDetector, SyncError, SyncManager, SyncState};

/// Trait for sending binary messages over a WebSocket-like transport.
pub trait BinarySender {
    fn send_binary(&mut self, data: Vec<u8>) -> Result<(), String>;
}

/// Async version of `BinarySender` for non-blocking WebSocket transports.
#[async_trait::async_trait]
pub trait AsyncBinarySender: Send {
    async fn send_binary(&mut self, data: Vec<u8>) -> Result<(), String>;
}

/// Builds device sync envelopes and sends them via the provided sender.
///
/// Returns the number of envelopes successfully sent.
pub fn send_device_sync(
    identity: &crate::Identity,
    storage: &crate::storage::Storage,
    sender: &mut dyn BinarySender,
) -> Result<u32, DeviceSyncError> {
    let envelopes = build_device_sync_envelopes(identity, storage)?;
    let mut sent = 0u32;
    for data in envelopes {
        if sender.send_binary(data).is_ok() {
            sent += 1;
        }
    }
    Ok(sent)
}

/// Async version of `send_device_sync` for non-blocking transports.
///
/// Returns the number of envelopes successfully sent.
pub async fn send_device_sync_async(
    identity: &crate::Identity,
    storage: &crate::storage::Storage,
    sender: &mut (dyn AsyncBinarySender + Send),
) -> Result<u32, DeviceSyncError> {
    let envelopes = build_device_sync_envelopes(identity, storage)?;
    let mut sent = 0u32;
    for data in envelopes {
        sender
            .send_binary(data)
            .await
            .map_err(|e| DeviceSyncError::SendFailed(e))?;
        sent += 1;
    }
    Ok(sent)
}

#[cfg(test)]
mod send_tests {
    use super::*;

    struct MockSender {
        messages: Vec<Vec<u8>>,
        fail_after: Option<usize>,
    }

    impl MockSender {
        fn new() -> Self {
            Self {
                messages: Vec::new(),
                fail_after: None,
            }
        }

        fn failing_after(n: usize) -> Self {
            Self {
                messages: Vec::new(),
                fail_after: Some(n),
            }
        }
    }

    impl BinarySender for MockSender {
        fn send_binary(&mut self, data: Vec<u8>) -> Result<(), String> {
            if let Some(limit) = self.fail_after {
                if self.messages.len() >= limit {
                    return Err("send failed".to_string());
                }
            }
            self.messages.push(data);
            Ok(())
        }
    }

    #[test]
    fn test_binary_sender_trait_works() {
        let mut sender = MockSender::new();
        sender.send_binary(vec![1, 2, 3]).unwrap();
        assert_eq!(sender.messages.len(), 1);
        assert_eq!(sender.messages[0], vec![1, 2, 3]);
    }

    #[test]
    fn test_binary_sender_failure() {
        let mut sender = MockSender::failing_after(0);
        let result = sender.send_binary(vec![1, 2, 3]);
        assert!(result.is_err());
    }

    // Async sender tests

    struct MockAsyncSender {
        messages: Vec<Vec<u8>>,
        fail_after: Option<usize>,
    }

    impl MockAsyncSender {
        fn new() -> Self {
            Self {
                messages: Vec::new(),
                fail_after: None,
            }
        }

        fn failing_after(n: usize) -> Self {
            Self {
                messages: Vec::new(),
                fail_after: Some(n),
            }
        }
    }

    #[async_trait::async_trait]
    impl AsyncBinarySender for MockAsyncSender {
        async fn send_binary(&mut self, data: Vec<u8>) -> Result<(), String> {
            if let Some(limit) = self.fail_after {
                if self.messages.len() >= limit {
                    return Err("send failed".to_string());
                }
            }
            self.messages.push(data);
            Ok(())
        }
    }

    #[test]
    fn test_async_binary_sender_trait_works() {
        // Use a simple block_on since we don't have tokio in core tests
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut sender = MockAsyncSender::new();
            sender.send_binary(vec![1, 2, 3]).await.unwrap();
            assert_eq!(sender.messages.len(), 1);
            assert_eq!(sender.messages[0], vec![1, 2, 3]);
        });
    }

    #[test]
    fn test_async_binary_sender_failure() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut sender = MockAsyncSender::failing_after(0);
            let result = sender.send_binary(vec![1, 2, 3]).await;
            assert!(result.is_err());
        });
    }
}

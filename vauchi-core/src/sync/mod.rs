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

pub use card_update::{CardUpdateResult, process_card_updates, process_single_card_update};
pub use delta::{CardDelta, DeltaError, FieldChange, ValidationSummary};
pub use device_orchestrator::{DeviceSyncOrchestrator, build_device_sync_envelopes};
pub use device_sync::{
    ContactSyncData, DeviceSyncError, DeviceSyncPayload, InterDeviceSyncState, SyncItem,
    VersionVector, validate_timestamp,
};
pub use merkle::MerkleTree;
pub use state::{ReplayDetector, SyncError, SyncManager, SyncState};

/// Error from sending binary messages over a transport.
#[derive(Debug, thiserror::Error)]
pub enum BinarySendError {
    #[error("{0}")]
    SendFailed(String),
}

/// Trait for sending binary messages over a WebSocket-like transport.
pub trait BinarySender {
    fn send_binary(&mut self, data: Vec<u8>) -> Result<(), BinarySendError>;
}

/// Async version of `BinarySender` for non-blocking WebSocket transports.
#[async_trait::async_trait]
pub trait AsyncBinarySender: Send {
    async fn send_binary(&mut self, data: Vec<u8>) -> Result<(), BinarySendError>;
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
            .map_err(|e| DeviceSyncError::SendFailed(e.to_string()))?;
        sent += 1;
    }
    Ok(sent)
}

// INLINE_TEST_REQUIRED: Tests for BinarySender/AsyncBinarySender traits defined in this module
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
        fn send_binary(&mut self, data: Vec<u8>) -> Result<(), BinarySendError> {
            if let Some(limit) = self.fail_after
                && self.messages.len() >= limit
            {
                return Err(BinarySendError::SendFailed("send failed".to_string()));
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
        result.expect_err("expected error");
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
        async fn send_binary(&mut self, data: Vec<u8>) -> Result<(), BinarySendError> {
            if let Some(limit) = self.fail_after
                && self.messages.len() >= limit
            {
                return Err(BinarySendError::SendFailed("send failed".to_string()));
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
            result.expect_err("expected error");
        });
    }
}

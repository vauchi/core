// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mock transport channel for testing.
//!
//! Provides a configurable [`MockTransportChannel`] that implements
//! [`TransportChannel`] with pre-loaded receive queues and send inspection.

use super::caps::TransportCaps;
use super::channel::{PeerInfo, TransportChannel, TransportError, TransportType};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Internal mutable state for the mock transport.
#[derive(Debug)]
struct MockState {
    receive_queue: VecDeque<Vec<u8>>,
    sent_data: Vec<Vec<u8>>,
}

/// A configurable mock implementation of [`TransportChannel`] for tests.
///
/// Uses `Arc<Mutex<...>>` for interior mutability so the mock can be
/// shared and inspected after use.
#[derive(Debug, Clone)]
pub struct MockTransportChannel {
    transport_type: TransportType,
    available: bool,
    send_error: Option<String>,
    state: Arc<Mutex<MockState>>,
}

impl MockTransportChannel {
    /// Create a new mock transport of the given type.
    /// Defaults to available with no errors configured.
    pub fn new(transport_type: TransportType) -> Self {
        Self {
            transport_type,
            available: true,
            send_error: None,
            state: Arc::new(Mutex::new(MockState {
                receive_queue: VecDeque::new(),
                sent_data: Vec::new(),
            })),
        }
    }

    /// Set whether this transport reports as available.
    pub fn with_available(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    /// Configure send to always fail with the given reason.
    pub fn with_send_error(mut self, reason: &str) -> Self {
        self.send_error = Some(reason.to_string());
        self
    }

    /// Pre-load data that will be returned by the next `receive()` call.
    pub fn queue_receive(&self, data: Vec<u8>) {
        let mut state = self.state.lock().expect("mock state lock poisoned");
        state.receive_queue.push_back(data);
    }

    /// Return all data that was passed to `send()`.
    pub fn sent_data(&self) -> Vec<Vec<u8>> {
        let state = self.state.lock().expect("mock state lock poisoned");
        state.sent_data.clone()
    }
}

impl TransportChannel for MockTransportChannel {
    fn transport_type(&self) -> TransportType {
        self.transport_type
    }

    fn is_available(&self) -> Result<bool, TransportError> {
        Ok(self.available)
    }

    fn discover_peer(&self, _timeout: Duration) -> Result<PeerInfo, TransportError> {
        Ok(PeerInfo {
            peer_id: format!("mock-peer-{}", self.transport_type),
            capabilities: TransportCaps::all(),
            rssi: None,
        })
    }

    fn send(&self, data: &[u8]) -> Result<(), TransportError> {
        if let Some(ref reason) = self.send_error {
            return Err(TransportError::SendFailed {
                transport: self.transport_type,
                reason: reason.clone(),
            });
        }
        let mut state = self.state.lock().expect("mock state lock poisoned");
        state.sent_data.push(data.to_vec());
        Ok(())
    }

    fn receive(&self, _timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let mut state = self.state.lock().expect("mock state lock poisoned");
        state
            .receive_queue
            .pop_front()
            .ok_or_else(|| TransportError::ReceiveFailed {
                transport: self.transport_type,
                reason: "receive queue empty".to_string(),
            })
    }

    fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }

    fn max_payload_size(&self) -> usize {
        65536
    }

    fn requires_chunking(&self) -> bool {
        false
    }
}

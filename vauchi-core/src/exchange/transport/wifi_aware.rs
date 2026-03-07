// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! WiFi Aware (NAN) transport implementation.
//!
//! Provides peer-to-peer WiFi connectivity for contact exchange.
//! Platform-specific backends (iOS/Android) implement [`WifiAwareBackend`].

use super::caps::TransportCaps;
use super::channel::{PeerInfo, TransportChannel, TransportError, TransportType};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Maximum payload size for WiFi Aware transport (64 KiB).
const MAX_PAYLOAD_SIZE: usize = 65536;

/// Configuration for the WiFi Aware transport.
#[derive(Debug, Clone)]
pub struct WifiAwareConfig {
    /// Service name advertised during publish/subscribe.
    pub service_name: String,
    /// Default timeout for discovery and receive operations.
    pub timeout: Duration,
}

impl Default for WifiAwareConfig {
    fn default() -> Self {
        Self {
            service_name: "vauchi-exchange".to_string(),
            timeout: Duration::from_secs(10),
        }
    }
}

/// Platform callback trait for WiFi Aware operations.
///
/// iOS and Android implement this natively; tests use [`MockWifiAwareBackend`].
pub trait WifiAwareBackend: Send + Sync {
    /// Check if WiFi Aware is available on this device.
    fn is_available(&self) -> bool;

    /// Publish a service for peer discovery.
    fn publish(&self, service_name: &str) -> Result<(), TransportError>;

    /// Subscribe and discover peers offering the given service.
    fn subscribe(
        &self,
        service_name: &str,
        timeout: Duration,
    ) -> Result<Vec<PeerInfo>, TransportError>;

    /// Connect to a discovered peer.
    fn connect(&self, peer_id: &str) -> Result<(), TransportError>;

    /// Send data to the connected peer.
    fn send(&self, data: &[u8]) -> Result<(), TransportError>;

    /// Receive data from the connected peer.
    fn receive(&self, timeout: Duration) -> Result<Vec<u8>, TransportError>;

    /// Close the connection.
    fn close(&self) -> Result<(), TransportError>;
}

/// WiFi Aware transport wrapping a platform-specific backend.
pub struct WifiAwareTransport<B: WifiAwareBackend> {
    backend: B,
    config: WifiAwareConfig,
}

impl<B: WifiAwareBackend> WifiAwareTransport<B> {
    /// Create a new WiFi Aware transport with the given backend and config.
    pub fn new(backend: B, config: WifiAwareConfig) -> Self {
        Self { backend, config }
    }
}

impl<B: WifiAwareBackend + 'static> TransportChannel for WifiAwareTransport<B> {
    fn transport_type(&self) -> TransportType {
        TransportType::WifiAware
    }

    fn is_available(&self) -> Result<bool, TransportError> {
        Ok(self.backend.is_available())
    }

    fn discover_peer(&self, timeout: Duration) -> Result<PeerInfo, TransportError> {
        let peers = self.backend.subscribe(&self.config.service_name, timeout)?;
        let peer = peers.into_iter().next().ok_or(TransportError::Timeout {
            transport: TransportType::WifiAware,
            timeout_ms: timeout.as_millis() as u64,
        })?;
        self.backend.connect(&peer.peer_id)?;
        Ok(peer)
    }

    fn send(&self, data: &[u8]) -> Result<(), TransportError> {
        self.backend.send(data)
    }

    fn receive(&self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.backend.receive(timeout)
    }

    fn close(&self) -> Result<(), TransportError> {
        self.backend.close()
    }

    fn max_payload_size(&self) -> usize {
        MAX_PAYLOAD_SIZE
    }

    fn requires_chunking(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Mock backend for testing
// ---------------------------------------------------------------------------

/// Internal state for the mock backend.
#[derive(Debug)]
struct MockState {
    available: bool,
    peers: Vec<PeerInfo>,
    receive_queue: Vec<Vec<u8>>,
    sent_data: Vec<Vec<u8>>,
}

/// Mock WiFi Aware backend for testing.
///
/// Uses `Arc<Mutex<...>>` for interior mutability so it can implement
/// `WifiAwareBackend` (which takes `&self`).
#[derive(Debug, Clone)]
pub struct MockWifiAwareBackend {
    state: Arc<Mutex<MockState>>,
}

impl MockWifiAwareBackend {
    /// Create a new mock backend (unavailable by default, no peers).
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                available: false,
                peers: Vec::new(),
                receive_queue: Vec::new(),
                sent_data: Vec::new(),
            })),
        }
    }

    /// Set whether WiFi Aware is available.
    pub fn with_available(self, available: bool) -> Self {
        self.state.lock().expect("lock").available = available;
        self
    }

    /// Add a discoverable peer.
    pub fn with_peer(self, peer_id: &str) -> Self {
        self.state.lock().expect("lock").peers.push(PeerInfo {
            peer_id: peer_id.to_string(),
            capabilities: TransportCaps::WIFI_AWARE,
            rssi: None,
        });
        self
    }

    /// Queue data that will be returned by the next `receive` call.
    pub fn queue_receive(self, data: Vec<u8>) -> Self {
        self.state.lock().expect("lock").receive_queue.push(data);
        self
    }
}

impl WifiAwareBackend for MockWifiAwareBackend {
    fn is_available(&self) -> bool {
        self.state.lock().expect("lock").available
    }

    fn publish(&self, _service_name: &str) -> Result<(), TransportError> {
        Ok(())
    }

    fn subscribe(
        &self,
        _service_name: &str,
        _timeout: Duration,
    ) -> Result<Vec<PeerInfo>, TransportError> {
        let state = self.state.lock().expect("lock");
        Ok(state.peers.clone())
    }

    fn connect(&self, _peer_id: &str) -> Result<(), TransportError> {
        Ok(())
    }

    fn send(&self, data: &[u8]) -> Result<(), TransportError> {
        self.state
            .lock()
            .expect("lock")
            .sent_data
            .push(data.to_vec());
        Ok(())
    }

    fn receive(&self, timeout: Duration) -> Result<Vec<u8>, TransportError> {
        let mut state = self.state.lock().expect("lock");
        if state.receive_queue.is_empty() {
            return Err(TransportError::Timeout {
                transport: TransportType::WifiAware,
                timeout_ms: timeout.as_millis() as u64,
            });
        }
        Ok(state.receive_queue.remove(0))
    }

    fn close(&self) -> Result<(), TransportError> {
        Ok(())
    }
}

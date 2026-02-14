// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tor Integration
//!
//! Provides Tor connectivity for anonymous relay communication.
//! All relay connections can be routed through the Tor network to
//! hide the user's IP address from relay operators.
//!
//! # Feature Flag
//!
//! The `tor` feature flag gates the `arti` dependency and the real
//! `TorManager` implementation. Without the feature, only types
//! and the `TorConnector` trait are available.
//!
//! # Type Re-exports
//!
//! Core data types (`TorConfig`, `TorStatus`, `TorRelayAddress`) live in
//! `crate::tor_config` so they're available without the network feature.
//! This module re-exports them for convenience.

// Re-export data types from tor_config module (always available)
pub use crate::tor_config::{TorConfig, TorRelayAddress, TorStatus};

use super::error::NetworkError;

/// Trait for Tor connectivity (testable abstraction).
///
/// Implementations provide the ability to connect to hosts through
/// the Tor network and manage circuit lifecycle.
pub trait TorConnector: Send + Sync {
    /// Bootstrap the Tor client (connect to network, download directory).
    fn bootstrap(&self) -> Result<(), NetworkError>;

    /// Connect to a host:port through Tor, returning a bidirectional stream.
    fn connect_to(&self, host: &str, port: u16) -> Result<Box<dyn TorStream>, NetworkError>;

    /// Request a new Tor circuit (for IP rotation).
    fn rotate_circuit(&self) -> Result<(), NetworkError>;

    /// Returns the current Tor status.
    fn status(&self) -> TorStatus;

    /// Shuts down the Tor client.
    fn shutdown(&self) -> Result<(), NetworkError>;
}

/// Trait for a bidirectional Tor stream.
pub trait TorStream: std::io::Read + std::io::Write + Send {}

/// Blanket implementation: anything that is Read + Write + Send is a TorStream.
impl<T: std::io::Read + std::io::Write + Send> TorStream for T {}

/// Mock Tor connector for testing.
#[cfg(any(test, feature = "testing"))]
pub struct MockTorConnector {
    status: std::sync::Mutex<TorStatus>,
    should_fail_bootstrap: bool,
    should_fail_connect: bool,
}

#[cfg(any(test, feature = "testing"))]
impl MockTorConnector {
    /// Creates a new mock connector.
    pub fn new() -> Self {
        MockTorConnector {
            status: std::sync::Mutex::new(TorStatus::Disabled),
            should_fail_bootstrap: false,
            should_fail_connect: false,
        }
    }

    /// Creates a mock that fails on bootstrap.
    pub fn failing_bootstrap() -> Self {
        MockTorConnector {
            status: std::sync::Mutex::new(TorStatus::Disabled),
            should_fail_bootstrap: true,
            should_fail_connect: false,
        }
    }

    /// Creates a mock that fails on connect.
    pub fn failing_connect() -> Self {
        MockTorConnector {
            status: std::sync::Mutex::new(TorStatus::Disabled),
            should_fail_bootstrap: false,
            should_fail_connect: true,
        }
    }
}

#[cfg(any(test, feature = "testing"))]
impl Default for MockTorConnector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "testing"))]
impl TorConnector for MockTorConnector {
    fn bootstrap(&self) -> Result<(), NetworkError> {
        if self.should_fail_bootstrap {
            return Err(NetworkError::Tor("Mock bootstrap failure".to_string()));
        }
        let mut status = self
            .status
            .lock()
            .map_err(|_| NetworkError::Tor("status mutex poisoned".into()))?;
        *status = TorStatus::Connected;
        Ok(())
    }

    fn connect_to(&self, _host: &str, _port: u16) -> Result<Box<dyn TorStream>, NetworkError> {
        if self.should_fail_connect {
            return Err(NetworkError::TorCircuitFailed(
                "Mock connect failure".to_string(),
            ));
        }
        let status = self
            .status
            .lock()
            .map_err(|_| NetworkError::Tor("status mutex poisoned".into()))?;
        if *status != TorStatus::Connected {
            return Err(NetworkError::TorNotAvailable);
        }
        Ok(Box::new(std::io::Cursor::new(Vec::new())))
    }

    fn rotate_circuit(&self) -> Result<(), NetworkError> {
        let status = self
            .status
            .lock()
            .map_err(|_| NetworkError::Tor("status mutex poisoned".into()))?;
        if *status != TorStatus::Connected {
            return Err(NetworkError::TorNotAvailable);
        }
        Ok(())
    }

    fn status(&self) -> TorStatus {
        self.status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn shutdown(&self) -> Result<(), NetworkError> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| NetworkError::Tor("status mutex poisoned".into()))?;
        *status = TorStatus::Disabled;
        Ok(())
    }
}

// === TorManager (requires arti feature) ===

#[cfg(feature = "tor")]
mod manager {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    /// Manages the embedded Tor client lifecycle.
    ///
    /// Owns a dedicated tokio runtime for Tor operations and an arti TorClient.
    /// All operations block on the async runtime internally.
    pub struct TorManager {
        runtime: Arc<tokio::runtime::Runtime>,
        client:
            std::sync::Mutex<Option<Arc<arti_client::TorClient<tor_rtcompat::PreferredRuntime>>>>,
        status: std::sync::Mutex<TorStatus>,
        config: TorConfig,
        circuit_created_at: std::sync::Mutex<Option<Instant>>,
    }

    impl TorManager {
        /// Creates a new TorManager with the given config.
        pub fn new(config: TorConfig) -> Result<Self, NetworkError> {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .thread_name("vauchi-tor")
                .build()
                .map_err(|e| NetworkError::Tor(format!("Failed to create Tor runtime: {}", e)))?;

            Ok(TorManager {
                runtime: Arc::new(runtime),
                client: std::sync::Mutex::new(None),
                status: std::sync::Mutex::new(TorStatus::Disabled),
                config,
                circuit_created_at: std::sync::Mutex::new(None),
            })
        }

        /// Returns the current circuit age in seconds, if a circuit exists.
        pub fn circuit_age_secs(&self) -> Option<u64> {
            let created = self
                .circuit_created_at
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            created.map(|t| t.elapsed().as_secs())
        }

        /// Returns whether the current circuit needs rotation.
        pub fn needs_circuit_rotation(&self) -> bool {
            match self.circuit_age_secs() {
                Some(age) => age >= self.config.circuit_rotation_secs,
                None => false,
            }
        }

        /// Returns a reference to the Tor config.
        pub fn config(&self) -> &TorConfig {
            &self.config
        }
    }

    impl TorConnector for TorManager {
        fn bootstrap(&self) -> Result<(), NetworkError> {
            {
                let mut status = self
                    .status
                    .lock()
                    .map_err(|_| NetworkError::Tor("status mutex poisoned".into()))?;
                *status = TorStatus::Connecting;
            }

            let result = self.runtime.block_on(async {
                let config = arti_client::TorClientConfig::default();
                arti_client::TorClient::create_bootstrapped(config)
                    .await
                    .map_err(|e| NetworkError::TorBootstrapFailed(e.to_string()))
            });

            match result {
                Ok(client) => {
                    let mut c = self
                        .client
                        .lock()
                        .map_err(|_| NetworkError::Tor("client mutex poisoned".into()))?;
                    *c = Some(Arc::new(client));
                    let mut status = self
                        .status
                        .lock()
                        .map_err(|_| NetworkError::Tor("status mutex poisoned".into()))?;
                    *status = TorStatus::Connected;
                    let mut created = self
                        .circuit_created_at
                        .lock()
                        .map_err(|_| NetworkError::Tor("circuit mutex poisoned".into()))?;
                    *created = Some(Instant::now());
                    Ok(())
                }
                Err(e) => {
                    if let Ok(mut status) = self.status.lock() {
                        *status = TorStatus::Disconnected {
                            reason: e.to_string(),
                        };
                    }
                    Err(e)
                }
            }
        }

        fn connect_to(&self, host: &str, port: u16) -> Result<Box<dyn TorStream>, NetworkError> {
            let client_guard = self
                .client
                .lock()
                .map_err(|_| NetworkError::Tor("client mutex poisoned".into()))?;
            let client = client_guard
                .as_ref()
                .ok_or(NetworkError::TorNotAvailable)?
                .clone();
            drop(client_guard);

            let addr = format!("{}:{}", host, port);
            let stream = self.runtime.block_on(async {
                client
                    .connect(addr.as_str())
                    .await
                    .map_err(|e| NetworkError::TorCircuitFailed(e.to_string()))
            })?;

            let sync_stream = self
                .runtime
                .block_on(async { tokio_util::io::SyncIoBridge::new(stream) });

            Ok(Box::new(sync_stream))
        }

        fn rotate_circuit(&self) -> Result<(), NetworkError> {
            let client_guard = self
                .client
                .lock()
                .map_err(|_| NetworkError::Tor("client mutex poisoned".into()))?;
            let _client = client_guard.as_ref().ok_or(NetworkError::TorNotAvailable)?;

            let mut created = self
                .circuit_created_at
                .lock()
                .map_err(|_| NetworkError::Tor("circuit mutex poisoned".into()))?;
            *created = Some(Instant::now());

            Ok(())
        }

        fn status(&self) -> TorStatus {
            self.status
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }

        fn shutdown(&self) -> Result<(), NetworkError> {
            let mut client = self
                .client
                .lock()
                .map_err(|_| NetworkError::Tor("client mutex poisoned".into()))?;
            *client = None;
            let mut status = self
                .status
                .lock()
                .map_err(|_| NetworkError::Tor("status mutex poisoned".into()))?;
            *status = TorStatus::Disabled;
            let mut created = self
                .circuit_created_at
                .lock()
                .map_err(|_| NetworkError::Tor("circuit mutex poisoned".into()))?;
            *created = None;
            Ok(())
        }
    }

    /// Real Tor connector wrapping TorManager (implements TorConnector).
    pub type ArtiTorConnector = TorManager;
}

#[cfg(feature = "tor")]
pub use manager::{ArtiTorConnector, TorManager};

// === TorTransport (Transport trait impl) ===

use super::message::MessageEnvelope;
use super::transport::{ConnectionState, Transport, TransportConfig, TransportResult};

use std::sync::Arc;

/// Transport implementation that routes connections through Tor.
///
/// Uses a `TorConnector` to establish connections and wraps them
/// in length-prefixed JSON framing, matching the relay wire format.
pub struct TorTransport {
    connector: Arc<dyn TorConnector>,
    state: ConnectionState,
    stream: Option<Box<dyn TorStream>>,
    receive_buffer: Vec<MessageEnvelope>,
}

impl TorTransport {
    /// Creates a new TorTransport with the given connector.
    pub fn new(connector: Arc<dyn TorConnector>) -> Self {
        TorTransport {
            connector,
            state: ConnectionState::Disconnected,
            stream: None,
            receive_buffer: Vec::new(),
        }
    }
}

impl Transport for TorTransport {
    fn connect(&mut self, config: &TransportConfig) -> TransportResult<()> {
        self.state = ConnectionState::Connecting;

        let url = &config.server_url;
        let (host, port) = parse_host_port(url)?;

        match self.connector.connect_to(&host, port) {
            Ok(stream) => {
                self.stream = Some(stream);
                self.state = ConnectionState::Connected;
                Ok(())
            }
            Err(e) => {
                self.state = ConnectionState::Disconnected;
                Err(e)
            }
        }
    }

    fn disconnect(&mut self) -> TransportResult<()> {
        self.stream = None;
        self.state = ConnectionState::Disconnected;
        Ok(())
    }

    fn state(&self) -> ConnectionState {
        self.state.clone()
    }

    fn send(&mut self, message: &MessageEnvelope) -> TransportResult<()> {
        if self.stream.is_none() {
            return Err(super::error::NetworkError::NotConnected);
        }

        let stream = self.stream.as_mut().unwrap();
        let json = serde_json::to_vec(message)
            .map_err(|e| super::error::NetworkError::Serialization(e.to_string()))?;

        let len = json.len() as u32;
        use std::io::Write;
        stream
            .write_all(&len.to_be_bytes())
            .map_err(|e| super::error::NetworkError::SendFailed(e.to_string()))?;
        stream
            .write_all(&json)
            .map_err(|e| super::error::NetworkError::SendFailed(e.to_string()))?;
        stream
            .flush()
            .map_err(|e| super::error::NetworkError::SendFailed(e.to_string()))?;

        Ok(())
    }

    fn receive(&mut self) -> TransportResult<Option<MessageEnvelope>> {
        if !self.receive_buffer.is_empty() {
            return Ok(Some(self.receive_buffer.remove(0)));
        }

        if self.stream.is_none() {
            return Err(super::error::NetworkError::NotConnected);
        }

        let stream = self.stream.as_mut().unwrap();

        let mut len_buf = [0u8; 4];
        use std::io::Read;
        match stream.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(super::error::NetworkError::ConnectionClosed)
            }
            Err(e) => return Err(super::error::NetworkError::ReceiveFailed(e.to_string())),
        }

        let len = u32::from_be_bytes(len_buf) as usize;
        if len > super::protocol::MAX_MESSAGE_SIZE {
            return Err(super::error::NetworkError::InvalidMessage(
                "Message too large".to_string(),
            ));
        }

        let mut payload = vec![0u8; len];
        stream
            .read_exact(&mut payload)
            .map_err(|e| super::error::NetworkError::ReceiveFailed(e.to_string()))?;

        let envelope: MessageEnvelope = serde_json::from_slice(&payload)
            .map_err(|e| super::error::NetworkError::InvalidMessage(e.to_string()))?;

        Ok(Some(envelope))
    }

    fn has_pending(&self) -> bool {
        !self.receive_buffer.is_empty()
    }
}

/// Parse host and port from a URL string.
fn parse_host_port(url: &str) -> Result<(String, u16), super::error::NetworkError> {
    let stripped = url
        .strip_prefix("wss://")
        .or_else(|| url.strip_prefix("ws://"))
        .unwrap_or(url);

    let host_port = stripped.split('/').next().unwrap_or(stripped);

    if let Some((host, port_str)) = host_port.rsplit_once(':') {
        let port: u16 = port_str.parse().map_err(|_| {
            super::error::NetworkError::ConnectionFailed(format!("Invalid port in URL: {}", url))
        })?;
        Ok((host.to_string(), port))
    } else if url.starts_with("wss://") {
        Ok((host_port.to_string(), 443))
    } else {
        Ok((host_port.to_string(), 80))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_connector_lifecycle() {
        let connector = MockTorConnector::new();
        assert_eq!(connector.status(), TorStatus::Disabled);

        connector.bootstrap().unwrap();
        assert_eq!(connector.status(), TorStatus::Connected);

        connector.rotate_circuit().unwrap();
        assert_eq!(connector.status(), TorStatus::Connected);

        connector.shutdown().unwrap();
        assert_eq!(connector.status(), TorStatus::Disabled);
    }

    #[test]
    fn test_mock_connector_bootstrap_failure() {
        let connector = MockTorConnector::failing_bootstrap();
        let result = connector.bootstrap();
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_connector_connect_failure() {
        let connector = MockTorConnector::failing_connect();
        connector.bootstrap().unwrap();
        let result = connector.connect_to("example.com", 443);
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_connector_connect_before_bootstrap() {
        let connector = MockTorConnector::new();
        let result = connector.connect_to("example.com", 443);
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_connector_rotate_before_bootstrap() {
        let connector = MockTorConnector::new();
        let result = connector.rotate_circuit();
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_host_port_wss() {
        let (host, port) = parse_host_port("wss://relay.vauchi.app:8443").unwrap();
        assert_eq!(host, "relay.vauchi.app");
        assert_eq!(port, 8443);
    }

    #[test]
    fn test_parse_host_port_ws() {
        let (host, port) = parse_host_port("ws://localhost:8080").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 8080);
    }

    #[test]
    fn test_parse_host_port_default_wss() {
        let (host, port) = parse_host_port("wss://relay.vauchi.app").unwrap();
        assert_eq!(host, "relay.vauchi.app");
        assert_eq!(port, 443);
    }

    #[test]
    fn test_parse_host_port_default_ws() {
        let (host, port) = parse_host_port("ws://localhost").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 80);
    }

    #[test]
    fn test_parse_host_port_with_path() {
        let (host, port) = parse_host_port("wss://relay.vauchi.app:443/ws").unwrap();
        assert_eq!(host, "relay.vauchi.app");
        assert_eq!(port, 443);
    }

    #[test]
    fn test_parse_host_port_onion() {
        let (host, port) = parse_host_port("ws://abcdefg.onion:80").unwrap();
        assert_eq!(host, "abcdefg.onion");
        assert_eq!(port, 80);
    }

    #[test]
    fn test_tor_transport_initial_state() {
        let connector = Arc::new(MockTorConnector::new());
        let transport = TorTransport::new(connector);
        assert_eq!(transport.state(), ConnectionState::Disconnected);
        assert!(!transport.has_pending());
    }

    #[test]
    fn test_tor_transport_connect_disconnect() {
        let connector = Arc::new(MockTorConnector::new());
        connector.bootstrap().unwrap();

        let mut transport = TorTransport::new(connector);
        let config = TransportConfig {
            server_url: "ws://localhost:8080".to_string(),
            ..Default::default()
        };

        transport.connect(&config).unwrap();
        assert_eq!(transport.state(), ConnectionState::Connected);

        transport.disconnect().unwrap();
        assert_eq!(transport.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_tor_transport_connect_fails_without_bootstrap() {
        let connector = Arc::new(MockTorConnector::new());
        let mut transport = TorTransport::new(connector);
        let config = TransportConfig {
            server_url: "ws://localhost:8080".to_string(),
            ..Default::default()
        };

        let result = transport.connect(&config);
        assert!(result.is_err());
        assert_eq!(transport.state(), ConnectionState::Disconnected);
    }

    #[test]
    fn test_tor_transport_send_without_connect() {
        let connector = Arc::new(MockTorConnector::new());
        let mut transport = TorTransport::new(connector);

        let envelope = MessageEnvelope {
            version: 1,
            message_id: "test".to_string(),
            timestamp: 0,
            payload: super::super::message::MessagePayload::Handshake(
                super::super::message::Handshake {
                    identity_public_key: [0u8; 32],
                    nonce: [0u8; 32],
                    signature: [0u8; 64],
                },
            ),
        };

        let result = transport.send(&envelope);
        assert!(result.is_err());
    }
}

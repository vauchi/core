// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Network + Transport Layer
//!
//! Provides transport abstractions and relay client for delivering encrypted
//! contact card updates between users.
//!
//! # Architecture
//!
//! The network layer consists of:
//! - **Transport trait**: Platform-agnostic interface for network I/O
//! - **Message types**: Wire protocol for relay communication
//! - **Protocol layer**: Message serialization and framing
//! - **Connection manager**: Automatic reconnection and handshake
//! - **Relay client**: Queue management, retries, and acknowledgment tracking
//!
//! # Example
//!
//! ```ignore
//! use vauchi_core::network::{RelayClient, RelayClientConfig, MockTransport};
//!
//! // Create a relay client with mock transport (for testing)
//! let transport = MockTransport::new();
//! let config = RelayClientConfig::default();
//! let mut client = RelayClient::new(transport, config, "my-identity".into());
//!
//! // Connect and send updates
//! client.connect()?;
//! let msg_id = client.send_update(recipient_id, &mut ratchet, &payload, update_id)?;
//!
//! // Process acknowledgments
//! let acked = client.process_incoming()?;
//! ```

pub mod anonymous;

#[cfg(feature = "testing")]
pub mod connection;
#[cfg(not(feature = "testing"))]
mod connection;

#[cfg(feature = "testing")]
pub mod error;
#[cfg(not(feature = "testing"))]
mod error;

#[cfg(feature = "testing")]
pub mod message;
#[cfg(not(feature = "testing"))]
mod message;

#[cfg(feature = "testing")]
pub mod mock;
#[cfg(not(feature = "testing"))]
mod mock;

#[cfg(feature = "testing")]
pub mod protocol;
#[cfg(not(feature = "testing"))]
mod protocol;

#[cfg(feature = "testing")]
pub mod relay_client;
#[cfg(not(feature = "testing"))]
mod relay_client;

pub mod simple_message;

#[cfg(feature = "testing")]
pub mod transport;
#[cfg(not(feature = "testing"))]
mod transport;

#[cfg(feature = "testing")]
pub mod multi_relay;
#[cfg(not(feature = "testing"))]
mod multi_relay;

#[cfg(feature = "testing")]
pub mod websocket;
#[cfg(not(feature = "testing"))]
mod websocket;

#[cfg(feature = "testing")]
pub mod noise;
#[cfg(not(feature = "testing"))]
mod noise;

pub mod forwarding;
pub mod pinning;
pub mod relay_url;
pub mod revocation;
pub mod tor;

// Error types
pub use error::NetworkError;

// Message types
pub use message::{
    negotiate_version, AccountDeletionNotice, AccountRevoked, AckStatus, Acknowledgment,
    DeletionStage, DeviceSyncMessage, EmergencyAlert, EncryptedUpdate, ForwardingHint,
    ForwardingHints, GeoLocation, Handshake, MessageEnvelope, MessageId, MessagePayload,
    PresenceStatus, PresenceUpdate, PurgeRequest, RatchetHeader, VersionNegotiation,
    PROTOCOL_VERSION,
};

// Protocol utilities
pub use protocol::{
    create_envelope, decode_message, encode_message, FRAME_HEADER_SIZE, MAX_MESSAGE_SIZE,
};

// Transport abstraction
pub use transport::{ConnectionState, ProxyConfig, Transport, TransportConfig, TransportResult};

// Mock transport for testing
pub use mock::MockTransport;

// WebSocket transport for production
pub use websocket::WebSocketTransport;

// Connection management
pub use connection::ConnectionManager;

// Relay client
pub use relay_client::{AckEvent, IncomingResult, ProcessResult, RelayClient, RelayClientConfig};

// Multi-relay support
pub use multi_relay::{
    MultiRelayConfig, MultiRelayConfigBuilder, MultiRelayError, MultiRelayManager, RelayHealth,
    RelaySelector,
};

// Anonymous sender identifiers
pub use anonymous::{compute_anonymous_id, current_epoch, resolve_sender, AnonymousSender};

// Noise NK inner transport encryption
pub use noise::{parse_relay_noise_pubkey, NoiseInitiator, NoiseTransport as NoiseSession};

// Certificate pinning
pub use pinning::{verify_pin, PinnedCertificate};

// Tor transport
pub use tor::{TorConfig, TorConnector, TorRelayAddress, TorStatus, TorTransport};

#[cfg(feature = "tor")]
pub use tor::{ArtiTorConnector, TorManager};

// Message classification
mod classify;
pub use classify::{classify_message, MessageType};

// Delivery service (message delivery tracking, retries, offline queue)
pub mod delivery;
pub use delivery::error_messages::failure_to_user_message;
pub use delivery::{
    CleanupResult, ConnectivityDiagnostics, ConnectivityReport, DeliveryAckStatus, DeliveryService,
    KeyRotationDetector, KeyRotationError, OfflineManager, RetryScheduler, RetryTickResult,
};

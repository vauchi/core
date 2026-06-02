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
pub mod mailbox_token;

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

pub mod escrow_client;

#[cfg(feature = "testing")]
pub mod transport;
#[cfg(not(feature = "testing"))]
mod transport;

#[cfg(feature = "testing")]
pub mod multi_relay;
#[cfg(not(feature = "testing"))]
mod multi_relay;

// websocket and noise modules removed — relay uses HTTP v2 transport

pub mod forwarding;
#[cfg(feature = "network-http")]
pub mod http_adapter;
#[cfg(feature = "network-http")]
pub mod http_transport;
#[cfg(feature = "network-http")]
pub mod ohttp_client;
pub mod pinning;
pub mod relay_url;
pub mod revocation;
#[cfg(feature = "network-http")]
pub mod tls_pinning;

// Error types
pub use error::NetworkError;

// Message types
pub use message::{
    AckStatus, Acknowledgment, DeletionStage, DeregisterMailbox, EmergencyAlert, EncryptedUpdate,
    ForwardingHint, ForwardingHints, GeoLocation, Handshake, IdentityDeletionNotice,
    IdentityRevoked, MessageEnvelope, MessageId, MessagePayload, PROTOCOL_VERSION, PresenceStatus,
    PresenceUpdate, PurgeRequest, RatchetHeader, RegisterMailbox, VersionNegotiation,
    negotiate_version,
};

// Protocol utilities
pub use protocol::{
    FRAME_HEADER_SIZE, MAX_MESSAGE_SIZE, create_envelope, decode_message, encode_message,
};

// Transport abstraction
pub use transport::{ConnectionState, ProxyConfig, Transport, TransportConfig, TransportResult};

// Mock transport for testing
pub use mock::MockTransport;

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
pub use anonymous::{
    AnonymousSender, SenderIndex, compute_anonymous_id, current_epoch, resolve_sender,
    resolve_sender_id,
};

// Certificate pinning
pub use pinning::{PinnedCertificate, verify_pin};

// Message classification
mod classify;
pub use classify::{MessageType, classify_message};

// HTTP transport for relay v2 protocol
#[cfg(feature = "network-http")]
pub use http_transport::{HttpTransport, HttpTransportConfig};
#[cfg(feature = "network-http")]
pub use vauchi_protocol::v2::FetchedBlob;

// OHTTP client-side encryption (RFC 9458)
#[cfg(feature = "network-http")]
pub use ohttp_client::{OhttpClient, ResponseDecryptor};

// HTTP transport adapter (implements Transport trait for v2 HTTP protocol)
#[cfg(feature = "network-http")]
pub use http_adapter::HttpTransportAdapter;

// Delivery service (message delivery tracking, retries, offline queue)
pub mod delivery;
pub use delivery::error_messages::failure_to_user_message;
pub use delivery::{
    CleanupResult, ConnectivityDiagnostics, ConnectivityReport, DeliveryAckStatus, DeliveryService,
    KeyRotationDetector, KeyRotationError, OfflineManager, RetryScheduler, RetryTickResult,
};

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

pub use error::NetworkError;

pub use message::{
    AckStatus, Acknowledgment, DeletionStage, DeregisterMailbox, EmergencyAlert, EncryptedUpdate,
    GeoLocation, Handshake, IdentityDeletionNotice, IdentityRevoked, MessageEnvelope, MessageId,
    MessagePayload, PROTOCOL_VERSION, PresenceStatus, PresenceUpdate, PurgeRequest, RatchetHeader,
    RegisterMailbox, VersionNegotiation, negotiate_version,
};

pub use protocol::{
    FRAME_HEADER_SIZE, MAX_MESSAGE_SIZE, create_envelope, decode_message, encode_message,
};

pub use transport::{ConnectionState, ProxyConfig, Transport, TransportConfig, TransportResult};

pub use mock::MockTransport;

pub use connection::ConnectionManager;

pub use relay_client::{AckEvent, IncomingResult, ProcessResult, RelayClient, RelayClientConfig};

pub use multi_relay::{
    MultiRelayConfig, MultiRelayConfigBuilder, MultiRelayError, MultiRelayManager, RelayHealth,
    RelaySelector,
};

pub use anonymous::{
    AnonymousSender, SenderIndex, compute_anonymous_id, compute_anonymous_id_for_device,
    current_epoch, resolve_sender, resolve_sender_device, resolve_sender_id,
};

pub use pinning::{PinnedCertificate, verify_pin};

#[cfg(feature = "network-http")]
pub use http_transport::{HttpTransport, HttpTransportConfig};
#[cfg(feature = "network-http")]
pub use vauchi_protocol::v2::FetchedBlob;

// OHTTP client-side encryption (RFC 9458)
#[cfg(feature = "network-http")]
pub use ohttp_client::{OhttpClient, ResponseDecryptor};

#[cfg(feature = "network-http")]
pub use http_adapter::HttpTransportAdapter;

pub mod delivery;
pub use delivery::error_messages::failure_to_user_message;
pub use delivery::{
    CleanupResult, ConnectivityDiagnostics, ConnectivityReport, DeliveryAckStatus, DeliveryService,
    KeyRotationDetector, KeyRotationError, OfflineManager, RetryScheduler, RetryTickResult,
};

/// Ensure a rustls crypto provider is installed globally.
///
/// `vauchi-app` compiles reqwest with `rustls-tls-webpki-roots-no-provider`
/// so that `vauchi-core`'s existing `aws_lc_rs` provider is reused instead
/// of pulling in `ring`. Calling this before constructing a reqwest `Client`
/// prevents the "No provider set" panic on the first HTTPS request.
#[cfg(feature = "network-rustls")]
pub fn ensure_rustls_provider_installed() {
    // Installing a provider is idempotent; drop the result so a prior
    // installation does not trip clippy::let_underscore_must_use.
    drop(rustls::crypto::aws_lc_rs::default_provider().install_default());
}

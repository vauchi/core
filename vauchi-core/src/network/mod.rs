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

// Error types
pub use error::NetworkError;

// Message types
pub use message::{
    AckStatus, Acknowledgment, DeviceSyncMessage, EncryptedUpdate, Handshake, MessageEnvelope,
    MessageId, MessagePayload, PresenceStatus, PresenceUpdate, RatchetHeader, PROTOCOL_VERSION,
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
pub use relay_client::{ProcessResult, RelayClient, RelayClientConfig};

// Multi-relay support
pub use multi_relay::{MultiRelayConfig, MultiRelayConfigBuilder, MultiRelayError, RelayHealth, RelaySelector};

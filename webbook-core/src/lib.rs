//! WebBook Core Library
//!
//! Privacy-focused contact card exchange library.
//! All cryptographic operations use the audited `ring` crate.

pub mod crypto;
pub mod identity;
pub mod contact_card;
pub mod exchange;
pub mod contact;
pub mod storage;
pub mod sync;
pub mod network;
pub mod api;
pub mod social;

pub use crypto::{SigningKeyPair, PublicKey, Signature, SymmetricKey, encrypt, decrypt, ExchangeKeyPair};
pub use identity::{Identity, IdentityBackup};
pub use contact_card::{ContactCard, ContactField, FieldType, ValidationError};
pub use exchange::{ExchangeQR, X3DH, X3DHKeyPair, ExchangeError, ProximityVerifier, ProximityError, MockProximityVerifier, ExchangeSession};
pub use contact::{Contact, FieldVisibility, VisibilityRules};
pub use storage::{Storage, StorageError, PendingUpdate, UpdateStatus};
pub use sync::{SyncState, SyncManager, SyncError, CardDelta, FieldChange, DeltaError};
pub use network::{NetworkError, Transport, RelayClient, RelayClientConfig, MockTransport, ConnectionState, MessageEnvelope};
pub use api::{WebBook, WebBookBuilder, WebBookConfig, WebBookError, WebBookResult, WebBookEvent};
pub use social::{SocialNetwork, SocialNetworkRegistry, ProfileValidation, TrustLevel, ValidationStatus};

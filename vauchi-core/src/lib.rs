//! Vauchi Core Library
//!
//! Privacy-focused contact card exchange library.
//! All cryptographic operations use the audited `ring` crate.

#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub mod api;
pub mod contact;
pub mod contact_card;
pub mod content;
pub mod crypto;
pub mod exchange;
pub mod identity;
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub mod network;
pub mod recovery;
pub mod social;
pub mod storage;
pub mod sync;

#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub use api::{Vauchi, VauchiBuilder, VauchiConfig, VauchiError, VauchiEvent, VauchiResult};
pub use contact::{
    Contact, FieldVisibility, LabelError, LabelManager, VisibilityLabel, VisibilityRules,
    MAX_LABELS, SUGGESTED_LABELS,
};
pub use contact_card::{
    is_allowed_scheme, is_blocked_scheme, is_safe_url, ContactCard, ContactField, FieldType,
    ValidationError,
};
pub use crypto::{decrypt, encrypt, PublicKey, Signature, SigningKeyPair, SymmetricKey};
pub use exchange::{
    EncryptedExchangeMessage, ExchangeError, ExchangeEvent, ExchangeQR, ExchangeSession,
    MockProximityVerifier, ProximityError, ProximityVerifier, X3DHKeyPair, X3DH,
};
pub use identity::{Identity, IdentityBackup};
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub use network::{
    ConnectionState, MessageEnvelope, MockTransport, NetworkError, RelayClient, RelayClientConfig,
    Transport, WebSocketTransport,
};
pub use recovery::{
    ConflictingClaim, RecoveryClaim, RecoveryConflict, RecoveryError, RecoveryProof,
    RecoveryReminder, RecoveryRevocation, RecoverySettings, RecoveryVoucher, VerificationResult,
};
pub use social::{
    ProfileValidation, SocialNetwork, SocialNetworkRegistry, TrustLevel, ValidationStatus,
};
pub use storage::{PendingUpdate, Storage, StorageError, UpdateStatus};
pub use sync::{CardDelta, DeltaError, FieldChange, SyncError, SyncManager, SyncState};

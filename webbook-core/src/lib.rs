//! WebBook Core Library
//!
//! Privacy-focused contact card exchange library.
//! All cryptographic operations use the audited `ring` crate.

#[cfg(feature = "network")]
pub mod api;
pub mod contact;
pub mod contact_card;
pub mod crypto;
pub mod exchange;
pub mod identity;
#[cfg(feature = "network")]
pub mod network;
pub mod recovery;
pub mod social;
pub mod storage;
pub mod sync;

#[cfg(feature = "network")]
pub use api::{WebBook, WebBookBuilder, WebBookConfig, WebBookError, WebBookEvent, WebBookResult};
pub use contact::{Contact, FieldVisibility, VisibilityRules};
pub use contact_card::{ContactCard, ContactField, FieldType, ValidationError};
pub use crypto::{decrypt, encrypt, PublicKey, Signature, SigningKeyPair, SymmetricKey};
pub use exchange::{
    ExchangeError, ExchangeQR, ExchangeSession, MockProximityVerifier, ProximityError,
    ProximityVerifier, X3DHKeyPair, X3DH,
};
pub use identity::{Identity, IdentityBackup};
#[cfg(feature = "network")]
pub use network::{
    ConnectionState, MessageEnvelope, MockTransport, NetworkError, RelayClient, RelayClientConfig,
    Transport,
};
pub use social::{
    ProfileValidation, SocialNetwork, SocialNetworkRegistry, TrustLevel, ValidationStatus,
};
pub use recovery::{
    ConflictingClaim, RecoveryClaim, RecoveryConflict, RecoveryError, RecoveryProof,
    RecoveryReminder, RecoveryRevocation, RecoverySettings, RecoveryVoucher, VerificationResult,
};
pub use storage::{PendingUpdate, Storage, StorageError, UpdateStatus};
pub use sync::{CardDelta, DeltaError, FieldChange, SyncError, SyncManager, SyncState};

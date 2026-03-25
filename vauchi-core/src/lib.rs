// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi Core Library
//!
//! Privacy-focused contact card exchange library.
//! Cryptographic operations use audited RustCrypto crates (`ed25519-dalek`, `x25519-dalek`,
//! `sha2`, `hmac`, `hkdf`, `chacha20poly1305`, `argon2`). TLS uses `aws-lc-rs` via rustls.
//!
//! App-layer modules (i18n, help, theme, ui, content) live in the `vauchi-app` crate.

pub mod crypto;
pub use crypto::{DhError, PublicKey, Signature, SigningKeyPair, SymmetricKey, decrypt, encrypt};

pub mod types;

pub mod text;
pub use types::{AudioCapability, ExchangeTransport, ProximityConfidence};
pub mod contact;
pub mod contact_card;
pub mod identity;
pub use contact::merge::DuplicatePair;
pub use contact::{
    Contact, ContactError, ContactKind, ExchangedData, FieldVisibility, Group, GroupError,
    GroupManager, ImportSource, ImportedData, MAX_LABELS, SUGGESTED_LABELS, VisibilityRules,
    resolve_visible_fields,
};
pub use contact_card::vcard_import::{VCardImportError, import_vcf};
pub use contact_card::{
    ContactCard, ContactField, FieldType, ValidationError, is_allowed_scheme, is_blocked_scheme,
    is_safe_url,
};
pub use identity::{Identity, IdentityBackup};

#[cfg(feature = "storage")]
pub mod storage;
#[cfg(feature = "storage")]
pub use storage::{PendingUpdate, Storage, StorageError, UpdateStatus};

pub mod exchange;
#[cfg(any(test, feature = "testing"))]
pub use exchange::MockProximityVerifier;
pub use exchange::capability;
pub use exchange::{
    EncryptedExchangeMessage, ExchangeCommand, ExchangeError, ExchangeEvent, ExchangeHardwareEvent,
    ExchangeQR, ExchangeSession, ProximityError, ProximityVerifier, TransportProximity,
    TrustMetrics, X3DH, X3DHKeyPair,
};

pub mod recovery;
pub use recovery::{
    ConflictingClaim, RecoveryClaim, RecoveryConflict, RecoveryError, RecoveryProof,
    RecoveryRateLimiter, RecoveryReminder, RecoveryResponse, RecoveryRevocation, RecoverySettings,
    RecoveryVoucher, VerificationResult,
};

#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub mod network;
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub use network::{
    ConnectionState, EmergencyAlert, GeoLocation, MessageEnvelope, MessageType, MockTransport,
    NetworkError, RelayClient, RelayClientConfig, Transport, WebSocketTransport, classify_message,
};
#[cfg(feature = "storage")]
pub mod sync;
#[cfg(feature = "storage")]
pub use sync::{CardDelta, DeltaError, FieldChange, SyncError, SyncManager, SyncState};

#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub mod api;
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub use api::{
    AppPasswordConfig, AuthMode, AuthResult, BroadcastResult, CallbackHandler, ConsentStatus,
    DuressAlert, DuressAlertType, DuressSettings, EmergencyBroadcastConfig, EmergencyWipeStatus,
    HandlerId, RecoveryReadiness, SetupProgress, Vauchi, VauchiBuilder, VauchiConfig, VauchiError,
    VauchiEvent, VauchiResult,
};
pub mod aha_moments;
pub use aha_moments::{AhaMoment, AhaMomentTracker, AhaMomentType};
pub mod demo_contact;
pub use demo_contact::{
    DEMO_CONTACT_ID, DEMO_CONTACT_NAME, DemoContactCard, DemoContactState, DemoTip,
    DemoTipCategory, generate_demo_contact_card, get_demo_tips,
};
pub mod diagnostic;
pub mod onboarding;
pub use onboarding::{OnboardingProgress, OnboardingStep, display_name_suggestions};
pub mod social;
pub use social::{
    ProfileValidation, SocialNetwork, SocialNetworkRegistry, ValidationConfidence,
    ValidationStatus, ValidatorMeta, calculate_trust_weight, check_sybil_resistance,
    filter_blocked_validations,
};

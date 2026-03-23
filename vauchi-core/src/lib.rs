// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi Core Library
//!
//! Privacy-focused contact card exchange library.
//! Cryptographic operations use audited RustCrypto crates (`ed25519-dalek`, `x25519-dalek`,
//! `sha2`, `hmac`, `hkdf`, `chacha20poly1305`, `argon2`). TLS uses `aws-lc-rs` via rustls.

// --- future vauchi-crypto ---
pub mod crypto;
pub use crypto::{DhError, PublicKey, Signature, SigningKeyPair, SymmetricKey, decrypt, encrypt};

// --- future vauchi-i18n ---
pub mod i18n;
pub use i18n::{
    I18nError, Locale, LocaleInfo, get_all_strings, get_available_locales, get_locale_info,
    get_string, get_string_with_args,
};

// --- future vauchi-types ---
pub mod types;
pub use types::{AudioCapability, ExchangeTransport, ProximityConfidence};
pub mod contact;
pub mod contact_card;
pub mod identity;
pub mod tor_config;
pub use contact::merge::DuplicatePair;
pub use contact::{
    Contact, FieldVisibility, Group, GroupError, GroupManager, MAX_LABELS, SUGGESTED_LABELS,
    VisibilityRules, resolve_visible_fields,
};
pub use contact_card::{
    ContactCard, ContactField, FieldType, ValidationError, is_allowed_scheme, is_blocked_scheme,
    is_safe_url,
};
pub use identity::{Identity, IdentityBackup};
pub use tor_config::{TorConfig, TorConfigError, TorRelayAddress, TorStatus};

// --- future vauchi-content ---
pub mod content;

// --- future vauchi-storage ---
#[cfg(feature = "storage")]
pub mod storage;
#[cfg(feature = "storage")]
pub use storage::{PendingUpdate, Storage, StorageError, UpdateStatus};

// --- future vauchi-exchange ---
pub mod exchange;
#[cfg(any(test, feature = "testing"))]
pub use exchange::MockProximityVerifier;
pub use exchange::capability;
pub use exchange::{
    EncryptedExchangeMessage, ExchangeCommand, ExchangeError, ExchangeEvent, ExchangeHardwareEvent,
    ExchangeQR, ExchangeSession, ProximityError, ProximityVerifier, X3DH, X3DHKeyPair,
};

// --- future vauchi-recovery ---
pub mod recovery;
pub use recovery::{
    ConflictingClaim, RecoveryClaim, RecoveryConflict, RecoveryError, RecoveryProof,
    RecoveryRateLimiter, RecoveryReminder, RecoveryResponse, RecoveryRevocation, RecoverySettings,
    RecoveryVoucher, VerificationResult,
};

// --- future vauchi-network ---
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

// --- stays in vauchi-core (orchestrator) ---
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
pub mod help;
pub use help::{
    FaqItem, HelpCategory, get_faq_by_id, get_faq_by_id_localized, get_faqs, get_faqs_by_category,
    get_faqs_by_category_localized, get_faqs_localized, search_faqs, search_faqs_localized,
};
pub mod onboarding;
pub use onboarding::{OnboardingProgress, OnboardingStep, display_name_suggestions};
pub mod social;
pub use social::{
    ProfileValidation, SocialNetwork, SocialNetworkRegistry, TrustLevel, ValidationStatus,
    ValidatorMeta, calculate_trust_weight, check_sybil_resistance, filter_blocked_validations,
};
pub mod theme;
pub use theme::{
    BorderRadiusTokens, DesignTokens, SpacingTokens, Theme, ThemeColors, ThemeError, ThemeMode,
    TypographyTokens, default_theme, load_themes_from_json, validate_hex_color,
};
pub mod ui;

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi Core Library
//!
//! Privacy-focused contact card exchange library.
//! All cryptographic operations use the audited `aws-lc-rs` crate (FIPS 140-3 certified).

// --- future vauchi-crypto ---
pub mod crypto;
pub use crypto::{decrypt, encrypt, PublicKey, Signature, SigningKeyPair, SymmetricKey};

// --- future vauchi-i18n ---
pub mod i18n;
pub use i18n::{
    get_all_strings, get_available_locales, get_locale_info, get_string, get_string_with_args,
    I18nError, Locale, LocaleInfo,
};

// --- future vauchi-types ---
pub mod contact;
pub mod contact_card;
pub mod identity;
pub mod tor_config;
pub use contact::merge::DuplicatePair;
pub use contact::{
    resolve_visible_fields, Contact, FieldVisibility, Group, GroupManager, LabelError,
    VisibilityRules, MAX_LABELS, SUGGESTED_LABELS,
};
pub use contact_card::{
    is_allowed_scheme, is_blocked_scheme, is_safe_url, ContactCard, ContactField, FieldType,
    ValidationError,
};
pub use identity::{Identity, IdentityBackup};
pub use tor_config::{TorConfig, TorRelayAddress, TorStatus};

// --- future vauchi-content ---
pub mod content;

// --- future vauchi-storage ---
pub mod storage;
pub use storage::{PendingUpdate, Storage, StorageError, UpdateStatus};

// --- future vauchi-exchange ---
pub mod capability;
pub mod exchange;
pub use exchange::{
    EncryptedExchangeMessage, ExchangeError, ExchangeEvent, ExchangeQR, ExchangeSession,
    MockProximityVerifier, ProximityConfidence, ProximityError, ProximityVerifier, X3DHKeyPair,
    X3DH,
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
    classify_message, ConnectionState, EmergencyAlert, GeoLocation, MessageEnvelope, MessageType,
    MockTransport, NetworkError, RelayClient, RelayClientConfig, Transport, WebSocketTransport,
};
pub mod sync;
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
    generate_demo_contact_card, get_demo_tips, DemoContactCard, DemoContactState, DemoTip,
    DemoTipCategory, DEMO_CONTACT_ID, DEMO_CONTACT_NAME,
};
pub mod diagnostic;
pub mod help;
pub use help::{
    get_faq_by_id, get_faq_by_id_localized, get_faqs, get_faqs_by_category,
    get_faqs_by_category_localized, get_faqs_localized, search_faqs, search_faqs_localized,
    FaqItem, HelpCategory,
};
pub mod onboarding;
pub use onboarding::{display_name_suggestions, OnboardingProgress, OnboardingStep};
pub mod social;
pub use social::{
    calculate_trust_weight, check_sybil_resistance, filter_blocked_validations, ProfileValidation,
    SocialNetwork, SocialNetworkRegistry, TrustLevel, ValidationStatus, ValidatorMeta,
};
pub mod theme;
pub use theme::{
    default_theme, load_themes_from_json, validate_hex_color, BorderRadiusTokens, DesignTokens,
    SpacingTokens, Theme, ThemeColors, ThemeError, ThemeMode, TypographyTokens,
};
pub mod ui;

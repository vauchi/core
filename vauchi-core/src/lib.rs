// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi Core Library
//!
//! Privacy-focused contact card exchange library.
//! All cryptographic operations use the audited `ring` crate.

pub mod aha_moments;
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub mod api;
pub mod capability;
pub mod contact;
pub mod contact_card;
pub mod content;
pub mod crypto;
pub mod demo_contact;
pub mod exchange;
pub mod help;
pub mod i18n;
pub mod identity;
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub mod network;
pub mod recovery;
pub mod social;
pub mod storage;
pub mod sync;
pub mod theme;
pub mod tor_config;

pub use aha_moments::{AhaMoment, AhaMomentTracker, AhaMomentType};
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub use api::{
    AppPasswordConfig, AuthMode, AuthResult, BroadcastResult, CallbackHandler, ConsentStatus,
    DuressAlert, DuressAlertType, DuressSettings, EmergencyBroadcastConfig, HandlerId,
    RecoveryReadiness, Vauchi, VauchiBuilder, VauchiConfig, VauchiError, VauchiEvent, VauchiResult,
};
pub use contact::{
    Contact, FieldVisibility, LabelError, LabelManager, VisibilityLabel, VisibilityRules,
    MAX_LABELS, SUGGESTED_LABELS,
};
pub use contact_card::{
    is_allowed_scheme, is_blocked_scheme, is_safe_url, ContactCard, ContactField, FieldType,
    ValidationError,
};
pub use crypto::{decrypt, encrypt, PublicKey, Signature, SigningKeyPair, SymmetricKey};
pub use demo_contact::{
    generate_demo_contact_card, get_demo_tips, DemoContactCard, DemoContactState, DemoTip,
    DemoTipCategory, DEMO_CONTACT_ID, DEMO_CONTACT_NAME,
};
pub use exchange::{
    EncryptedExchangeMessage, ExchangeError, ExchangeEvent, ExchangeQR, ExchangeSession,
    MockProximityVerifier, ProximityConfidence, ProximityError, ProximityVerifier, X3DHKeyPair,
    X3DH,
};
pub use help::{
    get_faq_by_id, get_faq_by_id_localized, get_faqs, get_faqs_by_category,
    get_faqs_by_category_localized, get_faqs_localized, search_faqs, search_faqs_localized,
    FaqItem, HelpCategory,
};
pub use i18n::{
    get_all_strings, get_available_locales, get_locale_info, get_string, get_string_with_args,
    I18nError, Locale, LocaleInfo,
};
pub use identity::{Identity, IdentityBackup};
#[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
pub use network::{
    classify_message, ConnectionState, EmergencyAlert, GeoLocation, MessageEnvelope, MessageType,
    MockTransport, NetworkError, RelayClient, RelayClientConfig, Transport, WebSocketTransport,
};
pub use recovery::{
    ConflictingClaim, RecoveryClaim, RecoveryConflict, RecoveryError, RecoveryProof,
    RecoveryRateLimiter, RecoveryReminder, RecoveryResponse, RecoveryRevocation, RecoverySettings,
    RecoveryVoucher, VerificationResult,
};
pub use social::{
    calculate_trust_weight, check_sybil_resistance, filter_blocked_validations, ProfileValidation,
    SocialNetwork, SocialNetworkRegistry, TrustLevel, ValidationStatus, ValidatorMeta,
};
pub use storage::{PendingUpdate, Storage, StorageError, UpdateStatus};
pub use sync::{CardDelta, DeltaError, FieldChange, SyncError, SyncManager, SyncState};
pub use theme::{
    default_theme, load_themes_from_json, validate_hex_color, Theme, ThemeColors, ThemeError,
    ThemeMode,
};
pub use tor_config::{TorConfig, TorRelayAddress, TorStatus};

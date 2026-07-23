// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

// Memory safety by construction (ADR-055): core carries no unsafe. The
// FFI crates (vauchi-platform, vauchi-cabi) are exempt — forbid is
// per-crate, not workspace-wide.
#![forbid(unsafe_code)]

//! Vauchi Core Library
//!
//! Privacy-focused contact card exchange library.
//! Cryptographic operations use audited RustCrypto crates (`ed25519-dalek`, `x25519-dalek`,
//! `sha2`, `hmac`, `hkdf`, `chacha20poly1305`, `argon2`). TLS uses `aws-lc-rs` via rustls.
//!
//! App-layer modules (i18n, help, theme, ui, content) live in the `vauchi-app` crate.

// Silent error swallowing is a real security risk in this crate
// (storage faults conflated with absent state, AEAD decrypt failure
// indistinguishable from network drop, etc.). The 2026-05-21 audit
// confirmed ~11 sites with privacy/safety impact. This crate-level
// warn forces every `let _ = fallible_call()` site to be either
// propagated or annotated with `#[allow(...)]` + a justification.
// See `_private/docs/problems/2026-05-21-silent-failures-in-security-paths/`.
#![warn(clippy::let_underscore_must_use)]

pub mod clock;
pub mod monotonic;
pub mod rng;
pub mod sleeper;

pub mod crypto;
pub use crypto::{DhError, PublicKey, Signature, SigningKeyPair, SymmetricKey, decrypt, encrypt};

#[cfg(feature = "flame")]
pub mod flame;

// Auto-install the flame subscriber when the crate is compiled with the
// `flame` feature AND `cfg(test)`. Without `cfg(test)` this would also
// fire for downstream consumers that happen to enable the feature, which
// is undesirable — flame is a dev tool.
#[cfg(all(test, feature = "flame"))]
#[ctor::ctor]
fn _flame_install() {
    crate::flame::init_layer();
}

pub mod emergency;
pub mod exchange_types;
pub mod reminders;
pub mod settings;
pub mod types;
pub mod visibility;

pub mod identifiers;

pub mod text;

#[cfg(feature = "storage")]
pub mod install_id;
pub use types::{
    AhaMomentTracker, AhaMomentType, AudioCapability, BackupReminderState, BiometricUnlockOutcome,
    ConsentRecord, ConsentType, DEFAULT_EMERGENCY_MESSAGE, DemoContactState, DuressSettings,
    EmergencyBroadcastConfig, EmergencyWipeStatus, EventOrigin, ExchangeTransport,
    MAX_TRUSTED_CONTACTS, OnboardingProgress, OnboardingStep, OwnCardRepropagateState,
    ProximityConfidence, ReminderFrequency,
};
pub mod consent;
pub mod contact;
pub mod contact_card;
pub mod identity;
pub use contact::display::{
    AvatarOption, AvatarPreference, ContactDisplayOptions, DisplayNamePreference, NameOption,
    SharedAvatar, SharedName,
};
pub use contact::merge::DuplicatePair;
pub use contact::{
    Contact, ContactError, ContactKind, ExchangedData, FieldVisibility, Group, GroupError,
    GroupManager, ImportSource, ImportedData, MAX_LABELS, SUGGESTED_LABELS, VisibilityRules,
};
pub use contact_card::vcard_import::{VCardImportError, import_vcf};
pub use contact_card::{
    ContactCard, ContactField, FieldType, ValidationError, is_allowed_scheme, is_blocked_scheme,
    is_safe_url, is_valid_relay_url, normalize_avatar,
};
pub use identity::{Identity, IdentityBackup};

#[cfg(feature = "storage")]
pub mod storage;
#[cfg(feature = "storage")]
pub use storage::{
    ActivityLogStore, ConsentStore, ContactStore, DecoyStore, DeliveryStore, DeviceDeliveryStore,
    DeviceStore, DuressStore, EmergencyStore, FieldNoteStore, IdentityStore, LabelStore,
    OhttpCacheStore, PendingStore, PendingUpdate, PinCacheStore, PlaceStore, RatchetStore,
    RecoveryStore, ReplayStore, RetryStore, Storage, StorageError, SyncStore, TagStore,
    UpdateStatus, UxStore,
};

pub mod exchange;
#[cfg(any(test, feature = "testing"))]
pub use exchange::MockProximityVerifier;
pub use exchange::capability;
pub use exchange::{
    EncryptedExchangeMessage, ExchangeError, ExchangeEvent, ExchangeQR, ExchangeSession,
    ProximityError, ProximityVerifier, TransportProximity, TrustMetrics, X3DH, X3DHKeyPair,
};

pub mod platform;
pub use platform::{BleLinkDirection, Command, Event, FilePickPurpose, Orientation};

pub mod recovery;
pub mod relay_url;
pub use recovery::{
    ConflictingClaim, RecoveryClaim, RecoveryConflict, RecoveryError, RecoveryProgress,
    RecoveryProof, RecoveryRateLimiter, RecoveryReminder, RecoveryResponse, RecoveryRevocation,
    RecoverySettings, RecoveryVoucher, VerificationResult,
};

#[cfg(feature = "network-rustls")]
pub mod network;
#[cfg(feature = "network-rustls")]
pub use network::{
    ConnectionState, EmergencyAlert, GeoLocation, MessageEnvelope, MockTransport, NetworkError,
    RelayClient, RelayClientConfig, Transport,
};
#[cfg(feature = "storage")]
pub mod sync;
#[cfg(feature = "storage")]
pub use sync::{CardDelta, DeltaError, FieldChange, SyncState};

#[cfg(feature = "network-rustls")]
pub mod api;
#[cfg(feature = "network-rustls")]
pub use api::{
    AppPasswordConfig, AuthMode, AuthResult, BIOMETRIC_UNLOCK_MIN_DURATION, BroadcastResult,
    ConsentStatus, DeviceSyncOrchestrator, DuressAlert, DuressAlertType, GroupDraft, HandlerId,
    RecoveryReadiness, SearchFacets, SetupProgress, SyncError, SyncManager, Vauchi, VauchiBuilder,
    VauchiConfig, VauchiError, VauchiEvent, VauchiResult, VauchiSyncOutcome,
};
#[cfg(all(feature = "network-rustls", feature = "network-http"))]
pub use api::{PERIODIC_SYNC_INTERVAL_SECONDS, PERIODIC_SYNC_MAX_RETRIES};
pub mod aha_moments;
pub mod avatar;
pub use aha_moments::AhaMoment;
pub mod backup;
pub use backup::{
    BackupError, BackupKey, BackupKeyShard, BackupSections, FullBackupEnvelope,
    FullBackupIdentityData, IdentitySection, KeyShardConfig, KeyShardError, LabelSection,
    export_contact_backup, export_full_backup, export_guardian_backup, extract_master_seed,
    import_contact_backup, import_full_backup, import_guardian_backup, open_share_for_guardian,
    reconstruct_backup_key, restore_contacts_from_envelope, seal_share_for_guardian,
    split_backup_key,
};
pub mod demo_contact;
pub use demo_contact::{
    DEMO_CONTACT_ID, DEMO_CONTACT_NAME, DemoContactCard, DemoTip, DemoTipCategory,
    generate_demo_contact_card, get_demo_tips,
};
pub mod diagnostic;
pub mod onboarding;
pub mod qr;
pub use onboarding::display_name_suggestions;
pub mod social;
pub use social::{SocialNetwork, SocialNetworkRegistry};
pub mod version;

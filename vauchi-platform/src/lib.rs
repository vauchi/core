// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

// Deprecated direct help FFI functions are kept for backward compatibility
// with iOS/Android until Tasks 13-14 migrate them to ScreenModel routing.
#![allow(deprecated)]

//! Vauchi Mobile Bindings
//!
//! UniFFI bindings for Android and iOS platforms.
//! Exposes a simplified, mobile-friendly API on top of vauchi-core.
//!
//! Note: Storage connections are created on-demand for thread safety,
//! as rusqlite's Connection is not Sync.

use std::sync::Arc;

use vauchi_core::SymmetricKey;

// === Modules ===

mod content;
mod diagnostic;
mod domain_command;
mod error;
mod exchange;
mod exchange_view;
mod json_helpers;
mod mobile_contact_detail;
mod mobile_contacts;
mod mobile_import;
mod mobile_visibility;
mod multipart_qr;
mod pae_dispatch;
mod platform_app_engine;
mod platform_app_engine_internals;
mod platform_app_engine_test_helpers;
mod policies;
mod protocol;
mod types;
mod validation;

// Re-export public types
pub use content::{
    MobileApplyFailure, MobileApplyResult, MobileContentConfig, MobileContentType,
    MobileUpdateStatus,
};
// Production QR surface — always available.
// - generate_qr_bitmap: own-card QR display on Android/iOS
// - MobileQrBitmap, MobileQrEccLevel: params/return types for generation
// - MobileScannerBackend, MobileScanResult, scan_qr: production scanner
//   (rxing/rqrr pipeline from `vauchi_core::qr::scanner`)
pub use diagnostic::{
    MobileQrBitmap, MobileQrEccLevel, MobileScanResult, MobileScannerBackend, generate_qr_bitmap,
    scan_qr,
};

// Diagnostic benchmark harness surface — only built with
// `--features diagnostic-scanner`. Pulls imageproc + fast_image_resize
// and ~20 transitive crates. Must never ship in default production builds.
#[cfg(feature = "diagnostic-scanner")]
pub use diagnostic::{
    MobileCameraConfig, MobileDeviceCapabilityProfile, MobileFpsRange, MobilePlatform,
    MobilePreprocessConfig, MobileQrConfig, MobileQrTestPattern, MobileScoredConfig,
    MobileSweepMatrix, MobileThroughputFrame, MobileTuningResult,
    diagnostic_generate_extended_qr_test_patterns, diagnostic_generate_qr_test_patterns,
    diagnostic_generate_sweep_matrix, diagnostic_generate_throughput_sequence,
    diagnostic_rank_configs, diagnostic_scan_qr_with_config, diagnostic_score_config,
};
pub use domain_command::{DomainCommand, DomainCommandResult};
pub use error::{KeychainError, MobileError};
pub use exchange::{MobileCommand, MobileEvent, MobileExchangeState};
pub use exchange_view::{MobileExchangeViewState, exchange_view_state};
pub use mobile_contact_detail::{
    MobileContactDetailAction, MobileContactDetailBadge, MobileContactDetailBanner,
    MobileContactDetailViewState,
};
pub use mobile_import::{MobileImportResult, MobileImportWarning};
pub use multipart_qr::{MultipartDecoder, encode_multipart};
pub use platform_app_engine::{PlatformAppEngine, PlatformEventListener};
#[doc(hidden)]
pub use platform_app_engine_test_helpers::PlatformAppEngineTestHelpers;
pub use policies::{
    MobileClipboardPolicy, mobile_clipboard_policy, mobile_generate_storage_key,
    mobile_storage_key_byte_length,
};
pub use types::{
    MobileAhaMoment, MobileAhaMomentType, MobileAuthMode, MobileBiometricUnlockOutcome,
    MobileBorderRadiusTokens, MobileBroadcastResult, MobileConsentRecord, MobileConsentStatus,
    MobileConsentType, MobileContact, MobileContactCard, MobileContactField,
    MobileContactTrustLevel, MobileDecoyContact, MobileDeletionInfo, MobileDeletionState,
    MobileDeliveryRecord, MobileDeliveryStatus, MobileDeliverySummary, MobileDemoContact,
    MobileDemoContactState, MobileDesignTokens, MobileDeviceDeliveryRecord,
    MobileDeviceDeliveryStatus, MobileDeviceInfo, MobileDeviceJoinResult,
    MobileDeviceLinkConfirmation, MobileDeviceLinkData, MobileDeviceLinkInfo,
    MobileDeviceLinkRequest, MobileDeviceLinkResult, MobileDuressSettings, MobileEmergencyConfig,
    MobileExchangeResult, MobileFieldNote, MobileFieldType, MobileGdprExport,
    MobileLabelContactBadge, MobileLabelContactRow, MobileLabelContactStatus, MobileLocale,
    MobileLocaleInfo, MobileMotionTokens, MobileNotificationCategory, MobileOnboardingProgress,
    MobileOnboardingStep, MobilePendingNotification, MobileRecoveryClaim, MobileRecoveryProgress,
    MobileRecoveryVerification, MobileRecoveryVoucher, MobileRetryEntry, MobileShredReport,
    MobileShredStatus, MobileShredToken, MobileSocialNetwork, MobileSpacingDirectionTokens,
    MobileSpacingTokens, MobileSyncIndicatorState, MobileSyncResult, MobileSyncStatus,
    MobileSyncStatusKind, MobileSyncStatusView, MobileTabInfo, MobileTabLayout, MobileTheme,
    MobileThemeColors, MobileThemeMode, MobileTouchTargetTokens, MobileTypographyTokens,
    MobileVisibilityLabel, MobileVisibilityLabelDetail, sync_status_view,
};
pub use validation::{
    mobile_is_valid_email, mobile_is_valid_pem_certificate, mobile_is_valid_phone,
    mobile_is_valid_relay_url, passcode_max_length, passcode_min_length, password_min_length,
    recovery_claim_min_input_length, recovery_public_key_hex_length,
};

uniffi::setup_scaffolding!();

/// Return the Rust core library version (compile-time constant).
///
/// Mobile apps log this at startup to detect mismatched builds.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Return the app compatibility version (monotonic u16).
///
/// Mobile apps send this as `X-App-Compat-Version` header.
#[uniffi::export]
pub fn app_compat_version() -> u16 {
    vauchi_core::version::APP_COMPAT_VERSION
}

// === Platform Secure Storage Callback ===

/// Callback interface for platform-specific secure key storage.
///
/// The mobile platform (iOS/Android) implements this interface to provide
/// access to the native keychain (iOS Keychain, Android KeyStore).
/// Used by shred operations to destroy the Shredding Master Key (SMK).
#[uniffi::export(callback_interface)]
pub trait MobilePlatformKeychain: Send + Sync {
    /// Saves a key to the platform keychain.
    fn save_key(&self, name: String, key: Vec<u8>) -> Result<(), KeychainError>;

    /// Loads a key from the platform keychain.
    /// Returns None if the key doesn't exist.
    fn load_key(&self, name: String) -> Result<Option<Vec<u8>>, KeychainError>;

    /// Deletes a key from the platform keychain.
    fn delete_key(&self, name: String) -> Result<(), KeychainError>;
}

/// Bridge that adapts the UniFFI callback interface to vauchi-core's SecureStorage trait.
///
/// `pub(crate)` so `PlatformAppEngine` (sibling module) can build a bridge
/// from its own keychain for the shred `DomainCommand` path (B7).
pub(crate) struct KeychainBridge {
    pub(crate) callback: Arc<dyn MobilePlatformKeychain>,
}

impl vauchi_core::storage::SecureStorage for KeychainBridge {
    fn save_key(&self, name: &str, key: &[u8]) -> Result<(), vauchi_core::StorageError> {
        self.callback
            .save_key(name.to_string(), key.to_vec())
            .map_err(|e| vauchi_core::StorageError::Encryption(e.to_string()))
    }

    fn load_key(&self, name: &str) -> Result<Option<Vec<u8>>, vauchi_core::StorageError> {
        self.callback
            .load_key(name.to_string())
            .map_err(|e| vauchi_core::StorageError::Encryption(e.to_string()))
    }

    fn delete_key(&self, name: &str) -> Result<(), vauchi_core::StorageError> {
        self.callback
            .delete_key(name.to_string())
            .map_err(|e| vauchi_core::StorageError::Encryption(e.to_string()))
    }
}

// === Thread-safe state ===

/// Generate a new random storage key.
///
/// Use this when setting up a new installation with secure storage.
/// The returned bytes should be stored in platform secure storage
/// (iOS Keychain or Android KeyStore).
#[uniffi::export]
pub fn generate_storage_key() -> Vec<u8> {
    SymmetricKey::generate().as_bytes().to_vec()
}

/// Check if a URL is safe to open in an external application.
///
/// Returns `true` for allowed schemes: http, https, tel, mailto, sms, geo.
/// Returns `false` for blocked schemes (javascript, data, file, etc.) or unknown schemes.
///
/// Use this to validate URLs before opening them to prevent security issues.
#[uniffi::export]
pub fn is_safe_url(url: String) -> bool {
    vauchi_core::is_safe_url(&url)
}

/// Validate a relay WebSocket URL.
///
/// Accepts `https://` for any host, `http://` only for localhost/loopback.
/// Use this to validate user-entered relay URLs before saving.
#[uniffi::export]
pub fn is_valid_relay_url(url: String) -> bool {
    vauchi_core::is_valid_relay_url(&url)
}

/// Check if a URL scheme is in the allowed list.
///
/// Allowed schemes: tel, mailto, sms, https, http, geo.
#[uniffi::export]
pub fn is_allowed_scheme(scheme: String) -> bool {
    vauchi_core::is_allowed_scheme(&scheme)
}

/// Check if a URL scheme is explicitly blocked.
///
/// Blocked schemes: javascript, vbscript, data, file, ftp, blob.
#[uniffi::export]
pub fn is_blocked_scheme(scheme: String) -> bool {
    vauchi_core::is_blocked_scheme(&scheme)
}

// ============================================================
// Theme Functions
// ============================================================

const THEMES_JSON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/themes.json"));

/// Get all available themes from themes.json.
#[uniffi::export]
pub fn get_available_themes() -> Vec<MobileTheme> {
    vauchi_app::theme::load_themes_from_json(THEMES_JSON)
        .unwrap_or_else(|_| vec![vauchi_app::theme::default_theme()])
        .iter()
        .map(MobileTheme::from)
        .collect()
}

/// Get a specific theme by ID.
///
/// Returns None if the theme is not found.
#[uniffi::export]
pub fn get_theme(theme_id: String) -> Option<MobileTheme> {
    vauchi_app::theme::load_themes_from_json(THEMES_JSON)
        .unwrap_or_default()
        .iter()
        .find(|t| t.id == theme_id)
        .map(MobileTheme::from)
}

/// Get the default theme ID based on system preference.
///
/// Returns "catppuccin-mocha" for dark mode, "catppuccin-latte" for light mode.
#[uniffi::export]
pub fn get_default_theme_id(prefer_dark: bool) -> String {
    if prefer_dark {
        "catppuccin-mocha".to_string()
    } else {
        "catppuccin-latte".to_string()
    }
}

// ============================================================
// i18n Functions
// ============================================================

/// Initialize the i18n system by loading locale files from a directory.
///
/// Must be called once at app startup before any i18n functions.
/// The resource_dir should point to a directory containing locale JSON files
/// (e.g., en.json, de.json, fr.json, es.json).
#[uniffi::export]
pub fn init_locales(resource_dir: String) -> Result<(), MobileError> {
    vauchi_app::i18n::init(std::path::Path::new(&resource_dir)).map_err(|e| MobileError::Other {
        detail: e.to_string(),
    })
}

/// Get all available locales.
#[uniffi::export]
pub fn get_available_locales() -> Vec<MobileLocaleInfo> {
    vauchi_app::i18n::get_available_locales()
        .into_iter()
        .map(|l| MobileLocaleInfo::from(vauchi_app::i18n::get_locale_info(l)))
        .collect()
}

/// Get information about a specific locale.
#[uniffi::export]
pub fn get_locale_info(locale: MobileLocale) -> MobileLocaleInfo {
    MobileLocaleInfo::from(vauchi_app::i18n::get_locale_info(locale.into()))
}

/// Get a localized string by key.
///
/// Falls back to English if the key is not found in the requested locale.
#[uniffi::export]
pub fn get_string(locale: MobileLocale, key: String) -> String {
    types::mobile_get_string(locale, key)
}

/// Get a localized string with argument interpolation.
///
/// Arguments are replaced in the string using {placeholder} syntax.
/// Falls back to English if the key is not found in the requested locale.
#[uniffi::export]
pub fn get_string_with_args(
    locale: MobileLocale,
    key: String,
    args: std::collections::HashMap<String, String>,
) -> String {
    types::mobile_get_string_with_args(locale, key, args)
}

/// Parse a locale code to MobileLocale.
///
/// Supports codes like "en", "en-US", "de-DE", etc.
/// Returns None if the code is not recognized.
#[uniffi::export]
pub fn parse_locale_code(code: String) -> Option<MobileLocale> {
    vauchi_app::i18n::Locale::from_code(&code).map(MobileLocale::from)
}

/// Get localized aha moment content for a given moment type.
///
/// Returns the title, message, and animation flag for display.
/// This is a stateless helper — it doesn't check whether the moment
/// has been seen. Use `try_trigger_aha_moment` on VauchiPlatform for
/// state-tracked triggering.
#[uniffi::export]
pub fn get_aha_moment_localized(
    moment_type: MobileAhaMomentType,
    locale: MobileLocale,
) -> MobileAhaMoment {
    let core_type: vauchi_core::AhaMomentType = moment_type.into();
    let core_locale: vauchi_app::i18n::Locale = locale.into();
    MobileAhaMoment {
        moment_type,
        title: vauchi_app::aha_moment_title_localized(core_type, core_locale),
        message: vauchi_app::aha_moment_message_localized(core_type, core_locale),
        has_animation: core_type.has_animation(),
    }
}

// === Widget Panic Shred ===

/// Widget confirmation mode for panic shred activation.
///
/// Defines how the user confirms a panic shred from the home screen widget.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum MobileWidgetConfirmationMode {
    /// Default: tap once, then confirm in a dialog.
    TapConfirm,
    /// Long press to trigger.
    LongPress,
    /// Double tap to trigger.
    DoubleTap,
}

/// Panic shred callable from a widget without full app initialization.
///
/// This is the key API for iOS/Android home screen widgets that need to
/// trigger emergency data destruction without opening the full app or
/// calling `open_vauchi()`.
///
/// Only requires:
/// - `data_dir`: The app's data directory path (String for UniFFI compat)
/// - `keychain`: Platform keychain callback for SMK destruction
///
/// **WARNING**: This operation is irreversible and immediate. All identity
/// data in the specified directory will be permanently destroyed.
#[uniffi::export]
pub fn widget_panic_shred(
    data_dir: String,
    keychain: Box<dyn MobilePlatformKeychain>,
) -> Result<MobileShredReport, MobileError> {
    let bridge = KeychainBridge {
        callback: Arc::from(keychain),
    };
    let path = std::path::Path::new(&data_dir);
    let report =
        vauchi_core::api::widget_panic_shred(path, &bridge).map_err(|e| MobileError::Other {
            detail: e.to_string(),
        })?;
    Ok(MobileShredReport::from(&report))
}

// === Shred Network Senders ===

/// Sends relay purge and revocation messages via HTTP transport during shred.
///
/// `pub(crate)` + `pub(crate) from_transport` so `PlatformAppEngine`'s
/// shred dispatch arms (B7 Phase 1b) can build purge/revocation senders.
pub(crate) struct MobileRelaySender {
    client: vauchi_core::network::RelayClient<vauchi_core::network::HttpTransportAdapter>,
}

impl MobileRelaySender {
    /// Wrap an already-built `HttpTransport` into a relay sender.
    ///
    /// The transport should be constructed via `Vauchi::build_relay_transport`
    /// so that shred requests flow through OHTTP when a gateway key is
    /// cached (ADR-037). A panic-wipe triggered before the first
    /// successful `connect()` falls back to direct HTTP — this is the
    /// narrow availability trade-off tracked by problem record
    /// `_private/docs/problems/2026-04-17-ohttp-allow-direct-fallback/`:
    /// the shred must complete even when OHTTP is unreachable, but once
    /// a key is cached the transport fails closed.
    pub(crate) fn from_transport(
        transport: vauchi_core::network::HttpTransport,
        relay_url: String,
        sender_id: &str,
    ) -> Self {
        use vauchi_core::network::{
            HttpTransportAdapter, RelayClient, RelayClientConfig, TransportConfig,
        };
        let adapter = HttpTransportAdapter::new(transport);
        let config = RelayClientConfig {
            transport: TransportConfig {
                server_url: relay_url,
                ..TransportConfig::default()
            },
            ..RelayClientConfig::default()
        };
        Self {
            client: RelayClient::new(adapter, config, sender_id.to_string()),
        }
    }
}

impl vauchi_core::api::RevocationSender for MobileRelaySender {
    fn send_revocation_delivery(
        &mut self,
        token: &str,
        blob_b64: &str,
        now: u64,
    ) -> Result<bool, vauchi_core::api::ShredError> {
        self.client
            .connect()
            .map_err(|e| vauchi_core::api::ShredError::FileError(format!("Connect: {e}")))?;
        self.client.send_revocation_delivery(token, blob_b64, now)
    }
}

impl vauchi_core::api::PurgeSender for MobileRelaySender {
    fn send_purge(
        &mut self,
        purge: &vauchi_core::api::PreSignedPurgeRequest,
        now: u64,
    ) -> Result<bool, vauchi_core::api::ShredError> {
        self.client
            .connect()
            .map_err(|e| vauchi_core::api::ShredError::FileError(format!("Connect: {e}")))?;
        self.client.send_purge(purge, now)
    }
}

// Methods extracted to child modules:
// - mobile_identity.rs: Identity operations, aha moments, demo contact
// - mobile_contacts.rs: Contact card/CRUD, hidden contacts, pagination, social networks, field validation
// - mobile_visibility.rs: Visibility operations and labels
// - mobile_exchange.rs: Contact exchange operations
// - mobile_delivery.rs: Sync, delivery status, retry/offline queue, multi-device, backup, async sync
// - mobile_gdpr.rs: GDPR, crypto-shredding, consent
// - mobile_content.rs: Content updates (feature-gated)

// INLINE_TEST_REQUIRED: Tests require tempfile for VauchiPlatform instance creation
#[cfg(test)]
mod tests {
    use super::*;

    // @scenario: device_sync:Sync result aggregation
    #[test]
    fn test_mobile_sync_result_total_and_has_changes() {
        let empty = MobileSyncResult {
            contacts_added: 0,
            cards_updated: 0,
            updates_sent: 0,
            total: 0,
            has_changes: false,
            updated_contact_names: vec![],
            blobs_fetched: 0,
            rejected: 0,
            unresolved: 0,
            reject_reasons: String::new(),
        };
        assert_eq!(empty.total, 0);
        assert!(!empty.has_changes);

        let with_changes = MobileSyncResult {
            contacts_added: 2,
            cards_updated: 1,
            updates_sent: 3,
            total: 6,
            has_changes: true,
            updated_contact_names: vec!["Alice".to_string()],
            blobs_fetched: 0,
            rejected: 0,
            unresolved: 0,
            reject_reasons: String::new(),
        };
        assert_eq!(with_changes.total, 6);
        assert!(with_changes.has_changes);

        let partial = MobileSyncResult {
            contacts_added: 0,
            cards_updated: 0,
            updates_sent: 1,
            total: 1,
            has_changes: true,
            updated_contact_names: vec![],
            blobs_fetched: 0,
            rejected: 0,
            unresolved: 0,
            reject_reasons: String::new(),
        };
        assert_eq!(partial.total, 1);
        assert!(partial.has_changes);
    }

    fn test_shred_sender() -> MobileRelaySender {
        use vauchi_core::network::{HttpTransport, HttpTransportConfig};
        let transport = HttpTransport::new(HttpTransportConfig::for_testing(
            "http://localhost:8080",
            1000,
        ));
        MobileRelaySender::from_transport(
            transport,
            "http://localhost:8080".to_string(),
            "abcd1234",
        )
    }

    // @scenario: privacy_compliance:Identity purge sends relay purge and revocations
    #[test]
    fn test_mobile_relay_sender_implements_revocation_trait() {
        // allow(zero_assertions): compile-time trait-impl check via dyn coercion.
        fn accepts_sender(_: &mut dyn vauchi_core::api::RevocationSender) {}
        let mut sender = test_shred_sender();
        accepts_sender(&mut sender);
    }

    // @scenario: privacy_compliance:Identity purge sends relay purge request
    #[test]
    fn test_mobile_relay_sender_implements_purge_trait() {
        // allow(zero_assertions): compile-time trait-impl check via dyn coercion.
        fn accepts_sender(_: &mut dyn vauchi_core::api::PurgeSender) {}
        let mut sender = test_shred_sender();
        accepts_sender(&mut sender);
    }

    // @scenario: security:Contact card signatures verified
    #[test]
    fn test_identity_revoked_fields() {
        let revoked = vauchi_core::network::IdentityRevoked {
            sender_id: "sender_hex".to_string().into(),
            recipient_id: "recipient_hex".to_string().into(),
            timestamp: 1700000000,
            signature: [0xAB; 64],
        };
        assert_eq!(revoked.sender_id, "sender_hex");
        assert_eq!(revoked.recipient_id, "recipient_hex");
        assert_eq!(revoked.timestamp, 1700000000);
        assert_eq!(revoked.signature.len(), 64);
        assert!(revoked.signature.iter().all(|b| *b == 0xAB));
    }

    // === Fingerprint Verification Tests (P0-4) ===

    // ── Import contacts via FFI ─────────────────────────────────────────────
    // Coverage moved to `tests/it/platform_app_engine_domain_command_tests.rs`
    // (search `import_contacts_from_vcf_*`) — the canonical surface is now
    // `DomainCommand::ImportContactsFromVcf` via `dispatch_domain_command`.
    // The legacy `VauchiPlatform::import_contacts_from_vcf` UniFFI export
    // was retired 2026-05-23 (Track A); no hand-written consumer existed.

    // F2-MED-2 regression: ensures `get_identity` falls back to disk
    // when the in-memory cache is empty. The two `VauchiPlatform`
    // instances simulate the production layout where a sibling
    // `PlatformAppEngine` (constructed in the same process at the same
    // data dir) writes the identity via storage but does NOT populate
    // this struct's `identity_data` mutex. Pre-fix, `get_identity` on
    // the second instance returned `Other("Identity not found")` and
    // the first `sync()` call after onboarding surfaced as a
    // user-visible "Sync failed" toast. Post-fix, the storage fallback
    // mirrors `has_identity`'s pattern and the cache is populated
    // lazily on first read.
    //
    // F2-MED-2 regression part 2: ensures `get_identity` decodes the
    // raw `Identity::to_storage_bytes()` format that `vauchi-core`'s
    // `Vauchi::create_identity` writes (the production path on
    // Android via `PlatformAppEngine`). Pre-fix this branch surfaced
    // as `Other("Invalid backup or wrong password")` because the
    // decoder only knew the encrypted-`IdentityBackup` format that
    // `VauchiPlatform`'s own `create_identity` writes.
    //
}

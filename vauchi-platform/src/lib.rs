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

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use vauchi_core::{ContactCard, Identity, Storage, SymmetricKey, Vauchi, VauchiConfig};

// === Modules ===

mod content;
mod diagnostic;
mod domain_command;
mod error;
mod exchange;
mod json_helpers;
mod link_responder_session;
mod mobile_ble;
mod mobile_contact_detail;
mod mobile_contacts;
mod mobile_delivery;
mod mobile_device_link;
mod mobile_device_link_session;
mod mobile_exchange;
mod mobile_gdpr;
mod mobile_identity;
mod mobile_import;
mod mobile_verifier_event;
mod mobile_visibility;
mod multipart_qr;
mod multistage_exchange;
mod platform_app_engine;
mod platform_app_engine_device_link;
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
use error::lock_or;
pub use error::{KeychainError, MobileError};
pub use exchange::{
    MobileBleExchangeStatus, MobileCommand, MobileEvent, MobileExchangeSession,
    MobileExchangeState, MobileProximityHandler, create_qr_exchange_manual,
    create_qr_exchange_proximity,
};
pub use link_responder_session::{
    LinkResponderSessionListener, MobileLinkResponderFailureReason, MobileLinkResponderSession,
    MobileLinkResponderState,
};
pub use mobile_ble::{
    MobileBleDelegate, MobileBleError, MobileBleExchangeResult, MobileBleExchangeSession,
    MobileBleField, MobileBleState, MobileBleTransportError,
};
pub use mobile_contact_detail::{
    MobileContactDetailAction, MobileContactDetailBadge, MobileContactDetailBanner,
    MobileContactDetailViewState,
};
pub use mobile_device_link_session::{DeviceLinkSessionListener, MobileDeviceLinkSession};
pub use mobile_import::{MobileImportResult, MobileImportWarning};
pub use mobile_verifier_event::{
    MobileProximityConfidence, MobileProximityVerifierEvent, MobileVerifierMethod,
};
pub use multipart_qr::{MultipartDecoder, encode_multipart};
pub use multistage_exchange::{
    MobileAudioProximityState, MobileMultiStageSession, MobileProtocolState, MobileQrPayload,
    MultiStageAudioListener, MultiStageSessionListener,
};
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
    MobileExchangeResult, MobileFaqItem, MobileFieldNote, MobileFieldType, MobileGdprExport,
    MobileHelpCategory, MobileHelpCategoryInfo, MobileLabelContactBadge, MobileLabelContactRow,
    MobileLabelContactStatus, MobileLocale, MobileLocaleInfo, MobileMotionTokens,
    MobileNotificationCategory, MobileOnboardingProgress, MobileOnboardingStep,
    MobilePendingNotification, MobileRecoveryClaim, MobileRecoveryProgress,
    MobileRecoveryVerification, MobileRecoveryVoucher, MobileRetryEntry, MobileShredReport,
    MobileShredStatus, MobileShredToken, MobileSocialNetwork, MobileSpacingDirectionTokens,
    MobileSpacingTokens, MobileSyncResult, MobileSyncStatus, MobileTabInfo, MobileTheme,
    MobileThemeColors, MobileThemeMode, MobileTouchTargetTokens, MobileTypographyTokens,
    MobileVisibilityLabel, MobileVisibilityLabelDetail,
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
struct KeychainBridge {
    callback: Arc<dyn MobilePlatformKeychain>,
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

// === Password Strength ===

/// Password strength level for display to users.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobilePasswordStrength {
    /// Score 0-1: Too weak to use
    TooWeak,
    /// Score 2: Fair but not recommended
    Fair,
    /// Score 3: Strong enough
    Strong,
    /// Score 4: Very strong
    VeryStrong,
}

/// Result of password strength check.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobilePasswordCheck {
    /// The strength level
    pub strength: MobilePasswordStrength,
    /// Human-readable description
    pub description: String,
    /// Feedback/suggestions for improvement (empty if strong enough)
    pub feedback: String,
    /// Whether the password is acceptable for backup
    pub is_acceptable: bool,
}

/// Check password strength for backup encryption.
///
/// Returns strength level, description, and feedback for improvement.
#[uniffi::export]
pub fn check_password_strength(password: String) -> MobilePasswordCheck {
    use vauchi_core::identity::password::{password_feedback, validate_password};

    // Short passwords get immediate feedback
    if password.len() < 8 {
        return MobilePasswordCheck {
            strength: MobilePasswordStrength::TooWeak,
            description: "Too short".to_string(),
            feedback: "Password must be at least 8 characters".to_string(),
            is_acceptable: false,
        };
    }

    // Check with zxcvbn via core
    match validate_password(&password) {
        Ok(strength) => {
            use vauchi_core::identity::password::PasswordStrength;
            let (level, description) = match strength {
                PasswordStrength::Strong => (MobilePasswordStrength::Strong, "Strong"),
                PasswordStrength::VeryStrong => (MobilePasswordStrength::VeryStrong, "Very strong"),
                _ => (MobilePasswordStrength::Fair, "Fair"),
            };
            MobilePasswordCheck {
                strength: level,
                description: description.to_string(),
                feedback: String::new(),
                is_acceptable: true,
            }
        }
        Err(_) => {
            // Get feedback for weak passwords
            let feedback = password_feedback(&password);
            let estimate = zxcvbn::zxcvbn(&password, &[]);
            let (level, description) = match estimate.score() {
                zxcvbn::Score::Zero | zxcvbn::Score::One => {
                    (MobilePasswordStrength::TooWeak, "Too weak")
                }
                zxcvbn::Score::Two => (MobilePasswordStrength::Fair, "Fair"),
                _ => (MobilePasswordStrength::Fair, "Fair"),
            };
            MobilePasswordCheck {
                strength: level,
                description: description.to_string(),
                feedback: if feedback.is_empty() {
                    "Add more words or use a passphrase".to_string()
                } else {
                    feedback
                },
                is_acceptable: false,
            }
        }
    }
}

// === Thread-safe state ===

/// Serializable identity data for thread-safe storage.
#[derive(Clone)]
struct IdentityData {
    backup_data: Vec<u8>,
}

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

// ============================================================
// Help Functions
// ============================================================

/// Get all help categories with their display names.
#[deprecated(note = "Use HelpWorkflowEngine via ScreenModel routing instead")]
#[uniffi::export]
pub fn get_help_categories() -> Vec<MobileHelpCategoryInfo> {
    vauchi_app::help::HelpCategory::all()
        .iter()
        .map(|c| MobileHelpCategoryInfo {
            category: (*c).into(),
            display_name: c.display_name().to_string(),
        })
        .collect()
}

/// Get all FAQ items.
#[deprecated(note = "Use HelpWorkflowEngine via ScreenModel routing instead")]
#[uniffi::export]
pub fn get_faqs() -> Vec<MobileFaqItem> {
    vauchi_app::help::get_faqs()
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get FAQ items for a specific category.
#[deprecated(note = "Use HelpWorkflowEngine via ScreenModel routing instead")]
#[uniffi::export]
pub fn get_faqs_by_category(category: MobileHelpCategory) -> Vec<MobileFaqItem> {
    vauchi_app::help::get_faqs_by_category(category.into())
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get a specific FAQ item by ID.
///
/// Returns None if the FAQ is not found.
#[deprecated(note = "Use HelpWorkflowEngine via ScreenModel routing instead")]
#[uniffi::export]
pub fn get_faq_by_id(id: String) -> Option<MobileFaqItem> {
    vauchi_app::help::get_faq_by_id(&id).map(|f| MobileFaqItem::from(&f))
}

/// Search FAQs by query text.
///
/// Searches in both questions and answers (case-insensitive).
#[deprecated(note = "Use HelpWorkflowEngine via ScreenModel routing instead")]
#[uniffi::export]
pub fn search_faqs(query: String) -> Vec<MobileFaqItem> {
    vauchi_app::help::search_faqs(&query)
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get all FAQ items in the specified locale.
#[deprecated(note = "Use HelpWorkflowEngine via ScreenModel routing instead")]
#[uniffi::export]
pub fn get_faqs_localized(locale: MobileLocale) -> Vec<MobileFaqItem> {
    vauchi_app::help::get_faqs_localized(locale.into())
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get FAQ items for a specific category in the specified locale.
#[deprecated(note = "Use HelpWorkflowEngine via ScreenModel routing instead")]
#[uniffi::export]
pub fn get_faqs_by_category_localized(
    category: MobileHelpCategory,
    locale: MobileLocale,
) -> Vec<MobileFaqItem> {
    vauchi_app::help::get_faqs_by_category_localized(category.into(), locale.into())
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get a specific FAQ item by ID in the specified locale.
///
/// Returns None if the FAQ is not found.
#[deprecated(note = "Use HelpWorkflowEngine via ScreenModel routing instead")]
#[uniffi::export]
pub fn get_faq_by_id_localized(id: String, locale: MobileLocale) -> Option<MobileFaqItem> {
    vauchi_app::help::get_faq_by_id_localized(&id, locale.into()).map(|f| MobileFaqItem::from(&f))
}

/// Search FAQs by query text in the specified locale.
///
/// Searches in both questions and answers (case-insensitive).
#[deprecated(note = "Use HelpWorkflowEngine via ScreenModel routing instead")]
#[uniffi::export]
pub fn search_faqs_localized(query: String, locale: MobileLocale) -> Vec<MobileFaqItem> {
    vauchi_app::help::search_faqs_localized(&query, locale.into())
        .iter()
        .map(MobileFaqItem::from)
        .collect()
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
struct MobileRelaySender {
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
    fn from_transport(
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
    fn send_revocation(
        &mut self,
        revocation: &vauchi_core::network::IdentityRevoked,
        now: u64,
    ) -> Result<bool, vauchi_core::api::ShredError> {
        self.client
            .connect()
            .map_err(|e| vauchi_core::api::ShredError::FileError(format!("Connect: {e}")))?;
        self.client.send_revocation(revocation, now)
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

// === Main Interface ===

/// Main Vauchi interface for mobile platforms.
///
/// Uses on-demand storage connections for thread safety.
#[derive(uniffi::Object)]
pub struct VauchiPlatform {
    pub(crate) storage_path: PathBuf,
    pub(crate) storage_key: SymmetricKey,
    relay_url: String,
    /// Optional PEM-encoded certificate for TLS pinning.
    pinned_cert_pem: Mutex<Option<String>>,
    identity_data: Mutex<Option<IdentityData>>,
    sync_status: Mutex<MobileSyncStatus>,
    /// Platform keychain for crypto-shredding operations.
    platform_keychain: Mutex<Option<Arc<dyn MobilePlatformKeychain>>>,
}

impl VauchiPlatform {
    /// Opens a storage connection.
    ///
    /// `pub` (not `pub(crate)`) so
    /// `tests/it/multistage_persistence_regression.rs` can verify
    /// post-exchange contact persistence after the contact-CRUD
    /// `impl VauchiPlatform` block retires (slice 32g-B Tier 2).
    /// Not `#[uniffi::export]`-marked, so it stays out of the FFI
    /// surface.
    pub fn open_storage(&self) -> Result<Storage, MobileError> {
        Storage::open(&self.storage_path, self.storage_key.clone()).map_err(|e| {
            MobileError::StorageError {
                detail: e.to_string(),
            }
        })
    }

    /// Opens a Vauchi API instance backed by the same storage.
    ///
    /// Use this for operations that must dispatch events (e.g. hide/unhide contact).
    /// Operations that only read data can continue using `open_storage()` directly.
    pub(crate) fn open_vauchi(&self) -> Result<Vauchi, MobileError> {
        let config = VauchiConfig::with_storage_path(&self.storage_path)
            .with_relay_url(&self.relay_url)
            .with_storage_key(self.storage_key.clone());
        Vauchi::new(config).map_err(|e| MobileError::Other {
            detail: e.to_string(),
        })
    }

    /// Opens a Vauchi instance with identity loaded **and** the OHTTP
    /// gateway key resolved (cache → bundled).
    ///
    /// Use for flows that immediately issue a relay request (device
    /// link, shred, exchange). Resolving OHTTP here lets the call
    /// chain's `build_relay_transport` wire encryption on the first
    /// request — without this, `ohttp_key.is_none()` flips
    /// `allow_direct = true` and the first request leaks the client
    /// IP to the relay (ADR-037 §Bootstrap Exceptions).
    ///
    /// Neither step hits the network in production:
    /// `OhttpConfig::bundled_gateway_key` is always set by default, so
    /// key resolution is in-process.
    ///
    /// Returns `Err(IdentityNotInitialized)` if no identity exists —
    /// relay-bound flows require one. If `connect()` fails (corrupt
    /// bundled key, storage error), the returned `Vauchi` still has
    /// the identity set and `build_relay_transport` falls back to the
    /// `allow_direct` path — functionality preserved, privacy degraded.
    pub(crate) fn open_vauchi_for_relay(&self) -> Result<Vauchi, MobileError> {
        let mut vauchi = self.open_vauchi()?;
        // `Vauchi::new` (in `init`) auto-loads identity from storage via
        // `Identity::from_storage_bytes` — the same raw format
        // `vauchi-core::Vauchi::create_identity` writes. When that
        // succeeds we don't need (or want) to call `set_identity`
        // again: it errors with `AlreadyInitialized`. F2-MED-2: pre-fix
        // (encrypted-backup-only `get_identity`) hid this because
        // get_identity returned `Identity not found` first; post-fix
        // (raw-format-aware `get_identity`) both load paths succeed
        // and the redundant `set_identity` surfaced as
        // `Sync failed: detail=already initialized`.
        if vauchi.identity().is_none() {
            let identity = self.get_identity()?;
            vauchi
                .set_identity(identity)
                .map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
        } else {
            // Identity already loaded by Vauchi::new — but still
            // require it to exist on disk to satisfy the
            // "Identity not found" precondition every relay-bound
            // flow expects. The check is cheap (in-memory cache hit
            // after Vauchi::new's load).
            let _ = self.get_identity()?;
        }
        let _ = vauchi.connect();
        Ok(vauchi)
    }

    /// Build the pair of `MobileRelaySender`s used by shred/purge flows.
    ///
    /// Both senders share one fresh `Vauchi` instance so the OHTTP key
    /// cache is reused. `open_vauchi_for_relay` resolves the bundled
    /// OHTTP key eagerly so the first shred request goes through OHTTP
    /// instead of leaking the client IP via the `allow_direct` fallback.
    /// If the instance cannot be opened (no identity, storage error),
    /// both results carry the same error string — shred itself proceeds
    /// best-effort without relay-side purge/revocation.
    pub(crate) fn build_shred_senders(
        &self,
        sender_id: &str,
    ) -> (
        Result<MobileRelaySender, String>,
        Result<MobileRelaySender, String>,
    ) {
        let vauchi = match self.open_vauchi_for_relay() {
            Ok(v) => v,
            Err(e) => {
                let msg = e.to_string();
                return (Err(msg.clone()), Err(msg));
            }
        };
        let purge_transport = vauchi.build_relay_transport(self.relay_url.clone(), 10_000);
        let rev_transport = vauchi.build_relay_transport(self.relay_url.clone(), 10_000);
        let purge =
            MobileRelaySender::from_transport(purge_transport, self.relay_url.clone(), sender_id);
        let rev =
            MobileRelaySender::from_transport(rev_transport, self.relay_url.clone(), sender_id);
        (Ok(purge), Ok(rev))
    }

    /// Save a contact directly to storage.
    ///
    /// Used by integration tests that need exchanged or imported contacts
    /// without running a full exchange flow or VCF import.
    /// Not exported via UniFFI (outside `#[uniffi::export]` block).
    #[doc(hidden)]
    pub fn save_test_contact(&self, contact: &vauchi_core::Contact) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage
            .save_contact(contact)
            .map_err(|e| MobileError::StorageError {
                detail: e.to_string(),
            })
    }

    /// Returns the data directory (parent of the database file).
    pub(crate) fn data_dir(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .to_path_buf()
    }

    /// Gets the platform keychain bridge for shred operations.
    pub(crate) fn get_keychain_bridge(&self) -> Result<KeychainBridge, MobileError> {
        let lock = lock_or(&self.platform_keychain)?;
        let callback = lock
            .as_ref()
            .ok_or_else(|| MobileError::Other {
                detail: "Platform keychain not set. Call set_platform_keychain() first.".into(),
            })?
            .clone();
        Ok(KeychainBridge { callback })
    }

    /// Gets the identity from stored data.
    ///
    /// Falls back to disk when the in-memory cache is empty — same shape
    /// as [`has_identity`]. F2-MED-2 (2026-05-09 device-test campaign)
    /// repro'd a "Sync failed: detail=Identity not found" toast on
    /// freshly-onboarded Pixel installs because onboarding writes via
    /// the sibling `PlatformAppEngine` (which shares the data dir but
    /// not this struct's `identity_data` mutex). Without the storage
    /// fallback, the first `sync()` after onboarding hit
    /// `data.as_ref()` → `None` → "Identity not found", even though
    /// the identity was on disk.
    ///
    /// Two on-disk formats coexist today and both are accepted:
    ///
    ///   1. `IdentityBackup` encrypted with `__internal_storage_key__`
    ///      — the format `VauchiPlatform::create_identity` writes
    ///      directly. Tests construct identities this way.
    ///   2. Raw `Identity::to_storage_bytes()` — the format
    ///      `Vauchi::create_identity` (vauchi-core) writes when
    ///      `PlatformAppEngine`'s `CreateIdentity` runs through it.
    ///      This is the production layout used by every Pixel/iOS
    ///      onboarding (the F2-MED-2 trigger).
    ///
    /// Both formats are tried in order; whichever decodes wins. Long-term
    /// cleanup is to consolidate on a single format and retire the
    /// duplicate caching in this struct entirely — but that is an
    /// architectural sweep outside the scope of F2-MED-2.
    pub(crate) fn get_identity(&self) -> Result<Identity, MobileError> {
        // 1. Hot path — in-memory cache hit.
        {
            let data = lock_or(&self.identity_data)?;
            if let Some(identity_data) = data.as_ref() {
                return Self::decode_identity_blob(&identity_data.backup_data);
            }
        }

        // 2. Storage fallback — a sibling instance (`PlatformAppEngine`)
        //    may have written the identity after this struct was
        //    constructed. Mirrors `has_identity`'s pattern; populates
        //    the cache so subsequent calls take the hot path.
        let storage = self.open_storage()?;
        let (backup_data, _display_name) = storage
            .load_identity()
            .map_err(|e| MobileError::Other {
                detail: format!("Identity load failed: {e}"),
            })?
            .ok_or(MobileError::Other {
                detail: "Identity not found".to_string(),
            })?;

        let cached = IdentityData {
            backup_data: backup_data.clone(),
        };
        *lock_or(&self.identity_data)? = Some(cached);

        Self::decode_identity_blob(&backup_data)
    }

    /// Decode a stored identity blob, accepting both formats currently
    /// in use (see [`get_identity`] doc-comment). Tries the encrypted
    /// `IdentityBackup` form first because it's what
    /// `VauchiPlatform`'s own `create_identity` writes; falls through
    /// to raw `Identity::to_storage_bytes()` when the blob comes from
    /// `vauchi-core`'s `save_identity` (the production path on
    /// Android via `PlatformAppEngine`).
    fn decode_identity_blob(blob: &[u8]) -> Result<Identity, MobileError> {
        Identity::from_storage_blob(
            blob,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .map_err(|e| MobileError::Other {
            detail: format!("Identity decode failed: {e}"),
        })
    }

    /// Get our contact card, or create a default one from the identity.
    pub(crate) fn get_own_card_or_default(
        &self,
        identity: &Identity,
    ) -> Result<ContactCard, MobileError> {
        let storage = self.open_storage()?;
        Ok(storage
            .load_own_card()
            .ok()
            .flatten()
            .unwrap_or_else(|| ContactCard::new(identity.display_name())))
    }
}

#[uniffi::export]
impl VauchiPlatform {
    /// Create a new VauchiPlatform instance with a platform-provided secure key.
    ///
    /// This is the recommended constructor. The platform (iOS/Android) should:
    /// 1. Generate a 32-byte key if one doesn't exist in secure storage
    /// 2. Store it in platform-specific secure storage (Keychain/KeyStore)
    /// 3. Pass the key bytes to this constructor
    #[uniffi::constructor]
    pub fn new_with_secure_key(
        data_dir: String,
        relay_url: String,
        storage_key_bytes: Vec<u8>,
    ) -> Result<Arc<Self>, MobileError> {
        let data_path = PathBuf::from(&data_dir);

        std::fs::create_dir_all(&data_path).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;

        let storage_path = data_path.join("vauchi.db");

        let key_array: [u8; 32] =
            storage_key_bytes
                .try_into()
                .map_err(|_| MobileError::StorageError {
                    detail: "Storage key must be exactly 32 bytes".to_string(),
                })?;
        let storage_key =
            SymmetricKey::try_from_bytes(key_array).map_err(|_| MobileError::StorageError {
                detail: "Degenerate storage key rejected".to_string(),
            })?;

        // Storage handle is opened lazily on first use; the constructor does not
        // pre-open it. Pre-opening would run schema migrations and startup
        // maintenance during cold start (audit finding F4, 2026-04-17) for a
        // handle that is immediately dropped — Storage is not retained on
        // VauchiPlatform; every operation re-opens via storage_path + storage_key.

        Ok(Arc::new(VauchiPlatform {
            storage_path,
            storage_key,
            relay_url,
            pinned_cert_pem: Mutex::new(None),
            identity_data: Mutex::new(None),
            sync_status: Mutex::new(MobileSyncStatus::Idle),
            platform_keychain: Mutex::new(None),
        }))
    }

    /// Create a new VauchiPlatform instance (legacy constructor).
    ///
    /// WARNING: This constructor stores the encryption key in a plaintext file.
    /// Use `new_with_secure_key` instead for production.
    #[uniffi::constructor]
    pub fn new(data_dir: String, relay_url: String) -> Result<Arc<Self>, MobileError> {
        let data_path = PathBuf::from(&data_dir);

        std::fs::create_dir_all(&data_path).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;

        let storage_path = data_path.join("vauchi.db");
        let key_path = data_path.join("storage.key");

        let storage_key = if key_path.exists() {
            let key_bytes = std::fs::read(&key_path).map_err(|e| MobileError::StorageError {
                detail: format!("Failed to read key: {}", e),
            })?;
            let key_array: [u8; 32] =
                key_bytes
                    .try_into()
                    .map_err(|_| MobileError::StorageError {
                        detail: "Invalid key length".to_string(),
                    })?;
            SymmetricKey::try_from_bytes(key_array).map_err(|_| MobileError::StorageError {
                detail: "Degenerate storage key rejected".to_string(),
            })?
        } else {
            let key = SymmetricKey::generate();
            std::fs::write(&key_path, key.as_bytes()).map_err(|e| MobileError::StorageError {
                detail: format!("Failed to save key: {}", e),
            })?;
            key
        };

        // Storage handle opened lazily — see new_with_secure_key for rationale.

        Ok(Arc::new(VauchiPlatform {
            storage_path,
            storage_key,
            relay_url,
            pinned_cert_pem: Mutex::new(None),
            identity_data: Mutex::new(None),
            sync_status: Mutex::new(MobileSyncStatus::Idle),
            platform_keychain: Mutex::new(None),
        }))
    }

    /// Set the pinned certificate for relay TLS connections.
    ///
    /// The certificate should be in PEM format. Once set, only connections
    /// to relay servers presenting this exact certificate will be allowed.
    pub fn set_pinned_certificate(&self, cert_pem: String) {
        let Ok(mut pinned) = lock_or(&self.pinned_cert_pem) else {
            return;
        };
        if cert_pem.is_empty() {
            *pinned = None;
        } else {
            *pinned = Some(cert_pem);
        }
    }

    /// Check if certificate pinning is enabled.
    pub fn is_certificate_pinning_enabled(&self) -> bool {
        let Ok(guard) = lock_or(&self.pinned_cert_pem) else {
            return false;
        };
        guard.is_some()
    }
}

// Methods extracted to child modules:
// - mobile_identity.rs: Identity operations, aha moments, demo contact
// - mobile_contacts.rs: Contact card/CRUD, hidden contacts, pagination, social networks, field validation
// - mobile_visibility.rs: Visibility operations and labels
// - mobile_exchange.rs: Contact exchange operations
// - mobile_delivery.rs: Sync, delivery status, retry/offline queue, multi-device, backup, async sync
// - mobile_gdpr.rs: GDPR, crypto-shredding, consent
// - mobile_device_link.rs: Device linking, relay transport, multipart QR
// - mobile_content.rs: Content updates (feature-gated)

// INLINE_TEST_REQUIRED: Tests require tempfile for VauchiPlatform instance creation
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_instance() -> (Arc<VauchiPlatform>, TempDir) {
        let dir = TempDir::new().unwrap();
        let wb = VauchiPlatform::new(
            dir.path().to_string_lossy().to_string(),
            "http://localhost:8080".to_string(),
        )
        .unwrap();
        (wb, dir)
    }

    // @scenario: identity_management:User creates a new identity
    #[test]
    fn test_create_identity() {
        let (wb, _dir) = create_test_instance();
        assert!(!wb.has_identity());

        wb.create_identity("Alice".to_string()).unwrap();
        assert!(wb.has_identity());

        let name = wb.get_display_name().unwrap();
        assert_eq!(name, "Alice");
    }

    // @scenario: contact_exchange:Two users exchange contact cards via QR code
    #[test]
    fn test_exchange_session_qr() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let session = wb.create_qr_exchange_manual().unwrap();
        let qr_data = session.generate_qr().unwrap();
        assert!(
            qr_data.starts_with("wb://"),
            "QR data should start with wb://"
        );
    }

    // @scenario: device_management:User views linked devices
    #[test]
    fn test_get_devices_no_registry() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Without a registry, should return just the current device
        let devices = wb.get_devices().unwrap();
        assert_eq!(devices.len(), 1);
        assert!(devices[0].is_current);
        assert!(devices[0].is_active);
        assert_eq!(devices[0].device_index, 0);
    }

    // @scenario: device_management:User links a new device
    #[test]
    fn test_generate_device_link_qr() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let link_data = wb.generate_device_link_qr().unwrap();
        assert!(!link_data.qr_data.is_empty());
        assert!(!link_data.identity_public_key.is_empty());
        assert!(link_data.expires_at > link_data.timestamp);
    }

    // @scenario: device_management:User links a new device
    #[test]
    fn test_parse_device_link_qr() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let link_data = wb.generate_device_link_qr().unwrap();
        let parsed = wb.parse_device_link_qr(link_data.qr_data).unwrap();

        assert_eq!(parsed.identity_public_key, link_data.identity_public_key);
        assert!(!parsed.is_expired);
    }

    // @scenario: device_management:Invalid device link QR rejected
    #[test]
    fn test_parse_device_link_qr_invalid() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let result = wb.parse_device_link_qr("invalid_qr_data".to_string());
        result.expect_err("expected error");
    }

    // @scenario: device_management:User views linked devices
    #[test]
    fn test_device_count() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let count = wb.device_count().unwrap();
        assert_eq!(count, 1);
    }

    // @scenario: device_management:User views linked devices
    #[test]
    fn test_is_primary_device() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let is_primary = wb.is_primary_device().unwrap();
        assert!(is_primary);
    }

    // @scenario: device_management:User unlinks a device
    #[test]
    fn test_unlink_device_no_registry() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // No registry means no devices to unlink
        let result = wb.unlink_device(1).unwrap();
        assert!(!result);
    }

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

    // @scenario: identity_management:Identity verification via public key fingerprint
    #[test]
    fn test_get_own_fingerprint() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let fp = wb.get_own_fingerprint().unwrap();

        // Must be 16 groups of 4 uppercase hex chars
        let groups: Vec<&str> = fp.split(' ').collect();
        assert_eq!(groups.len(), 16, "own fingerprint should have 16 groups");
        for group in groups {
            assert_eq!(group.len(), 4);
            assert!(
                group
                    .chars()
                    .all(|c: char| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
            );
        }
    }

    // =========================================================================
    // Multipart QR via VauchiPlatform
    // =========================================================================

    #[test]
    fn test_encode_multipart_qr_returns_chunks() {
        let (wb, _dir) = create_test_instance();
        let data = vec![0xABu8; 5000];
        let chunks = wb.encode_multipart_qr(data.clone());

        assert!(
            chunks.len() >= 3,
            "5KB payload should produce multiple chunks, got {}",
            chunks.len()
        );

        // Verify each chunk is valid format: index/total/crc32/data
        for (i, chunk) in chunks.iter().enumerate() {
            let parts: Vec<&str> = chunk.splitn(4, '/').collect();
            assert_eq!(
                parts.len(),
                4,
                "chunk {i} must have 4 slash-separated parts"
            );
        }
    }

    #[test]
    fn test_encode_multipart_qr_roundtrip() {
        let (wb, _dir) = create_test_instance();
        let original = b"End-to-end test: VauchiPlatform encodes, MultipartDecoder decodes.";
        let chunks = wb.encode_multipart_qr(original.to_vec());

        let mut decoder = multipart_qr::MultipartDecoder::new();
        for chunk in &chunks {
            decoder.add_chunk(chunk).expect("valid chunk");
        }

        assert!(decoder.is_complete(), "decoder should be complete");
        let assembled = decoder.assemble().expect("assemble should succeed");
        assert_eq!(
            assembled,
            original.to_vec(),
            "roundtrip via VauchiPlatform + MultipartDecoder must preserve data"
        );
    }

    // ── Import contacts via FFI ─────────────────────────────────────────────
    // Coverage moved to `tests/it/platform_app_engine_domain_command_tests.rs`
    // (search `import_contacts_from_vcf_*`) — the canonical surface is now
    // `DomainCommand::ImportContactsFromVcf` via `dispatch_domain_command`.
    // The legacy `VauchiPlatform::import_contacts_from_vcf` UniFFI export
    // was retired 2026-05-23 (Track A); no hand-written consumer existed.

    // @internal
    #[test]
    fn test_open_vauchi_for_relay_without_identity_errors() {
        let (wb, _dir) = create_test_instance();
        let result = wb.open_vauchi_for_relay();
        assert!(result.is_err(), "expected IdentityNotFound error");
        assert!(
            matches!(
                &result,
                Err(MobileError::Other { detail }) if detail == "Identity not found"
            ),
            "open_vauchi_for_relay should fail with Other(Identity not found) when no identity exists"
        );
    }

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
    // @internal
    #[test]
    fn test_get_identity_storage_fallback_after_sibling_write() {
        let dir = TempDir::new().unwrap();
        let dir_path = dir.path().to_string_lossy().to_string();

        // First instance writes the identity to disk.
        let wb1 =
            VauchiPlatform::new(dir_path.clone(), "http://localhost:8080".to_string()).unwrap();
        wb1.create_identity("Alice".to_string()).unwrap();
        let public_id_1 = wb1.get_public_id().unwrap();

        // Second instance at the same dir starts with an empty
        // `identity_data` cache — the production case where the sibling
        // is `PlatformAppEngine`, not another `VauchiPlatform`.
        let wb2 = VauchiPlatform::new(dir_path, "http://localhost:8080".to_string()).unwrap();

        // get_identity must fall back to storage and return the
        // identity that the sibling persisted.
        let identity = wb2
            .get_identity()
            .expect("get_identity should fall back to storage when cache is empty");
        assert_eq!(
            identity.public_id(),
            public_id_1,
            "storage fallback should return the same identity the sibling wrote"
        );

        // Calling again should hit the now-populated cache (not visible
        // from the API surface, but verified by no error path firing).
        let identity_again = wb2.get_identity().unwrap();
        assert_eq!(identity_again.public_id(), public_id_1);
    }

    // F2-MED-2 regression part 2: ensures `get_identity` decodes the
    // raw `Identity::to_storage_bytes()` format that `vauchi-core`'s
    // `Vauchi::create_identity` writes (the production path on
    // Android via `PlatformAppEngine`). Pre-fix this branch surfaced
    // as `Other("Invalid backup or wrong password")` because the
    // decoder only knew the encrypted-`IdentityBackup` format that
    // `VauchiPlatform`'s own `create_identity` writes.
    //
    // @internal
    #[test]
    fn test_get_identity_decodes_vauchi_core_raw_storage_bytes_format() {
        use vauchi_core::Identity;

        let dir = TempDir::new().unwrap();
        let dir_path = dir.path().to_string_lossy().to_string();
        let wb = VauchiPlatform::new(dir_path, "http://localhost:8080".to_string()).unwrap();

        // Bypass create_identity and write raw `to_storage_bytes` directly
        // through the storage layer the platform uses — same shape as
        // what `Vauchi::create_identity` (vauchi-core) produces when
        // `PlatformAppEngine` orchestrates onboarding.
        let identity = Identity::create(
            "Carol",
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        );
        let raw_bytes = identity.to_storage_bytes();
        let display_name = identity.display_name().to_string();
        let storage = wb.open_storage().unwrap();
        storage.save_identity(&raw_bytes, &display_name).unwrap();
        drop(storage);

        let recovered = wb
            .get_identity()
            .expect("get_identity must accept Vauchi::to_storage_bytes blobs");
        assert_eq!(
            recovered.public_id(),
            identity.public_id(),
            "raw-format decoder must return the same identity the sibling wrote"
        );
    }

    // @internal
    #[test]
    fn test_open_vauchi_for_relay_with_identity_populates_ohttp_key() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let vauchi = wb.open_vauchi_for_relay().unwrap();
        assert!(vauchi.identity().is_some());
        assert!(
            vauchi.has_ohttp_key(),
            "open_vauchi_for_relay should eagerly resolve the bundled \
             OHTTP key so device-link/shred flows route through OHTTP \
             on first use (ADR-037)"
        );
    }

    // @internal
    #[test]
    fn test_open_vauchi_for_relay_transport_has_ohttp_wired() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let vauchi = wb.open_vauchi_for_relay().unwrap();
        let transport = vauchi.build_relay_transport("http://localhost:8080".to_string(), 1_000);
        assert!(
            transport.has_ohttp(),
            "transport built after open_vauchi_for_relay must have OHTTP wired — \
             without this, device-link and shred leak the client IP to the relay \
             (problem record 2026-04-17-ohttp-allow-direct-fallback)"
        );
    }
}

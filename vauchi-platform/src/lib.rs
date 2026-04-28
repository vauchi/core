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

use vauchi_core::{
    ContactCard, Identity, IdentityBackup, SocialNetworkRegistry, Storage, SymmetricKey, Vauchi,
    VauchiConfig,
};

// === Modules ===

mod content;
mod diagnostic;
mod domain_command;
mod error;
mod exchange;
mod json_helpers;
mod mobile_animated_qr;
mod mobile_ble;
mod mobile_contact_detail;
mod mobile_contacts;
mod mobile_content;
mod mobile_delivery;
mod mobile_device_link;
mod mobile_device_link_session;
mod mobile_exchange;
mod mobile_gdpr;
mod mobile_identity;
mod mobile_import;
mod mobile_nfc;
mod mobile_onboarding;
mod mobile_recovery;
mod mobile_security;
mod mobile_ui;
mod mobile_verifier_event;
mod mobile_visibility;
mod mobile_wifi_aware;
mod multipart_qr;
mod multistage_exchange;
mod platform_app_engine;
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
    MobileBleExchangeStatus, MobileExchangeCommand, MobileExchangeHardwareEvent,
    MobileExchangeSession, MobileExchangeState, MobileProximityHandler, create_qr_exchange_manual,
    create_qr_exchange_proximity,
};
pub use mobile_animated_qr::{
    MobileAnimatedQrConfig, MobileAnimatedQrError, MobileAnimatedQrProgress,
    MobileAnimatedQrReceiver, MobileAnimatedQrSender,
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
pub use mobile_nfc::{
    MobileNfcExchangeResult, MobileNfcHandshake, MobileNfcKeyAckResult, MobileNfcState,
    MobileNfcTransport, MobileNfcTransportError,
};
pub use mobile_ui::MobileOnboardingWorkflow;
pub use mobile_verifier_event::{
    MobileProximityConfidence, MobileProximityVerifierEvent, MobileVerifierMethod,
};
pub use mobile_wifi_aware::{MobileWifiAwareStatus, wifi_aware_check_availability};
pub use multipart_qr::{MobileMultipartDecoder, MultipartDecoder, encode_multipart};
pub use multistage_exchange::{
    MobileMultiStageSession, MobileProtocolState, MobileQrPayload, MultiStageSessionListener,
};
pub use platform_app_engine::{PlatformAppEngine, PlatformEventListener};
pub use policies::{
    MobileClipboardPolicy, mobile_clipboard_policy, mobile_generate_storage_key,
    mobile_storage_key_byte_length,
};
pub use types::{
    MobileAhaMoment, MobileAhaMomentType, MobileAuthMode, MobileBorderRadiusTokens,
    MobileBroadcastResult, MobileConsentRecord, MobileConsentStatus, MobileConsentType,
    MobileContact, MobileContactCard, MobileContactField, MobileContactTrustLevel,
    MobileDecoyContact, MobileDeletionInfo, MobileDeletionState, MobileDeliveryRecord,
    MobileDeliveryStatus, MobileDeliverySummary, MobileDemoContact, MobileDemoContactState,
    MobileDesignTokens, MobileDeviceDeliveryRecord, MobileDeviceDeliveryStatus, MobileDeviceInfo,
    MobileDeviceJoinResult, MobileDeviceLinkConfirmation, MobileDeviceLinkData,
    MobileDeviceLinkInfo, MobileDeviceLinkRequest, MobileDeviceLinkResult, MobileDuressSettings,
    MobileEmergencyConfig, MobileExchangeResult, MobileFaqItem, MobileFieldNote, MobileFieldType,
    MobileGdprExport, MobileHelpCategory, MobileHelpCategoryInfo, MobileLabelContactRow,
    MobileLabelContactStatus, MobileLocale, MobileLocaleInfo, MobileMotionTokens,
    MobileNotificationCategory, MobileOnboardingProgress, MobileOnboardingStep,
    MobilePendingNotification, MobileRecoveryClaim, MobileRecoveryProgress,
    MobileRecoveryVerification, MobileRecoveryVoucher, MobileRetryEntry, MobileShredReport,
    MobileShredStatus, MobileShredToken, MobileShredVerification, MobileSocialNetwork,
    MobileSpacingDirectionTokens, MobileSpacingTokens, MobileSyncResult, MobileSyncStatus,
    MobileTabInfo, MobileTheme, MobileThemeColors, MobileThemeMode, MobileTouchTargetTokens,
    MobileTypographyTokens, MobileVisibilityLabel, MobileVisibilityLabelDetail,
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

// === Device Link Wrapper Objects ===

use vauchi_core::exchange::{
    DeviceLinkInitiator, DeviceLinkRequest, DeviceLinkResponder, ProximityProof,
    compute_confirmation_mac,
};

/// UniFFI-exposed wrapper around DeviceLinkInitiator.
///
/// Uses Mutex for interior mutability (required by UniFFI's Arc<T>).
/// Holds both the initiator and a pending request between prepare_confirmation
/// and confirm_link calls.
#[deprecated(
    note = "Use MobileDeviceLinkSession via create_device_link_session_initiator. \
            Will be removed in Phase 3 of the device-link orchestrator rollout."
)]
#[derive(uniffi::Object)]
pub struct MobileDeviceLinkInitiator {
    inner: Mutex<DeviceLinkInitiator>,
    /// Pending request stored after prepare_confirmation for use in confirm_link.
    pending_request: Mutex<Option<DeviceLinkRequest>>,
}

#[uniffi::export]
impl MobileDeviceLinkInitiator {
    /// Returns the QR data string for display.
    pub fn qr_data(&self) -> String {
        let Ok(guard) = lock_or(&self.inner) else {
            return String::new();
        };
        guard.qr().to_data_string()
    }

    /// Returns the Unix timestamp (seconds) when the QR code expires.
    pub fn expires_at(&self) -> u64 {
        let Ok(guard) = lock_or(&self.inner) else {
            return 0;
        };
        guard.qr().expires_at()
    }

    /// Returns the 16-byte proximity challenge.
    pub fn proximity_challenge(&self) -> Vec<u8> {
        let Ok(guard) = lock_or(&self.inner) else {
            return Vec::new();
        };
        guard.proximity_challenge().to_vec()
    }

    /// Decrypts an incoming link request and returns confirmation details.
    ///
    /// The caller displays the confirmation code and device name to the user.
    pub fn prepare_confirmation(
        &self,
        encrypted_request: Vec<u8>,
    ) -> Result<MobileDeviceLinkConfirmation, MobileError> {
        let initiator = lock_or(&self.inner)?;
        let (confirmation, request) =
            initiator
                .prepare_confirmation(&encrypted_request)
                .map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;

        // Store request for confirm_link
        *lock_or(&self.pending_request)? = Some(request);

        Ok(MobileDeviceLinkConfirmation {
            device_name: confirmation.device_name,
            confirmation_code: confirmation.confirmation_code,
            identity_fingerprint: confirmation.identity_fingerprint,
        })
    }

    /// After user confirms, creates the encrypted response with ultrasonic proof.
    ///
    /// Must call prepare_confirmation() first. The `challenge_response` is the
    /// 16-byte proximity challenge echoed back, and `verified_at` is the Unix
    /// timestamp (seconds) when verification completed.
    pub fn confirm_link_ultrasonic(
        &self,
        challenge_response: Vec<u8>,
        verified_at: u64,
    ) -> Result<MobileDeviceLinkResult, MobileError> {
        let response_bytes: [u8; 16] =
            challenge_response
                .try_into()
                .map_err(|_| MobileError::Other {
                    detail: "challenge_response must be exactly 16 bytes".into(),
                })?;
        let proof = ProximityProof::Ultrasonic {
            challenge_response: response_bytes,
            verified_at,
        };
        self.confirm_link_with_proof(&proof)
    }

    /// After user confirms codes match, creates the encrypted response.
    ///
    /// For manual confirmation: pass the raw confirmation code string
    /// (displayed during linking). Rust computes the HMAC internally
    /// so the link key never crosses the FFI boundary.
    ///
    /// Must call prepare_confirmation() first. The `confirmation_code` is the
    /// human-readable code (e.g. "123-456") displayed during linking, and
    /// `confirmed_at` is the Unix timestamp (seconds) when the user confirmed.
    pub fn confirm_link_manual(
        &self,
        confirmation_code: String,
        confirmed_at: u64,
    ) -> Result<MobileDeviceLinkResult, MobileError> {
        let initiator = lock_or(&self.inner)?;
        let mac = compute_confirmation_mac(initiator.qr().link_key(), &confirmation_code);
        drop(initiator); // Release lock before calling confirm_link_with_proof

        let proof = ProximityProof::ManualConfirmation {
            confirmation_code_mac: mac,
            confirmed_at,
        };
        self.confirm_link_with_proof(&proof)
    }
}

impl MobileDeviceLinkInitiator {
    /// Internal: confirms the link with the given proximity proof.
    fn confirm_link_with_proof(
        &self,
        proof: &ProximityProof,
    ) -> Result<MobileDeviceLinkResult, MobileError> {
        let request = lock_or(&self.pending_request)?
            .take()
            .ok_or_else(|| MobileError::Other {
                detail: "No pending request — call prepare_confirmation first".into(),
            })?;

        let initiator = lock_or(&self.inner)?;
        let (encrypted_response, _registry, device_info) = initiator
            .confirm_link(&request, proof)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;

        Ok(MobileDeviceLinkResult {
            success: true,
            device_name: device_info.device_name().to_string(),
            device_index: device_info.device_index(),
            error_message: None,
            encrypted_response: Some(encrypted_response),
        })
    }
}

/// UniFFI-exposed wrapper around DeviceLinkResponder.
#[deprecated(note = "Reserved for the deferred responder-side orchestrator. \
            Will be replaced by MobileDeviceLinkSession's responder \
            constructor in a future Phase.")]
#[derive(uniffi::Object)]
pub struct MobileDeviceLinkResponder {
    inner: Mutex<DeviceLinkResponder>,
}

#[uniffi::export]
impl MobileDeviceLinkResponder {
    /// Creates an encrypted request to send to the existing device.
    pub fn create_request(&self) -> Result<Vec<u8>, MobileError> {
        lock_or(&self.inner)?
            .create_request()
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })
    }

    /// Computes the confirmation code (must call create_request first).
    pub fn compute_confirmation_code(&self) -> Result<String, MobileError> {
        lock_or(&self.inner)?
            .compute_confirmation_code()
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })
    }

    /// Returns the identity fingerprint from the QR.
    pub fn identity_fingerprint(&self) -> String {
        let Ok(guard) = lock_or(&self.inner) else {
            return String::new();
        };
        guard.identity_fingerprint()
    }

    /// Processes the encrypted response from the existing device.
    pub fn finish_join(
        &self,
        encrypted_response: Vec<u8>,
    ) -> Result<MobileDeviceJoinResult, MobileError> {
        let responder = lock_or(&self.inner)?;
        let response = responder
            .process_response(&encrypted_response)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;

        Ok(MobileDeviceJoinResult {
            success: true,
            display_name: response.display_name().to_string(),
            device_index: response.device_index(),
            error_message: None,
        })
    }
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

/// Classify a device type from its name string.
///
/// Returns a device type for UI icon selection. The classification
/// logic lives in core so all platforms produce consistent results.
#[uniffi::export]
pub fn classify_device_type(name: String) -> types::MobileDeviceType {
    vauchi_core::identity::classify_device_type(&name).into()
}

// === Thread-safe state ===

/// Serializable identity data for thread-safe storage.
#[derive(Clone)]
#[allow(dead_code)]
struct IdentityData {
    backup_data: Vec<u8>,
    display_name: String, // Reserved for future use
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
    ) -> Result<bool, vauchi_core::api::ShredError> {
        self.client
            .connect()
            .map_err(|e| vauchi_core::api::ShredError::FileError(format!("Connect: {e}")))?;
        self.client.send_revocation(revocation)
    }
}

impl vauchi_core::api::PurgeSender for MobileRelaySender {
    fn send_purge(
        &mut self,
        purge: &vauchi_core::api::PreSignedPurgeRequest,
    ) -> Result<bool, vauchi_core::api::ShredError> {
        self.client
            .connect()
            .map_err(|e| vauchi_core::api::ShredError::FileError(format!("Connect: {e}")))?;
        self.client.send_purge(purge)
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
    social_registry: SocialNetworkRegistry,
    sync_status: Mutex<MobileSyncStatus>,
    /// Platform keychain for crypto-shredding operations.
    platform_keychain: Mutex<Option<Arc<dyn MobilePlatformKeychain>>>,
    /// Whether delivery receipts (ReceivedByRecipient ACKs) are enabled.
    delivery_receipts_enabled: Mutex<bool>,
    /// Whether to suppress presence (online status) at the relay.
    suppress_presence: Mutex<bool>,
}

impl VauchiPlatform {
    /// Opens a storage connection.
    pub(crate) fn open_storage(&self) -> Result<Storage, MobileError> {
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
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
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

    /// Save a delivery record directly to storage.
    ///
    /// Used by integration tests that need delivery records of specific statuses
    /// without running the full sync/delivery pipeline.
    /// Not exported via UniFFI (outside `#[uniffi::export]` block).
    #[doc(hidden)]
    pub fn save_test_delivery_record(
        &self,
        record: &vauchi_core::storage::DeliveryRecord,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage
            .create_delivery_record(record)
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
    pub(crate) fn get_identity(&self) -> Result<Identity, MobileError> {
        let data = lock_or(&self.identity_data)?;
        let identity_data = data.as_ref().ok_or(MobileError::Other {
            detail: "Identity not found".to_string(),
        })?;

        let backup = IdentityBackup::new(identity_data.backup_data.clone());
        Identity::import_backup(&backup, "__internal_storage_key__").map_err(|e| {
            MobileError::Other {
                detail: e.to_string(),
            }
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

    /// Get the path to the recovery proof file.
    pub(crate) fn recovery_proof_path(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".recovery_proof")
    }

    // === Aha Moments (internal helpers) ===

    /// Get the path to the aha moments state file.
    pub(crate) fn aha_moments_path(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".aha_moments")
    }

    /// Load the aha moments tracker from storage.
    pub(crate) fn load_aha_tracker(&self) -> vauchi_core::AhaMomentTracker {
        let path = self.aha_moments_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            vauchi_core::AhaMomentTracker::from_json(&data).unwrap_or_default()
        } else {
            vauchi_core::AhaMomentTracker::new()
        }
    }

    /// Save the aha moments tracker to storage.
    pub(crate) fn save_aha_tracker(
        &self,
        tracker: &vauchi_core::AhaMomentTracker,
    ) -> Result<(), MobileError> {
        let path = self.aha_moments_path();
        let data = tracker.to_json().map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        std::fs::write(&path, data).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        Ok(())
    }

    // === Demo Contact (internal helpers) ===

    /// Get the path to the demo contact state file.
    pub(crate) fn demo_contact_path(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".demo_contact")
    }

    /// Load the demo contact state from storage.
    pub(crate) fn load_demo_state(&self) -> vauchi_core::DemoContactState {
        let path = self.demo_contact_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            vauchi_core::DemoContactState::from_json(&data).unwrap_or_default()
        } else {
            vauchi_core::DemoContactState::default()
        }
    }

    /// Save the demo contact state to storage.
    pub(crate) fn save_demo_state(
        &self,
        state: &vauchi_core::DemoContactState,
    ) -> Result<(), MobileError> {
        let path = self.demo_contact_path();
        let data = state.to_json().map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        std::fs::write(&path, data).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        Ok(())
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
            social_registry: SocialNetworkRegistry::with_defaults(),
            sync_status: Mutex::new(MobileSyncStatus::Idle),
            platform_keychain: Mutex::new(None),
            delivery_receipts_enabled: Mutex::new(true),
            suppress_presence: Mutex::new(false),
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
            social_registry: SocialNetworkRegistry::with_defaults(),
            sync_status: Mutex::new(MobileSyncStatus::Idle),
            platform_keychain: Mutex::new(None),
            delivery_receipts_enabled: Mutex::new(true),
            suppress_presence: Mutex::new(false),
        }))
    }

    /// Export the current storage key bytes for migration to secure storage.
    pub fn export_storage_key(&self) -> Vec<u8> {
        self.storage_key.as_bytes().to_vec()
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
// - mobile_onboarding.rs: Onboarding progress, display name suggestions
// - mobile_contacts.rs: Contact card/CRUD, hidden contacts, pagination, social networks, field validation
// - mobile_security.rs: Password/duress, emergency broadcast, decoy contacts
// - mobile_visibility.rs: Visibility operations and labels
// - mobile_exchange.rs: Contact exchange operations
// - mobile_delivery.rs: Sync, delivery status, retry/offline queue, multi-device, backup, async sync
// - mobile_gdpr.rs: GDPR, crypto-shredding, consent
// - mobile_recovery.rs: Recovery operations
// - mobile_device_link.rs: Device linking, relay transport, multipart QR
// - mobile_content.rs: Content updates (feature-gated)

// INLINE_TEST_REQUIRED: Tests require tempfile for VauchiPlatform instance creation
// and access to internal Arc<VauchiPlatform> which cannot be accessed from external tests.
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

    // @scenario: contact_card_management:User adds a field to their card
    #[test]
    fn test_add_field() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.add_field(
            MobileFieldType::Email,
            "work".to_string(),
            "alice@company.com".to_string(),
        )
        .unwrap();

        let card = wb.get_own_card().unwrap();
        assert_eq!(card.fields.len(), 1);
        assert_eq!(card.fields[0].label, "work");
        assert_eq!(card.fields[0].value, "alice@company.com");
    }

    // @scenario: contact_card_management:User edits a field on their card
    #[test]
    fn test_update_field() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.add_field(
            MobileFieldType::Phone,
            "mobile".to_string(),
            "+1234567890".to_string(),
        )
        .unwrap();

        wb.update_field("mobile".to_string(), "+0987654321".to_string())
            .unwrap();

        let card = wb.get_own_card().unwrap();
        assert_eq!(card.fields[0].value, "+0987654321");
    }

    // @scenario: contact_card_management:User removes a field from their card
    #[test]
    fn test_remove_field() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.add_field(
            MobileFieldType::Email,
            "work".to_string(),
            "alice@company.com".to_string(),
        )
        .unwrap();

        let removed = wb.remove_field("work".to_string()).unwrap();
        assert!(removed);

        let card = wb.get_own_card().unwrap();
        assert!(card.fields.is_empty());
    }

    // @scenario: contact_card_management:Social network profile links
    #[test]
    fn test_social_networks() {
        let (wb, _dir) = create_test_instance();

        let networks = wb.list_social_networks();
        assert!(!networks.is_empty());

        let github = networks.iter().find(|n| n.id == "github");
        github.expect("expected Some");

        let url = wb.get_profile_url("github".to_string(), "octocat".to_string());
        assert_eq!(url, Some("https://github.com/octocat".to_string()));
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

    // @scenario: identity_management:User exports and imports backup
    #[test]
    fn test_backup_restore() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.add_field(
            MobileFieldType::Email,
            "work".to_string(),
            "alice@company.com".to_string(),
        )
        .unwrap();

        let backup = wb
            .export_backup("correct-horse-battery-staple".to_string())
            .unwrap();
        assert!(!backup.is_empty());

        let dir2 = TempDir::new().unwrap();
        let wb2 = VauchiPlatform::new(
            dir2.path().to_string_lossy().to_string(),
            "http://localhost:8080".to_string(),
        )
        .unwrap();

        wb2.import_backup(backup, "correct-horse-battery-staple".to_string())
            .unwrap();

        assert!(wb2.has_identity());
        let name = wb2.get_display_name().unwrap();
        assert_eq!(name, "Alice");
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

    // === GDPR Tests ===

    // @scenario: privacy_compliance:User exports GDPR data
    #[test]
    fn test_export_gdpr_data() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.add_field(
            MobileFieldType::Email,
            "work".to_string(),
            "alice@company.com".to_string(),
        )
        .unwrap();

        let export = wb.export_gdpr_data().unwrap();
        assert_eq!(export.version, 3);
        assert!(export.exported_at > 0);

        // Verify JSON is parseable and contains expected fields
        let parsed: serde_json::Value = serde_json::from_str(&export.json_data).unwrap();
        assert_eq!(parsed["version"], 3);
        assert!(parsed["contacts"].is_array());
        assert!(parsed["settings"].is_object());
    }

    // @scenario: privacy_compliance:User schedules identity deletion
    #[test]
    fn test_schedule_cancel_deletion() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Initially no deletion
        let info = wb.get_deletion_state().unwrap();
        assert_eq!(info.state, MobileDeletionState::None);

        // Schedule deletion
        let info = wb.schedule_identity_deletion().unwrap();
        assert_eq!(info.state, MobileDeletionState::Scheduled);
        assert!(info.scheduled_at > 0);
        assert!(info.execute_at > info.scheduled_at);

        // Cancel deletion
        wb.cancel_identity_deletion().unwrap();
        let info = wb.get_deletion_state().unwrap();
        assert_eq!(info.state, MobileDeletionState::None);
    }

    // @scenario: privacy_compliance:User manages consent preferences
    #[test]
    fn test_consent_grant_revoke() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Initially not granted
        let granted = wb
            .check_consent(MobileConsentType::RecoveryVouching)
            .unwrap();
        assert!(!granted);

        // Grant consent
        wb.grant_consent(MobileConsentType::RecoveryVouching)
            .unwrap();
        let granted = wb
            .check_consent(MobileConsentType::RecoveryVouching)
            .unwrap();
        assert!(granted);

        // No sleep needed: consent query uses ORDER BY timestamp DESC, rowid DESC
        // so same-second inserts are correctly ordered by rowid (CC-06)

        // Revoke consent
        wb.revoke_consent(MobileConsentType::RecoveryVouching)
            .unwrap();
        let granted = wb
            .check_consent(MobileConsentType::RecoveryVouching)
            .unwrap();
        assert!(!granted);
    }

    // @scenario: privacy_compliance:User manages consent preferences
    #[test]
    fn test_consent_records_list() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.grant_consent(MobileConsentType::DataProcessing).unwrap();
        wb.grant_consent(MobileConsentType::ContactSharing).unwrap();
        wb.grant_consent(MobileConsentType::RecoveryVouching)
            .unwrap();

        let records = wb.get_consent_records().unwrap();
        assert!(records.len() >= 3);
    }

    // @scenario: privacy_compliance:User views consent status
    #[test]
    fn test_get_consent_status_returns_granted_with_timestamp() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Before any consent action, status should be not granted with no timestamp
        let status = wb
            .get_consent_status(MobileConsentType::RecoveryVouching)
            .unwrap();
        assert!(!status.granted);
        assert!(status.last_changed_at.is_none());
        assert!(status.policy_version.is_none());

        // After granting, status should be granted with a timestamp
        wb.grant_consent(MobileConsentType::RecoveryVouching)
            .unwrap();
        let status = wb
            .get_consent_status(MobileConsentType::RecoveryVouching)
            .unwrap();
        assert!(status.granted);
        status.last_changed_at.expect("expected Some");
        assert!(status.last_changed_at.unwrap() > 0);

        // After revoking, status should be not granted but still have a timestamp
        wb.revoke_consent(MobileConsentType::RecoveryVouching)
            .unwrap();
        let status = wb
            .get_consent_status(MobileConsentType::RecoveryVouching)
            .unwrap();
        assert!(!status.granted);
        status.last_changed_at.expect("expected Some");
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
            sender_id: "sender_hex".to_string(),
            recipient_id: "recipient_hex".to_string(),
            timestamp: 1700000000,
            signature: [0xAB; 64],
        };
        assert_eq!(revoked.sender_id, "sender_hex");
        assert_eq!(revoked.recipient_id, "recipient_hex");
        assert_eq!(revoked.timestamp, 1700000000);
        assert_eq!(revoked.signature.len(), 64);
        assert!(revoked.signature.iter().all(|b| *b == 0xAB));
    }

    // @scenario: contacts_management:View contacts list
    #[test]
    fn test_list_contacts_paginated() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Paginate with 0 contacts should return empty
        let page = wb.list_contacts_paginated(0, 3).unwrap();
        assert!(page.is_empty());
    }

    // === Fingerprint Verification Tests (P0-4) ===

    // @scenario: security:Verify contact fingerprint manually
    #[test]
    fn test_mobile_contact_has_fingerprint_field() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Add a contact via exchange simulation
        let contact = vauchi_core::Contact::from_exchange(
            [0xAB; 32],
            vauchi_core::contact_card::ContactCard::new("Bob"),
            vauchi_core::crypto::SymmetricKey::generate(),
        );
        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact).unwrap();

        let contacts = wb.list_contacts().unwrap();
        assert_eq!(contacts.len(), 1);

        // MobileContact must have a pre-formatted fingerprint field
        let mc = &contacts[0];
        assert!(
            !mc.fingerprint.is_empty(),
            "MobileContact should have a fingerprint field"
        );

        // Must match Contact::fingerprint() format: 16 groups of 4 uppercase hex
        let groups: Vec<&str> = mc.fingerprint.split(' ').collect();
        assert_eq!(groups.len(), 16);
    }

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

    // === Device Link Initiator/Responder Tests ===

    // @scenario: device_management:Generate device linking code
    #[test]
    fn test_start_device_link_returns_initiator() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let initiator = wb.start_device_link().unwrap();
        let qr_data = initiator.qr_data();
        assert!(!qr_data.is_empty());

        let challenge = initiator.proximity_challenge();
        assert_eq!(challenge.len(), 16);
    }

    // @scenario: device_management:Link new device via QR code
    #[test]
    fn test_start_device_join_returns_responder() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Generate QR from initiator
        let initiator = wb.start_device_link().unwrap();
        let qr_data = initiator.qr_data();

        // Create responder from QR (new device)
        let (wb2, _dir2) = create_test_instance();
        let responder = wb2
            .start_device_join(qr_data, "Bob's Phone".to_string())
            .unwrap();

        let request_bytes = responder.create_request().unwrap();
        assert!(!request_bytes.is_empty());

        let code = responder.compute_confirmation_code().unwrap();
        assert_eq!(code.len(), 7); // "XXX-XXX"
    }

    // @scenario: device_management:Link new device via QR code
    #[test]
    fn test_device_link_full_flow() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Step 1: Existing device creates initiator
        let initiator = wb.start_device_link().unwrap();
        let qr_data = initiator.qr_data();

        // Step 2: New device scans QR and creates request
        let (wb2, _dir2) = create_test_instance();
        let responder = wb2
            .start_device_join(qr_data, "Bob's Phone".to_string())
            .unwrap();
        let request_bytes = responder.create_request().unwrap();
        let responder_code = responder.compute_confirmation_code().unwrap();

        // Step 3: Existing device prepares confirmation
        let confirmation = initiator.prepare_confirmation(request_bytes).unwrap();
        assert_eq!(confirmation.device_name, "Bob's Phone");
        assert_eq!(confirmation.confirmation_code.len(), 7);
        assert!(!confirmation.identity_fingerprint.is_empty());

        // Codes should match
        assert_eq!(confirmation.confirmation_code, responder_code);

        // Step 4: Existing device confirms with ultrasonic proof
        let challenge = initiator.proximity_challenge();
        let verified_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let result = initiator
            .confirm_link_ultrasonic(challenge, verified_at)
            .unwrap();
        assert!(result.success);
        assert_eq!(result.device_name, "Bob's Phone");
        assert!(result.device_index > 0);

        // Step 5: New device processes response
        let response_bytes = result
            .encrypted_response
            .expect("should have response bytes");
        let join_result = responder.finish_join(response_bytes).unwrap();
        assert!(join_result.success);
    }

    // @scenario: device_management:Linking requires proximity verification
    #[test]
    fn test_device_link_confirm_without_proximity_fails() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let initiator = wb.start_device_link().unwrap();
        let qr_data = initiator.qr_data();

        let (wb2, _dir2) = create_test_instance();
        let responder = wb2
            .start_device_join(qr_data, "Bob's Phone".to_string())
            .unwrap();
        let request_bytes = responder.create_request().unwrap();

        let _confirmation = initiator.prepare_confirmation(request_bytes).unwrap();

        // Should fail with wrong proximity proof
        let wrong_challenge = vec![0xFFu8; 16];
        let verified_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let result = initiator.confirm_link_ultrasonic(wrong_challenge, verified_at);
        result.expect_err("expected error");
    }

    // @scenario: device_management:Link new device via manual confirmation
    #[test]
    fn test_device_link_manual_confirmation_succeeds() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Step 1: Existing device creates initiator
        let initiator = wb.start_device_link().unwrap();
        let qr_data = initiator.qr_data();

        // Step 2: New device scans QR and creates request
        let (wb2, _dir2) = create_test_instance();
        let responder = wb2
            .start_device_join(qr_data, "Carol's Tablet".to_string())
            .unwrap();
        let request_bytes = responder.create_request().unwrap();
        let responder_code = responder.compute_confirmation_code().unwrap();

        // Step 3: Existing device prepares confirmation
        let confirmation = initiator.prepare_confirmation(request_bytes).unwrap();
        assert_eq!(confirmation.device_name, "Carol's Tablet");
        assert_eq!(confirmation.confirmation_code, responder_code);

        // Step 4: Existing device confirms with manual proof (raw code string)
        let confirmed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let result = initiator
            .confirm_link_manual(confirmation.confirmation_code.clone(), confirmed_at)
            .unwrap();
        assert!(result.success);
        assert_eq!(result.device_name, "Carol's Tablet");
        assert!(result.device_index > 0);
        assert!(
            result.encrypted_response.is_some(),
            "manual confirmation should produce encrypted response"
        );

        // Step 5: New device processes response
        let response_bytes = result
            .encrypted_response
            .expect("should have response bytes");
        let join_result = responder.finish_join(response_bytes).unwrap();
        assert!(join_result.success);
    }

    // @scenario: device_management:Linking requires correct confirmation code
    #[test]
    fn test_device_link_manual_confirmation_wrong_code_fails() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Step 1: Existing device creates initiator
        let initiator = wb.start_device_link().unwrap();
        let qr_data = initiator.qr_data();

        // Step 2: New device scans QR and creates request
        let (wb2, _dir2) = create_test_instance();
        let responder = wb2
            .start_device_join(qr_data, "Eve's Phone".to_string())
            .unwrap();
        let request_bytes = responder.create_request().unwrap();

        // Step 3: Existing device prepares confirmation
        let _confirmation = initiator.prepare_confirmation(request_bytes).unwrap();

        // Step 4: Attempt manual confirmation with WRONG code
        let confirmed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let result = initiator.confirm_link_manual("000-000".to_string(), confirmed_at);
        assert!(
            result.is_err(),
            "manual confirmation with wrong code must fail"
        );
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
    fn test_encode_multipart_qr_roundtrip_with_mobile_decoder() {
        let (wb, _dir) = create_test_instance();
        let original = b"End-to-end test: VauchiPlatform encodes, MobileMultipartDecoder decodes.";
        let chunks = wb.encode_multipart_qr(original.to_vec());

        let decoder = multipart_qr::MobileMultipartDecoder::new();
        for chunk in &chunks {
            decoder.add_chunk(chunk.clone()).expect("valid chunk");
        }

        assert!(decoder.is_complete(), "decoder should be complete");
        let assembled = decoder.assemble().expect("assemble should succeed");
        assert_eq!(
            assembled,
            original.to_vec(),
            "roundtrip via VauchiPlatform + MobileMultipartDecoder must preserve data"
        );
    }

    // === Privacy Indicator Tests (SP-12b Phase 2) ===

    // @scenario: message_delivery:Delivery receipts enabled by default
    #[test]
    fn test_delivery_receipts_enabled_by_default() {
        let (wb, _dir) = create_test_instance();
        assert!(
            wb.is_delivery_receipts_enabled(),
            "Delivery receipts should be enabled by default"
        );
    }

    // @scenario: message_delivery:User can disable delivery receipts
    #[test]
    fn test_set_delivery_receipts_disabled() {
        let (wb, _dir) = create_test_instance();
        wb.set_delivery_receipts_enabled(false);
        assert!(
            !wb.is_delivery_receipts_enabled(),
            "Delivery receipts should be disabled after setting"
        );
    }

    // @scenario: message_delivery:Suppress presence defaults to false
    #[test]
    fn test_suppress_presence_defaults_to_false() {
        let (wb, _dir) = create_test_instance();
        assert!(
            !wb.is_suppress_presence_enabled(),
            "Suppress presence should default to false"
        );
    }

    // @scenario: message_delivery:User can enable suppress presence
    #[test]
    fn test_set_suppress_presence_enabled() {
        let (wb, _dir) = create_test_instance();
        wb.set_suppress_presence_enabled(true);
        assert!(
            wb.is_suppress_presence_enabled(),
            "Suppress presence should be enabled after setting"
        );
    }

    // ============================================================================
    // MobileContactTrustLevel, exchange_transport, proximity_confidence, proposal_trusted
    // Based on: features/contact_trust.feature
    // ============================================================================

    fn make_test_contact() -> vauchi_core::Contact {
        vauchi_core::Contact::from_exchange(
            [0xAB; 32],
            vauchi_core::contact_card::ContactCard::new("Bob"),
            vauchi_core::crypto::SymmetricKey::generate(),
        )
    }

    // @scenario: contact_trust:Standard trust contact has correct fields in MobileContact
    #[test]
    fn test_mobile_contact_trust_level_standard() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let contact = make_test_contact();
        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact).unwrap();

        let contacts = wb.list_contacts().unwrap();
        assert_eq!(contacts.len(), 1);
        let mc = &contacts[0];

        // Default exchange has no proximity verification — must be Standard
        assert_eq!(mc.trust_level, MobileContactTrustLevel::Standard);
        assert_eq!(mc.exchange_transport, "qr");
        assert_eq!(mc.proximity_confidence, "unknown");
        assert!(!mc.proposal_trusted);
    }

    // @scenario: contact_trust:Fingerprint-verified contact maps to Verified trust level
    #[test]
    fn test_mobile_contact_trust_level_verified() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let mut contact = make_test_contact();
        contact.mark_fingerprint_verified().unwrap();
        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact).unwrap();

        let contacts = wb.list_contacts().unwrap();
        let mc = &contacts[0];
        assert_eq!(mc.trust_level, MobileContactTrustLevel::Verified);
        assert!(mc.is_verified);
    }

    // @scenario: contact_trust:proposal_trusted flag round-trips via set_proposal_trusted
    #[test]
    fn test_set_proposal_trusted_round_trip() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let contact = make_test_contact();
        let contact_id = contact.id().to_string();
        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact).unwrap();

        // Initially false
        let mc = wb.get_contact(contact_id.clone()).unwrap().unwrap();
        assert!(!mc.proposal_trusted);

        // Set trusted
        wb.set_proposal_trusted(contact_id.clone(), true).unwrap();
        let mc = wb.get_contact(contact_id.clone()).unwrap().unwrap();
        assert!(mc.proposal_trusted);

        // Unset trusted
        wb.set_proposal_trusted(contact_id.clone(), false).unwrap();
        let mc = wb.get_contact(contact_id).unwrap().unwrap();
        assert!(!mc.proposal_trusted);
    }

    // @scenario: contact_trust:set_proposal_trusted returns ContactNotFound for unknown ID
    #[test]
    fn test_set_proposal_trusted_contact_not_found() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let err = wb
            .set_proposal_trusted("nonexistent_id".to_string(), true)
            .unwrap_err();
        assert!(
            matches!(
                &err,
                MobileError::Other { detail } if detail.starts_with("Contact not found:")
            ),
            "expected Other(Contact not found: …), got {err:?}"
        );
    }

    // ============================================================================
    // Personal Notes (set_contact_note / get_contact_note / delete_contact_note)
    // Based on: features/contact_notes.feature
    // ============================================================================

    // @scenario: contact_notes:Personal note round-trips via set/get
    #[test]
    fn test_personal_note_round_trip() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let contact = make_test_contact();
        let contact_id = contact.id().to_string();
        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact).unwrap();

        // No note yet
        let note = wb.get_contact_note(contact_id.clone()).unwrap();
        assert!(note.is_none(), "expected no note, got {note:?}");

        // Save and retrieve
        wb.set_contact_note(contact_id.clone(), "Met at conf.".to_string())
            .unwrap();
        let note = wb.get_contact_note(contact_id.clone()).unwrap();
        assert_eq!(note.as_deref(), Some("Met at conf."));

        // Delete clears it
        wb.delete_contact_note(contact_id.clone()).unwrap();
        let note = wb.get_contact_note(contact_id).unwrap();
        assert!(note.is_none(), "note should be gone after delete");
    }

    // @scenario: contact_notes:Overwriting a note replaces the previous value
    #[test]
    fn test_personal_note_overwrite() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let contact = make_test_contact();
        let contact_id = contact.id().to_string();
        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact).unwrap();

        wb.set_contact_note(contact_id.clone(), "first".to_string())
            .unwrap();
        wb.set_contact_note(contact_id.clone(), "second".to_string())
            .unwrap();

        let note = wb.get_contact_note(contact_id).unwrap();
        assert_eq!(note.as_deref(), Some("second"));
    }

    // ============================================================================
    // Contact Field Notes (set/get/delete_contact_field_note)
    // Based on: features/contact_notes.feature
    // ============================================================================

    // @scenario: contact_notes:Field note round-trips via set/get
    #[test]
    fn test_contact_field_note_round_trip() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let contact = make_test_contact();
        let contact_id = contact.id().to_string();
        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact).unwrap();

        // No notes initially
        let notes = wb.get_contact_field_notes(contact_id.clone()).unwrap();
        assert!(notes.is_empty(), "expected no notes, got {notes:?}");

        // Save a note for field "field_001"
        wb.set_contact_field_note(
            contact_id.clone(),
            "field_001".to_string(),
            "home number".to_string(),
        )
        .unwrap();

        let notes = wb.get_contact_field_notes(contact_id.clone()).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].field_id, "field_001");
        assert_eq!(notes[0].note, "home number");

        // Delete removes it
        wb.delete_contact_field_note(contact_id.clone(), "field_001".to_string())
            .unwrap();
        let notes = wb.get_contact_field_notes(contact_id).unwrap();
        assert!(notes.is_empty(), "note should be gone after delete");
    }

    // @scenario: contact_notes:Multiple field notes are returned sorted by field_id
    #[test]
    fn test_contact_field_notes_multiple_sorted() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let contact = make_test_contact();
        let contact_id = contact.id().to_string();
        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact).unwrap();

        wb.set_contact_field_note(
            contact_id.clone(),
            "zzz_field".to_string(),
            "last note".to_string(),
        )
        .unwrap();
        wb.set_contact_field_note(
            contact_id.clone(),
            "aaa_field".to_string(),
            "first note".to_string(),
        )
        .unwrap();

        let notes = wb.get_contact_field_notes(contact_id).unwrap();
        assert_eq!(notes.len(), 2);
        // Sorted by field_id
        assert_eq!(notes[0].field_id, "aaa_field");
        assert_eq!(notes[1].field_id, "zzz_field");
    }

    // ============================================================================
    // MobileContactField.note — field note surfaced via ContactCard
    // Based on: features/contact_notes.feature
    // ============================================================================

    // @scenario: contact_notes:ContactField note is exposed in MobileContactField
    #[test]
    fn test_mobile_contact_field_note_exposed() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Build a contact whose card has a field with a private note
        let email_field = vauchi_core::ContactField::new(
            vauchi_core::FieldType::Email,
            "work",
            "bob@example.com",
        )
        .with_note("Bob's work email".to_string());

        let mut card = vauchi_core::contact_card::ContactCard::new("Bob");
        card.add_field(email_field).unwrap();

        let contact = vauchi_core::Contact::from_exchange(
            [0xBC; 32],
            card,
            vauchi_core::crypto::SymmetricKey::generate(),
        );

        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact).unwrap();

        let contacts = wb.list_contacts().unwrap();
        assert_eq!(contacts.len(), 1);
        let field = &contacts[0].card.fields[0];
        assert_eq!(field.label, "work");
        assert_eq!(field.note.as_deref(), Some("Bob's work email"));
    }

    // @scenario: contact_notes:ContactField without note has None in MobileContactField
    #[test]
    fn test_mobile_contact_field_note_none_when_absent() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let contact = make_test_contact();
        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact).unwrap();

        let contacts = wb.list_contacts().unwrap();
        // Bob's default card has no fields — verify contact serialises cleanly
        let mc = &contacts[0];
        assert!(mc.card.fields.is_empty());

        // Add a field without a note via the API and confirm note is None
        let email_field = vauchi_core::ContactField::new(
            vauchi_core::FieldType::Email,
            "personal",
            "bob@personal.com",
        );
        let mut card = vauchi_core::contact_card::ContactCard::new("Bob");
        card.add_field(email_field).unwrap();

        let contact_with_field = vauchi_core::Contact::from_exchange(
            [0xCD; 32],
            card,
            vauchi_core::crypto::SymmetricKey::generate(),
        );
        let storage = wb.open_storage().unwrap();
        storage.save_contact(&contact_with_field).unwrap();

        let all = wb.list_contacts().unwrap();
        let with_field = all
            .iter()
            .find(|c| c.exchange_transport == "qr" && !c.card.fields.is_empty())
            .expect("should find contact with field");
        assert!(
            with_field.card.fields[0].note.is_none(),
            "field without note should have note = None"
        );
    }

    // ── Import contacts via FFI ─────────────────────────────────────────────

    // @scenario: contact_import.feature - Import vCard file
    #[test]
    fn test_import_vcf_creates_contacts() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let vcf = b"BEGIN:VCARD\r\n\
            VERSION:3.0\r\n\
            FN:Bob Smith\r\n\
            TEL:+1234567890\r\n\
            END:VCARD\r\n\
            BEGIN:VCARD\r\n\
            VERSION:3.0\r\n\
            FN:Carol Jones\r\n\
            EMAIL:carol@example.com\r\n\
            END:VCARD\r\n";

        let result = wb.import_contacts_from_vcf(vcf.to_vec()).unwrap();
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped, 0);
        assert!(result.warnings.is_empty());

        let contacts = wb.list_contacts().unwrap();
        assert_eq!(contacts.len(), 2);
    }

    // @scenario: contact_import.feature - Duplicate vCard UIDs are skipped
    #[test]
    fn test_import_vcf_skips_duplicates() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let vcf = b"BEGIN:VCARD\r\n\
            VERSION:3.0\r\n\
            UID:unique-bob-123\r\n\
            FN:Bob Smith\r\n\
            END:VCARD\r\n";

        let r1 = wb.import_contacts_from_vcf(vcf.to_vec()).unwrap();
        assert_eq!(r1.imported, 1);

        let r2 = wb.import_contacts_from_vcf(vcf.to_vec()).unwrap();
        assert_eq!(r2.imported, 0);
        assert_eq!(r2.skipped, 1);
        assert_eq!(r2.warnings[0].key, "import.warning.duplicate_uid");
        assert!(r2.warnings[0].legacy_text.contains("duplicate"));
    }

    // @scenario: contact_import.feature - Empty vCard data returns zero imports
    #[test]
    fn test_import_empty_vcf() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let result = wb.import_contacts_from_vcf(Vec::new());
        // Empty data is either zero imports or an error — both acceptable
        if let Ok(r) = result {
            assert_eq!(r.imported, 0);
        }
    }

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

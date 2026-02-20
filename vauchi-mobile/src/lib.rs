// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Vauchi Mobile Bindings
//!
//! UniFFI bindings for Android and iOS platforms.
//! Exposes a simplified, mobile-friendly API on top of vauchi-core.
//!
//! Note: Storage connections are created on-demand for thread safety,
//! as rusqlite's Connection is not Sync.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;

use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::{DeviceLinkQR, EncryptedExchangeMessage};
use vauchi_core::recovery::{RecoveryClaim, RecoveryProof, RecoveryVoucher};
use vauchi_core::{
    ContactCard, ContactField, Identity, IdentityBackup, SocialNetworkRegistry, Storage,
    SymmetricKey, Vauchi, VauchiConfig,
};

#[cfg(feature = "content-updates")]
use vauchi_core::content::{ContentConfig, ContentManager};

// === Modules ===

mod audio;
mod cert_pinning;
mod content;
mod error;
mod exchange;
mod protocol;
mod sync;
mod types;

// Re-export public types
pub use audio::{MobileProximityResult, MobileProximityVerifier, PlatformAudioHandler};
pub use content::{
    MobileApplyFailure, MobileApplyResult, MobileContentConfig, MobileContentType,
    MobileUpdateStatus,
};
pub use error::{KeychainError, MobileError};
pub use exchange::{
    MobileBleExchangeStatus, MobileExchangeSession, MobileExchangeState, MobileProximityHandler,
};
pub use types::{
    MobileAhaMoment, MobileAhaMomentType, MobileAuthMode, MobileBroadcastResult,
    MobileConsentRecord, MobileConsentType, MobileContact, MobileContactCard, MobileContactField,
    MobileDecoyContact, MobileDeletionInfo, MobileDeletionState, MobileDeliveryRecord,
    MobileDeliveryStatus, MobileDeliverySummary, MobileDemoContact, MobileDemoContactState,
    MobileDeviceDeliveryRecord, MobileDeviceDeliveryStatus, MobileDeviceInfo,
    MobileDeviceJoinResult, MobileDeviceLinkConfirmation, MobileDeviceLinkData,
    MobileDeviceLinkInfo, MobileDeviceLinkResult, MobileDuressSettings, MobileEmergencyConfig,
    MobileExchangeResult, MobileFaqItem, MobileFieldType, MobileFieldValidation, MobileGdprExport,
    MobileHelpCategory, MobileHelpCategoryInfo, MobileLocale, MobileLocaleInfo,
    MobileRecoveryClaim, MobileRecoveryProgress, MobileRecoveryVerification, MobileRecoveryVoucher,
    MobileRetryEntry, MobileShredReport, MobileShredStatus, MobileShredToken,
    MobileShredVerification, MobileSocialNetwork, MobileSyncResult, MobileSyncStatus, MobileTheme,
    MobileThemeColors, MobileThemeMode, MobileTrustLevel, MobileValidationStatus,
    MobileVisibilityLabel, MobileVisibilityLabelDetail,
};

uniffi::setup_scaffolding!();

// === Device Link Wrapper Objects ===

use vauchi_core::exchange::{DeviceLinkInitiator, DeviceLinkRequest, DeviceLinkResponder};

/// UniFFI-exposed wrapper around DeviceLinkInitiator.
///
/// Uses Mutex for interior mutability (required by UniFFI's Arc<T>).
/// Holds both the initiator and a pending request between prepare_confirmation
/// and confirm_link calls.
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
        self.inner.lock().unwrap().qr().to_data_string()
    }

    /// Returns the 16-byte proximity challenge.
    pub fn proximity_challenge(&self) -> Vec<u8> {
        self.inner.lock().unwrap().proximity_challenge().to_vec()
    }

    /// Decrypts an incoming link request and returns confirmation details.
    ///
    /// The caller displays the confirmation code and device name to the user.
    pub fn prepare_confirmation(
        &self,
        encrypted_request: Vec<u8>,
    ) -> Result<MobileDeviceLinkConfirmation, MobileError> {
        let initiator = self.inner.lock().unwrap();
        let (confirmation, request) = initiator
            .prepare_confirmation(&encrypted_request)
            .map_err(|e| MobileError::ExchangeFailed(e.to_string()))?;

        // Store request for confirm_link
        *self.pending_request.lock().unwrap() = Some(request);

        Ok(MobileDeviceLinkConfirmation {
            device_name: confirmation.device_name,
            confirmation_code: confirmation.confirmation_code,
            identity_fingerprint: confirmation.identity_fingerprint,
        })
    }

    /// Marks proximity as verified.
    pub fn set_proximity_verified(&self) {
        self.inner.lock().unwrap().set_proximity_verified();
    }

    /// After user confirms, creates the encrypted response.
    ///
    /// Must call prepare_confirmation() and set_proximity_verified() first.
    pub fn confirm_link(&self) -> Result<MobileDeviceLinkResult, MobileError> {
        let request = self
            .pending_request
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| {
                MobileError::ExchangeFailed("No pending request — call prepare_confirmation first".into())
            })?;

        let initiator = self.inner.lock().unwrap();
        let (encrypted_response, _registry, device_info) = initiator
            .confirm_link(&request)
            .map_err(|e| MobileError::ExchangeFailed(e.to_string()))?;

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
#[derive(uniffi::Object)]
pub struct MobileDeviceLinkResponder {
    inner: Mutex<DeviceLinkResponder>,
}

#[uniffi::export]
impl MobileDeviceLinkResponder {
    /// Creates an encrypted request to send to the existing device.
    pub fn create_request(&self) -> Result<Vec<u8>, MobileError> {
        self.inner
            .lock()
            .unwrap()
            .create_request()
            .map_err(|e| MobileError::ExchangeFailed(e.to_string()))
    }

    /// Computes the confirmation code (must call create_request first).
    pub fn compute_confirmation_code(&self) -> Result<String, MobileError> {
        self.inner
            .lock()
            .unwrap()
            .compute_confirmation_code()
            .map_err(|e| MobileError::ExchangeFailed(e.to_string()))
    }

    /// Returns the identity fingerprint from the QR.
    pub fn identity_fingerprint(&self) -> String {
        self.inner.lock().unwrap().identity_fingerprint()
    }

    /// Processes the encrypted response from the existing device.
    pub fn finish_join(&self, encrypted_response: Vec<u8>) -> Result<MobileDeviceJoinResult, MobileError> {
        let responder = self.inner.lock().unwrap();
        let response = responder
            .process_response(&encrypted_response)
            .map_err(|e| MobileError::ExchangeFailed(e.to_string()))?;

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

/// Get all available bundled themes.
#[uniffi::export]
pub fn get_available_themes() -> Vec<MobileTheme> {
    vauchi_core::theme::get_bundled_themes()
        .iter()
        .map(MobileTheme::from)
        .collect()
}

/// Get a specific theme by ID.
///
/// Returns None if the theme is not found.
#[uniffi::export]
pub fn get_theme(theme_id: String) -> Option<MobileTheme> {
    vauchi_core::theme::get_theme_by_id(&theme_id).map(|t| MobileTheme::from(&t))
}

/// Get the default theme ID based on system preference.
///
/// Returns "default-dark" for dark mode, "default-light" for light mode.
#[uniffi::export]
pub fn get_default_theme_id(prefer_dark: bool) -> String {
    if prefer_dark {
        "default-dark".to_string()
    } else {
        "default-light".to_string()
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
    vauchi_core::i18n::init(std::path::Path::new(&resource_dir))
        .map_err(|e| MobileError::InitError(e.to_string()))
}

/// Get all available locales.
#[uniffi::export]
pub fn get_available_locales() -> Vec<MobileLocaleInfo> {
    vauchi_core::i18n::get_available_locales()
        .into_iter()
        .map(|l| MobileLocaleInfo::from(vauchi_core::i18n::get_locale_info(l)))
        .collect()
}

/// Get information about a specific locale.
#[uniffi::export]
pub fn get_locale_info(locale: MobileLocale) -> MobileLocaleInfo {
    MobileLocaleInfo::from(vauchi_core::i18n::get_locale_info(locale.into()))
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
    vauchi_core::i18n::Locale::from_code(&code).map(MobileLocale::from)
}

// ============================================================
// Help Functions
// ============================================================

/// Get all help categories with their display names.
#[uniffi::export]
pub fn get_help_categories() -> Vec<MobileHelpCategoryInfo> {
    vauchi_core::help::HelpCategory::all()
        .iter()
        .map(|c| MobileHelpCategoryInfo {
            category: (*c).into(),
            display_name: c.display_name().to_string(),
        })
        .collect()
}

/// Get all FAQ items.
#[uniffi::export]
pub fn get_faqs() -> Vec<MobileFaqItem> {
    vauchi_core::help::get_faqs()
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get FAQ items for a specific category.
#[uniffi::export]
pub fn get_faqs_by_category(category: MobileHelpCategory) -> Vec<MobileFaqItem> {
    vauchi_core::help::get_faqs_by_category(category.into())
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get a specific FAQ item by ID.
///
/// Returns None if the FAQ is not found.
#[uniffi::export]
pub fn get_faq_by_id(id: String) -> Option<MobileFaqItem> {
    vauchi_core::help::get_faq_by_id(&id).map(|f| MobileFaqItem::from(&f))
}

/// Search FAQs by query text.
///
/// Searches in both questions and answers (case-insensitive).
#[uniffi::export]
pub fn search_faqs(query: String) -> Vec<MobileFaqItem> {
    vauchi_core::help::search_faqs(&query)
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get all FAQ items in the specified locale.
#[uniffi::export]
pub fn get_faqs_localized(locale: MobileLocale) -> Vec<MobileFaqItem> {
    vauchi_core::help::get_faqs_localized(locale.into())
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get FAQ items for a specific category in the specified locale.
#[uniffi::export]
pub fn get_faqs_by_category_localized(
    category: MobileHelpCategory,
    locale: MobileLocale,
) -> Vec<MobileFaqItem> {
    vauchi_core::help::get_faqs_by_category_localized(category.into(), locale.into())
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get a specific FAQ item by ID in the specified locale.
///
/// Returns None if the FAQ is not found.
#[uniffi::export]
pub fn get_faq_by_id_localized(id: String, locale: MobileLocale) -> Option<MobileFaqItem> {
    vauchi_core::help::get_faq_by_id_localized(&id, locale.into()).map(|f| MobileFaqItem::from(&f))
}

/// Search FAQs by query text in the specified locale.
///
/// Searches in both questions and answers (case-insensitive).
#[uniffi::export]
pub fn search_faqs_localized(query: String, locale: MobileLocale) -> Vec<MobileFaqItem> {
    vauchi_core::help::search_faqs_localized(&query, locale.into())
        .iter()
        .map(MobileFaqItem::from)
        .collect()
}

/// Get localized aha moment content for a given moment type.
///
/// Returns the title, message, and animation flag for display.
/// This is a stateless helper — it doesn't check whether the moment
/// has been seen. Use `try_trigger_aha_moment` on VauchiMobile for
/// state-tracked triggering.
#[uniffi::export]
pub fn get_aha_moment_localized(
    moment_type: MobileAhaMomentType,
    locale: MobileLocale,
) -> MobileAhaMoment {
    let core_type: vauchi_core::AhaMomentType = moment_type.into();
    let core_locale: vauchi_core::i18n::Locale = locale.into();
    MobileAhaMoment {
        moment_type,
        title: core_type.title_localized(core_locale),
        message: core_type.message_localized(core_locale),
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
/// **WARNING**: This operation is irreversible and immediate. All account
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
    let report = vauchi_core::api::widget_panic_shred(path, &bridge)
        .map_err(|e| MobileError::ShredError(e.to_string()))?;
    Ok(MobileShredReport::from(&report))
}

// === Main Interface ===

/// Main Vauchi interface for mobile platforms.
///
/// Uses on-demand storage connections for thread safety.
#[derive(uniffi::Object)]
pub struct VauchiMobile {
    storage_path: PathBuf,
    storage_key: SymmetricKey,
    relay_url: String,
    /// Optional PEM-encoded certificate for TLS pinning.
    pinned_cert_pem: Mutex<Option<String>>,
    identity_data: Mutex<Option<IdentityData>>,
    social_registry: SocialNetworkRegistry,
    sync_status: Mutex<MobileSyncStatus>,
    /// Platform keychain for crypto-shredding operations.
    platform_keychain: Mutex<Option<Arc<dyn MobilePlatformKeychain>>>,
}

impl VauchiMobile {
    /// Opens a storage connection.
    fn open_storage(&self) -> Result<Storage, MobileError> {
        Storage::open(&self.storage_path, self.storage_key.clone())
            .map_err(|e| MobileError::StorageError(e.to_string()))
    }

    /// Opens a Vauchi API instance backed by the same storage.
    ///
    /// Use this for operations that must dispatch events (e.g. hide/unhide contact).
    /// Operations that only read data can continue using `open_storage()` directly.
    fn open_vauchi(&self) -> Result<Vauchi, MobileError> {
        let config = VauchiConfig::with_storage_path(&self.storage_path)
            .with_relay_url(&self.relay_url)
            .with_storage_key(self.storage_key.clone());
        Vauchi::new(config).map_err(|e| MobileError::Internal(e.to_string()))
    }

    /// Returns the data directory (parent of the database file).
    fn data_dir(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .to_path_buf()
    }

    /// Gets the platform keychain bridge for shred operations.
    fn get_keychain_bridge(&self) -> Result<KeychainBridge, MobileError> {
        let lock = self.platform_keychain.lock().unwrap();
        let callback = lock
            .as_ref()
            .ok_or_else(|| {
                MobileError::ShredError(
                    "Platform keychain not set. Call set_platform_keychain() first.".into(),
                )
            })?
            .clone();
        Ok(KeychainBridge { callback })
    }

    /// Gets the identity from stored data.
    fn get_identity(&self) -> Result<Identity, MobileError> {
        let data = self.identity_data.lock().unwrap();
        let identity_data = data.as_ref().ok_or(MobileError::IdentityNotFound)?;

        let backup = IdentityBackup::new(identity_data.backup_data.clone());
        Identity::import_backup(&backup, "__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))
    }

    /// Get pinned certificate if set.
    fn get_pinned_cert(&self) -> Option<String> {
        self.pinned_cert_pem.lock().unwrap().clone()
    }

    /// Get our contact card, or create a default one from the identity.
    fn get_own_card_or_default(&self, identity: &Identity) -> Result<ContactCard, MobileError> {
        let storage = self.open_storage()?;
        Ok(storage
            .load_own_card()
            .ok()
            .flatten()
            .unwrap_or_else(|| ContactCard::new(identity.display_name())))
    }

    /// Get the path to the recovery proof file.
    fn recovery_proof_path(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".recovery_proof")
    }

    // === Aha Moments (internal helpers) ===

    /// Get the path to the aha moments state file.
    fn aha_moments_path(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".aha_moments")
    }

    /// Load the aha moments tracker from storage.
    fn load_aha_tracker(&self) -> vauchi_core::AhaMomentTracker {
        let path = self.aha_moments_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            vauchi_core::AhaMomentTracker::from_json(&data).unwrap_or_default()
        } else {
            vauchi_core::AhaMomentTracker::new()
        }
    }

    /// Save the aha moments tracker to storage.
    fn save_aha_tracker(&self, tracker: &vauchi_core::AhaMomentTracker) -> Result<(), MobileError> {
        let path = self.aha_moments_path();
        let data = tracker
            .to_json()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        std::fs::write(&path, data).map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(())
    }

    // === Demo Contact (internal helpers) ===

    /// Get the path to the demo contact state file.
    fn demo_contact_path(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".demo_contact")
    }

    /// Load the demo contact state from storage.
    fn load_demo_state(&self) -> vauchi_core::DemoContactState {
        let path = self.demo_contact_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            vauchi_core::DemoContactState::from_json(&data).unwrap_or_default()
        } else {
            vauchi_core::DemoContactState::default()
        }
    }

    /// Save the demo contact state to storage.
    fn save_demo_state(&self, state: &vauchi_core::DemoContactState) -> Result<(), MobileError> {
        let path = self.demo_contact_path();
        let data = state
            .to_json()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        std::fs::write(&path, data).map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(())
    }
}

#[uniffi::export]
impl VauchiMobile {
    /// Create a new VauchiMobile instance with a platform-provided secure key.
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

        std::fs::create_dir_all(&data_path)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let storage_path = data_path.join("vauchi.db");

        let key_array: [u8; 32] = storage_key_bytes.try_into().map_err(|_| {
            MobileError::StorageError("Storage key must be exactly 32 bytes".to_string())
        })?;
        let storage_key = SymmetricKey::from_bytes_unchecked(key_array);

        let _storage = Storage::open(&storage_path, storage_key.clone())
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        Ok(Arc::new(VauchiMobile {
            storage_path,
            storage_key,
            relay_url,
            pinned_cert_pem: Mutex::new(None),
            identity_data: Mutex::new(None),
            social_registry: SocialNetworkRegistry::with_defaults(),
            sync_status: Mutex::new(MobileSyncStatus::Idle),
            platform_keychain: Mutex::new(None),
        }))
    }

    /// Create a new VauchiMobile instance (legacy constructor).
    ///
    /// WARNING: This constructor stores the encryption key in a plaintext file.
    /// Use `new_with_secure_key` instead for production.
    #[uniffi::constructor]
    pub fn new(data_dir: String, relay_url: String) -> Result<Arc<Self>, MobileError> {
        let data_path = PathBuf::from(&data_dir);

        std::fs::create_dir_all(&data_path)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let storage_path = data_path.join("vauchi.db");
        let key_path = data_path.join("storage.key");

        let storage_key = if key_path.exists() {
            let key_bytes = std::fs::read(&key_path)
                .map_err(|e| MobileError::StorageError(format!("Failed to read key: {}", e)))?;
            let key_array: [u8; 32] = key_bytes
                .try_into()
                .map_err(|_| MobileError::StorageError("Invalid key length".to_string()))?;
            SymmetricKey::from_bytes_unchecked(key_array)
        } else {
            let key = SymmetricKey::generate();
            std::fs::write(&key_path, key.as_bytes())
                .map_err(|e| MobileError::StorageError(format!("Failed to save key: {}", e)))?;
            key
        };

        let _storage = Storage::open(&storage_path, storage_key.clone())
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        Ok(Arc::new(VauchiMobile {
            storage_path,
            storage_key,
            relay_url,
            pinned_cert_pem: Mutex::new(None),
            identity_data: Mutex::new(None),
            social_registry: SocialNetworkRegistry::with_defaults(),
            sync_status: Mutex::new(MobileSyncStatus::Idle),
            platform_keychain: Mutex::new(None),
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
        let mut pinned = self.pinned_cert_pem.lock().unwrap();
        if cert_pem.is_empty() {
            *pinned = None;
        } else {
            *pinned = Some(cert_pem);
        }
    }

    /// Check if certificate pinning is enabled.
    pub fn is_certificate_pinning_enabled(&self) -> bool {
        self.pinned_cert_pem.lock().unwrap().is_some()
    }

    // === Identity Operations ===

    /// Check if identity exists.
    pub fn has_identity(&self) -> bool {
        {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                return true;
            }
        }

        if let Ok(storage) = self.open_storage() {
            if let Ok(Some((backup_data, display_name))) = storage.load_identity() {
                let identity_data = IdentityData {
                    backup_data,
                    display_name,
                };
                *self.identity_data.lock().unwrap() = Some(identity_data);
                return true;
            }
        }

        false
    }

    /// Create a new identity.
    pub fn create_identity(&self, display_name: String) -> Result<(), MobileError> {
        {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                return Err(MobileError::AlreadyInitialized);
            }
        }

        let identity = Identity::create(&display_name);

        let backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let backup_data = backup.as_bytes().to_vec();

        let storage = self.open_storage()?;
        storage.save_identity(&backup_data, &display_name)?;

        let identity_data = IdentityData {
            backup_data,
            display_name: display_name.clone(),
        };
        *self.identity_data.lock().unwrap() = Some(identity_data);

        let card = ContactCard::new(&display_name);
        storage.save_own_card(&card)?;

        Ok(())
    }

    /// Get public ID.
    pub fn get_public_id(&self) -> Result<String, MobileError> {
        let identity = self.get_identity()?;
        Ok(identity.public_id())
    }

    /// Get display name.
    pub fn get_display_name(&self) -> Result<String, MobileError> {
        let storage = self.open_storage()?;
        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        Ok(card.display_name().to_string())
    }

    // === Contact Card Operations ===

    /// Get own contact card.
    pub fn get_own_card(&self) -> Result<MobileContactCard, MobileError> {
        let storage = self.open_storage()?;
        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        Ok(MobileContactCard::from(&card))
    }

    /// Add field to own card.
    pub fn add_field(
        &self,
        field_type: MobileFieldType,
        label: String,
        value: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        let field = ContactField::new(field_type.into(), &label, &value);
        card.add_field(field)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;

        storage.save_own_card(&card)?;
        Ok(())
    }

    /// Update field value.
    pub fn update_field(&self, label: String, new_value: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        let field_id = card
            .fields()
            .iter()
            .find(|f| f.label() == label)
            .ok_or_else(|| MobileError::InvalidInput(format!("Field '{}' not found", label)))?
            .id()
            .to_string();

        card.update_field_value(&field_id, &new_value)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;

        storage.save_own_card(&card)?;
        Ok(())
    }

    /// Remove field from card.
    pub fn remove_field(&self, label: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        let field_id = match card.fields().iter().find(|f| f.label() == label) {
            Some(f) => f.id().to_string(),
            None => return Ok(false),
        };

        card.remove_field(&field_id)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_own_card(&card)?;

        Ok(true)
    }

    /// Set display name.
    pub fn set_display_name(&self, name: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;

        card.set_display_name(&name)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))?;
        storage.save_own_card(&card)?;

        Ok(())
    }

    // === Contact Operations ===

    /// List all contacts.
    pub fn list_contacts(&self) -> Result<Vec<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        Ok(contacts.iter().map(MobileContact::from).collect())
    }

    /// Get single contact by ID.
    pub fn get_contact(&self, id: String) -> Result<Option<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contact = storage.load_contact(&id)?;
        Ok(contact.as_ref().map(MobileContact::from))
    }

    /// Search contacts using SQL-level search.
    pub fn search_contacts(&self, query: String) -> Result<Vec<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.search_contacts(&query)?;
        Ok(contacts.iter().map(MobileContact::from).collect())
    }

    /// Get contact count.
    pub fn contact_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        Ok(contacts.len() as u32)
    }

    /// Remove contact.
    pub fn remove_contact(&self, id: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let removed = storage.delete_contact(&id)?;
        Ok(removed)
    }

    /// Verify contact fingerprint.
    pub fn verify_contact(&self, id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&id)?
            .ok_or_else(|| MobileError::ContactNotFound(id.clone()))?;

        contact.mark_fingerprint_verified();
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Mark a contact as trusted for recovery.
    ///
    /// Blocked contacts cannot be trusted for recovery.
    pub fn trust_contact_for_recovery(&self, id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&id)?
            .ok_or_else(|| MobileError::ContactNotFound(id.clone()))?;

        if contact.is_blocked() {
            return Err(MobileError::InvalidInput(
                "Blocked contacts cannot be trusted for recovery".to_string(),
            ));
        }

        contact.trust_for_recovery();
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Remove recovery trust from a contact.
    pub fn untrust_contact_for_recovery(&self, id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&id)?
            .ok_or_else(|| MobileError::ContactNotFound(id.clone()))?;

        contact.untrust_for_recovery();
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Get the number of contacts trusted for recovery.
    pub fn trusted_contact_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        let count = contacts.iter().filter(|c| c.is_recovery_trusted()).count();
        Ok(count as u32)
    }

    // === Hidden Contact Operations ===

    /// Hides a contact from the main contact list.
    ///
    /// Hidden contacts provide plausible deniability - they only appear
    /// via secret access (gesture, PIN, or special settings navigation).
    /// Routes through the Vauchi API to ensure `ContactHidden` events are dispatched.
    pub fn hide_contact(&self, contact_id: String) -> Result<(), MobileError> {
        let vauchi = self.open_vauchi()?;
        vauchi.hide_contact(&contact_id)?;
        Ok(())
    }

    /// Unhides a contact, making it visible in the main contact list again.
    /// Routes through the Vauchi API to ensure `ContactUnhidden` events are dispatched.
    pub fn unhide_contact(&self, contact_id: String) -> Result<(), MobileError> {
        let vauchi = self.open_vauchi()?;
        vauchi.unhide_contact(&contact_id)?;
        Ok(())
    }

    /// Lists all hidden contacts.
    /// Routes through the Vauchi API for consistency with hide/unhide operations.
    pub fn list_hidden_contacts(&self) -> Result<Vec<MobileContact>, MobileError> {
        let vauchi = self.open_vauchi()?;
        let contacts = vauchi.list_hidden_contacts()?;
        Ok(contacts.iter().map(MobileContact::from).collect())
    }

    // === App Password / Duress PIN ===

    /// Sets up an app password (PIN).
    ///
    /// Requires an identity to be created first.
    pub fn setup_app_password(&self, password: String) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        // Restore identity into the Vauchi instance
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Internal(e.to_string()))?;
        vauchi.setup_app_password(&password)?;
        Ok(())
    }

    /// Sets up a duress PIN.
    ///
    /// Requires an app password to be configured first.
    pub fn setup_duress_password(&self, duress_password: String) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Internal(e.to_string()))?;
        vauchi.setup_duress_password(&duress_password)?;
        Ok(())
    }

    /// Authenticates with a password.
    ///
    /// Returns the authentication mode:
    /// - `Normal` if the real password matches
    /// - `Duress` if the duress PIN matches
    /// - Returns an error if neither matches
    pub fn authenticate(&self, password: String) -> Result<MobileAuthMode, MobileError> {
        let mut vauchi = self.open_vauchi()?;
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Internal(e.to_string()))?;
        let mode = vauchi.authenticate(&password)?;
        match mode {
            vauchi_core::AuthMode::Normal => Ok(MobileAuthMode::Normal),
            vauchi_core::AuthMode::Duress => Ok(MobileAuthMode::Duress),
            vauchi_core::AuthMode::Unauthenticated => Ok(MobileAuthMode::Normal),
        }
    }

    /// Returns whether an app password has been configured.
    pub fn is_password_enabled(&self) -> Result<bool, MobileError> {
        let vauchi = self.open_vauchi()?;
        Ok(vauchi.is_password_enabled()?)
    }

    /// Returns whether duress mode is enabled.
    pub fn is_duress_enabled(&self) -> Result<bool, MobileError> {
        let vauchi = self.open_vauchi()?;
        Ok(vauchi.is_duress_enabled()?)
    }

    /// Disables duress mode and clears duress hash/salt.
    pub fn disable_duress(&self) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        vauchi.disable_duress()?;
        Ok(())
    }

    // === Duress Settings ===

    /// Configures duress alert settings.
    ///
    /// Sets which contacts receive alerts, the alert message, and
    /// whether to include device location.
    pub fn configure_duress_alerts(
        &self,
        contact_ids: Vec<String>,
        message: String,
    ) -> Result<(), MobileError> {
        let vauchi = self.open_vauchi()?;
        let settings = vauchi_core::DuressSettings {
            alert_contact_ids: contact_ids,
            alert_message: message,
            include_location: false,
        };
        vauchi.save_duress_settings(&settings)?;
        Ok(())
    }

    /// Gets the current duress alert settings.
    ///
    /// Returns `None` if no settings have been configured.
    pub fn get_duress_settings(&self) -> Result<Option<MobileDuressSettings>, MobileError> {
        let vauchi = self.open_vauchi()?;
        let settings = vauchi.load_duress_settings()?;
        Ok(settings.map(|s| MobileDuressSettings {
            alert_contact_ids: s.alert_contact_ids,
            alert_message: s.alert_message,
            include_location: s.include_location,
        }))
    }

    // === Emergency Broadcast ===

    /// Configures the emergency broadcast system.
    ///
    /// Sets which contacts receive emergency alerts, the alert message,
    /// and whether to include device location.
    pub fn configure_emergency_broadcast(
        &self,
        contact_ids: Vec<String>,
        message: String,
        include_location: bool,
    ) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Internal(e.to_string()))?;
        vauchi.configure_emergency_broadcast(contact_ids, message, include_location)?;
        Ok(())
    }

    /// Sends an emergency broadcast to all trusted contacts.
    ///
    /// Returns the number of alerts sent and total configured.
    pub fn send_emergency_broadcast(&self) -> Result<MobileBroadcastResult, MobileError> {
        let mut vauchi = self.open_vauchi()?;
        let identity = self.get_identity()?;
        vauchi
            .set_identity(identity)
            .map_err(|e| MobileError::Internal(e.to_string()))?;
        let result = vauchi.send_emergency_broadcast()?;
        Ok(MobileBroadcastResult {
            sent: result.sent as u32,
            total: result.total as u32,
        })
    }

    /// Gets the current emergency broadcast configuration.
    ///
    /// Returns `None` if no configuration has been set.
    pub fn get_emergency_config(&self) -> Result<Option<MobileEmergencyConfig>, MobileError> {
        let vauchi = self.open_vauchi()?;
        let config = vauchi.load_emergency_config()?;
        Ok(config.map(|c| MobileEmergencyConfig {
            trusted_contact_ids: c.trusted_contact_ids,
            message: c.message,
            include_location: c.include_location,
        }))
    }

    /// Disables the emergency broadcast by deleting the configuration.
    pub fn disable_emergency_broadcast(&self) -> Result<(), MobileError> {
        let mut vauchi = self.open_vauchi()?;
        vauchi.delete_emergency_config()?;
        Ok(())
    }

    // === Decoy Contacts ===

    /// Adds a decoy contact for duress mode.
    ///
    /// The card_json should be a JSON-serialized ContactCard.
    /// Returns the generated ID.
    pub fn add_decoy_contact(
        &self,
        name: String,
        card_json: String,
    ) -> Result<String, MobileError> {
        let vauchi = self.open_vauchi()?;
        let card: ContactCard = serde_json::from_str(&card_json)
            .map_err(|e| MobileError::SerializationError(e.to_string()))?;
        let id = format!(
            "decoy-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        vauchi.add_decoy_contact(&id, &name, &card)?;
        Ok(id)
    }

    /// Lists all decoy contacts.
    pub fn list_decoy_contacts(&self) -> Result<Vec<MobileDecoyContact>, MobileError> {
        let vauchi = self.open_vauchi()?;
        let decoys = vauchi.list_decoy_contacts()?;
        Ok(decoys
            .into_iter()
            .map(|(id, display_name, _card)| MobileDecoyContact { id, display_name })
            .collect())
    }

    /// Deletes a decoy contact by ID.
    pub fn delete_decoy_contact(&self, id: String) -> Result<(), MobileError> {
        let vauchi = self.open_vauchi()?;
        vauchi.remove_decoy_contact(&id)?;
        Ok(())
    }

    // === Visibility Operations ===

    /// Hide field from contact.
    pub fn hide_field_from_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::ContactNotFound(contact_id.clone()))?;

        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

        contact.visibility_rules_mut().set_nobody(field.id());
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Show field to contact.
    pub fn show_field_to_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let mut contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::ContactNotFound(contact_id.clone()))?;

        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

        contact.visibility_rules_mut().set_everyone(field.id());
        storage.save_contact(&contact)?;

        Ok(())
    }

    /// Check if field is visible to contact.
    pub fn is_field_visible_to_contact(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;

        let contact = storage
            .load_contact(&contact_id)?
            .ok_or_else(|| MobileError::ContactNotFound(contact_id.clone()))?;

        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

        Ok(contact.visibility_rules().can_see(field.id(), &contact_id))
    }

    // === Visibility Labels ===

    /// List all visibility labels.
    pub fn list_labels(&self) -> Result<Vec<MobileVisibilityLabel>, MobileError> {
        let storage = self.open_storage()?;
        let labels = storage.load_all_labels()?;
        Ok(labels.iter().map(MobileVisibilityLabel::from).collect())
    }

    /// Create a new visibility label.
    pub fn create_label(&self, name: String) -> Result<MobileVisibilityLabel, MobileError> {
        let storage = self.open_storage()?;
        let label = storage.create_label(&name)?;
        Ok(MobileVisibilityLabel::from(&label))
    }

    /// Get a label by ID with full details.
    pub fn get_label(&self, label_id: String) -> Result<MobileVisibilityLabelDetail, MobileError> {
        let storage = self.open_storage()?;
        let label = storage.load_label(&label_id)?;
        Ok(MobileVisibilityLabelDetail::from(&label))
    }

    /// Rename a label.
    pub fn rename_label(&self, label_id: String, new_name: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.rename_label(&label_id, &new_name)?;
        Ok(())
    }

    /// Delete a label.
    pub fn delete_label(&self, label_id: String) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.delete_label(&label_id)?;
        Ok(())
    }

    /// Add a contact to a label.
    pub fn add_contact_to_label(
        &self,
        label_id: String,
        contact_id: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.add_contact_to_label(&label_id, &contact_id)?;
        Ok(())
    }

    /// Remove a contact from a label.
    pub fn remove_contact_from_label(
        &self,
        label_id: String,
        contact_id: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        storage.remove_contact_from_label(&label_id, &contact_id)?;
        Ok(())
    }

    /// Get all labels that contain a contact.
    pub fn get_labels_for_contact(
        &self,
        contact_id: String,
    ) -> Result<Vec<MobileVisibilityLabel>, MobileError> {
        let storage = self.open_storage()?;
        let labels = storage.get_labels_for_contact(&contact_id)?;
        Ok(labels.iter().map(MobileVisibilityLabel::from).collect())
    }

    /// Set whether a field is visible to contacts in a label.
    pub fn set_label_field_visibility(
        &self,
        label_id: String,
        field_label: String,
        is_visible: bool,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        // Get field ID from label
        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

        storage.set_label_field_visibility(&label_id, field.id(), is_visible)?;
        Ok(())
    }

    /// Set a per-contact override for field visibility.
    ///
    /// Per-contact overrides take precedence over label-based visibility.
    pub fn set_contact_field_override(
        &self,
        contact_id: String,
        field_label: String,
        is_visible: bool,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

        storage.save_contact_override(&contact_id, field.id(), is_visible)?;
        Ok(())
    }

    /// Remove a per-contact override for field visibility.
    pub fn remove_contact_field_override(
        &self,
        contact_id: String,
        field_label: String,
    ) -> Result<(), MobileError> {
        let storage = self.open_storage()?;

        let card = storage
            .load_own_card()?
            .ok_or(MobileError::IdentityNotFound)?;
        let field = card
            .fields()
            .iter()
            .find(|f| f.label() == field_label)
            .ok_or_else(|| {
                MobileError::InvalidInput(format!("Field not found: {}", field_label))
            })?;

        storage.delete_contact_override(&contact_id, field.id())?;
        Ok(())
    }

    /// Get suggested default labels.
    pub fn get_suggested_labels(&self) -> Vec<String> {
        vauchi_core::SUGGESTED_LABELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    // === Exchange Operations ===

    /// Create a QR exchange session with proximity verification.
    ///
    /// Both parties display and scan QR codes. Uses fresh ephemeral keys
    /// for full forward secrecy.
    pub fn create_qr_exchange(
        &self,
        proximity: Box<dyn MobileProximityHandler>,
    ) -> Result<Arc<MobileExchangeSession>, MobileError> {
        let identity = self.get_identity()?;
        let our_card = self.get_own_card_or_default(&identity)?;
        Ok(exchange::create_qr_exchange_proximity(
            identity, our_card, proximity,
        ))
    }

    /// Create a QR exchange session with manual confirmation (no audio hardware).
    pub fn create_qr_exchange_manual(&self) -> Result<Arc<MobileExchangeSession>, MobileError> {
        let identity = self.get_identity()?;
        let our_card = self.get_own_card_or_default(&identity)?;
        Ok(exchange::create_qr_exchange_manual(identity, our_card))
    }

    /// Finalize a completed exchange session.
    ///
    /// Extracts the contact from the session's Complete state, saves it to storage,
    /// initializes the double ratchet, and sends the encrypted exchange message via relay.
    ///
    /// The session must be in the Complete state (i.e., the state machine has been
    /// driven through all steps).
    pub fn finalize_exchange(
        &self,
        session: &MobileExchangeSession,
    ) -> Result<MobileExchangeResult, MobileError> {
        let contact = session.extract_contact()?;
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let contact_id = contact.id().to_string();
        let contact_name = contact.display_name().to_string();

        // Check for duplicate
        if storage.load_contact(&contact_id)?.is_some() {
            return Err(MobileError::ExchangeFailed(
                "Contact already exists".to_string(),
            ));
        }

        // Save contact
        storage.save_contact(&contact)?;

        // Initialize double ratchet
        let shared_key = contact.shared_key().clone();
        let their_exchange_key = *contact.public_key();
        let ratchet = DoubleRatchetState::initialize_initiator(&shared_key, their_exchange_key);
        storage.save_ratchet_state(&contact_id, &ratchet, true)?;

        // Send encrypted exchange message via relay (async, uses block_on)
        {
            let our_x3dh = identity.x3dh_keypair();
            let (encrypted_msg, _) = EncryptedExchangeMessage::create(
                &our_x3dh,
                &their_exchange_key,
                identity.signing_public_key(),
                identity.display_name(),
            )
            .map_err(|e| MobileError::ExchangeFailed(format!("Key agreement failed: {:?}", e)))?;

            let our_id = identity.public_id();
            let pinned_cert = self.get_pinned_cert();
            let relay_url = self.relay_url.clone();

            let update = protocol::EncryptedUpdate {
                recipient_id: contact_id.clone(),
                sender_id: our_id,
                ciphertext: encrypted_msg.to_bytes(),
            };

            let envelope =
                protocol::create_envelope(protocol::MessagePayload::EncryptedUpdate(update));
            let data = protocol::encode_message(&envelope).map_err(MobileError::SyncFailed)?;

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| MobileError::Internal(format!("Runtime error: {}", e)))?;

            rt.block_on(async {
                let mut socket =
                    cert_pinning::connect_with_pinning(&relay_url, pinned_cert.as_deref())
                        .await
                        .map_err(MobileError::NetworkError)?;

                let handshake =
                    vauchi_core::network::simple_message::create_signed_handshake(&identity, None);
                let hs_envelope =
                    protocol::create_envelope(protocol::MessagePayload::Handshake(handshake));
                let hs_data = protocol::encode_message(&hs_envelope)
                    .map_err(|e| MobileError::SyncFailed(format!("Encode error: {}", e)))?;
                socket
                    .send(Message::Binary(hs_data))
                    .await
                    .map_err(|e| MobileError::NetworkError(e.to_string()))?;

                socket
                    .send(Message::Binary(data))
                    .await
                    .map_err(|e| MobileError::NetworkError(e.to_string()))?;

                tokio::time::sleep(Duration::from_millis(100)).await;
                let _ = socket.close(None).await;

                Ok::<(), MobileError>(())
            })?;
        }

        Ok(MobileExchangeResult {
            contact_id,
            contact_name,
            success: true,
            error_message: None,
        })
    }

    // === Sync Operations ===

    /// Sync with relay server.
    pub fn sync(&self) -> Result<MobileSyncResult, MobileError> {
        *self.sync_status.lock().unwrap() = MobileSyncStatus::Syncing;

        let identity = self.get_identity()?;
        let pinned_cert = self.get_pinned_cert();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| MobileError::Internal(format!("Runtime error: {}", e)))?;

        let result = rt.block_on(sync::do_sync_async(
            &self.storage_path,
            self.storage_key.clone(),
            &identity,
            &self.relay_url,
            pinned_cert.as_deref(),
        ));

        match &result {
            Ok(_) => *self.sync_status.lock().unwrap() = MobileSyncStatus::Idle,
            Err(_) => *self.sync_status.lock().unwrap() = MobileSyncStatus::Error,
        }

        result
    }

    /// Get sync status.
    pub fn get_sync_status(&self) -> MobileSyncStatus {
        *self.sync_status.lock().unwrap()
    }

    /// Get pending update count.
    pub fn pending_update_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        let mut total = 0u32;
        for contact in contacts {
            let pending = storage.get_pending_updates(contact.id())?;
            total += pending.len() as u32;
        }
        Ok(total)
    }

    // === Delivery Status Operations ===

    /// Get delivery record for a message.
    pub fn get_delivery_record(
        &self,
        message_id: String,
    ) -> Result<Option<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let record = storage.get_delivery_record(&message_id)?;
        Ok(record.as_ref().map(MobileDeliveryRecord::from))
    }

    /// Get all delivery records.
    pub fn get_all_delivery_records(&self) -> Result<Vec<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_all_delivery_records()?;
        Ok(records.iter().map(MobileDeliveryRecord::from).collect())
    }

    /// Get all delivery records for a recipient.
    pub fn get_delivery_records_for_contact(
        &self,
        recipient_id: String,
    ) -> Result<Vec<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_delivery_records_for_recipient(&recipient_id)?;
        Ok(records.iter().map(MobileDeliveryRecord::from).collect())
    }

    /// Count failed deliveries.
    pub fn count_failed_deliveries(&self) -> Result<u32, MobileError> {
        use vauchi_core::storage::DeliveryStatus;
        let storage = self.open_storage()?;
        let count = storage.count_deliveries_by_status(&DeliveryStatus::Failed {
            reason: String::new(),
        })?;
        Ok(count as u32)
    }

    /// Manually retry a failed delivery.
    ///
    /// Returns true if the retry entry was found and rescheduled.
    pub fn manual_retry(&self, message_id: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;

        // Check if there's a retry entry for this message
        let entry = storage.get_retry_entry(&message_id)?;
        if entry.is_none() {
            return Ok(false);
        }

        // Reschedule for immediate retry
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        storage.update_retry_next_time(&message_id, now)?;
        Ok(true)
    }

    /// Get all pending (non-terminal) deliveries.
    pub fn get_pending_deliveries(&self) -> Result<Vec<MobileDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_pending_deliveries()?;
        Ok(records.iter().map(MobileDeliveryRecord::from).collect())
    }

    /// Get delivery count by status.
    pub fn get_delivery_count_by_status(
        &self,
        status: MobileDeliveryStatus,
    ) -> Result<u32, MobileError> {
        use vauchi_core::storage::DeliveryStatus;
        let core_status = match status {
            MobileDeliveryStatus::Queued => DeliveryStatus::Queued,
            MobileDeliveryStatus::Sent => DeliveryStatus::Sent,
            MobileDeliveryStatus::Stored => DeliveryStatus::Stored,
            MobileDeliveryStatus::Delivered => DeliveryStatus::Delivered,
            MobileDeliveryStatus::Expired => DeliveryStatus::Expired,
            MobileDeliveryStatus::Failed => DeliveryStatus::Failed {
                reason: String::new(),
            },
        };
        let storage = self.open_storage()?;
        let count = storage.count_deliveries_by_status(&core_status)?;
        Ok(count as u32)
    }

    // === Retry Queue Operations ===

    /// Get all retry entries that are due for retry.
    pub fn get_due_retries(&self) -> Result<Vec<MobileRetryEntry>, MobileError> {
        let storage = self.open_storage()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let entries = storage.get_due_retries(now)?;
        Ok(entries.iter().map(MobileRetryEntry::from).collect())
    }

    /// Get all retry entries for a contact.
    pub fn get_retries_for_contact(
        &self,
        contact_id: String,
    ) -> Result<Vec<MobileRetryEntry>, MobileError> {
        let storage = self.open_storage()?;
        let entries = storage.get_retry_entries_for_recipient(&contact_id)?;
        Ok(entries.iter().map(MobileRetryEntry::from).collect())
    }

    /// Get the total count of retry entries.
    pub fn get_retry_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.count_retry_entries()?;
        Ok(count as u32)
    }

    /// Delete a retry entry (after successful delivery or max attempts).
    pub fn delete_retry(&self, message_id: String) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let deleted = storage.delete_retry_entry(&message_id)?;
        Ok(deleted)
    }

    /// Calculate the backoff time for a given retry attempt.
    ///
    /// Returns seconds until next retry: 2^attempt, max 3600 (1 hour).
    pub fn calculate_retry_backoff(&self, attempt: u32) -> u64 {
        use vauchi_core::storage::RetryQueue;
        let queue = RetryQueue::new();
        queue.backoff_seconds(attempt)
    }

    // === Offline Queue Operations ===

    /// Get total count of all pending updates across all contacts.
    pub fn get_total_pending_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.count_all_pending_updates()?;
        Ok(count as u32)
    }

    /// Check if the offline queue is full.
    ///
    /// Default max size is 1000 updates.
    pub fn is_offline_queue_full(&self) -> Result<bool, MobileError> {
        use vauchi_core::storage::OfflineQueue;
        let storage = self.open_storage()?;
        let queue = OfflineQueue::new();
        queue
            .is_full(&storage)
            .map_err(|e| MobileError::StorageError(e.to_string()))
    }

    /// Get remaining capacity in the offline queue.
    pub fn get_offline_queue_capacity(&self) -> Result<u32, MobileError> {
        use vauchi_core::storage::OfflineQueue;
        let storage = self.open_storage()?;
        let queue = OfflineQueue::new();
        let remaining = queue
            .remaining_capacity(&storage)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;
        Ok(remaining as u32)
    }

    /// Clear all pending updates for a contact.
    ///
    /// Returns the number of cleared updates.
    pub fn clear_pending_updates_for_contact(
        &self,
        contact_id: String,
    ) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.delete_pending_updates_for_contact(&contact_id)?;
        Ok(count as u32)
    }

    // === Multi-Device Delivery Operations ===

    /// Get delivery summary for a message (X of Y devices delivered).
    pub fn get_delivery_summary(
        &self,
        message_id: String,
    ) -> Result<MobileDeliverySummary, MobileError> {
        let storage = self.open_storage()?;
        let summary = storage.get_delivery_summary(&message_id)?;
        Ok(MobileDeliverySummary::from(&summary))
    }

    /// Get all device delivery records for a message.
    pub fn get_device_deliveries(
        &self,
        message_id: String,
    ) -> Result<Vec<MobileDeviceDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_device_deliveries_for_message(&message_id)?;
        Ok(records
            .iter()
            .map(MobileDeviceDeliveryRecord::from)
            .collect())
    }

    /// Get all pending device deliveries.
    pub fn get_pending_device_deliveries(
        &self,
    ) -> Result<Vec<MobileDeviceDeliveryRecord>, MobileError> {
        let storage = self.open_storage()?;
        let records = storage.get_pending_device_deliveries()?;
        Ok(records
            .iter()
            .map(MobileDeviceDeliveryRecord::from)
            .collect())
    }

    // === Backup Operations ===

    /// Export encrypted backup.
    pub fn export_backup(&self, password: String) -> Result<String, MobileError> {
        let identity = self.get_identity()?;

        let backup = identity
            .export_backup(&password)
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(backup.as_bytes());

        Ok(encoded)
    }

    /// Import backup.
    pub fn import_backup(&self, backup_data: String, password: String) -> Result<(), MobileError> {
        {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                return Err(MobileError::AlreadyInitialized);
            }
        }

        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&backup_data)
            .map_err(|_| MobileError::InvalidInput("Invalid base64".to_string()))?;

        let backup = IdentityBackup::new(bytes);
        let identity = Identity::import_backup(&backup, &password)
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let internal_backup = identity
            .export_backup("__internal_storage_key__")
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let internal_backup_data = internal_backup.as_bytes().to_vec();
        let display_name = identity.display_name().to_string();

        let storage = self.open_storage()?;
        storage.save_identity(&internal_backup_data, &display_name)?;

        let identity_data = IdentityData {
            backup_data: internal_backup_data,
            display_name: display_name.clone(),
        };
        *self.identity_data.lock().unwrap() = Some(identity_data);

        if storage.load_own_card()?.is_none() {
            let card = ContactCard::new(&display_name);
            storage.save_own_card(&card)?;
        }

        Ok(())
    }

    // === GDPR Operations ===

    /// Export all user data for GDPR compliance.
    pub fn export_gdpr_data(&self) -> Result<MobileGdprExport, MobileError> {
        let storage = self.open_storage()?;
        let export = vauchi_core::api::export_all_data(&storage)?;

        let json_data = serde_json::to_string_pretty(&export)
            .map_err(|e| MobileError::GdprError(e.to_string()))?;

        Ok(MobileGdprExport {
            json_data,
            exported_at: export.exported_at,
            version: export.version,
        })
    }

    /// Schedule account deletion with 7-day grace period.
    pub fn schedule_account_deletion(&self) -> Result<MobileDeletionInfo, MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::DeletionManager::new(&storage);
        manager
            .schedule_deletion()
            .map_err(|e| MobileError::DeletionNotAllowed(e.to_string()))?;

        let state = manager
            .deletion_state()
            .map_err(|e| MobileError::GdprError(e.to_string()))?;
        Ok(MobileDeletionInfo::from(&state))
    }

    /// Cancel a scheduled account deletion.
    pub fn cancel_account_deletion(&self) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::DeletionManager::new(&storage);
        manager
            .cancel_deletion()
            .map_err(|e| MobileError::GdprError(e.to_string()))?;
        Ok(())
    }

    /// Execute account deletion (only after grace period).
    ///
    /// Generates revocation messages for all contacts and shreds CEKs.
    /// Returns the number of revocation messages generated (caller should
    /// arrange relay delivery).
    pub fn execute_account_deletion(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let manager = vauchi_core::api::DeletionManager::new(&storage);
        let result = manager
            .execute_deletion(&identity)
            .map_err(|e| MobileError::DeletionNotAllowed(e.to_string()))?;
        Ok(result.revocations.len() as u32)
    }

    /// Get current deletion state.
    pub fn get_deletion_state(&self) -> Result<MobileDeletionInfo, MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::DeletionManager::new(&storage);
        let state = manager
            .deletion_state()
            .map_err(|e| MobileError::GdprError(e.to_string()))?;
        Ok(MobileDeletionInfo::from(&state))
    }

    // === Crypto-Shredding Operations ===

    /// Set the platform keychain for crypto-shredding operations.
    ///
    /// Must be called before any shred operation. The keychain provides
    /// access to the platform's native secure storage (iOS Keychain,
    /// Android KeyStore) for SMK management.
    pub fn set_platform_keychain(&self, keychain: Box<dyn MobilePlatformKeychain>) {
        let mut lock = self.platform_keychain.lock().unwrap();
        *lock = Some(Arc::from(keychain));
    }

    /// Schedule crypto-shredding with 7-day grace period (Soft Shred).
    ///
    /// Returns a token that must be passed to `hard_shred()` after the grace period.
    /// Also refreshes the pre-signed messages file for future panic shred.
    ///
    /// Requires `set_platform_keychain()` to be called first.
    pub fn soft_shred(&self) -> Result<MobileShredToken, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let bridge = self.get_keychain_bridge()?;
        let data_dir = self.data_dir();

        let manager = vauchi_core::api::ShredManager::new(&storage, &bridge, &identity, &data_dir);
        let token = manager
            .soft_shred()
            .map_err(|e| MobileError::ShredError(e.to_string()))?;
        Ok(MobileShredToken::from(&token))
    }

    /// Cancel a scheduled shred during the grace period.
    pub fn cancel_shred(&self, token: MobileShredToken) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let bridge = self.get_keychain_bridge()?;
        let data_dir = self.data_dir();

        let core_token = vauchi_core::api::ShredToken::from_created_at(token.created_at);
        let manager = vauchi_core::api::ShredManager::new(&storage, &bridge, &identity, &data_dir);
        manager
            .cancel_shred(core_token)
            .map_err(|e| MobileError::ShredError(e.to_string()))?;
        Ok(())
    }

    /// Execute irreversible crypto-shredding (Hard Shred).
    ///
    /// Requires the grace period to have elapsed. Destroys all key material,
    /// secure-deletes the database, and removes all local data.
    ///
    /// **WARNING**: This operation is irreversible. All account data will be
    /// permanently destroyed.
    pub fn hard_shred(&self, token: MobileShredToken) -> Result<MobileShredReport, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let bridge = self.get_keychain_bridge()?;
        let data_dir = self.data_dir();

        let core_token = vauchi_core::api::ShredToken::from_created_at(token.created_at);
        let manager = vauchi_core::api::ShredManager::new(&storage, &bridge, &identity, &data_dir);
        let report = manager
            // GDPR Gap 4 deferred: mobile has no relay client yet, so no purge sender.
            // When mobile gains a relay connection, pass a PurgeSender here.
            .hard_shred(core_token, None, None)
            .map_err(|e| MobileError::ShredError(e.to_string()))?;
        Ok(MobileShredReport::from(&report))
    }

    /// Execute immediate crypto-shredding without grace period (Panic Shred).
    ///
    /// Loads pre-signed messages before destroying keys, then sends them
    /// best-effort. Use only in emergencies.
    ///
    /// **WARNING**: This operation is irreversible and immediate. No grace period.
    pub fn panic_shred(&self) -> Result<MobileShredReport, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let bridge = self.get_keychain_bridge()?;
        let data_dir = self.data_dir();

        let manager = vauchi_core::api::ShredManager::new(&storage, &bridge, &identity, &data_dir);
        let report = manager
            // GDPR Gap 4 deferred: mobile has no relay client yet, so no purge sender.
            // When mobile gains a relay connection, pass a PurgeSender here.
            .panic_shred(None, None)
            .map_err(|e| MobileError::ShredError(e.to_string()))?;
        Ok(MobileShredReport::from(&report))
    }

    /// Verify that shredding was successful by checking for residual data.
    ///
    /// Returns verification results showing which items were confirmed destroyed.
    pub fn verify_shred(&self) -> Result<MobileShredVerification, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;
        let bridge = self.get_keychain_bridge()?;
        let data_dir = self.data_dir();

        let manager = vauchi_core::api::ShredManager::new(&storage, &bridge, &identity, &data_dir);
        let verification = manager.verify_shred();
        Ok(MobileShredVerification::from(&verification))
    }

    /// Get current shred status.
    ///
    /// Returns whether no shred is in progress, one is scheduled (with remaining
    /// time), or has been executed.
    pub fn shred_status(&self) -> Result<MobileShredStatus, MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::DeletionManager::new(&storage);
        let state = manager
            .deletion_state()
            .map_err(|e| MobileError::ShredError(e.to_string()))?;

        match state {
            vauchi_core::storage::DeletionState::None => Ok(MobileShredStatus::None),
            vauchi_core::storage::DeletionState::Scheduled { execute_at, .. } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let remaining = execute_at.saturating_sub(now);
                Ok(MobileShredStatus::Scheduled {
                    remaining_secs: remaining,
                })
            }
            vauchi_core::storage::DeletionState::Executed { .. } => Ok(MobileShredStatus::Executed),
        }
    }

    /// Grant consent for a specific type.
    pub fn grant_consent(&self, consent_type: MobileConsentType) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::ConsentManager::new(&storage);
        manager.grant(vauchi_core::api::ConsentType::from(consent_type))?;
        Ok(())
    }

    /// Revoke consent for a specific type.
    pub fn revoke_consent(&self, consent_type: MobileConsentType) -> Result<(), MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::ConsentManager::new(&storage);
        manager.revoke(vauchi_core::api::ConsentType::from(consent_type))?;
        Ok(())
    }

    /// Check whether consent is currently granted for a type.
    pub fn check_consent(&self, consent_type: MobileConsentType) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::ConsentManager::new(&storage);
        let granted = manager.check(&vauchi_core::api::ConsentType::from(consent_type))?;
        Ok(granted)
    }

    /// Get all consent records.
    pub fn get_consent_records(&self) -> Result<Vec<MobileConsentRecord>, MobileError> {
        let storage = self.open_storage()?;
        let manager = vauchi_core::api::ConsentManager::new(&storage);
        let records = manager
            .export_consent_log_with_version()
            .map_err(|e| MobileError::GdprError(e.to_string()))?;
        Ok(records.iter().map(MobileConsentRecord::from).collect())
    }

    /// List contacts with pagination.
    pub fn list_contacts_paginated(
        &self,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<MobileContact>, MobileError> {
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts_paginated(offset as usize, limit as usize)?;
        Ok(contacts.iter().map(MobileContact::from).collect())
    }

    // === Social Networks ===

    /// List available social networks.
    pub fn list_social_networks(&self) -> Vec<MobileSocialNetwork> {
        self.social_registry
            .all()
            .iter()
            .map(|sn| MobileSocialNetwork {
                id: sn.id().to_string(),
                display_name: sn.display_name().to_string(),
                url_template: sn.profile_url_template().to_string(),
            })
            .collect()
    }

    /// Search social networks.
    pub fn search_social_networks(&self, query: String) -> Vec<MobileSocialNetwork> {
        self.social_registry
            .search(&query)
            .iter()
            .map(|sn| MobileSocialNetwork {
                id: sn.id().to_string(),
                display_name: sn.display_name().to_string(),
                url_template: sn.profile_url_template().to_string(),
            })
            .collect()
    }

    /// Get profile URL for a social field.
    pub fn get_profile_url(&self, network_id: String, username: String) -> Option<String> {
        self.social_registry.profile_url(&network_id, &username)
    }

    // === Recovery ===

    /// Create a recovery claim for a lost identity.
    ///
    /// The old_pk_hex is the hex-encoded public key of the lost identity.
    /// This starts the recovery process by creating a claim that contacts
    /// can vouch for.
    pub fn create_recovery_claim(
        &self,
        old_pk_hex: String,
    ) -> Result<MobileRecoveryClaim, MobileError> {
        use base64::Engine;
        let identity = self.get_identity()?;

        // Parse old public key
        let old_pk_bytes = hex::decode(&old_pk_hex)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid hex: {}", e)))?;
        let old_pk: [u8; 32] = old_pk_bytes
            .try_into()
            .map_err(|_| MobileError::InvalidInput("Public key must be 32 bytes".to_string()))?;

        // Create claim
        let new_pk = *identity.signing_public_key();
        let claim = RecoveryClaim::new(&old_pk, &new_pk);

        // Create proof to store vouchers and save to file
        let proof = RecoveryProof::new(&old_pk, &new_pk, 3); // Default threshold of 3
        std::fs::write(self.recovery_proof_path(), proof.to_bytes())
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        // Encode claim for sharing
        let claim_data = base64::engine::general_purpose::STANDARD.encode(claim.to_bytes());

        Ok(MobileRecoveryClaim {
            old_public_key: old_pk_hex,
            new_public_key: hex::encode(new_pk),
            claim_data,
            is_expired: claim.is_expired(),
        })
    }

    /// Parse a recovery claim from base64.
    ///
    /// Used to inspect a claim before vouching for it.
    pub fn parse_recovery_claim(
        &self,
        claim_b64: String,
    ) -> Result<MobileRecoveryClaim, MobileError> {
        use base64::Engine;
        let claim_bytes = base64::engine::general_purpose::STANDARD
            .decode(&claim_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let claim = RecoveryClaim::from_bytes(&claim_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid claim: {}", e)))?;

        Ok(MobileRecoveryClaim {
            old_public_key: hex::encode(claim.old_pk()),
            new_public_key: hex::encode(claim.new_pk()),
            claim_data: claim_b64,
            is_expired: claim.is_expired(),
        })
    }

    /// Create a voucher for someone's recovery claim.
    ///
    /// This vouches that you trust the person claiming to own the old identity
    /// is the same person as the new identity.
    pub fn create_recovery_voucher(
        &self,
        claim_b64: String,
    ) -> Result<MobileRecoveryVoucher, MobileError> {
        use base64::Engine;
        let identity = self.get_identity()?;

        let claim_bytes = base64::engine::general_purpose::STANDARD
            .decode(&claim_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let claim = RecoveryClaim::from_bytes(&claim_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid claim: {}", e)))?;

        if claim.is_expired() {
            return Err(MobileError::InvalidInput("Claim has expired".to_string()));
        }

        let voucher = RecoveryVoucher::create_from_claim(&claim, identity.signing_keypair())
            .map_err(|e| MobileError::CryptoError(e.to_string()))?;

        let voucher_data = base64::engine::general_purpose::STANDARD.encode(voucher.to_bytes());

        Ok(MobileRecoveryVoucher {
            voucher_public_key: hex::encode(voucher.voucher_pk()),
            voucher_data,
        })
    }

    /// Add a voucher to the current recovery claim.
    ///
    /// Returns the updated progress.
    pub fn add_recovery_voucher(
        &self,
        voucher_b64: String,
    ) -> Result<MobileRecoveryProgress, MobileError> {
        use base64::Engine;
        let voucher_bytes = base64::engine::general_purpose::STANDARD
            .decode(&voucher_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let voucher = RecoveryVoucher::from_bytes(&voucher_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid voucher: {}", e)))?;

        if !voucher.verify() {
            return Err(MobileError::InvalidInput(
                "Invalid voucher signature".to_string(),
            ));
        }

        // Load current proof from file
        let proof_path = self.recovery_proof_path();
        let mut proof = if proof_path.exists() {
            let proof_bytes =
                std::fs::read(&proof_path).map_err(|e| MobileError::StorageError(e.to_string()))?;
            RecoveryProof::from_bytes(&proof_bytes)
                .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?
        } else {
            return Err(MobileError::InvalidInput(
                "No recovery in progress".to_string(),
            ));
        };

        // Add voucher (enforce trusted contacts only)
        let storage = self.open_storage()?;
        let contacts = storage.list_contacts()?;
        let trusted_keys: std::collections::HashSet<[u8; 32]> = contacts
            .iter()
            .filter(|c| c.is_recovery_trusted())
            .map(|c| *c.public_key())
            .collect();

        match proof.add_voucher_trusted(voucher, &trusted_keys) {
            Ok(()) => {}
            Err(vauchi_core::recovery::RecoveryError::UntrustedVoucher) => {
                return Err(MobileError::InvalidInput(
                    "Voucher is from an untrusted contact. Only contacts marked as recovery-trusted can provide valid vouchers.".to_string(),
                ));
            }
            Err(e) => {
                return Err(MobileError::InvalidInput(format!(
                    "Cannot add voucher: {}",
                    e
                )));
            }
        }

        // Save updated proof
        std::fs::write(&proof_path, proof.to_bytes())
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let is_complete = proof.voucher_count() >= proof.threshold() as usize;

        Ok(MobileRecoveryProgress {
            old_public_key: hex::encode(proof.old_pk()),
            new_public_key: hex::encode(proof.new_pk()),
            vouchers_collected: proof.voucher_count() as u32,
            vouchers_needed: proof.threshold(),
            is_complete,
        })
    }

    /// Get the current recovery progress.
    ///
    /// Returns None if no recovery is in progress.
    pub fn get_recovery_status(&self) -> Result<Option<MobileRecoveryProgress>, MobileError> {
        let proof_path = self.recovery_proof_path();

        if !proof_path.exists() {
            return Ok(None);
        }

        let proof_bytes =
            std::fs::read(&proof_path).map_err(|e| MobileError::StorageError(e.to_string()))?;

        let proof = RecoveryProof::from_bytes(&proof_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?;

        let is_complete = proof.voucher_count() >= proof.threshold() as usize;

        Ok(Some(MobileRecoveryProgress {
            old_public_key: hex::encode(proof.old_pk()),
            new_public_key: hex::encode(proof.new_pk()),
            vouchers_collected: proof.voucher_count() as u32,
            vouchers_needed: proof.threshold(),
            is_complete,
        }))
    }

    /// Get the completed recovery proof as base64.
    ///
    /// Returns None if recovery is not complete.
    pub fn get_recovery_proof(&self) -> Result<Option<String>, MobileError> {
        use base64::Engine;
        let proof_path = self.recovery_proof_path();

        if !proof_path.exists() {
            return Ok(None);
        }

        let proof_bytes =
            std::fs::read(&proof_path).map_err(|e| MobileError::StorageError(e.to_string()))?;

        let proof = RecoveryProof::from_bytes(&proof_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?;

        if proof.voucher_count() >= proof.threshold() as usize {
            let proof_data = base64::engine::general_purpose::STANDARD.encode(proof.to_bytes());
            Ok(Some(proof_data))
        } else {
            Ok(None)
        }
    }

    /// Verify a recovery proof from a contact.
    ///
    /// This checks if the proof is valid and provides a recommendation
    /// on whether to accept the recovered identity.
    pub fn verify_recovery_proof(
        &self,
        proof_b64: String,
    ) -> Result<MobileRecoveryVerification, MobileError> {
        use base64::Engine;
        let storage = self.open_storage()?;

        let proof_bytes = base64::engine::general_purpose::STANDARD
            .decode(&proof_b64)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid base64: {}", e)))?;

        let proof = RecoveryProof::from_bytes(&proof_bytes)
            .map_err(|e| MobileError::InvalidInput(format!("Invalid proof: {}", e)))?;

        // Validate the proof
        proof
            .validate()
            .map_err(|e| MobileError::InvalidInput(format!("Proof validation failed: {}", e)))?;

        // Count known vouchers (vouchers from our contacts)
        let contacts = storage
            .list_contacts()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let contact_pks: std::collections::HashSet<[u8; 32]> =
            contacts.iter().map(|c| *c.public_key()).collect();

        let known_voucher_count = proof
            .vouchers()
            .iter()
            .filter(|v| contact_pks.contains(v.voucher_pk()))
            .count();

        // Determine confidence
        let (confidence, recommendation) = if known_voucher_count >= 2 {
            (
                "high".to_string(),
                "Multiple contacts you know have vouched. Safe to accept.".to_string(),
            )
        } else if known_voucher_count == 1 {
            (
                "medium".to_string(),
                "One contact you know has vouched. Consider verifying in person.".to_string(),
            )
        } else {
            (
                "low".to_string(),
                "No known contacts have vouched. Verify identity carefully before accepting."
                    .to_string(),
            )
        };

        Ok(MobileRecoveryVerification {
            old_public_key: hex::encode(proof.old_pk()),
            new_public_key: hex::encode(proof.new_pk()),
            voucher_count: proof.voucher_count() as u32,
            known_vouchers: known_voucher_count as u32,
            confidence,
            recommendation,
        })
    }

    // === Content Updates ===

    /// Check if remote content updates are supported.
    ///
    /// Returns true if the content-updates feature is enabled at compile time.
    pub fn is_content_updates_supported(&self) -> bool {
        cfg!(feature = "content-updates")
    }

    /// Check for available content updates.
    ///
    /// This is a blocking call that checks the remote server for updates.
    /// Returns the update status indicating what updates are available.
    ///
    /// Note: Returns Disabled if the `content-updates` feature is not enabled.
    pub fn check_content_updates(&self) -> content::MobileUpdateStatus {
        #[cfg(feature = "content-updates")]
        {
            self.check_content_updates_impl()
        }

        #[cfg(not(feature = "content-updates"))]
        {
            content::MobileUpdateStatus::Disabled
        }
    }

    /// Apply available content updates.
    ///
    /// Downloads and caches any available updates. After applying,
    /// the new content will be used for subsequent operations.
    ///
    /// Note: Returns Disabled if the `content-updates` feature is not enabled.
    pub fn apply_content_updates(&self) -> content::MobileApplyResult {
        #[cfg(feature = "content-updates")]
        {
            self.apply_content_updates_impl()
        }

        #[cfg(not(feature = "content-updates"))]
        {
            content::MobileApplyResult::Disabled
        }
    }

    /// Reload social networks from content cache.
    ///
    /// Call this after applying content updates to refresh the
    /// list of social networks available in the app.
    pub fn reload_social_networks(&self) -> Vec<MobileSocialNetwork> {
        // Note: In a real implementation, we would update self.social_registry
        // For now, we return the current list which uses compile-time defaults
        // or cached content if ContentManager is used at construction
        self.list_social_networks()
    }

    // === Field Validation Operations ===

    /// Validate a contact's field.
    ///
    /// Creates a cryptographically signed validation record attesting
    /// that you believe this field value belongs to this contact.
    /// Returns the created validation.
    pub fn validate_field(
        &self,
        contact_id: String,
        field_id: String,
        field_value: String,
    ) -> Result<MobileFieldValidation, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        // Check we're not validating our own field
        let my_id = hex::encode(identity.signing_public_key());
        if contact_id == my_id {
            return Err(MobileError::InvalidInput(
                "Cannot validate your own field".to_string(),
            ));
        }

        // Check we haven't already validated this field
        let validator_id = hex::encode(identity.signing_public_key());
        if storage.has_validated(&contact_id, &field_id, &validator_id)? {
            return Err(MobileError::InvalidInput(
                "You have already validated this field".to_string(),
            ));
        }

        // Create signed validation
        let validation = vauchi_core::social::ProfileValidation::create_signed(
            &identity,
            &field_id,
            &field_value,
            &contact_id,
        );

        // Store it
        storage.save_validation(&validation)?;

        Ok(MobileFieldValidation::from(&validation))
    }

    /// Get validation status for a contact's field.
    ///
    /// Returns aggregated validation information including count, trust level,
    /// and whether you have validated this field.
    pub fn get_field_validation_status(
        &self,
        contact_id: String,
        field_id: String,
        field_value: String,
    ) -> Result<MobileValidationStatus, MobileError> {
        let storage = self.open_storage()?;
        let validations = storage.load_validations_for_field(&contact_id, &field_id)?;

        // Get current user's ID if available
        let my_id = {
            let data = self.identity_data.lock().unwrap();
            if data.is_some() {
                match self.get_identity() {
                    Ok(identity) => Some(hex::encode(identity.signing_public_key())),
                    Err(_) => None,
                }
            } else {
                None
            }
        };

        let blocked = std::collections::HashSet::new();
        let status = vauchi_core::social::ValidationStatus::from_validations(
            &validations,
            &field_value,
            my_id.as_deref(),
            &blocked,
        );

        Ok(MobileValidationStatus::from(&status))
    }

    /// Revoke your validation of a contact's field.
    ///
    /// Returns true if a validation was revoked, false if you hadn't validated.
    pub fn revoke_field_validation(
        &self,
        contact_id: String,
        field_id: String,
    ) -> Result<bool, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let validator_id = hex::encode(identity.signing_public_key());
        let deleted = storage.delete_validation(&contact_id, &field_id, &validator_id)?;

        Ok(deleted)
    }

    /// List all validations you have made.
    ///
    /// Returns a list of all fields you have validated, sorted by
    /// validation timestamp (most recent first).
    pub fn list_my_validations(&self) -> Result<Vec<MobileFieldValidation>, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let validator_id = hex::encode(identity.signing_public_key());
        let validations = storage.load_validations_by_validator(&validator_id)?;

        Ok(validations
            .iter()
            .map(MobileFieldValidation::from)
            .collect())
    }

    /// Check if you have validated a specific field.
    pub fn has_validated_field(
        &self,
        contact_id: String,
        field_id: String,
    ) -> Result<bool, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let validator_id = hex::encode(identity.signing_public_key());
        let validated = storage.has_validated(&contact_id, &field_id, &validator_id)?;

        Ok(validated)
    }

    /// Get the validation count for a field (quick check without full status).
    pub fn get_field_validation_count(
        &self,
        contact_id: String,
        field_id: String,
    ) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;
        let count = storage.count_validations_for_field(&contact_id, &field_id)?;
        Ok(count as u32)
    }

    // === Aha Moments (public API) ===

    /// Check if an aha moment has been seen.
    pub fn has_seen_aha_moment(&self, moment_type: MobileAhaMomentType) -> bool {
        let tracker = self.load_aha_tracker();
        tracker.has_seen(moment_type.into())
    }

    /// Try to trigger an aha moment. Returns the moment if not yet seen, None otherwise.
    pub fn try_trigger_aha_moment(
        &self,
        moment_type: MobileAhaMomentType,
    ) -> Result<Option<MobileAhaMoment>, MobileError> {
        let mut tracker = self.load_aha_tracker();
        let core_type: vauchi_core::AhaMomentType = moment_type.into();

        if let Some(moment) = tracker.try_trigger(core_type) {
            self.save_aha_tracker(&tracker)?;
            Ok(Some(MobileAhaMoment {
                moment_type,
                title: moment.title().to_string(),
                message: moment.message(),
                has_animation: moment.has_animation(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Try to trigger an aha moment with context (e.g., contact name).
    pub fn try_trigger_aha_moment_with_context(
        &self,
        moment_type: MobileAhaMomentType,
        context: String,
    ) -> Result<Option<MobileAhaMoment>, MobileError> {
        let mut tracker = self.load_aha_tracker();
        let core_type: vauchi_core::AhaMomentType = moment_type.into();

        if let Some(moment) = tracker.try_trigger_with_context(core_type, context) {
            self.save_aha_tracker(&tracker)?;
            Ok(Some(MobileAhaMoment {
                moment_type,
                title: moment.title().to_string(),
                message: moment.message(),
                has_animation: moment.has_animation(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the count of seen aha moments.
    pub fn aha_moments_seen_count(&self) -> u32 {
        let tracker = self.load_aha_tracker();
        tracker.seen_count() as u32
    }

    /// Get the total count of aha moments.
    pub fn aha_moments_total_count(&self) -> u32 {
        let tracker = self.load_aha_tracker();
        tracker.total_count() as u32
    }

    /// Reset all aha moments (for testing/debugging).
    pub fn reset_aha_moments(&self) -> Result<(), MobileError> {
        let mut tracker = self.load_aha_tracker();
        tracker.reset();
        self.save_aha_tracker(&tracker)
    }

    // === Demo Contact (public API) ===

    /// Initialize the demo contact if user has no real contacts.
    /// Call this after onboarding completes.
    pub fn init_demo_contact_if_needed(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        // Check if user has any real contacts
        let storage = self.open_storage()?;
        let contacts = storage
            .list_contacts()
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        if !contacts.is_empty() {
            // User has contacts, don't show demo
            return Ok(None);
        }

        // Check current state
        let mut state = self.load_demo_state();
        if state.was_dismissed || state.auto_removed {
            // User dismissed or it was auto-removed
            return Ok(None);
        }

        // Activate demo contact if not already
        if !state.is_active {
            state = vauchi_core::DemoContactState::new_active();
            self.save_demo_state(&state)?;
        }

        // Get current demo card
        if let Some(tip) = state.current_tip() {
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }

    /// Get the current demo contact if active.
    pub fn get_demo_contact(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        let state = self.load_demo_state();
        if !state.is_active {
            return Ok(None);
        }

        if let Some(tip) = state.current_tip() {
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }

    /// Get the demo contact state.
    pub fn get_demo_contact_state(&self) -> MobileDemoContactState {
        let state = self.load_demo_state();
        MobileDemoContactState {
            is_active: state.is_active,
            was_dismissed: state.was_dismissed,
            auto_removed: state.auto_removed,
            update_count: state.update_count,
        }
    }

    /// Check if a demo update is available.
    pub fn is_demo_update_available(&self) -> bool {
        let state = self.load_demo_state();
        state.is_update_due()
    }

    /// Trigger a demo update and get the new content.
    pub fn trigger_demo_update(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        let mut state = self.load_demo_state();
        if !state.is_active {
            return Ok(None);
        }

        if let Some(tip) = state.advance_to_next_tip() {
            self.save_demo_state(&state)?;
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }

    /// Dismiss the demo contact.
    pub fn dismiss_demo_contact(&self) -> Result<(), MobileError> {
        let mut state = self.load_demo_state();
        state.dismiss();
        self.save_demo_state(&state)
    }

    /// Auto-remove demo contact after first real exchange.
    /// Call this after a successful contact exchange.
    pub fn auto_remove_demo_contact(&self) -> Result<bool, MobileError> {
        let mut state = self.load_demo_state();
        if state.is_active {
            state.auto_remove();
            self.save_demo_state(&state)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Restore the demo contact from Settings.
    pub fn restore_demo_contact(&self) -> Result<Option<MobileDemoContact>, MobileError> {
        let mut state = self.load_demo_state();
        state.restore();
        self.save_demo_state(&state)?;

        if let Some(tip) = state.current_tip() {
            let card = vauchi_core::generate_demo_contact_card(&tip);
            Ok(Some(card.into()))
        } else {
            Ok(None)
        }
    }

    // === Device Linking Operations ===

    /// Get list of linked devices.
    ///
    /// Returns information about all devices linked to this identity.
    /// The first device (index 0) is the primary device.
    pub fn get_devices(&self) -> Result<Vec<MobileDeviceInfo>, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;

        // Load device registry from storage
        let registry = match storage.load_device_registry()? {
            Some(r) => r,
            None => {
                // Return just the current device if no registry exists
                let device_info = identity.device_info();
                return Ok(vec![MobileDeviceInfo {
                    device_index: device_info.device_index(),
                    device_name: device_info.device_name().to_string(),
                    is_current: true,
                    is_active: true,
                    public_key_prefix: hex::encode(&device_info.device_id()[..8]),
                    created_at: device_info.created_at(),
                }]);
            }
        };

        let current_device_id = identity.device_info().device_id();
        let devices = registry
            .all_devices()
            .iter()
            .enumerate()
            .map(
                |(idx, d): (usize, &vauchi_core::identity::RegisteredDevice)| MobileDeviceInfo {
                    device_index: idx as u32,
                    device_name: d.device_name.clone(),
                    is_current: d.device_id == *current_device_id,
                    is_active: d.is_active(),
                    public_key_prefix: hex::encode(&d.device_id[..8]),
                    created_at: d.created_at,
                },
            )
            .collect();

        Ok(devices)
    }

    /// Generate a device link QR code.
    ///
    /// Display this QR code on the existing device for a new device to scan.
    /// The QR expires after 10 minutes.
    pub fn generate_device_link_qr(&self) -> Result<MobileDeviceLinkData, MobileError> {
        let identity = self.get_identity()?;

        let qr = DeviceLinkQR::generate(&identity);
        let qr_data = qr.to_data_string();

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let expires_at = timestamp + 600; // 10 minutes

        Ok(MobileDeviceLinkData {
            qr_data,
            identity_public_key: hex::encode(identity.signing_public_key()),
            timestamp,
            expires_at,
        })
    }

    /// Parse a device link QR code.
    ///
    /// Call this on the new device after scanning the QR code displayed
    /// on an existing device. Returns information about the identity
    /// to link with.
    pub fn parse_device_link_qr(
        &self,
        qr_data: String,
    ) -> Result<MobileDeviceLinkInfo, MobileError> {
        let qr =
            DeviceLinkQR::from_data_string(&qr_data).map_err(|_| MobileError::InvalidQrCode)?;

        Ok(MobileDeviceLinkInfo {
            identity_public_key: hex::encode(qr.identity_public_key()),
            timestamp: qr.timestamp(),
            is_expired: qr.is_expired(),
        })
    }

    /// Start a device link as the existing device (initiator).
    ///
    /// Returns a `MobileDeviceLinkInitiator` that holds the QR data and can
    /// process incoming link requests from new devices.
    pub fn start_device_link(&self) -> Result<Arc<MobileDeviceLinkInitiator>, MobileError> {
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let registry = storage
            .load_device_registry()?
            .unwrap_or_else(|| identity.initial_device_registry());

        let initiator = identity.create_device_link_initiator(registry);

        Ok(Arc::new(MobileDeviceLinkInitiator {
            inner: Mutex::new(initiator),
            pending_request: Mutex::new(None),
        }))
    }

    /// Start a device join as the new device (responder).
    ///
    /// Parses the QR data scanned from the existing device and returns a
    /// `MobileDeviceLinkResponder` that can create requests and process responses.
    pub fn start_device_join(
        &self,
        qr_data: String,
        device_name: String,
    ) -> Result<Arc<MobileDeviceLinkResponder>, MobileError> {
        let qr =
            DeviceLinkQR::from_data_string(&qr_data).map_err(|_| MobileError::InvalidQrCode)?;

        let responder = DeviceLinkResponder::from_qr(qr, device_name)
            .map_err(|e| MobileError::ExchangeFailed(e.to_string()))?;

        Ok(Arc::new(MobileDeviceLinkResponder {
            inner: Mutex::new(responder),
        }))
    }

    /// Get the device count.
    ///
    /// Returns the number of devices linked to this identity.
    pub fn device_count(&self) -> Result<u32, MobileError> {
        let storage = self.open_storage()?;

        match storage.load_device_registry()? {
            Some(r) => Ok(r.device_count() as u32),
            None => Ok(1), // Just this device
        }
    }

    /// Unlink a device from this identity.
    ///
    /// This marks the device as revoked. It will no longer receive updates
    /// and its keys will be rotated out. Returns true if the device was
    /// found and unlinked.
    ///
    /// Note: Cannot unlink the current device (use account deletion instead).
    /// The device_index is the position in the devices list (0-based).
    pub fn unlink_device(&self, device_index: u32) -> Result<bool, MobileError> {
        let storage = self.open_storage()?;
        let identity = self.get_identity()?;

        // Load registry
        let mut registry = match storage.load_device_registry()? {
            Some(r) => r,
            None => return Ok(false), // No registry means no other devices
        };

        // Get device at index
        let devices = registry.all_devices();
        if device_index as usize >= devices.len() {
            return Ok(false);
        }

        let device_id = devices[device_index as usize].device_id;
        let current_device_id = identity.device_info().device_id();

        // Cannot unlink current device
        if device_id == *current_device_id {
            return Err(MobileError::InvalidInput(
                "Cannot unlink the current device".to_string(),
            ));
        }

        // Try to revoke the device
        match registry.revoke_device(&device_id, identity.signing_keypair()) {
            Ok(()) => {
                storage.save_device_registry(&registry)?;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    /// Check if this device is the primary device (index 0).
    pub fn is_primary_device(&self) -> Result<bool, MobileError> {
        let identity = self.get_identity()?;
        Ok(identity.device_info().device_index() == 0)
    }
}

// Async sync method — runs sync in a background thread to prevent UI freeze.
// Feature-gated behind `async-sync` (default) which pulls in tokio.
#[cfg(feature = "async-sync")]
#[uniffi::export(async_runtime = "tokio")]
impl VauchiMobile {
    /// Async version of sync using native async WebSocket.
    ///
    /// Use this from mobile UI threads to prevent freezing.
    /// Storage is opened in scoped blocks and dropped before `.await` to keep
    /// the future `Send` (required by UniFFI async exports).
    pub async fn sync_async(self: Arc<Self>) -> Result<MobileSyncResult, MobileError> {
        *self.sync_status.lock().unwrap() = MobileSyncStatus::Syncing;

        let identity = self.get_identity()?;
        let pinned_cert = self.get_pinned_cert();

        let result = sync::do_sync_async(
            &self.storage_path,
            self.storage_key.clone(),
            &identity,
            &self.relay_url,
            pinned_cert.as_deref(),
        )
        .await;

        match &result {
            Ok(_) => *self.sync_status.lock().unwrap() = MobileSyncStatus::Idle,
            Err(_) => *self.sync_status.lock().unwrap() = MobileSyncStatus::Error,
        }

        result
    }
}

// Internal implementation methods for content updates (feature-gated)
#[cfg(feature = "content-updates")]
impl VauchiMobile {
    fn check_content_updates_impl(&self) -> content::MobileUpdateStatus {
        use content::MobileUpdateStatus;

        let config = ContentConfig {
            storage_path: self
                .storage_path
                .parent()
                .unwrap_or(&self.storage_path)
                .to_path_buf(),
            remote_updates_enabled: true,
            ..Default::default()
        };

        let manager = match ContentManager::new(config) {
            Ok(m) => m,
            Err(e) => {
                return MobileUpdateStatus::CheckFailed {
                    error: e.to_string(),
                }
            }
        };

        let rt: tokio::runtime::Runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                return MobileUpdateStatus::CheckFailed {
                    error: e.to_string(),
                }
            }
        };

        rt.block_on(async { manager.check_for_updates().await.into() })
    }

    fn apply_content_updates_impl(&self) -> content::MobileApplyResult {
        use content::{MobileApplyFailure, MobileApplyResult, MobileContentType};

        let config = ContentConfig {
            storage_path: self
                .storage_path
                .parent()
                .unwrap_or(&self.storage_path)
                .to_path_buf(),
            remote_updates_enabled: true,
            ..Default::default()
        };

        let manager = match ContentManager::new(config) {
            Ok(m) => m,
            Err(e) => {
                return MobileApplyResult::Error {
                    error: e.to_string(),
                }
            }
        };

        let rt: tokio::runtime::Runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                return MobileApplyResult::Error {
                    error: e.to_string(),
                }
            }
        };

        rt.block_on(async {
            match manager.apply_updates().await {
                Ok(result) => match result {
                    vauchi_core::content::ApplyResult::NoUpdates => MobileApplyResult::NoUpdates,
                    vauchi_core::content::ApplyResult::Disabled => MobileApplyResult::Disabled,
                    vauchi_core::content::ApplyResult::Applied { applied, failed } => {
                        MobileApplyResult::Applied {
                            applied: applied.into_iter().map(MobileContentType::from).collect(),
                            failed: failed
                                .into_iter()
                                .map(|(ct, err)| MobileApplyFailure {
                                    content_type: MobileContentType::from(ct),
                                    error: err,
                                })
                                .collect(),
                        }
                    }
                },
                Err(e) => MobileApplyResult::Error {
                    error: e.to_string(),
                },
            }
        })
    }
}

// INLINE_TEST_REQUIRED: Tests require tempfile for VauchiMobile instance creation
// and access to internal Arc<VauchiMobile> which cannot be accessed from external tests.
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_instance() -> (Arc<VauchiMobile>, TempDir) {
        let dir = TempDir::new().unwrap();
        let wb = VauchiMobile::new(
            dir.path().to_string_lossy().to_string(),
            "ws://localhost:8080".to_string(),
        )
        .unwrap();
        (wb, dir)
    }

    #[test]
    fn test_create_identity() {
        let (wb, _dir) = create_test_instance();
        assert!(!wb.has_identity());

        wb.create_identity("Alice".to_string()).unwrap();
        assert!(wb.has_identity());

        let name = wb.get_display_name().unwrap();
        assert_eq!(name, "Alice");
    }

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

    #[test]
    fn test_social_networks() {
        let (wb, _dir) = create_test_instance();

        let networks = wb.list_social_networks();
        assert!(!networks.is_empty());

        let github = networks.iter().find(|n| n.id == "github");
        assert!(github.is_some());

        let url = wb.get_profile_url("github".to_string(), "octocat".to_string());
        assert_eq!(url, Some("https://github.com/octocat".to_string()));
    }

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
        let wb2 = VauchiMobile::new(
            dir2.path().to_string_lossy().to_string(),
            "ws://localhost:8080".to_string(),
        )
        .unwrap();

        wb2.import_backup(backup, "correct-horse-battery-staple".to_string())
            .unwrap();

        assert!(wb2.has_identity());
        let name = wb2.get_display_name().unwrap();
        assert_eq!(name, "Alice");
    }

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

    #[test]
    fn test_generate_device_link_qr() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let link_data = wb.generate_device_link_qr().unwrap();
        assert!(!link_data.qr_data.is_empty());
        assert!(!link_data.identity_public_key.is_empty());
        assert!(link_data.expires_at > link_data.timestamp);
    }

    #[test]
    fn test_parse_device_link_qr() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let link_data = wb.generate_device_link_qr().unwrap();
        let parsed = wb.parse_device_link_qr(link_data.qr_data).unwrap();

        assert_eq!(parsed.identity_public_key, link_data.identity_public_key);
        assert!(!parsed.is_expired);
    }

    #[test]
    fn test_parse_device_link_qr_invalid() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let result = wb.parse_device_link_qr("invalid_qr_data".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_device_count() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let count = wb.device_count().unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_is_primary_device() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        let is_primary = wb.is_primary_device().unwrap();
        assert!(is_primary);
    }

    #[test]
    fn test_unlink_device_no_registry() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // No registry means no devices to unlink
        let result = wb.unlink_device(1).unwrap();
        assert!(!result);
    }

    // === GDPR Tests ===

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

    #[test]
    fn test_schedule_cancel_deletion() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Initially no deletion
        let info = wb.get_deletion_state().unwrap();
        assert_eq!(info.state, MobileDeletionState::None);

        // Schedule deletion
        let info = wb.schedule_account_deletion().unwrap();
        assert_eq!(info.state, MobileDeletionState::Scheduled);
        assert!(info.scheduled_at > 0);
        assert!(info.execute_at > info.scheduled_at);

        // Cancel deletion
        wb.cancel_account_deletion().unwrap();
        let info = wb.get_deletion_state().unwrap();
        assert_eq!(info.state, MobileDeletionState::None);
    }

    #[test]
    fn test_consent_grant_revoke() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Initially not granted
        let granted = wb.check_consent(MobileConsentType::Analytics).unwrap();
        assert!(!granted);

        // Grant consent
        wb.grant_consent(MobileConsentType::Analytics).unwrap();
        let granted = wb.check_consent(MobileConsentType::Analytics).unwrap();
        assert!(granted);

        // Sleep to ensure different timestamp (consent check orders by timestamp)
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Revoke consent
        wb.revoke_consent(MobileConsentType::Analytics).unwrap();
        let granted = wb.check_consent(MobileConsentType::Analytics).unwrap();
        assert!(!granted);
    }

    #[test]
    fn test_consent_records_list() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        wb.grant_consent(MobileConsentType::DataProcessing).unwrap();
        wb.grant_consent(MobileConsentType::ContactSharing).unwrap();
        wb.grant_consent(MobileConsentType::Analytics).unwrap();

        let records = wb.get_consent_records().unwrap();
        assert!(records.len() >= 3);
    }

    #[test]
    fn test_list_contacts_paginated() {
        let (wb, _dir) = create_test_instance();
        wb.create_identity("Alice".to_string()).unwrap();

        // Paginate with 0 contacts should return empty
        let page = wb.list_contacts_paginated(0, 3).unwrap();
        assert!(page.is_empty());
    }

    // === Device Link Initiator/Responder Tests ===

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

        // Step 4: Existing device confirms (after proximity verification)
        initiator.set_proximity_verified();
        let result = initiator.confirm_link().unwrap();
        assert!(result.success);
        assert_eq!(result.device_name, "Bob's Phone");
        assert!(result.device_index > 0);

        // Step 5: New device processes response
        let response_bytes = result.encrypted_response.expect("should have response bytes");
        let join_result = responder.finish_join(response_bytes).unwrap();
        assert!(join_result.success);
    }

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

        // Should fail without set_proximity_verified
        let result = initiator.confirm_link();
        assert!(result.is_err());
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! FFI Boundary Tests
//!
//! Tests the FFI boundary between Rust and mobile platforms.
//! Focuses on type conversions, error handling, and standalone functions
//! that can be tested without a VauchiPlatform instance.
//!
//! Note: Tests requiring VauchiPlatform are in src/lib.rs as inline tests
//! because they need access to Arc<VauchiPlatform> internals.

use std::sync::Once;

use vauchi_platform::{
    MobileAhaMomentType, MobileHelpCategory, MobileLocale, MobilePasswordStrength,
    check_password_strength, generate_storage_key, get_aha_moment_localized,
    get_faq_by_id_localized, get_faqs_by_category_localized, get_faqs_localized, is_allowed_scheme,
    is_blocked_scheme, is_safe_url, search_faqs_localized,
};

static INIT: Once = Once::new();
fn ensure_init() {
    INIT.call_once(|| {
        // vauchi-platform/ -> ../../locales/ (sibling locales repo)
        let locales_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales");
        let _ = vauchi_app::i18n::init(&locales_dir);
    });
}

// ============================================================================
// Password Strength Tests
// Based on: features/identity_management.feature - Backup security
// ============================================================================

// @scenario: identity_management:Password strength validation
/// Test: Short passwords are rejected
#[test]
fn test_password_too_short() {
    let result = check_password_strength("short".to_string());
    assert!(matches!(result.strength, MobilePasswordStrength::TooWeak));
    assert!(!result.is_acceptable);
    assert!(result.feedback.contains("8 characters"));
}

// @scenario: identity_management:Password strength validation
/// Test: Common passwords are weak
#[test]
fn test_common_passwords_are_weak() {
    let common_passwords = ["password", "12345678", "qwertyui", "abcdefgh"];

    for password in common_passwords {
        let result = check_password_strength(password.to_string());
        assert!(
            !result.is_acceptable || matches!(result.strength, MobilePasswordStrength::Fair),
            "Password '{}' should be weak or fair, got {:?}",
            password,
            result.strength
        );
    }
}

// @scenario: identity_management:Password strength validation
/// Test: Strong passwords are accepted
#[test]
fn test_strong_passwords() {
    let strong_passwords = [
        "correct-horse-battery-staple",
        "My$ecureP@ssw0rd!2024",
        "xK9#mL2$vB7@nQ4&jR",
    ];

    for password in strong_passwords {
        let result = check_password_strength(password.to_string());
        assert!(
            result.is_acceptable,
            "Password should be acceptable: {:?}",
            result
        );
    }
}

// @scenario: identity_management:Password strength validation
/// Test: Empty password is too weak
#[test]
fn test_empty_password() {
    let result = check_password_strength(String::new());
    assert!(matches!(result.strength, MobilePasswordStrength::TooWeak));
    assert!(!result.is_acceptable);
}

// @scenario: identity_management:Password strength validation
/// Test: Exactly 8 character password
#[test]
fn test_minimum_length_password() {
    let result = check_password_strength("abcd1234".to_string());
    // 8 chars but weak content - should not be acceptable
    assert!(!result.is_acceptable || !result.feedback.is_empty());
}

// ============================================================================
// Storage Key Generation Tests
// Based on: features/identity_management.feature - Secure storage
// ============================================================================

// @scenario: security:Sufficient key lengths
/// Test: Storage key is 32 bytes
#[test]
fn test_storage_key_length() {
    let key = generate_storage_key();
    assert_eq!(key.len(), 32, "Storage key must be exactly 32 bytes");
}

// @scenario: security:Sufficient key lengths
/// Test: Storage keys are unique
#[test]
fn test_storage_keys_are_unique() {
    let key1 = generate_storage_key();
    let key2 = generate_storage_key();
    let key3 = generate_storage_key();

    assert_ne!(key1, key2, "Keys should be unique");
    assert_ne!(key2, key3, "Keys should be unique");
    assert_ne!(key1, key3, "Keys should be unique");
}

// @scenario: security:Sufficient key lengths
/// Test: Storage key is not all zeros
#[test]
fn test_storage_key_not_zeros() {
    let key = generate_storage_key();
    let all_zeros = vec![0u8; 32];
    assert_ne!(key, all_zeros, "Key should not be all zeros");
}

// ============================================================================
// URL Safety Tests
// Based on: Security requirements for URL handling
// ============================================================================

// @scenario: security:No weak cryptography
/// Test: HTTPS URLs are safe
#[test]
fn test_https_urls_safe() {
    assert!(is_safe_url("https://example.com".to_string()));
    assert!(is_safe_url("https://example.com/path".to_string()));
    assert!(is_safe_url(
        "https://sub.example.com/path?query=1".to_string()
    ));
}

// @scenario: security:No weak cryptography
/// Test: HTTP URLs are safe (will be upgraded)
#[test]
fn test_http_urls_safe() {
    assert!(is_safe_url("http://example.com".to_string()));
}

// @scenario: security:No weak cryptography
/// Test: Tel URLs are safe
#[test]
fn test_tel_urls_safe() {
    assert!(is_safe_url("tel:+1234567890".to_string()));
    assert!(is_safe_url("tel:123-456-7890".to_string()));
}

// @scenario: security:No weak cryptography
/// Test: Mailto URLs are safe
#[test]
fn test_mailto_urls_safe() {
    assert!(is_safe_url("mailto:user@example.com".to_string()));
    assert!(is_safe_url(
        "mailto:user@example.com?subject=Hello".to_string()
    ));
}

// @scenario: security:No weak cryptography
/// Test: SMS URLs are safe
#[test]
fn test_sms_urls_safe() {
    assert!(is_safe_url("sms:+1234567890".to_string()));
}

// @scenario: security:No weak cryptography
/// Test: Geo URLs are safe
#[test]
fn test_geo_urls_safe() {
    assert!(is_safe_url("geo:37.7749,-122.4194".to_string()));
}

// @scenario: security:No weak cryptography
/// Test: JavaScript URLs are blocked
#[test]
fn test_javascript_urls_blocked() {
    assert!(!is_safe_url("javascript:alert(1)".to_string()));
    assert!(!is_safe_url("JAVASCRIPT:void(0)".to_string()));
}

// @scenario: security:No weak cryptography
/// Test: Data URLs are blocked
#[test]
fn test_data_urls_blocked() {
    assert!(!is_safe_url(
        "data:text/html,<script>alert(1)</script>".to_string()
    ));
}

// @scenario: security:No weak cryptography
/// Test: File URLs are blocked
#[test]
fn test_file_urls_blocked() {
    assert!(!is_safe_url("file:///etc/passwd".to_string()));
}

// ============================================================================
// Scheme Validation Tests
// ============================================================================

// @scenario: security:No weak cryptography
/// Test: Allowed schemes
#[test]
fn test_allowed_schemes() {
    assert!(is_allowed_scheme("https".to_string()));
    assert!(is_allowed_scheme("http".to_string()));
    assert!(is_allowed_scheme("tel".to_string()));
    assert!(is_allowed_scheme("mailto".to_string()));
    assert!(is_allowed_scheme("sms".to_string()));
    assert!(is_allowed_scheme("geo".to_string()));
}

// @scenario: security:No weak cryptography
/// Test: Blocked schemes
#[test]
fn test_blocked_schemes() {
    assert!(is_blocked_scheme("javascript".to_string()));
    assert!(is_blocked_scheme("vbscript".to_string()));
    assert!(is_blocked_scheme("data".to_string()));
    assert!(is_blocked_scheme("file".to_string()));
    assert!(is_blocked_scheme("ftp".to_string()));
    assert!(is_blocked_scheme("blob".to_string()));
}

// @scenario: security:No weak cryptography
/// Test: Unknown schemes are not explicitly allowed or blocked
#[test]
fn test_unknown_schemes() {
    // Unknown schemes should not be in the allowed list
    assert!(!is_allowed_scheme("custom".to_string()));
    assert!(!is_allowed_scheme("myapp".to_string()));

    // But they're also not explicitly blocked
    assert!(!is_blocked_scheme("custom".to_string()));
    assert!(!is_blocked_scheme("myapp".to_string()));
}

// ============================================================================
// Edge Cases
// ============================================================================

// @scenario: security:No weak cryptography
/// Test: Empty URL
#[test]
fn test_empty_url() {
    // Empty URL should not crash
    let result = is_safe_url(String::new());
    // Expected to be false (invalid URL)
    assert!(!result);
}

// @scenario: security:No weak cryptography
/// Test: Malformed URLs
#[test]
fn test_malformed_urls() {
    // These should not crash and should return false
    assert!(!is_safe_url("not-a-url".to_string()));
    assert!(!is_safe_url("://missing-scheme".to_string()));
}

// @scenario: security:No weak cryptography
/// Test: Unicode in URLs
#[test]
fn test_unicode_urls() {
    // allow(zero_assertions): No-panic boundary test — validates exotic input doesn't crash
    let result = is_safe_url("https://例え.jp".to_string());
    let _ = result;
}

// @scenario: security:No weak cryptography
/// Test: Very long URL
#[test]
fn test_long_url() {
    // allow(zero_assertions): No-panic boundary test — validates long input doesn't crash
    let long_path = "a".repeat(10000);
    let url = format!("https://example.com/{}", long_path);
    let _ = is_safe_url(url);
}

// ============================================================================
// Localized FAQ Tests
// Based on: features/help_system.feature - Localized help content
// ============================================================================

// @scenario: help_faq:FAQ localization for supported languages
/// Test: Get all FAQs in German returns same count as English
#[test]
fn test_get_faqs_localized_german_count() {
    ensure_init();
    let english = get_faqs_localized(MobileLocale::English);
    let german = get_faqs_localized(MobileLocale::German);
    assert_eq!(english.len(), german.len());
    assert!(!german.is_empty());
}

// @scenario: help_faq:FAQ localization for supported languages
/// Test: German FAQs contain German text
#[test]
fn test_get_faqs_localized_german_content() {
    ensure_init();
    let german = get_faqs_localized(MobileLocale::German);
    let phone_lost = german.iter().find(|f| f.id == "faq-phone-lost").unwrap();
    assert!(
        phone_lost.question.contains("Telefon"),
        "German FAQ should contain 'Telefon', got: {}",
        phone_lost.question
    );
}

// @scenario: help_faq:FAQ localization for supported languages
/// Test: French FAQs contain French text
#[test]
fn test_get_faqs_localized_french_content() {
    ensure_init();
    let french = get_faqs_localized(MobileLocale::French);
    let phone_lost = french.iter().find(|f| f.id == "faq-phone-lost").unwrap();
    assert!(
        phone_lost.question.contains("telephone"),
        "French FAQ should contain 'telephone', got: {}",
        phone_lost.question
    );
}

// @scenario: help_faq:FAQ localization for supported languages
/// Test: Spanish FAQs contain Spanish text
#[test]
fn test_get_faqs_localized_spanish_content() {
    ensure_init();
    let spanish = get_faqs_localized(MobileLocale::Spanish);
    let phone_lost = spanish.iter().find(|f| f.id == "faq-phone-lost").unwrap();
    assert!(
        phone_lost.question.contains("telefono"),
        "Spanish FAQ should contain 'telefono', got: {}",
        phone_lost.question
    );
}

// @scenario: help_faq:Browse FAQs in a category
/// Test: Get FAQs by category in German
#[test]
fn test_get_faqs_by_category_localized() {
    ensure_init();
    let german_privacy =
        get_faqs_by_category_localized(MobileHelpCategory::Privacy, MobileLocale::German);
    assert!(!german_privacy.is_empty());
    for faq in &german_privacy {
        assert_eq!(faq.category, MobileHelpCategory::Privacy);
    }
}

// @scenario: help_faq:View a specific FAQ
/// Test: Get specific FAQ by ID in German
#[test]
fn test_get_faq_by_id_localized() {
    ensure_init();
    let faq = get_faq_by_id_localized("faq-phone-lost".to_string(), MobileLocale::German);
    assert!(faq.is_some(), "expected Some value");
    assert!(faq.unwrap().question.contains("Telefon"));
}

// @scenario: help_faq:View a specific FAQ
/// Test: Get FAQ by ID that doesn't exist
#[test]
fn test_get_faq_by_id_localized_not_found() {
    ensure_init();
    let faq = get_faq_by_id_localized("nonexistent".to_string(), MobileLocale::German);
    assert!(faq.is_none());
}

// @scenario: help_faq:Search FAQs by keyword
/// Test: Search FAQs in German
#[test]
fn test_search_faqs_localized_german() {
    ensure_init();
    let results = search_faqs_localized("Verschluesselung".to_string(), MobileLocale::German);
    assert!(!results.is_empty(), "Should find German encryption FAQ");
}

// @scenario: help_faq:Search FAQs by keyword
/// Test: Search FAQs in English
#[test]
fn test_search_faqs_localized_english() {
    ensure_init();
    let results = search_faqs_localized("encrypt".to_string(), MobileLocale::English);
    assert!(!results.is_empty(), "Should find English encryption FAQ");
}

// @scenario: help_faq:Search with no results
/// Test: Search with no results
#[test]
fn test_search_faqs_localized_no_results() {
    ensure_init();
    let results = search_faqs_localized("xyznonexistent123".to_string(), MobileLocale::German);
    assert!(results.is_empty());
}

// ============================================================================
// Localized Aha Moment Tests
// Based on: features/aha_moments.feature - Localized milestone celebrations
// ============================================================================

// @scenario: aha_moments:Card creation shows completion message
/// Test: Get aha moment localized in German
#[test]
fn test_aha_moment_localized_german() {
    ensure_init();
    let moment = get_aha_moment_localized(
        MobileAhaMomentType::CardCreationComplete,
        MobileLocale::German,
    );
    assert!(
        moment.title.contains("Karte"),
        "German title should contain 'Karte', got: {}",
        moment.title
    );
    assert!(!moment.message.is_empty());
    assert!(moment.has_animation);
}

// @scenario: aha_moments:Card creation shows completion message
/// Test: Get aha moment localized in English
#[test]
fn test_aha_moment_localized_english() {
    ensure_init();
    let moment = get_aha_moment_localized(
        MobileAhaMomentType::CardCreationComplete,
        MobileLocale::English,
    );
    assert!(!moment.title.is_empty());
    assert!(!moment.message.is_empty());
}

// @scenario: aha_moments:Aha moments are tracked per milestone
/// Test: All moment types return localized content
#[test]
fn test_all_aha_moments_localized() {
    ensure_init();
    let types = [
        MobileAhaMomentType::CardCreationComplete,
        MobileAhaMomentType::FirstEdit,
        MobileAhaMomentType::FirstContactAdded,
        MobileAhaMomentType::FirstUpdateReceived,
        MobileAhaMomentType::FirstOutboundDelivered,
    ];
    for moment_type in types {
        let moment = get_aha_moment_localized(moment_type, MobileLocale::German);
        assert!(
            !moment.title.is_empty(),
            "Localized title should not be empty for {:?}",
            moment_type
        );
        assert!(
            !moment.message.is_empty(),
            "Localized message should not be empty for {:?}",
            moment_type
        );
    }
}

// @scenario: aha_moments:First edit shows would-update feedback
/// Test: French aha moment content
#[test]
fn test_aha_moment_localized_french() {
    ensure_init();
    let moment = get_aha_moment_localized(MobileAhaMomentType::FirstEdit, MobileLocale::French);
    assert!(!moment.title.is_empty());
    assert!(!moment.message.contains("Missing"));
}

// ============================================================================
// DL-5: UniFFI Contract Test — Proof Construction
// Verifies: mobile binding computes same HMAC as core for identical inputs
// Based on: device_management.feature - Linking requires proximity verification
// ============================================================================

// @scenario: device_management.feature:Linking requires proximity verification
/// DL-5: Verify compute_confirmation_mac produces deterministic, non-trivial output
/// and is accepted by core's validate_proximity_proof.
///
/// This is the contract test proving the mobile binding path (which calls
/// compute_confirmation_mac internally) produces MACs that core validates correctly.
#[test]
fn test_confirmation_mac_contract_deterministic_and_accepted() {
    use std::time::{SystemTime, UNIX_EPOCH};
    use vauchi_core::exchange::{
        DeviceLinkInitiator, DeviceLinkQR, DeviceLinkResponder, ProximityProof,
        compute_confirmation_mac,
    };
    use vauchi_core::identity::{DeviceRegistry, Identity};

    fn now_unix_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    let master_seed = [0x42u8; 32];
    let identity = Identity::create("Alice");
    let device_info = identity.device_info();
    let registry = DeviceRegistry::new(
        device_info.to_registered(&master_seed),
        identity.signing_keypair(),
    );

    let initiator = DeviceLinkInitiator::new(master_seed, &identity, registry);

    let qr_string = initiator.qr().to_data_string();
    let scanned_qr = DeviceLinkQR::from_data_string(&qr_string).unwrap();
    let mut responder = DeviceLinkResponder::from_qr(scanned_qr, "Phone".to_string()).unwrap();

    let encrypted_request = responder.create_request().unwrap();
    let (confirmation, request) = initiator.prepare_confirmation(&encrypted_request).unwrap();

    let link_key = initiator.qr().link_key();
    let code = &confirmation.confirmation_code;

    // Contract property 1: deterministic — same inputs always produce same output
    let mac1 = compute_confirmation_mac(link_key, code);
    let mac2 = compute_confirmation_mac(link_key, code);
    assert_eq!(mac1, mac2, "MAC must be deterministic for same inputs");

    // Contract property 2: non-trivial — MAC is not all zeros
    assert_ne!(mac1, [0u8; 32], "MAC must not be all zeros");

    // Contract property 3: accepted by core — the MAC the mobile binding computes
    // internally (via compute_confirmation_mac) must be accepted by confirm_link
    let proof = ProximityProof::ManualConfirmation {
        confirmation_code_mac: mac1,
        confirmed_at: now_unix_secs(),
    };

    let result = initiator.confirm_link(&request, &proof);
    assert!(
        result.is_ok(),
        "Core must accept MAC computed by compute_confirmation_mac (the same function mobile uses). Error: {:?}",
        result.err()
    );
}

// ── Design Tokens FFI Boundary ───────────────────────────────

// @scenario: theming :: MobileTheme includes design tokens
#[test]
fn test_mobile_theme_includes_design_tokens() {
    let theme = vauchi_app::theme::default_theme();
    let mobile: vauchi_platform::MobileTheme = (&theme).into();

    // Verify tokens are carried through the FFI boundary
    assert_eq!(mobile.tokens.spacing.md, 16);
    assert_eq!(mobile.tokens.spacing.lg, 24);
    assert_eq!(mobile.tokens.border_radius.md_lg, 12);
    assert_eq!(mobile.tokens.typography.body_size, 16);
    assert_eq!(mobile.tokens.touch_target.minimum, 44);
    assert_eq!(mobile.tokens.motion.enter_duration_ms, 200);
}

// @scenario: theming :: MobileDesignTokens matches DesignTokens::default
#[test]
fn test_mobile_design_tokens_matches_core_defaults() {
    let core_tokens = vauchi_app::theme::DesignTokens::default();
    let mobile: vauchi_platform::MobileDesignTokens = (&core_tokens).into();

    assert_eq!(mobile.spacing.xs, core_tokens.spacing.xs);
    assert_eq!(mobile.spacing.xl, core_tokens.spacing.xl);
    assert_eq!(
        mobile.spacing_direction.content_start,
        core_tokens.spacing_direction.content_start
    );
    assert_eq!(mobile.border_radius.md_lg, core_tokens.border_radius.md_lg);
    assert_eq!(
        mobile.motion.emphasis_duration_ms,
        core_tokens.motion.emphasis_duration_ms
    );
}

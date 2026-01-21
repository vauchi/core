//! FFI Boundary Tests
//!
//! Tests the FFI boundary between Rust and mobile platforms.
//! Focuses on type conversions, error handling, and standalone functions
//! that can be tested without a VauchiMobile instance.
//!
//! Note: Tests requiring VauchiMobile are in src/lib.rs as inline tests
//! because they need access to Arc<VauchiMobile> internals.

use vauchi_mobile::{
    check_password_strength, generate_storage_key, is_allowed_scheme, is_blocked_scheme,
    is_safe_url, MobilePasswordStrength,
};

// ============================================================================
// Password Strength Tests
// Based on: features/identity_management.feature - Backup security
// ============================================================================

/// Test: Short passwords are rejected
#[test]
fn test_password_too_short() {
    let result = check_password_strength("short".to_string());
    assert!(matches!(result.strength, MobilePasswordStrength::TooWeak));
    assert!(!result.is_acceptable);
    assert!(result.feedback.contains("8 characters"));
}

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

/// Test: Empty password is too weak
#[test]
fn test_empty_password() {
    let result = check_password_strength(String::new());
    assert!(matches!(result.strength, MobilePasswordStrength::TooWeak));
    assert!(!result.is_acceptable);
}

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

/// Test: Storage key is 32 bytes
#[test]
fn test_storage_key_length() {
    let key = generate_storage_key();
    assert_eq!(key.len(), 32, "Storage key must be exactly 32 bytes");
}

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

/// Test: HTTPS URLs are safe
#[test]
fn test_https_urls_safe() {
    assert!(is_safe_url("https://example.com".to_string()));
    assert!(is_safe_url("https://example.com/path".to_string()));
    assert!(is_safe_url(
        "https://sub.example.com/path?query=1".to_string()
    ));
}

/// Test: HTTP URLs are safe (will be upgraded)
#[test]
fn test_http_urls_safe() {
    assert!(is_safe_url("http://example.com".to_string()));
}

/// Test: Tel URLs are safe
#[test]
fn test_tel_urls_safe() {
    assert!(is_safe_url("tel:+1234567890".to_string()));
    assert!(is_safe_url("tel:123-456-7890".to_string()));
}

/// Test: Mailto URLs are safe
#[test]
fn test_mailto_urls_safe() {
    assert!(is_safe_url("mailto:user@example.com".to_string()));
    assert!(is_safe_url(
        "mailto:user@example.com?subject=Hello".to_string()
    ));
}

/// Test: SMS URLs are safe
#[test]
fn test_sms_urls_safe() {
    assert!(is_safe_url("sms:+1234567890".to_string()));
}

/// Test: Geo URLs are safe
#[test]
fn test_geo_urls_safe() {
    assert!(is_safe_url("geo:37.7749,-122.4194".to_string()));
}

/// Test: JavaScript URLs are blocked
#[test]
fn test_javascript_urls_blocked() {
    assert!(!is_safe_url("javascript:alert(1)".to_string()));
    assert!(!is_safe_url("JAVASCRIPT:void(0)".to_string()));
}

/// Test: Data URLs are blocked
#[test]
fn test_data_urls_blocked() {
    assert!(!is_safe_url(
        "data:text/html,<script>alert(1)</script>".to_string()
    ));
}

/// Test: File URLs are blocked
#[test]
fn test_file_urls_blocked() {
    assert!(!is_safe_url("file:///etc/passwd".to_string()));
}

// ============================================================================
// Scheme Validation Tests
// ============================================================================

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

/// Test: Empty URL
#[test]
fn test_empty_url() {
    // Empty URL should not crash
    let result = is_safe_url(String::new());
    // Expected to be false (invalid URL)
    assert!(!result);
}

/// Test: Malformed URLs
#[test]
fn test_malformed_urls() {
    // These should not crash and should return false
    assert!(!is_safe_url("not-a-url".to_string()));
    assert!(!is_safe_url("://missing-scheme".to_string()));
}

/// Test: Unicode in URLs
#[test]
fn test_unicode_urls() {
    // International domain names should work
    let result = is_safe_url("https://例え.jp".to_string());
    // May or may not be safe depending on URL parsing
    // Main thing is it shouldn't crash
    let _ = result;
}

/// Test: Very long URL
#[test]
fn test_long_url() {
    let long_path = "a".repeat(10000);
    let url = format!("https://example.com/{}", long_path);
    // Should not crash
    let _ = is_safe_url(url);
}

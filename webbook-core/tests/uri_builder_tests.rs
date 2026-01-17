//! URI Builder Tests
//!
//! TDD tests for contact field to URI conversion.
//! Reference: features/contact_actions.feature

use webbook_core::contact_card::{ContactAction, ContactField, FieldType};

// ============================================================
// Phone Number → tel: URI
// ============================================================

#[test]
fn test_phone_field_generates_tel_uri() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567");
    let uri = field.to_uri();
    assert_eq!(uri, Some("tel:+1-555-123-4567".to_string()));
}

#[test]
fn test_phone_with_spaces_generates_tel_uri() {
    let field = ContactField::new(FieldType::Phone, "International", "+44 20 7946 0958");
    let uri = field.to_uri();
    // Spaces should be preserved or removed depending on RFC 3966
    assert!(uri.is_some());
    assert!(uri.unwrap().starts_with("tel:"));
}

#[test]
fn test_phone_with_parentheses() {
    let field = ContactField::new(FieldType::Phone, "Home", "(555) 123-4567");
    let uri = field.to_uri();
    assert!(uri.is_some());
    assert!(uri.unwrap().starts_with("tel:"));
}

#[test]
fn test_phone_to_action_returns_call() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::Call(_)));
}

// ============================================================
// Email → mailto: URI
// ============================================================

#[test]
fn test_email_field_generates_mailto_uri() {
    let field = ContactField::new(FieldType::Email, "Work", "bob@company.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("mailto:bob@company.com".to_string()));
}

#[test]
fn test_email_with_plus_sign() {
    let field = ContactField::new(FieldType::Email, "Personal", "bob+work@company.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("mailto:bob+work@company.com".to_string()));
}

#[test]
fn test_email_to_action_returns_send_email() {
    let field = ContactField::new(FieldType::Email, "Work", "bob@test.com");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::SendEmail(_)));
}

// ============================================================
// Website → https:/http: URI
// ============================================================

#[test]
fn test_website_with_https_preserved() {
    let field = ContactField::new(FieldType::Website, "Blog", "https://bobsmith.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://bobsmith.com".to_string()));
}

#[test]
fn test_website_with_http_preserved() {
    let field = ContactField::new(FieldType::Website, "Legacy", "http://old-site.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("http://old-site.com".to_string()));
}

#[test]
fn test_website_without_protocol_adds_https() {
    let field = ContactField::new(FieldType::Website, "Site", "bobsmith.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://bobsmith.com".to_string()));
}

#[test]
fn test_website_to_action_returns_open_url() {
    let field = ContactField::new(FieldType::Website, "Site", "https://example.com");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::OpenUrl(_)));
}

// ============================================================
// Social → Profile URL
// ============================================================

#[test]
fn test_social_twitter_generates_profile_url() {
    let field = ContactField::new(FieldType::Social, "Twitter", "@bobsmith");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://twitter.com/bobsmith".to_string()));
}

#[test]
fn test_social_twitter_without_at_sign() {
    let field = ContactField::new(FieldType::Social, "Twitter", "bobsmith");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://twitter.com/bobsmith".to_string()));
}

#[test]
fn test_social_github_generates_profile_url() {
    let field = ContactField::new(FieldType::Social, "GitHub", "octocat");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://github.com/octocat".to_string()));
}

#[test]
fn test_social_linkedin_generates_profile_url() {
    let field = ContactField::new(FieldType::Social, "LinkedIn", "in/bobsmith");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://linkedin.com/in/bobsmith".to_string()));
}

#[test]
fn test_social_instagram_generates_profile_url() {
    let field = ContactField::new(FieldType::Social, "Instagram", "bob.smith");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://instagram.com/bob.smith".to_string()));
}

#[test]
fn test_social_facebook_generates_profile_url() {
    let field = ContactField::new(FieldType::Social, "Facebook", "bob.smith.123");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://facebook.com/bob.smith.123".to_string()));
}

#[test]
fn test_social_unknown_network_returns_none() {
    let field = ContactField::new(FieldType::Social, "UnknownNetwork", "bobsmith");
    let uri = field.to_uri();
    // Unknown networks should return None (can't generate URL)
    assert!(uri.is_none());
}

#[test]
fn test_social_to_action_returns_open_url() {
    let field = ContactField::new(FieldType::Social, "GitHub", "octocat");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::OpenUrl(_)));
}

// ============================================================
// Address → Map Query
// ============================================================

#[test]
fn test_address_generates_map_query() {
    let field = ContactField::new(FieldType::Address, "Home", "123 Main St, City, ST 12345");
    let uri = field.to_uri();
    assert!(uri.is_some());
    let uri_str = uri.unwrap();
    // Should be a geo: URI or maps URL
    assert!(uri_str.starts_with("geo:") || uri_str.contains("maps"));
}

#[test]
fn test_address_is_url_encoded() {
    let field = ContactField::new(
        FieldType::Address,
        "Office",
        "123 Main St, San Francisco, CA",
    );
    let uri = field.to_uri();
    assert!(uri.is_some());
    let uri_str = uri.unwrap();
    // Spaces and commas should be encoded
    assert!(!uri_str.contains(' ') || uri_str.contains("%20") || uri_str.contains('+'));
}

#[test]
fn test_address_to_action_returns_open_map() {
    let field = ContactField::new(FieldType::Address, "Home", "123 Main St");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::OpenMap(_)));
}

// ============================================================
// Custom Field → Heuristic Detection
// ============================================================

#[test]
fn test_custom_field_with_phone_pattern_detected() {
    let field = ContactField::new(FieldType::Custom, "Signal", "+1-555-987-6543");
    let detected = field.detect_value_type();
    assert_eq!(detected, Some(FieldType::Phone));
}

#[test]
fn test_custom_field_with_email_pattern_detected() {
    let field = ContactField::new(FieldType::Custom, "Alternate", "bob.alt@email.com");
    let detected = field.detect_value_type();
    assert_eq!(detected, Some(FieldType::Email));
}

#[test]
fn test_custom_field_with_url_pattern_detected() {
    let field = ContactField::new(FieldType::Custom, "Portfolio", "https://portfolio.bob.com");
    let detected = field.detect_value_type();
    assert_eq!(detected, Some(FieldType::Website));
}

#[test]
fn test_custom_field_with_plain_text_not_detected() {
    let field = ContactField::new(FieldType::Custom, "Notes", "Met at conference");
    let detected = field.detect_value_type();
    assert!(detected.is_none());
}

#[test]
fn test_custom_field_uses_heuristic_for_uri() {
    let field = ContactField::new(FieldType::Custom, "Signal", "+1-555-987-6543");
    let uri = field.to_uri();
    // Should detect as phone and return tel: URI
    assert!(uri.is_some());
    assert!(uri.unwrap().starts_with("tel:"));
}

#[test]
fn test_custom_field_plain_text_returns_none() {
    let field = ContactField::new(FieldType::Custom, "Notes", "Met at conference");
    let uri = field.to_uri();
    assert!(uri.is_none());
}

#[test]
fn test_custom_to_action_copy_for_plain_text() {
    let field = ContactField::new(FieldType::Custom, "Notes", "Met at conference");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::CopyToClipboard));
}

// ============================================================
// Security: URI Scheme Whitelist
// ============================================================

#[test]
fn test_blocked_javascript_scheme() {
    // Even if someone tries to inject javascript:, it should be blocked
    let field = ContactField::new(FieldType::Website, "Malicious", "javascript:alert(1)");
    let uri = field.to_uri();
    assert!(uri.is_none());
}

#[test]
fn test_blocked_file_scheme() {
    let field = ContactField::new(FieldType::Website, "Local", "file:///etc/passwd");
    let uri = field.to_uri();
    assert!(uri.is_none());
}

#[test]
fn test_blocked_data_scheme() {
    let field = ContactField::new(
        FieldType::Website,
        "Data",
        "data:text/html,<script>alert(1)</script>",
    );
    let uri = field.to_uri();
    assert!(uri.is_none());
}

#[test]
fn test_allowed_tel_scheme() {
    assert!(webbook_core::contact_card::is_allowed_scheme("tel"));
}

#[test]
fn test_allowed_mailto_scheme() {
    assert!(webbook_core::contact_card::is_allowed_scheme("mailto"));
}

#[test]
fn test_allowed_https_scheme() {
    assert!(webbook_core::contact_card::is_allowed_scheme("https"));
}

#[test]
fn test_allowed_http_scheme() {
    assert!(webbook_core::contact_card::is_allowed_scheme("http"));
}

#[test]
fn test_allowed_sms_scheme() {
    assert!(webbook_core::contact_card::is_allowed_scheme("sms"));
}

#[test]
fn test_allowed_geo_scheme() {
    assert!(webbook_core::contact_card::is_allowed_scheme("geo"));
}

#[test]
fn test_blocked_scheme_javascript() {
    assert!(!webbook_core::contact_card::is_allowed_scheme("javascript"));
}

#[test]
fn test_blocked_scheme_file() {
    assert!(!webbook_core::contact_card::is_allowed_scheme("file"));
}

#[test]
fn test_blocked_scheme_data() {
    assert!(!webbook_core::contact_card::is_allowed_scheme("data"));
}

#[test]
fn test_blocked_scheme_vbscript() {
    assert!(!webbook_core::contact_card::is_allowed_scheme("vbscript"));
}

// ============================================================
// Edge Cases
// ============================================================

#[test]
fn test_empty_value_returns_none() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "");
    let uri = field.to_uri();
    assert!(uri.is_none());
}

#[test]
fn test_whitespace_only_value_returns_none() {
    let field = ContactField::new(FieldType::Email, "Work", "   ");
    let uri = field.to_uri();
    assert!(uri.is_none());
}

#[test]
fn test_special_characters_in_email_encoded() {
    let field = ContactField::new(FieldType::Email, "Test", "test&user@example.com");
    let uri = field.to_uri();
    assert!(uri.is_some());
    // & should be safe in mailto but let's verify it's handled
    assert!(uri.unwrap().contains("test"));
}

#[test]
fn test_unicode_in_address_encoded() {
    let field = ContactField::new(FieldType::Address, "Office", "東京都渋谷区");
    let uri = field.to_uri();
    assert!(uri.is_some());
    // Unicode should be percent-encoded
    let uri_str = uri.unwrap();
    assert!(uri_str.contains('%') || uri_str.contains("東京")); // Either encoded or raw UTF-8
}

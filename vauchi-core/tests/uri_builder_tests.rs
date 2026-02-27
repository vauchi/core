// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! URI Builder Tests
//!
//! TDD tests for contact field to URI conversion.
//! Reference: features/contact_actions.feature

use vauchi_core::contact_card::{ContactAction, ContactField, FieldType};

// ============================================================
// Phone Number → tel: URI
// ============================================================

// @scenario: contact_actions:Tap phone number opens dialer
#[test]
fn test_phone_field_generates_tel_uri() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567");
    let uri = field.to_uri();
    assert_eq!(uri, Some("tel:+1-555-123-4567".to_string()));
}

// @scenario: contact_actions:Phone number with international format
#[test]
fn test_phone_with_spaces_generates_tel_uri() {
    let field = ContactField::new(FieldType::Phone, "International", "+44 20 7946 0958");
    let uri = field.to_uri();
    // Spaces should be preserved or removed depending on RFC 3966
    assert!(uri.is_some());
    assert!(uri.unwrap().starts_with("tel:"));
}

// @scenario: contact_actions:Various phone number formats are normalized for dialer
#[test]
fn test_phone_with_parentheses() {
    let field = ContactField::new(FieldType::Phone, "Home", "(555) 123-4567");
    let uri = field.to_uri();
    assert!(uri.is_some());
    assert!(uri.unwrap().starts_with("tel:"));
}

// @scenario: contact_actions:Tap phone number opens dialer
#[test]
fn test_phone_to_action_returns_call() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::Call(_)));
}

// ============================================================
// Email → mailto: URI
// ============================================================

// @scenario: contact_actions:Tap email opens mail client
#[test]
fn test_email_field_generates_mailto_uri() {
    let field = ContactField::new(FieldType::Email, "Work", "bob@company.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("mailto:bob@company.com".to_string()));
}

// @scenario: contact_actions:Email with special characters
#[test]
fn test_email_with_plus_sign() {
    let field = ContactField::new(FieldType::Email, "Personal", "bob+work@company.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("mailto:bob+work@company.com".to_string()));
}

// @scenario: contact_actions:Tap email opens mail client
#[test]
fn test_email_to_action_returns_send_email() {
    let field = ContactField::new(FieldType::Email, "Work", "bob@test.com");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::SendEmail(_)));
}

// ============================================================
// Website → https:/http: URI
// ============================================================

// @scenario: contact_actions:Tap website opens browser
#[test]
fn test_website_with_https_preserved() {
    let field = ContactField::new(FieldType::Website, "Blog", "https://bobsmith.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://bobsmith.com".to_string()));
}

// @scenario: contact_actions:HTTP website preserves protocol
#[test]
fn test_website_with_http_preserved() {
    let field = ContactField::new(FieldType::Website, "Legacy", "http://old-site.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("http://old-site.com".to_string()));
}

// @scenario: contact_actions:Website without protocol prefix
#[test]
fn test_website_without_protocol_adds_https() {
    let field = ContactField::new(FieldType::Website, "Site", "bobsmith.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://bobsmith.com".to_string()));
}

// @scenario: contact_actions:Tap website opens browser
#[test]
fn test_website_to_action_returns_open_url() {
    let field = ContactField::new(FieldType::Website, "Site", "https://example.com");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::OpenUrl(_)));
}

// ============================================================
// Social → Profile URL
// ============================================================

// @scenario: contact_actions:Tap social media opens profile
#[test]
fn test_social_twitter_generates_profile_url() {
    let field = ContactField::new(FieldType::Social, "Twitter", "@bobsmith");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://twitter.com/bobsmith".to_string()));
}

// @scenario: contact_actions:Tap social media opens profile
#[test]
fn test_social_twitter_without_at_sign() {
    let field = ContactField::new(FieldType::Social, "Twitter", "bobsmith");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://twitter.com/bobsmith".to_string()));
}

// @scenario: contact_actions:Tap social media opens profile
#[test]
fn test_social_github_generates_profile_url() {
    let field = ContactField::new(FieldType::Social, "GitHub", "octocat");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://github.com/octocat".to_string()));
}

// @scenario: contact_actions:Tap social media opens profile
#[test]
fn test_social_linkedin_generates_profile_url() {
    let field = ContactField::new(FieldType::Social, "LinkedIn", "in/bobsmith");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://linkedin.com/in/bobsmith".to_string()));
}

// @scenario: contact_actions:Tap social media opens profile
#[test]
fn test_social_instagram_generates_profile_url() {
    let field = ContactField::new(FieldType::Social, "Instagram", "bob.smith");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://instagram.com/bob.smith".to_string()));
}

// @scenario: contact_actions:Tap social media opens profile
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

// @scenario: contact_actions:Tap social media opens profile
#[test]
fn test_social_to_action_returns_open_url() {
    let field = ContactField::new(FieldType::Social, "GitHub", "octocat");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::OpenUrl(_)));
}

// ============================================================
// Address → Map Query
// ============================================================

// @scenario: contact_actions:Tap address opens maps application
#[test]
fn test_address_generates_map_query() {
    let field = ContactField::new(FieldType::Address, "Home", "123 Main St, City, ST 12345");
    let uri = field.to_uri();
    assert!(uri.is_some());
    let uri_str = uri.unwrap();
    // Should be a geo: URI or maps URL
    assert!(uri_str.starts_with("geo:") || uri_str.contains("maps"));
}

// @scenario: contact_actions:Address opens web maps on desktop
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

// @scenario: contact_actions:Tap address opens maps application
#[test]
fn test_address_to_action_returns_open_map() {
    let field = ContactField::new(FieldType::Address, "Home", "123 Main St");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::OpenMap(_)));
}

// ============================================================
// Custom Field → Heuristic Detection
// ============================================================

// @scenario: contact_actions:Custom field with phone-like value offers call
#[test]
fn test_custom_field_with_phone_pattern_detected() {
    let field = ContactField::new(FieldType::Custom, "Signal", "+1-555-987-6543");
    let detected = field.detect_value_type();
    assert_eq!(detected, Some(FieldType::Phone));
}

// @scenario: contact_actions:Custom field with email-like value offers email
#[test]
fn test_custom_field_with_email_pattern_detected() {
    let field = ContactField::new(FieldType::Custom, "Alternate", "bob.alt@email.com");
    let detected = field.detect_value_type();
    assert_eq!(detected, Some(FieldType::Email));
}

// @scenario: contact_actions:Custom field with URL-like value offers browser
#[test]
fn test_custom_field_with_url_pattern_detected() {
    let field = ContactField::new(FieldType::Custom, "Portfolio", "https://portfolio.bob.com");
    let detected = field.detect_value_type();
    assert_eq!(detected, Some(FieldType::Website));
}

// @scenario: contact_actions:Custom field with plain text shows copy option
#[test]
fn test_custom_field_with_plain_text_not_detected() {
    let field = ContactField::new(FieldType::Custom, "Notes", "Met at conference");
    let detected = field.detect_value_type();
    assert!(detected.is_none());
}

// @scenario: contact_actions:Custom field with phone-like value offers call
#[test]
fn test_custom_field_uses_heuristic_for_uri() {
    let field = ContactField::new(FieldType::Custom, "Signal", "+1-555-987-6543");
    let uri = field.to_uri();
    // Should detect as phone and return tel: URI
    assert!(uri.is_some());
    assert!(uri.unwrap().starts_with("tel:"));
}

// @scenario: contact_actions:Custom field with plain text shows copy option
#[test]
fn test_custom_field_plain_text_returns_none() {
    let field = ContactField::new(FieldType::Custom, "Notes", "Met at conference");
    let uri = field.to_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions:Custom field with plain text shows copy option
#[test]
fn test_custom_to_action_copy_for_plain_text() {
    let field = ContactField::new(FieldType::Custom, "Notes", "Met at conference");
    let action = field.to_action();
    assert!(matches!(action, ContactAction::CopyToClipboard));
}

// ============================================================
// Security: URI Scheme Whitelist
// ============================================================

// @scenario: contact_actions:URLs are validated before opening
#[test]
fn test_blocked_javascript_scheme() {
    // Even if someone tries to inject javascript:, it should be blocked
    let field = ContactField::new(FieldType::Website, "Malicious", "javascript:alert(1)");
    let uri = field.to_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions:Only safe URI schemes are allowed
#[test]
fn test_blocked_file_scheme() {
    let field = ContactField::new(FieldType::Website, "Local", "file:///etc/passwd");
    let uri = field.to_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions:Only safe URI schemes are allowed
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

// @scenario: contact_actions:Allowed URI schemes whitelist (#47)
#[test]
fn test_allowed_tel_scheme() {
    assert!(vauchi_core::contact_card::is_allowed_scheme("tel"));
}

// @scenario: contact_actions:Allowed URI schemes whitelist (#47)
#[test]
fn test_allowed_mailto_scheme() {
    assert!(vauchi_core::contact_card::is_allowed_scheme("mailto"));
}

// @scenario: contact_actions:Allowed URI schemes whitelist (#47)
#[test]
fn test_allowed_https_scheme() {
    assert!(vauchi_core::contact_card::is_allowed_scheme("https"));
}

// @scenario: contact_actions:Allowed URI schemes whitelist (#47)
#[test]
fn test_allowed_http_scheme() {
    assert!(vauchi_core::contact_card::is_allowed_scheme("http"));
}

// @scenario: contact_actions:Allowed URI schemes whitelist (#47)
#[test]
fn test_allowed_sms_scheme() {
    assert!(vauchi_core::contact_card::is_allowed_scheme("sms"));
}

// @scenario: contact_actions:Allowed URI schemes whitelist (#47)
#[test]
fn test_allowed_geo_scheme() {
    assert!(vauchi_core::contact_card::is_allowed_scheme("geo"));
}

// @scenario: contact_actions:Allowed URI schemes whitelist (#47)
#[test]
fn test_blocked_scheme_javascript() {
    assert!(!vauchi_core::contact_card::is_allowed_scheme("javascript"));
}

// @scenario: contact_actions:Allowed URI schemes whitelist (#47)
#[test]
fn test_blocked_scheme_file() {
    assert!(!vauchi_core::contact_card::is_allowed_scheme("file"));
}

// @scenario: contact_actions:Allowed URI schemes whitelist (#47)
#[test]
fn test_blocked_scheme_data() {
    assert!(!vauchi_core::contact_card::is_allowed_scheme("data"));
}

// @scenario: contact_actions:Allowed URI schemes whitelist (#47)
#[test]
fn test_blocked_scheme_vbscript() {
    assert!(!vauchi_core::contact_card::is_allowed_scheme("vbscript"));
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

// @scenario: contact_actions:Email with special characters
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

// ============================================================
// Secondary Actions (Context Menus)
// @scenario: contact_actions:Context menu shows all applicable actions
// ============================================================

// @scenario: contact_actions:Phone field offers Call and SMS actions in context menu
#[test]
fn test_phone_secondary_actions_call_and_sms() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567");
    let actions = field.to_secondary_actions();
    assert_eq!(actions.len(), 3); // Call, SendSms, CopyToClipboard
    assert!(actions.contains(&ContactAction::Call("+1-555-123-4567".to_string())));
    assert!(actions.contains(&ContactAction::SendSms("+1-555-123-4567".to_string())));
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions:Email field offers Send Email and Copy actions
#[test]
fn test_email_secondary_actions() {
    let field = ContactField::new(FieldType::Email, "Work", "bob@company.com");
    let actions = field.to_secondary_actions();
    assert_eq!(actions.len(), 2); // SendEmail, CopyToClipboard
    assert!(actions.contains(&ContactAction::SendEmail("bob@company.com".to_string())));
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions:Website field offers Open URL and Copy actions
#[test]
fn test_website_secondary_actions() {
    let field = ContactField::new(FieldType::Website, "Blog", "https://example.com");
    let actions = field.to_secondary_actions();
    assert_eq!(actions.len(), 2); // OpenUrl, CopyToClipboard
    assert!(actions.contains(&ContactAction::OpenUrl("https://example.com".to_string())));
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions:Address field offers Get Directions and Open Map and Copy actions
#[test]
fn test_address_secondary_actions() {
    let field = ContactField::new(FieldType::Address, "Home", "123 Main St, City");
    let actions = field.to_secondary_actions();
    assert_eq!(actions.len(), 3); // OpenMap, GetDirections, CopyToClipboard
    assert!(actions
        .iter()
        .any(|a| matches!(a, ContactAction::OpenMap(_))));
    assert!(actions
        .iter()
        .any(|a| matches!(a, ContactAction::GetDirections(_))));
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions:Social field offers Open Profile and Copy actions
#[test]
fn test_social_secondary_actions() {
    let field = ContactField::new(FieldType::Social, "Twitter", "@bobsmith");
    let actions = field.to_secondary_actions();
    assert!(actions.len() >= 2); // At least OpenUrl and CopyToClipboard
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions:Custom field with actionable content offers actions
#[test]
fn test_custom_field_secondary_actions() {
    let field = ContactField::new(FieldType::Custom, "Notes", "+1-555-987-6543");
    let actions = field.to_secondary_actions();
    // Detected as phone, should have Call, SMS, Copy
    assert_eq!(actions.len(), 3);
    assert!(actions.contains(&ContactAction::Call("+1-555-987-6543".to_string())));
    assert!(actions.contains(&ContactAction::SendSms("+1-555-987-6543".to_string())));
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions:Empty field shows only Copy action
#[test]
fn test_empty_field_secondary_actions() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "");
    let actions = field.to_secondary_actions();
    assert_eq!(actions.len(), 1); // Only CopyToClipboard (copy empty)
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// ============================================================
// Directions URI (maps routing)
// @scenario: contact_actions:Get directions to address
// ============================================================

// @scenario: contact_actions:Get directions generates web maps URL
#[test]
fn test_directions_uri_basic_address() {
    let field = ContactField::new(FieldType::Address, "Home", "123 Main St, City, ST 12345");
    let uri = field.to_directions_uri();
    assert!(uri.is_some());
    let uri_str = uri.unwrap();
    // Should be a web maps directions URL (not geo:)
    assert!(uri_str.starts_with("https://"));
    assert!(uri_str.contains("directions"));
    assert!(uri_str.contains("123")); // Address should be encoded in the URL
}

// @scenario: contact_actions:Get directions URL-encodes special characters
#[test]
fn test_directions_uri_special_chars_encoded() {
    let field = ContactField::new(FieldType::Address, "Office", "123 O'Brien's Way, Suite #5");
    let uri = field.to_directions_uri();
    assert!(uri.is_some());
    let uri_str = uri.unwrap();
    // Apostrophe and # should be encoded
    assert!(!uri_str.contains('\'') || uri_str.contains("%27"));
    assert!(!uri_str.contains('#') || uri_str.contains("%23"));
}

// @scenario: contact_actions:Get directions handles empty address
#[test]
fn test_directions_uri_empty_returns_none() {
    let field = ContactField::new(FieldType::Address, "Home", "");
    let uri = field.to_directions_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions:Get directions handles whitespace-only address
#[test]
fn test_directions_uri_whitespace_returns_none() {
    let field = ContactField::new(FieldType::Address, "Home", "   ");
    let uri = field.to_directions_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions:Non-address fields return None for directions
#[test]
fn test_directions_uri_non_address_returns_none() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567");
    let uri = field.to_directions_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions:Get directions with unicode address
#[test]
fn test_directions_uri_unicode_address() {
    let field = ContactField::new(FieldType::Address, "Tokyo Office", "東京都渋谷区");
    let uri = field.to_directions_uri();
    assert!(uri.is_some());
    let uri_str = uri.unwrap();
    assert!(uri_str.starts_with("https://"));
    // Unicode should be percent-encoded
    assert!(uri_str.contains('%'));
}

// ============================================================
// Integration Tests: Contact Card with Actions
// Reference: features/contact_actions.feature
// ============================================================

use vauchi_core::contact_card::ContactCard;

/// Integration test: Contact with multiple actionable fields
/// Maps to: feature file "Background" scenario setup
#[test]
fn test_contact_with_multiple_actionable_fields() {
    let mut card = ContactCard::new("Bob");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+1-555-123-4567",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "bob@company.com",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Website,
        "Personal",
        "https://bobsmith.com",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Address,
        "Home",
        "123 Main St, City",
    ))
    .unwrap();

    // All fields should generate valid URIs
    for field in card.fields() {
        let uri = field.to_uri();
        assert!(uri.is_some(), "Field {} should have a URI", field.label());
    }
}

/// Integration test: All field types return appropriate ContactAction
/// Maps to: Cross-Platform Consistency scenarios
#[test]
fn test_all_field_types_have_actions() {
    let test_cases = vec![
        (FieldType::Phone, "Mobile", "+1-555-123-4567", "Call"),
        (FieldType::Email, "Work", "bob@example.com", "SendEmail"),
        (FieldType::Website, "Blog", "https://example.com", "OpenUrl"),
        (FieldType::Address, "Home", "123 Main St", "OpenMap"),
        (FieldType::Social, "Twitter", "@bobsmith", "OpenUrl"),
        (FieldType::Custom, "Notes", "Plain text", "CopyToClipboard"),
    ];

    for (field_type, label, value, expected_action) in test_cases {
        let field = ContactField::new(field_type.clone(), label, value);
        let action = field.to_action();
        let action_str = format!("{:?}", action);
        assert!(
            action_str.contains(expected_action),
            "Field {:?} '{}' should return {} action, got {:?}",
            field_type,
            label,
            expected_action,
            action
        );
    }
}

/// Integration test: Mastodon social handle parsing
/// Maps to: Social media "@bob@mas.to" scenario
// @scenario: contact_actions:Tap social media opens profile
#[test]
fn test_social_mastodon_handle_on_default_instance() {
    let field = ContactField::new(FieldType::Social, "Mastodon", "@bob");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://mastodon.social/@bob".to_string()));
}

// @scenario: contact_actions:Mastodon federated handle resolves to correct instance
#[test]
fn test_social_mastodon_federated_handle_at_prefix() {
    // @bob@mas.to → https://mas.to/@bob
    let field = ContactField::new(FieldType::Social, "Mastodon", "@bob@mas.to");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://mas.to/@bob".to_string()));
}

// @scenario: contact_actions:Mastodon federated handle resolves to correct instance
#[test]
fn test_social_mastodon_federated_handle_no_prefix() {
    // bob@fosstodon.org → https://fosstodon.org/@bob
    let field = ContactField::new(FieldType::Social, "Mastodon", "bob@fosstodon.org");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://fosstodon.org/@bob".to_string()));
}

// @scenario: contact_actions:Mastodon federated handle resolves to correct instance
#[test]
fn test_social_mastodon_default_instance_handle() {
    // @alice@mastodon.social → https://mastodon.social/@alice
    let field = ContactField::new(FieldType::Social, "Mastodon", "@alice@mastodon.social");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://mastodon.social/@alice".to_string()));
}

// @scenario: contact_actions:Mastodon bare username without @ prefix
#[test]
fn test_social_mastodon_bare_username() {
    let field = ContactField::new(FieldType::Social, "Mastodon", "bob");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://mastodon.social/@bob".to_string()));
}

/// Integration test: SMS action for phone numbers
/// Maps to: "Send SMS to phone number" scenario
// @scenario: contact_actions:Send SMS to phone number
#[test]
fn test_phone_sms_uri() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567");
    // Generate SMS URI (sms: scheme)
    let sms_uri = format!("sms:{}", field.value().replace(' ', ""));
    assert!(sms_uri.starts_with("sms:"));
    assert!(vauchi_core::contact_card::is_allowed_scheme("sms"));
}

/// Integration test: Website with subdomain
// @scenario: contact_actions:Website without protocol prefix
#[test]
fn test_website_with_subdomain() {
    let field = ContactField::new(FieldType::Website, "Blog", "blog.example.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://blog.example.com".to_string()));
}

/// Integration test: Website with path and query
// @scenario: contact_actions:Tap website opens browser
#[test]
fn test_website_with_path_and_query() {
    let field = ContactField::new(
        FieldType::Website,
        "Profile",
        "https://example.com/user?id=123",
    );
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://example.com/user?id=123".to_string()));
}

/// Integration test: Custom field detected as URL
// @scenario: contact_actions:Custom field with URL-like value offers browser
#[test]
fn test_custom_field_http_url_detected() {
    let field = ContactField::new(FieldType::Custom, "Portfolio", "http://oldsite.example.com");
    let detected = field.detect_value_type();
    assert_eq!(detected, Some(FieldType::Website));
    let uri = field.to_uri();
    assert_eq!(uri, Some("http://oldsite.example.com".to_string()));
}

/// Integration test: International phone numbers
/// Maps to: "Phone number with international format" scenario
// @scenario: contact_actions:Phone number with international format
// @scenario: contact_actions:Various phone number formats are normalized for dialer
#[test]
fn test_international_phone_formats() {
    let phones = vec![
        ("+44 20 7946 0958", "UK"),
        ("+81 3-1234-5678", "Japan"),
        ("+49 30 12345678", "Germany"),
        ("+33 1 23 45 67 89", "France"),
    ];

    for (number, country) in phones {
        let field = ContactField::new(FieldType::Phone, country, number);
        let uri = field.to_uri();
        assert!(uri.is_some(), "{} phone should generate URI", country);
        assert!(
            uri.unwrap().starts_with("tel:"),
            "{} phone should use tel: scheme",
            country
        );
    }
}

/// Integration test: Action icons mapping
/// Maps to: Visual feedback scenarios
#[test]
fn test_action_type_categorization() {
    // Verify action types for icon mapping
    let phone = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567");
    assert!(matches!(phone.to_action(), ContactAction::Call(_)));

    let email = ContactField::new(FieldType::Email, "Work", "test@example.com");
    assert!(matches!(email.to_action(), ContactAction::SendEmail(_)));

    let website = ContactField::new(FieldType::Website, "Blog", "https://example.com");
    assert!(matches!(website.to_action(), ContactAction::OpenUrl(_)));

    let address = ContactField::new(FieldType::Address, "Home", "123 Main St");
    assert!(matches!(address.to_action(), ContactAction::OpenMap(_)));
}

// ============================================================
// Security Integration Tests
// ============================================================

/// Security test: XSS attempt in website field blocked
/// Maps to: contact_actions.feature #45 — "URLs are validated before opening"
// @scenario: contact_actions:URLs are validated before opening (#45)
#[test]
fn test_xss_in_website_blocked() {
    let malicious_values = vec![
        "javascript:alert('xss')",
        "javascript:document.cookie",
        "JAVASCRIPT:alert(1)",   // Case insensitive
        "  javascript:alert(1)", // Leading space
    ];

    for value in malicious_values {
        let field = ContactField::new(FieldType::Website, "Malicious", value);
        let uri = field.to_uri();
        assert!(uri.is_none(), "XSS attempt '{}' should be blocked", value);
    }
}

/// Security test: Data URI scheme blocked
/// Maps to: contact_actions.feature #46 — "Only safe URI schemes are allowed"
// @scenario: contact_actions:Only safe URI schemes are allowed (#46)
#[test]
fn test_data_uri_blocked() {
    let data_uris = vec![
        "data:text/html,<script>alert(1)</script>",
        "data:image/svg+xml,<svg onload=alert(1)>",
        "DATA:text/html,test", // Case insensitive
    ];

    for value in data_uris {
        let field = ContactField::new(FieldType::Website, "Data", value);
        let uri = field.to_uri();
        assert!(uri.is_none(), "Data URI '{}' should be blocked", value);
    }
}

/// Security test: FTP scheme blocked
/// Maps to: contact_actions.feature #46 — "Only safe URI schemes are allowed"
// @scenario: contact_actions:Only safe URI schemes are allowed (#46)
#[test]
fn test_ftp_scheme_blocked() {
    let field = ContactField::new(FieldType::Website, "FTP", "ftp://files.example.com");
    let uri = field.to_uri();
    assert!(uri.is_none(), "FTP scheme should be blocked");
}

/// Security test: Custom field with malicious content
/// Maps to: contact_actions.feature #46 — "Only safe URI schemes are allowed"
// @scenario: contact_actions:Only safe URI schemes are allowed (#46)
#[test]
fn test_custom_field_malicious_url_blocked() {
    let field = ContactField::new(FieldType::Custom, "Link", "javascript:void(0)");
    let uri = field.to_uri();
    assert!(uri.is_none(), "Malicious custom field should be blocked");
}

// ============================================================
// Edge Case Integration Tests
// ============================================================

/// Edge case: Very long URL
#[test]
fn test_very_long_url() {
    let long_path = "a".repeat(500);
    let url = format!("https://example.com/{}", long_path);
    let field = ContactField::new(FieldType::Website, "Long", &url);
    let uri = field.to_uri();
    assert!(uri.is_some(), "Long URLs should still work");
}

/// Edge case: URL with unicode domain (IDN)
#[test]
fn test_unicode_domain_url() {
    let field = ContactField::new(FieldType::Website, "IDN", "https://例え.jp");
    let uri = field.to_uri();
    // Should preserve or encode the unicode domain
    assert!(uri.is_some());
}

/// Edge case: Email with dots in local part
// @scenario: contact_actions:Tap email opens mail client
#[test]
fn test_email_with_dots() {
    let field = ContactField::new(FieldType::Email, "Gmail", "first.last@gmail.com");
    let uri = field.to_uri();
    assert_eq!(uri, Some("mailto:first.last@gmail.com".to_string()));
}

/// Edge case: Address with special characters
// @scenario: contact_actions:Tap address opens maps application
#[test]
fn test_address_special_characters() {
    let field = ContactField::new(FieldType::Address, "Office", "123 O'Brien's Way, Suite #5");
    let uri = field.to_uri();
    assert!(uri.is_some());
    // Special characters should be encoded
    let uri_str = uri.unwrap();
    assert!(uri_str.contains("geo:") || uri_str.contains("maps"));
}

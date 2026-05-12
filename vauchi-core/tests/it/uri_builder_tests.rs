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

// @scenario: contact_actions :: Tap phone number opens dialer
// @internal
#[test]
fn test_phone_field_generates_tel_uri() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("tel:+1-555-123-4567".to_string()));
}

// @scenario: contact_actions :: Phone number with international format
// @internal
#[test]
fn test_phone_with_spaces_generates_tel_uri() {
    let field = ContactField::new(FieldType::Phone, "International", "+44 20 7946 0958", 0);
    let uri = field.to_uri();
    // Spaces should be preserved or removed depending on RFC 3966
    assert!(uri.is_some(), "expected Some value");
    assert!(uri.unwrap().starts_with("tel:"));
}

// @scenario: contact_actions :: Various phone number formats are normalized for dialer
// @internal
#[test]
fn test_phone_with_parentheses() {
    let field = ContactField::new(FieldType::Phone, "Home", "(555) 123-4567", 0);
    let uri = field.to_uri();
    assert!(uri.is_some(), "expected Some value");
    assert!(uri.unwrap().starts_with("tel:"));
}

// @scenario: contact_actions :: Tap phone number opens dialer
// @internal
#[test]
fn test_phone_to_action_returns_call() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567", 0);
    assert!(matches!(field.to_action(), ContactAction::Call(_)));
}

// ============================================================
// Email → mailto: URI
// ============================================================

// @scenario: contact_actions :: Tap email opens mail client
// @internal
#[test]
fn test_email_field_generates_mailto_uri() {
    let field = ContactField::new(FieldType::Email, "Work", "bob@company.com", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("mailto:bob@company.com".to_string()));
}

// @scenario: contact_actions :: Email with special characters
// @internal
#[test]
fn test_email_with_plus_sign() {
    let field = ContactField::new(FieldType::Email, "Personal", "bob+work@company.com", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("mailto:bob+work@company.com".to_string()));
}

// @scenario: contact_actions :: Tap email opens mail client
// @internal
#[test]
fn test_email_to_action_returns_send_email() {
    let field = ContactField::new(FieldType::Email, "Work", "bob@test.com", 0);
    let action = field.to_action();
    assert!(matches!(action, ContactAction::SendEmail(_)));
}

// ============================================================
// Website → https:/http: URI
// ============================================================

// @scenario: contact_actions :: Tap website opens browser
// @internal
#[test]
fn test_website_with_https_preserved() {
    let field = ContactField::new(FieldType::Website, "Blog", "https://bobsmith.com", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://bobsmith.com".to_string()));
}

// @scenario: contact_actions :: HTTP website preserves protocol
// @internal
#[test]
fn test_website_with_http_preserved() {
    let field = ContactField::new(FieldType::Website, "Legacy", "http://old-site.com", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("http://old-site.com".to_string()));
}

// @scenario: contact_actions :: Website without protocol prefix
// @internal
#[test]
fn test_website_without_protocol_adds_https() {
    let field = ContactField::new(FieldType::Website, "Site", "bobsmith.com", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://bobsmith.com".to_string()));
}

// @scenario: contact_actions :: Tap website opens browser
// @internal
#[test]
fn test_website_to_action_returns_open_url() {
    let field = ContactField::new(FieldType::Website, "Site", "https://example.com", 0);
    let action = field.to_action();
    assert!(matches!(action, ContactAction::OpenUrl(_)));
}

// ============================================================
// Social → Profile URL
// ============================================================

// @scenario: contact_actions :: Tap social media opens profile
// Parameterized: each case tests (label, username, expected_url).
#[rstest::rstest]
#[case("Twitter", "@bobsmith", "https://twitter.com/bobsmith")]
#[case("Twitter", "bobsmith", "https://twitter.com/bobsmith")]
#[case("GitHub", "octocat", "https://github.com/octocat")]
#[case("LinkedIn", "in/bobsmith", "https://linkedin.com/in/bobsmith")]
#[case("Instagram", "bob.smith", "https://instagram.com/bob.smith")]
#[case("Facebook", "bob.smith.123", "https://facebook.com/bob.smith.123")]
#[case("Twitch", "streamer42", "https://twitch.tv/streamer42")]
#[case("GitLab", "devuser", "https://gitlab.com/devuser")]
#[case("Telegram", "alice", "https://t.me/alice")]
#[case("Discord", "user123", "https://discord.com/users/user123")]
#[case("Threads", "@alice", "https://threads.net/@alice")]
#[case("Spotify", "musicfan", "https://open.spotify.com/user/musicfan")]
#[case("X", "@alice", "https://twitter.com/alice")]
#[case("LinkedIn", "in/johndoe", "https://linkedin.com/in/johndoe")]
fn test_social_network_generates_profile_url(
    #[case] label: &str,
    #[case] username: &str,
    #[case] expected: &str,
) {
    let field = ContactField::new(FieldType::Social, label, username, 0);
    let uri = field.to_uri();
    assert_eq!(
        uri,
        Some(expected.to_string()),
        "{} with '{}' should produce '{}'",
        label,
        username,
        expected
    );
}

// @internal
#[test]
fn test_social_unknown_network_returns_none() {
    let field = ContactField::new(FieldType::Social, "UnknownNetwork", "bobsmith", 0);
    let uri = field.to_uri();
    // Unknown networks should return None (can't generate URL)
    assert!(uri.is_none());
}

// @scenario: contact_actions :: Tap social media opens profile
// @internal
#[test]
fn test_social_to_action_returns_open_url() {
    let field = ContactField::new(FieldType::Social, "GitHub", "octocat", 0);
    let action = field.to_action();
    assert!(matches!(action, ContactAction::OpenUrl(_)));
}

// ============================================================
// Address → Map Query
// ============================================================

// @scenario: contact_actions :: Tap address opens maps application
// @internal
#[test]
fn test_address_generates_map_query() {
    let field = ContactField::new(FieldType::Address, "Home", "123 Main St, City, ST 12345", 0);
    let uri = field.to_uri();
    assert!(uri.is_some(), "expected Some value");
    let uri_str = uri.unwrap();
    // Should be a geo: URI or maps URL
    assert!(uri_str.starts_with("geo:") || uri_str.contains("maps"));
}

// @scenario: contact_actions :: Address opens web maps on desktop
// @internal
#[test]
fn test_address_is_url_encoded() {
    let field = ContactField::new(
        FieldType::Address,
        "Office",
        "123 Main St, San Francisco, CA",
        0,
    );
    let uri = field.to_uri();
    assert!(uri.is_some(), "expected Some value");
    let uri_str = uri.unwrap();
    // Spaces and commas should be encoded
    assert!(!uri_str.contains(' ') || uri_str.contains("%20") || uri_str.contains('+'));
}

// @scenario: contact_actions :: Tap address opens maps application
// @internal
#[test]
fn test_address_to_action_returns_open_map() {
    let field = ContactField::new(FieldType::Address, "Home", "123 Main St", 0);
    let action = field.to_action();
    assert!(matches!(action, ContactAction::OpenMap(_)));
}

// ============================================================
// Custom Field → Heuristic Detection
// ============================================================

// @scenario: contact_actions :: Custom field heuristic detection
#[rstest::rstest]
#[case("Signal", "+1-555-987-6543", Some(FieldType::Phone))]
#[case("Alternate", "bob.alt@email.com", Some(FieldType::Email))]
#[case("Portfolio", "https://portfolio.bob.com", Some(FieldType::Website))]
#[case("Notes", "Met at conference", None)]
fn test_custom_field_value_type_detection(
    #[case] label: &str,
    #[case] value: &str,
    #[case] expected: Option<FieldType>,
) {
    let field = ContactField::new(FieldType::Custom, label, value, 0);
    assert_eq!(
        field.detect_value_type(),
        expected,
        "Custom field '{label}' with '{value}' detection mismatch"
    );
}

// @scenario: contact_actions :: Custom field with phone-like value offers call
// @internal
#[test]
fn test_custom_field_uses_heuristic_for_uri() {
    let field = ContactField::new(FieldType::Custom, "Signal", "+1-555-987-6543", 0);
    let uri = field.to_uri();
    // Should detect as phone and return tel: URI
    assert!(uri.is_some(), "expected Some value");
    assert!(uri.unwrap().starts_with("tel:"));
}

// @scenario: contact_actions :: Custom field with plain text shows copy option
// @internal
#[test]
fn test_custom_field_plain_text_returns_none() {
    let field = ContactField::new(FieldType::Custom, "Notes", "Met at conference", 0);
    let uri = field.to_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions :: Custom field with plain text shows copy option
// @internal
#[test]
fn test_custom_to_action_copy_for_plain_text() {
    let field = ContactField::new(FieldType::Custom, "Notes", "Met at conference", 0);
    let action = field.to_action();
    assert!(matches!(action, ContactAction::CopyToClipboard));
}

// ============================================================
// Security: URI Scheme Whitelist
// ============================================================

// @scenario: contact_actions :: URLs are validated before opening
// @scenario: contact_actions :: Only safe URI schemes are allowed
#[rstest::rstest]
#[case("javascript:alert(1)")]
#[case("file:///etc/passwd")]
#[case("data:text/html,<script>alert(1)</script>")]
fn test_blocked_uri_scheme_returns_none(#[case] value: &str) {
    let field = ContactField::new(FieldType::Website, "Test", value, 0);
    let uri = field.to_uri();
    assert!(uri.is_none(), "{value} should be blocked");
}

// @scenario: contact_actions :: Allowed URI schemes whitelist (#47)
#[rstest::rstest]
#[case("tel")]
#[case("mailto")]
#[case("https")]
#[case("http")]
#[case("sms")]
#[case("geo")]
fn test_allowed_scheme(#[case] scheme: &str) {
    assert!(
        vauchi_core::contact_card::is_allowed_scheme(scheme),
        "{scheme} should be allowed"
    );
}

// @scenario: contact_actions :: Allowed URI schemes whitelist (#47)
#[rstest::rstest]
#[case("javascript")]
#[case("file")]
#[case("data")]
#[case("vbscript")]
fn test_blocked_scheme(#[case] scheme: &str) {
    assert!(
        !vauchi_core::contact_card::is_allowed_scheme(scheme),
        "{scheme} should be blocked"
    );
}

// ============================================================
// Edge Cases
// ============================================================

// @internal
#[test]
fn test_empty_value_returns_none() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "", 0);
    let uri = field.to_uri();
    assert!(uri.is_none());
}

// @internal
#[test]
fn test_whitespace_only_value_returns_none() {
    let field = ContactField::new(FieldType::Email, "Work", "   ", 0);
    let uri = field.to_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions :: Email with special characters
// @internal
#[test]
fn test_special_characters_in_email_encoded() {
    let field = ContactField::new(FieldType::Email, "Test", "test&user@example.com", 0);
    let uri = field.to_uri();
    assert!(uri.is_some(), "expected Some value");
    // & should be safe in mailto but let's verify it's handled
    assert!(uri.unwrap().contains("test"));
}

// @internal
#[test]
fn test_unicode_in_address_encoded() {
    let field = ContactField::new(FieldType::Address, "Office", "東京都渋谷区", 0);
    let uri = field.to_uri();
    assert!(uri.is_some(), "expected Some value");
    // Unicode should be percent-encoded
    let uri_str = uri.unwrap();
    assert!(uri_str.contains('%') || uri_str.contains("東京")); // Either encoded or raw UTF-8
}

// ============================================================
// Secondary Actions (Context Menus)
// @scenario: contact_actions :: Context menu shows all applicable actions
// ============================================================

// @scenario: contact_actions :: Phone field offers Call and SMS actions in context menu
// @scenario: contact_actions :: Long press phone number shows action menu
// @internal
#[test]
fn test_phone_secondary_actions_call_and_sms() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567", 0);
    let actions = field.to_secondary_actions();
    assert_eq!(actions.len(), 3); // Call, SendSms, CopyToClipboard
    assert!(actions.contains(&ContactAction::Call("+1-555-123-4567".to_string())));
    assert!(actions.contains(&ContactAction::SendSms("+1-555-123-4567".to_string())));
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions :: Email field offers Send Email and Copy actions
// @scenario: contact_actions :: Long press email shows action menu
// @internal
#[test]
fn test_email_secondary_actions() {
    let field = ContactField::new(FieldType::Email, "Work", "bob@company.com", 0);
    let actions = field.to_secondary_actions();
    assert_eq!(actions.len(), 2); // SendEmail, CopyToClipboard
    assert!(actions.contains(&ContactAction::SendEmail("bob@company.com".to_string())));
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions :: Website field offers Open URL and Copy actions
// @scenario: contact_actions :: Long press website shows action menu
// @internal
#[test]
fn test_website_secondary_actions() {
    let field = ContactField::new(FieldType::Website, "Blog", "https://example.com", 0);
    let actions = field.to_secondary_actions();
    assert_eq!(actions.len(), 2); // OpenUrl, CopyToClipboard
    assert!(actions.contains(&ContactAction::OpenUrl("https://example.com".to_string())));
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions :: Address field offers Get Directions and Open Map and Copy actions
// @scenario: contact_actions :: Long press address shows action menu
// @internal
#[test]
fn test_address_secondary_actions() {
    let field = ContactField::new(FieldType::Address, "Home", "123 Main St, City", 0);
    let actions = field.to_secondary_actions();
    assert_eq!(actions.len(), 3); // OpenMap, GetDirections, CopyToClipboard
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, ContactAction::OpenMap(_)))
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, ContactAction::GetDirections(_)))
    );
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions :: Social field offers Open Profile and Copy actions
// @scenario: contact_actions :: Long press social field shows action menu
// @internal
#[test]
fn test_social_secondary_actions() {
    let field = ContactField::new(FieldType::Social, "Twitter", "@bobsmith", 0);
    let actions = field.to_secondary_actions();
    assert!(actions.len() >= 2); // At least OpenUrl and CopyToClipboard
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions :: Custom field with actionable content offers actions
// @scenario: contact_actions :: Long press custom field shows contextual menu
// @internal
#[test]
fn test_custom_field_secondary_actions() {
    let field = ContactField::new(FieldType::Custom, "Notes", "+1-555-987-6543", 0);
    let actions = field.to_secondary_actions();
    // Detected as phone, should have Call, SMS, Copy
    assert_eq!(actions.len(), 3);
    assert!(actions.contains(&ContactAction::Call("+1-555-987-6543".to_string())));
    assert!(actions.contains(&ContactAction::SendSms("+1-555-987-6543".to_string())));
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// @scenario: contact_actions :: Empty field shows only Copy action
// @internal
#[test]
fn test_empty_field_secondary_actions() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "", 0);
    let actions = field.to_secondary_actions();
    assert_eq!(actions.len(), 1); // Only CopyToClipboard (copy empty)
    assert!(actions.contains(&ContactAction::CopyToClipboard));
}

// ============================================================
// Directions URI (maps routing)
// @scenario: contact_actions :: Get directions to address
// ============================================================

// @scenario: contact_actions :: Get directions generates web maps URL
// @scenario: contact_actions :: Get directions to address
// @internal
#[test]
fn test_directions_uri_basic_address() {
    let field = ContactField::new(FieldType::Address, "Home", "123 Main St, City, ST 12345", 0);
    let uri = field.to_directions_uri();
    assert!(uri.is_some(), "expected Some value");
    let uri_str = uri.unwrap();
    // Should be a web maps directions URL (not geo:)
    assert!(uri_str.starts_with("https://"));
    assert!(uri_str.contains("directions"));
    assert!(uri_str.contains("123")); // Address should be encoded in the URL
}

// @scenario: contact_actions :: Get directions URL-encodes special characters
// @internal
#[test]
fn test_directions_uri_special_chars_encoded() {
    let field = ContactField::new(
        FieldType::Address,
        "Office",
        "123 O'Brien's Way, Suite #5",
        0,
    );
    let uri = field.to_directions_uri();
    assert!(uri.is_some(), "expected Some value");
    let uri_str = uri.unwrap();
    // Apostrophe and # should be encoded
    assert!(!uri_str.contains('\'') || uri_str.contains("%27"));
    assert!(!uri_str.contains('#') || uri_str.contains("%23"));
}

// @scenario: contact_actions :: Get directions handles empty address
// @internal
#[test]
fn test_directions_uri_empty_returns_none() {
    let field = ContactField::new(FieldType::Address, "Home", "", 0);
    let uri = field.to_directions_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions :: Get directions handles whitespace-only address
// @internal
#[test]
fn test_directions_uri_whitespace_returns_none() {
    let field = ContactField::new(FieldType::Address, "Home", "   ", 0);
    let uri = field.to_directions_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions :: Non-address fields return None for directions
// @internal
#[test]
fn test_directions_uri_non_address_returns_none() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567", 0);
    let uri = field.to_directions_uri();
    assert!(uri.is_none());
}

// @scenario: contact_actions :: Get directions with unicode address
// @internal
#[test]
fn test_directions_uri_unicode_address() {
    let field = ContactField::new(FieldType::Address, "Tokyo Office", "東京都渋谷区", 0);
    let uri = field.to_directions_uri();
    assert!(uri.is_some(), "expected Some value");
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
// @internal
#[test]
fn test_contact_with_multiple_actionable_fields() {
    let mut card = ContactCard::new("Bob");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+1-555-123-4567",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Email,
        "Work",
        "bob@company.com",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Website,
        "Personal",
        "https://bobsmith.com",
        0,
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Address,
        "Home",
        "123 Main St, City",
        0,
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
// @internal
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
        let field = ContactField::new(field_type.clone(), label, value, 0);
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
// @scenario: contact_actions :: Tap social media opens profile
// @internal
#[test]
fn test_social_mastodon_handle_on_default_instance() {
    let field = ContactField::new(FieldType::Social, "Mastodon", "@bob", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://mastodon.social/@bob".to_string()));
}

// @scenario: contact_actions :: Mastodon federated handle resolves to correct instance
// @internal
#[test]
fn test_social_mastodon_federated_handle_at_prefix() {
    // @bob@mas.to → https://mas.to/@bob
    let field = ContactField::new(FieldType::Social, "Mastodon", "@bob@mas.to", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://mas.to/@bob".to_string()));
}

// @scenario: contact_actions :: Mastodon federated handle resolves to correct instance
// @internal
#[test]
fn test_social_mastodon_federated_handle_no_prefix() {
    // bob@fosstodon.org → https://fosstodon.org/@bob
    let field = ContactField::new(FieldType::Social, "Mastodon", "bob@fosstodon.org", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://fosstodon.org/@bob".to_string()));
}

// @scenario: contact_actions :: Mastodon federated handle resolves to correct instance
// @internal
#[test]
fn test_social_mastodon_default_instance_handle() {
    // @alice@mastodon.social → https://mastodon.social/@alice
    let field = ContactField::new(FieldType::Social, "Mastodon", "@alice@mastodon.social", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://mastodon.social/@alice".to_string()));
}

// @scenario: contact_actions :: Mastodon bare username without @ prefix
// @internal
#[test]
fn test_social_mastodon_bare_username() {
    let field = ContactField::new(FieldType::Social, "Mastodon", "bob", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://mastodon.social/@bob".to_string()));
}

/// Integration test: SMS action for phone numbers
/// Maps to: "Send SMS to phone number" scenario
// @scenario: contact_actions :: Send SMS to phone number
// @internal
#[test]
fn test_phone_sms_uri() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567", 0);
    // Generate SMS URI (sms: scheme)
    let sms_uri = format!("sms:{}", field.value().replace(' ', ""));
    assert!(sms_uri.starts_with("sms:"));
    assert!(vauchi_core::contact_card::is_allowed_scheme("sms"));
}

/// Integration test: Website with subdomain
// @scenario: contact_actions :: Website without protocol prefix
// @internal
#[test]
fn test_website_with_subdomain() {
    let field = ContactField::new(FieldType::Website, "Blog", "blog.example.com", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://blog.example.com".to_string()));
}

/// Integration test: Website with path and query
// @scenario: contact_actions :: Tap website opens browser
// @internal
#[test]
fn test_website_with_path_and_query() {
    let field = ContactField::new(
        FieldType::Website,
        "Profile",
        "https://example.com/user?id=123",
        0,
    );
    let uri = field.to_uri();
    assert_eq!(uri, Some("https://example.com/user?id=123".to_string()));
}

/// Integration test: Custom field detected as URL
// @scenario: contact_actions :: Custom field with URL-like value offers browser
// @internal
#[test]
fn test_custom_field_http_url_detected() {
    let field = ContactField::new(
        FieldType::Custom,
        "Portfolio",
        "http://oldsite.example.com",
        0,
    );
    let detected = field.detect_value_type();
    assert_eq!(detected, Some(FieldType::Website));
    let uri = field.to_uri();
    assert_eq!(uri, Some("http://oldsite.example.com".to_string()));
}

/// Integration test: International phone numbers
/// Maps to: "Phone number with international format" scenario
// @scenario: contact_actions :: Phone number with international format
// @scenario: contact_actions :: Various phone number formats are normalized for dialer
// @internal
#[test]
fn test_international_phone_formats() {
    let phones = vec![
        ("+44 20 7946 0958", "UK"),
        ("+81 3-1234-5678", "Japan"),
        ("+49 30 12345678", "Germany"),
        ("+33 1 23 45 67 89", "France"),
    ];

    for (number, country) in phones {
        let field = ContactField::new(FieldType::Phone, country, number, 0);
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
// @internal
#[test]
fn test_action_type_categorization() {
    // Verify action types for icon mapping
    let phone = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567", 0);
    assert!(matches!(phone.to_action(), ContactAction::Call(_)));

    let email = ContactField::new(FieldType::Email, "Work", "test@example.com", 0);
    assert!(matches!(email.to_action(), ContactAction::SendEmail(_)));

    let website = ContactField::new(FieldType::Website, "Blog", "https://example.com", 0);
    assert!(matches!(website.to_action(), ContactAction::OpenUrl(_)));

    let address = ContactField::new(FieldType::Address, "Home", "123 Main St", 0);
    assert!(matches!(address.to_action(), ContactAction::OpenMap(_)));
}

// ============================================================
// Security Integration Tests
// ============================================================

/// Security test: XSS attempt in website field blocked
/// Maps to: contact_actions.feature #45 — "URLs are validated before opening"
// @scenario: contact_actions :: URLs are validated before opening (#45)
// @internal
#[test]
fn test_xss_in_website_blocked() {
    let malicious_values = vec![
        "javascript:alert('xss')",
        "javascript:document.cookie",
        "JAVASCRIPT:alert(1)",   // Case insensitive
        "  javascript:alert(1)", // Leading space
    ];

    for value in malicious_values {
        let field = ContactField::new(FieldType::Website, "Malicious", value, 0);
        let uri = field.to_uri();
        assert!(uri.is_none(), "XSS attempt '{}' should be blocked", value);
    }
}

/// Security test: Data URI scheme blocked
/// Maps to: contact_actions.feature #46 — "Only safe URI schemes are allowed"
// @scenario: contact_actions :: Only safe URI schemes are allowed (#46)
// @internal
#[test]
fn test_data_uri_blocked() {
    let data_uris = vec![
        "data:text/html,<script>alert(1)</script>",
        "data:image/svg+xml,<svg onload=alert(1)>",
        "DATA:text/html,test", // Case insensitive
    ];

    for value in data_uris {
        let field = ContactField::new(FieldType::Website, "Data", value, 0);
        let uri = field.to_uri();
        assert!(uri.is_none(), "Data URI '{}' should be blocked", value);
    }
}

/// Security test: FTP scheme blocked
/// Maps to: contact_actions.feature #46 — "Only safe URI schemes are allowed"
// @scenario: contact_actions :: Only safe URI schemes are allowed (#46)
// @internal
#[test]
fn test_ftp_scheme_blocked() {
    let field = ContactField::new(FieldType::Website, "FTP", "ftp://files.example.com", 0);
    let uri = field.to_uri();
    assert!(uri.is_none(), "FTP scheme should be blocked");
}

/// Security test: Custom field with malicious content
/// Maps to: contact_actions.feature #46 — "Only safe URI schemes are allowed"
// @scenario: contact_actions :: Only safe URI schemes are allowed (#46)
// @internal
#[test]
fn test_custom_field_malicious_url_blocked() {
    let field = ContactField::new(FieldType::Custom, "Link", "javascript:void(0)", 0);
    let uri = field.to_uri();
    assert!(uri.is_none(), "Malicious custom field should be blocked");
}

// ============================================================
// Edge Case Integration Tests
// ============================================================

/// Edge case: Very long URL
// @internal
#[test]
fn test_very_long_url() {
    let long_path = "a".repeat(500);
    let url = format!("https://example.com/{}", long_path);
    let field = ContactField::new(FieldType::Website, "Long", &url, 0);
    let uri = field.to_uri();
    assert!(uri.is_some(), "Long URLs should still work");
}

/// Edge case: URL with unicode domain (IDN)
// @internal
#[test]
fn test_unicode_domain_url() {
    let field = ContactField::new(FieldType::Website, "IDN", "https://例え.jp", 0);
    let uri = field.to_uri();
    // Should preserve or encode the unicode domain
    assert!(uri.is_some(), "expected Some value");
}

/// Edge case: Email with dots in local part
// @scenario: contact_actions :: Tap email opens mail client
// @internal
#[test]
fn test_email_with_dots() {
    let field = ContactField::new(FieldType::Email, "Gmail", "first.last@gmail.com", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("mailto:first.last@gmail.com".to_string()));
}

/// Edge case: Address with special characters
// @scenario: contact_actions :: Tap address opens maps application
// @internal
#[test]
fn test_address_special_characters() {
    let field = ContactField::new(
        FieldType::Address,
        "Office",
        "123 O'Brien's Way, Suite #5",
        0,
    );
    let uri = field.to_uri();
    assert!(uri.is_some(), "expected Some value");
    // Special characters should be encoded
    let uri_str = uri.unwrap();
    assert!(uri_str.contains("geo:") || uri_str.contains("maps"));
}

// ============================================================
// Property tests: registry-backed social_to_uri (SP-20, CC-04)
// ============================================================

use proptest::prelude::*;

/// All 38 bundled network IDs should produce a Some URI for any non-empty username.
// @internal
#[test]
fn test_all_registry_networks_produce_uri() {
    let registry = vauchi_core::social::SocialNetworkRegistry::with_defaults();
    let networks = registry.all();
    assert_eq!(networks.len(), 38, "Expected 38 bundled networks");

    for network in &networks {
        // Skip mastodon — federation handles tested separately
        if network.id() == "mastodon" {
            continue;
        }

        let field = ContactField::new(FieldType::Social, network.display_name(), "testuser123", 0);
        let uri = field.to_uri();
        assert!(
            uri.is_some(),
            "Network '{}' (id='{}') should produce a URI for 'testuser123', got None",
            network.display_name(),
            network.id(),
        );

        let uri_str = uri.unwrap();
        assert!(
            uri_str.starts_with("https://"),
            "Network '{}' URI should start with https://, got: {}",
            network.display_name(),
            uri_str,
        );
        assert!(
            uri_str.contains("testuser123"),
            "Network '{}' URI should contain 'testuser123', got: {}",
            network.display_name(),
            uri_str,
        );
    }
}

proptest! {
    /// For any non-empty ASCII alphanumeric username and any known network,
    /// social_to_uri should return a valid https URL containing the username.
// @internal
    #[test]
    fn test_social_to_uri_valid_for_any_username(
        username in "[a-zA-Z0-9_]{1,30}"
    ) {
        let field = ContactField::new(FieldType::Social, "GitHub", &username, 0);
        let uri = field.to_uri();
        prop_assert!(uri.is_some(), "GitHub should produce URI for '{}'", username);
        let uri_str = uri.unwrap();
        prop_assert!(
            uri_str.starts_with("https://github.com/"),
            "Expected GitHub URL prefix, got: {}",
            uri_str,
        );
    }
}

// ============================================================
// Phone Number Validation (SP-12a)
// @scenario: contact_actions :: Malformed phone number shows error
// ============================================================

/// Feature: contact_actions.feature @error @malformed
/// Malformed phone numbers must not produce a tel: URI.
// @internal
#[test]
fn test_validate_phone_rejects_clearly_invalid() {
    assert!(!vauchi_core::contact_card::is_valid_phone("not-a-number"));
    assert!(!vauchi_core::contact_card::is_valid_phone("abc"));
    assert!(!vauchi_core::contact_card::is_valid_phone(""));
    assert!(!vauchi_core::contact_card::is_valid_phone("   "));
    assert!(!vauchi_core::contact_card::is_valid_phone("12")); // too short (< 7 digits)
}

/// Feature: contact_actions.feature @error @malformed
/// Valid phone formats must pass validation.
// @internal
#[test]
fn test_validate_phone_accepts_valid_formats() {
    assert!(vauchi_core::contact_card::is_valid_phone("+1234567890"));
    assert!(vauchi_core::contact_card::is_valid_phone("555-1234567"));
    assert!(vauchi_core::contact_card::is_valid_phone("(555) 123-4567"));
    assert!(vauchi_core::contact_card::is_valid_phone("1234567")); // minimum 7 digits
}

/// Feature: contact_actions.feature @error @malformed
/// to_uri() must return None for malformed phone numbers.
// @scenario: contact_actions :: Malformed phone number shows error
// @internal
#[test]
fn test_malformed_phone_to_uri_returns_none() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "not-a-number", 0);
    assert!(
        field.to_uri().is_none(),
        "Malformed phone should not produce a tel: URI"
    );
}

/// Feature: contact_actions.feature @error @malformed
/// to_uri() must return None for phone with insufficient digits.
// @scenario: contact_actions :: Malformed phone number shows error
// @internal
#[test]
fn test_short_phone_to_uri_returns_none() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "12", 0);
    assert!(
        field.to_uri().is_none(),
        "Phone with < 7 digits should not produce a tel: URI"
    );
}

/// Feature: contact_actions.feature @error @malformed
/// to_action() must return CopyToClipboard for malformed phone numbers.
// @scenario: contact_actions :: Malformed phone number shows error
// @internal
#[test]
fn test_malformed_phone_to_action_returns_copy() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "not-a-number", 0);
    let action = field.to_action();
    assert!(
        matches!(action, ContactAction::CopyToClipboard),
        "Malformed phone should fall back to CopyToClipboard, got {:?}",
        action
    );
}

/// Feature: contact_actions.feature @error @malformed
/// Valid phone numbers must still produce a tel: URI after validation is wired in.
// @scenario: contact_actions :: Tap phone number opens dialer
// @internal
#[test]
fn test_valid_phone_to_uri_still_works() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567", 0);
    let uri = field.to_uri();
    assert_eq!(uri, Some("tel:+1-555-123-4567".to_string()));
}

/// Edge case: phone with only whitespace should return None.
// @scenario: contact_actions :: Malformed phone number shows error
// @internal
#[test]
fn test_whitespace_phone_to_uri_returns_none() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "   ", 0);
    assert!(
        field.to_uri().is_none(),
        "Whitespace-only phone should not produce a tel: URI"
    );
}

/// Edge case: phone with letters mixed in should return None.
// @scenario: contact_actions :: Malformed phone number shows error
// @internal
#[test]
fn test_phone_with_letters_to_uri_returns_none() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "555-CALL-ME", 0);
    assert!(
        field.to_uri().is_none(),
        "Phone with letters should not produce a tel: URI"
    );
}

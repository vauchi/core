//! Test Fixtures
//!
//! Pre-defined test data for common scenarios.

use webbook_core::{ContactCard, ContactField, FieldType};

/// Standard password for backup tests (meets complexity requirements).
pub const TEST_PASSWORD: &str = "SecureP@ssw0rd123!";

/// Weak password for testing rejection.
pub const WEAK_PASSWORD: &str = "password";

/// Maximum fields allowed on a card.
pub const MAX_CARD_FIELDS: usize = 25;

/// Create a sample contact card with common fields.
pub fn sample_contact_card(name: &str) -> ContactCard {
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        &format!("{}@example.com", name.to_lowercase()),
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+15551234567",
    ))
    .unwrap();
    card
}

/// Create a card with the maximum number of fields.
pub fn max_fields_card(name: &str) -> ContactCard {
    let mut card = ContactCard::new(name);
    for i in 0..MAX_CARD_FIELDS {
        card.add_field(ContactField::new(
            FieldType::Custom,
            &format!("field_{}", i),
            &format!("value_{}", i),
        ))
        .unwrap();
    }
    card
}

/// Create a card with various field types.
pub fn diverse_fields_card(name: &str) -> ContactCard {
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(
        FieldType::Email,
        "personal",
        "user@personal.com",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "user@company.com",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+15551234567",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Phone,
        "home",
        "+15559876543",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Website,
        "blog",
        "https://example.com",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Social,
        "twitter",
        "@username",
    ))
    .unwrap();
    card.add_field(ContactField::new(
        FieldType::Address,
        "home",
        "123 Main St, City, ST 12345",
    ))
    .unwrap();
    card
}

/// Unicode test strings for internationalization testing.
pub mod unicode {
    /// Japanese text.
    pub const JAPANESE: &str = "田中太郎";
    /// Arabic text (RTL).
    pub const ARABIC: &str = "مرحبا بالعالم";
    /// Emoji string.
    pub const EMOJI: &str = "👋🌍🎉";
    /// Mixed script text.
    pub const MIXED: &str = "Hello 世界 مرحبا";
    /// Long unicode string.
    pub const LONG_UNICODE: &str = "日本語テキストは長くなることがあります。これは例です。";
}

/// Edge case values for boundary testing.
pub mod edge_cases {
    /// Empty string.
    pub const EMPTY: &str = "";
    /// Single character.
    pub const SINGLE_CHAR: &str = "X";
    /// Maximum reasonable length string (255 chars).
    pub fn max_length_string() -> String {
        "X".repeat(255)
    }
    /// String with special characters.
    pub const SPECIAL_CHARS: &str = "!@#$%^&*()[]{}|;':\",./<>?`~";
    /// String with newlines.
    pub const WITH_NEWLINES: &str = "line1\nline2\nline3";
    /// String with tabs.
    pub const WITH_TABS: &str = "col1\tcol2\tcol3";
}

/// Sample hex-encoded public keys for testing.
pub mod keys {
    /// 32-byte zero key (hex).
    pub const ZERO_KEY_HEX: &str =
        "0000000000000000000000000000000000000000000000000000000000000000";
    /// 32-byte ones key (hex).
    pub const ONES_KEY_HEX: &str =
        "0101010101010101010101010101010101010101010101010101010101010101";
    /// Sample key bytes.
    pub const SAMPLE_KEY: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20,
    ];
}

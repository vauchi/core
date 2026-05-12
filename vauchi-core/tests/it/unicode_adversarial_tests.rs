// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial Unicode property-based tests (Audit T2-6).
//!
//! Verifies that `ContactCard` and `normalize_text` handle hostile,
//! exotic, and edge-case Unicode correctly: CJK, Arabic, Devanagari,
//! Thai, emoji/ZWJ, homoglyphs, combining characters, zero-width
//! codepoints, bidi overrides, null bytes, and max-length multi-byte
//! strings.

use proptest::prelude::*;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::text::normalize_text;

// ============================================================
// Strategies
// ============================================================

/// CJK Unified Ideographs (U+4E00..U+9FFF).
fn cjk_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[\u{4E00}-\u{9FFF}]{1,50}").unwrap()
}

/// Arabic script (U+0600..U+06FF).
fn arabic_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[\u{0600}-\u{06FF}]{1,50}").unwrap()
}

/// Devanagari script (U+0900..U+097F).
fn devanagari_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[\u{0900}-\u{097F}]{1,50}").unwrap()
}

/// Thai script (U+0E01..U+0E3A, U+0E40..U+0E4E).
fn thai_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[\u{0E01}-\u{0E3A}\u{0E40}-\u{0E4E}]{1,50}").unwrap()
}

/// Mixed Unicode letters + digits.
fn mixed_unicode_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[\\p{L}\\p{N}]{1,80}").unwrap()
}

// ============================================================
// 1. Normalization roundtrip — script-specific
// ============================================================

proptest! {
    /// CJK text survives normalize → serialize → deserialize → normalize.
    // @internal
    #[test]
    fn prop_cjk_normalization_roundtrip(name in cjk_strategy()) {
        let normalized = normalize_text(&name);
        let card = ContactCard::new(&name);
        let json = serde_json::to_string(&card).unwrap();
        let restored: ContactCard = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored.display_name(), normalized);
    }

    /// Arabic text survives normalize → serialize → deserialize → normalize.
    // @internal
    #[test]
    fn prop_arabic_normalization_roundtrip(name in arabic_strategy()) {
        let normalized = normalize_text(&name);
        let card = ContactCard::new(&name);
        let json = serde_json::to_string(&card).unwrap();
        let restored: ContactCard = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored.display_name(), normalized);
    }

    /// Devanagari text survives normalize → serialize → deserialize → normalize.
    // @internal
    #[test]
    fn prop_devanagari_normalization_roundtrip(name in devanagari_strategy()) {
        let normalized = normalize_text(&name);
        let card = ContactCard::new(&name);
        let json = serde_json::to_string(&card).unwrap();
        let restored: ContactCard = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored.display_name(), normalized);
    }

    /// Thai text survives normalize → serialize → deserialize → normalize.
    // @internal
    #[test]
    fn prop_thai_normalization_roundtrip(name in thai_strategy()) {
        let normalized = normalize_text(&name);
        let card = ContactCard::new(&name);
        let json = serde_json::to_string(&card).unwrap();
        let restored: ContactCard = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored.display_name(), normalized);
    }

    /// Mixed Unicode letters survive roundtrip.
    // @internal
    #[test]
    fn prop_mixed_unicode_normalization_roundtrip(name in mixed_unicode_strategy()) {
        let normalized = normalize_text(&name);
        prop_assume!(!normalized.is_empty());
        let card = ContactCard::new(&name);
        let json = serde_json::to_string(&card).unwrap();
        let restored: ContactCard = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored.display_name(), normalized);
    }
}

// ============================================================
// 2. Emoji & ZWJ sequences
// ============================================================

// @internal
#[test]
fn test_family_emoji_zwj_roundtrip() {
    // Family: man + ZWJ + woman + ZWJ + girl + ZWJ + boy
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
    let card = ContactCard::new(family);
    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.display_name(), normalize_text(family));
}

// @internal
#[test]
fn test_flag_sequence_roundtrip() {
    // Swiss flag: Regional Indicator C + H
    let flag = "\u{1F1E8}\u{1F1ED}";
    let card = ContactCard::new(flag);
    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.display_name(), normalize_text(flag));
}

// @internal
#[test]
fn test_skin_tone_modifier_roundtrip() {
    // Waving hand + medium skin tone
    let emoji = "\u{1F44B}\u{1F3FD}";
    let card = ContactCard::new(emoji);
    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.display_name(), normalize_text(emoji));
}

// @internal
#[test]
fn test_emoji_in_field_value_roundtrip() {
    let emoji_value = "Call me \u{1F4DE} or \u{1F4E7}";
    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Custom, "note", emoji_value, 0);
    card.add_field(field).unwrap();

    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    let restored_field = restored.fields().first().unwrap();
    assert_eq!(restored_field.value(), normalize_text(emoji_value));
}

// ============================================================
// 3. Homoglyph confusion — cross-script
// ============================================================

// @internal
#[test]
fn test_latin_vs_cyrillic_homoglyphs_differ() {
    let latin_a = "a"; // U+0061
    let cyrillic_a = "\u{0430}"; // U+0430

    // NFC does NOT merge cross-script homoglyphs — they must remain distinct.
    let norm_latin = normalize_text(latin_a);
    let norm_cyrillic = normalize_text(cyrillic_a);
    assert_ne!(norm_latin, norm_cyrillic);
}

// @internal
#[test]
fn test_latin_vs_cyrillic_display_names_differ() {
    let card_latin = ContactCard::new("Alice");
    // Cyrillic lookalike: А (U+0410) l i c e
    let card_cyrillic = ContactCard::new("\u{0410}lice");

    assert_ne!(
        card_latin.display_name(),
        card_cyrillic.display_name(),
        "Latin and Cyrillic lookalikes must produce different display names"
    );
}

// @internal
#[test]
fn test_greek_vs_latin_omicron_differ() {
    let latin_o = "o"; // U+006F
    let greek_o = "\u{03BF}"; // U+03BF Greek small letter omicron

    assert_ne!(normalize_text(latin_o), normalize_text(greek_o));
}

// ============================================================
// 4. Combining characters — NFC composition
// ============================================================

// @internal
#[test]
fn test_combining_acute_normalizes_to_precomposed() {
    let decomposed = "e\u{0301}"; // e + combining acute
    let precomposed = "\u{00E9}"; // é

    assert_eq!(
        normalize_text(decomposed),
        normalize_text(precomposed),
        "NFC must compose e + U+0301 into U+00E9"
    );
}

// @internal
#[test]
fn test_combining_diaeresis_normalizes() {
    let decomposed = "u\u{0308}"; // u + combining diaeresis
    let precomposed = "\u{00FC}"; // ü

    assert_eq!(normalize_text(decomposed), normalize_text(precomposed));
}

// @internal
#[test]
fn test_multiple_combining_marks() {
    // a + combining ring above + combining acute = normalization to NFC
    let stacked = "a\u{030A}\u{0301}";
    let once = normalize_text(stacked);
    let twice = normalize_text(&once);
    assert_eq!(once, twice, "NFC normalization must be idempotent");
}

// @internal
#[test]
fn test_combining_chars_in_card_roundtrip() {
    let nfd_name = "Jose\u{0301}"; // NFD form
    let card = ContactCard::new(nfd_name);
    // ContactCard::new normalizes to NFC
    assert_eq!(card.display_name(), "Jos\u{00E9}");

    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.display_name(), "Jos\u{00E9}");
}

// ============================================================
// 5. Zero-width characters — preserved by NFC
// ============================================================

// @internal
#[test]
fn test_zwj_preserved_in_normalization() {
    let with_zwj = "test\u{200D}name"; // ZWJ between words
    let normalized = normalize_text(with_zwj);
    assert!(
        normalized.contains('\u{200D}'),
        "ZWJ (U+200D) must be preserved by NFC normalization"
    );
}

// @internal
#[test]
fn test_zwnj_preserved_in_normalization() {
    let with_zwnj = "test\u{200C}name"; // ZWNJ
    let normalized = normalize_text(with_zwnj);
    assert!(
        normalized.contains('\u{200C}'),
        "ZWNJ (U+200C) must be preserved by NFC normalization"
    );
}

// @internal
#[test]
fn test_zws_preserved_in_normalization() {
    let with_zws = "test\u{200B}name"; // Zero-width space
    let normalized = normalize_text(with_zws);
    assert!(
        normalized.contains('\u{200B}'),
        "ZWS (U+200B) must be preserved by NFC normalization"
    );
}

// @internal
#[test]
fn test_zero_width_chars_survive_card_roundtrip() {
    let name = "A\u{200D}B\u{200C}C\u{200B}D";
    let card = ContactCard::new(name);
    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.display_name(), normalize_text(name));
    assert!(restored.display_name().contains('\u{200D}'));
    assert!(restored.display_name().contains('\u{200C}'));
    assert!(restored.display_name().contains('\u{200B}'));
}

// ============================================================
// 6. Bidi overrides — survive roundtrip
// ============================================================

// @internal
#[test]
fn test_rlo_survives_roundtrip() {
    // Right-to-Left Override (U+202E) — security-sensitive for display names
    let name = "normal\u{202E}desrever";
    let card = ContactCard::new(name);
    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.display_name(), normalize_text(name));
    assert!(
        restored.display_name().contains('\u{202E}'),
        "RLO (U+202E) survives roundtrip — frontends must handle bidi display"
    );
}

// @internal
#[test]
fn test_lro_survives_roundtrip() {
    // Left-to-Right Override (U+202D)
    let name = "test\u{202D}override";
    let card = ContactCard::new(name);
    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.display_name(), normalize_text(name));
}

// @internal
#[test]
fn test_bidi_in_field_value() {
    let value = "\u{202E}reversed text\u{202C}"; // RLO + PDF (Pop Directional Format)
    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Custom, "bidi", value, 0);
    card.add_field(field).unwrap();

    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    let restored_field = restored.fields().first().unwrap();
    assert_eq!(restored_field.value(), normalize_text(value));
}

// ============================================================
// 7. Null bytes
// ============================================================

// @internal
#[test]
fn test_null_byte_in_display_name() {
    let name_with_null = "Alice\0Bob";
    let card = ContactCard::new(name_with_null);
    // The null byte is valid Unicode (U+0000) — NFC preserves it.
    // Verify it survives JSON roundtrip (serde_json handles \u0000).
    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.display_name(), normalize_text(name_with_null));
}

// @internal
#[test]
fn test_null_byte_in_field_value() {
    let value = "before\0after";
    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Custom, "nulltest", value, 0);
    card.add_field(field).unwrap();

    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    let restored_field = restored.fields().first().unwrap();
    assert_eq!(restored_field.value(), normalize_text(value));
}

// @internal
#[test]
fn test_only_null_bytes() {
    let nulls = "\0\0\0";
    let card = ContactCard::new(nulls);
    let json = serde_json::to_string(&card).unwrap();
    let restored: ContactCard = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.display_name(), normalize_text(nulls));
}

// ============================================================
// 8. Max-length edge — byte vs char count
// ============================================================

// @internal
#[test]
fn test_max_length_multibyte_field_value() {
    // CJK characters are 3 bytes each in UTF-8.
    // 333 CJK chars = 999 bytes (under 1000 byte limit).
    let cjk_value: String = std::iter::repeat('\u{4E00}').take(333).collect();
    assert_eq!(cjk_value.len(), 999); // byte length

    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Custom, "cjk", &cjk_value, 0);
    let result = card.add_field(field);
    assert!(
        result.is_ok(),
        "333 CJK chars (999 bytes) should be under the 1000-byte limit"
    );
}

// @internal
#[test]
fn test_max_length_multibyte_over_limit() {
    // 334 CJK chars = 1002 bytes (over 1000 byte limit).
    let cjk_value: String = std::iter::repeat('\u{4E00}').take(334).collect();
    assert_eq!(cjk_value.len(), 1002); // byte length

    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Custom, "cjk", &cjk_value, 0);
    let result = card.add_field(field);
    assert!(
        result.is_err(),
        "334 CJK chars (1002 bytes) must exceed the 1000-byte limit"
    );
}

// @internal
#[test]
fn test_max_length_4byte_chars() {
    // Emoji characters are 4 bytes each in UTF-8.
    // 250 emoji = 1000 bytes (at the limit).
    let emoji_value: String = std::iter::repeat('\u{1F600}').take(250).collect();
    assert_eq!(emoji_value.len(), 1000); // exactly at byte limit

    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Custom, "emoji", &emoji_value, 0);
    let result = card.add_field(field);
    assert!(
        result.is_ok(),
        "250 emoji (1000 bytes exactly) should be at the byte limit"
    );
}

// @internal
#[test]
fn test_max_length_4byte_chars_over() {
    // 251 emoji = 1004 bytes (over limit).
    let emoji_value: String = std::iter::repeat('\u{1F600}').take(251).collect();
    assert_eq!(emoji_value.len(), 1004);

    let mut card = ContactCard::new("Test");
    let field = ContactField::new(FieldType::Custom, "emoji", &emoji_value, 0);
    let result = card.add_field(field);
    assert!(
        result.is_err(),
        "251 emoji (1004 bytes) must exceed the 1000-byte limit"
    );
}

// ============================================================
// 9. Serialization roundtrip proptest — arbitrary Unicode
// ============================================================

proptest! {
    /// Arbitrary Unicode strings survive ContactCard → JSON → ContactCard.
    // @internal
    #[test]
    fn prop_arbitrary_unicode_card_roundtrip(
        name in mixed_unicode_strategy(),
        label in "[a-z]{1,20}",
        value in mixed_unicode_strategy(),
    ) {
        let normalized_name = normalize_text(&name);
        prop_assume!(!normalized_name.is_empty());

        let mut card = ContactCard::new(&name);
        let normalized_value = normalize_text(&value);
        prop_assume!(normalized_value.len() <= 1000);
        let field = ContactField::new(FieldType::Custom, &label, &value, 0);
        card.add_field(field).unwrap();

        let json = serde_json::to_string(&card).unwrap();
        let restored: ContactCard = serde_json::from_str(&json).unwrap();

        prop_assert_eq!(restored.display_name(), normalized_name.as_str());
        prop_assert_eq!(restored.fields().len(), 1);
        prop_assert_eq!(restored.fields()[0].value(), normalized_value.as_str());
    }

    /// Normalization is idempotent for all Unicode scripts.
    // @internal
    #[test]
    fn prop_normalization_idempotent_all_scripts(
        s in prop_oneof![
            cjk_strategy(),
            arabic_strategy(),
            devanagari_strategy(),
            thai_strategy(),
            mixed_unicode_strategy(),
        ]
    ) {
        let once = normalize_text(&s);
        let twice = normalize_text(&once);
        prop_assert_eq!(&once, &twice, "NFC normalization must be idempotent");
    }

    /// CJK field values survive ContactCard roundtrip.
    // @internal
    #[test]
    fn prop_cjk_field_value_roundtrip(value in cjk_strategy()) {
        let normalized = normalize_text(&value);
        prop_assume!(normalized.len() <= 1000);

        let mut card = ContactCard::new("Test");
        let field = ContactField::new(FieldType::Custom, "cjk", &value, 0);
        card.add_field(field).unwrap();

        let json = serde_json::to_string(&card).unwrap();
        let restored: ContactCard = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored.fields()[0].value(), normalized.as_str());
    }

    /// Arabic field values survive ContactCard roundtrip.
    // @internal
    #[test]
    fn prop_arabic_field_value_roundtrip(value in arabic_strategy()) {
        let normalized = normalize_text(&value);
        prop_assume!(normalized.len() <= 1000);

        let mut card = ContactCard::new("Test");
        let field = ContactField::new(FieldType::Custom, "arabic", &value, 0);
        card.add_field(field).unwrap();

        let json = serde_json::to_string(&card).unwrap();
        let restored: ContactCard = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(restored.fields()[0].value(), normalized.as_str());
    }
}

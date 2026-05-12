// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact_card::field
//! Extracted from field.rs

use vauchi_core::contact_card::*;

// @scenario: contact_card_management :: Add field to contact card
// @internal
#[test]
fn test_create_field() {
    let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-1234", 0);
    assert_eq!(field.field_type(), FieldType::Phone);
    assert_eq!(field.label(), "Mobile");
    assert_eq!(field.value(), "+1-555-1234");
}

// @scenario: field_validation :: Valid phone number formats
// @internal
#[test]
fn test_validate_valid_phone() {
    let field = ContactField::new(FieldType::Phone, "Test", "+1-555-123-4567", 0);
    field.validate().expect("expected success");
}

// @scenario: field_validation :: Valid email address formats
// @internal
#[test]
fn test_validate_valid_email() {
    let field = ContactField::new(FieldType::Email, "Test", "test@example.com", 0);
    field.validate().expect("expected success");
}

// @scenario: unicode_normalization :: Field label NFC normalization
// @internal
#[test]
fn test_field_label_normalized_nfc() {
    let field = ContactField::new(FieldType::Phone, "Te\u{0301}le\u{0301}phone", "+41", 0);
    assert_eq!(field.label(), "T\u{00E9}l\u{00E9}phone");
}

// @scenario: unicode_normalization :: Field value NFC normalization
// @internal
#[test]
fn test_field_value_normalized_nfc() {
    let mut field = ContactField::new(FieldType::Custom, "Note", "cafe\u{0301}", 0);
    assert_eq!(field.value(), "caf\u{00E9}");
    field.set_value("n\u{0303}", 0);
    assert_eq!(field.value(), "\u{00F1}");
}

// --- note field tests ---

// @internal
#[test]
fn test_field_note_default_none() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41 79 123 45 67", 0);
    assert_eq!(f.note(), None);
}

// @internal
#[test]
fn test_field_with_note() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41 79 123 45 67", 0)
        .with_note("check spam".to_string());
    assert_eq!(f.note(), Some("check spam"));
}

// @internal
#[test]
fn test_field_note_truncated_at_500_chars() {
    let long_note = "x".repeat(600);
    let f = ContactField::new(FieldType::Phone, "Work", "+41...", 0).with_note(long_note);
    assert_eq!(f.note().unwrap().chars().count(), 500);
}

// @internal
#[test]
fn test_field_empty_note_is_none() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41...", 0).with_note("".to_string());
    assert_eq!(f.note(), None);
}

// @internal
#[test]
fn test_strip_private_removes_note() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41 79 123 45 67", 0)
        .with_note("secret".to_string());
    let stripped = f.strip_private();
    assert_eq!(stripped.note(), None);
    assert_eq!(stripped.value(), f.value());
    assert_eq!(stripped.label(), f.label());
    assert_eq!(stripped.id(), f.id());
}

// @internal
#[test]
fn test_strip_private_on_field_without_note() {
    let f = ContactField::new(FieldType::Phone, "Work", "+41...", 0);
    let stripped = f.strip_private();
    assert_eq!(stripped.note(), None);
    assert_eq!(stripped.value(), f.value());
}

// @internal
#[test]
fn test_note_serde_roundtrip() {
    let f =
        ContactField::new(FieldType::Phone, "Work", "+41...", 0).with_note("my note".to_string());
    let json = serde_json::to_string(&f).unwrap();
    let restored: ContactField = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.note(), Some("my note"));
}

// @internal
#[test]
fn test_note_backward_compat_deserialize() {
    // Old JSON without note field should deserialize fine
    let json =
        r#"{"id":"abc","field_type":"Phone","label":"Work","value":"+41...","updated_at":0}"#;
    let f: ContactField = serde_json::from_str(json).unwrap();
    assert_eq!(f.note(), None);
}

// @internal
#[test]
fn test_field_note_truncated_multibyte_utf8() {
    // 600 CJK characters (3 bytes each = 1800 bytes) should truncate to 500 characters
    let cjk_note: String = "\u{4e16}".repeat(600); // 世
    assert_eq!(cjk_note.chars().count(), 600);
    let f = ContactField::new(FieldType::Custom, "Note", "val", 0).with_note(cjk_note);
    let note = f.note().unwrap();
    assert_eq!(note.chars().count(), 500);
    // All characters should be the same CJK character
    assert!(note.chars().all(|c| c == '\u{4e16}'));
}

// @scenario: contact_card_management :: Field type alias resolution
// @internal
#[test]
fn test_field_type_from_alias_phone() {
    for alias in &["phone", "Phone", "tel", "TEL", "telephone"] {
        let (ft, label) = FieldType::from_alias(alias).unwrap();
        assert_eq!(ft, FieldType::Phone);
        assert!(label.is_none());
    }
}

// @scenario: contact_card_management :: Field type alias resolution
// @internal
#[test]
fn test_field_type_from_alias_email() {
    for alias in &["email", "mail", "EMAIL"] {
        let (ft, label) = FieldType::from_alias(alias).unwrap();
        assert_eq!(ft, FieldType::Email);
        assert!(label.is_none());
    }
}

// @scenario: contact_card_management :: Field type alias resolution
// @internal
#[test]
fn test_field_type_from_alias_social_with_label() {
    let (ft, label) = FieldType::from_alias("twitter").unwrap();
    assert_eq!(ft, FieldType::Social);
    assert_eq!(label.as_deref(), Some("Twitter"));

    let (ft, label) = FieldType::from_alias("Instagram").unwrap();
    assert_eq!(ft, FieldType::Social);
    assert_eq!(label.as_deref(), Some("Instagram"));

    let (ft, label) = FieldType::from_alias("linkedin").unwrap();
    assert_eq!(ft, FieldType::Social);
    assert_eq!(label.as_deref(), Some("LinkedIn"));

    let (ft, label) = FieldType::from_alias("github").unwrap();
    assert_eq!(ft, FieldType::Social);
    assert_eq!(label.as_deref(), Some("GitHub"));
}

// @scenario: contact_card_management :: Field type alias resolution
// @internal
#[test]
fn test_field_type_from_alias_generic_social() {
    let (ft, label) = FieldType::from_alias("social").unwrap();
    assert_eq!(ft, FieldType::Social);
    assert!(label.is_none());
}

// @scenario: contact_card_management :: Field type alias resolution
// @internal
#[test]
fn test_field_type_from_alias_unknown_returns_none() {
    assert!(FieldType::from_alias("fax").is_none());
    assert!(FieldType::from_alias("").is_none());
    assert!(FieldType::from_alias("unknown").is_none());
}

// @scenario: contact_card_management :: Field type categorization
// @internal
#[test]
fn test_field_type_is_social() {
    assert!(FieldType::Social.is_social());
    assert!(!FieldType::Phone.is_social());
    assert!(!FieldType::Email.is_social());
    assert!(!FieldType::Custom.is_social());
}

// =============================================================================
// Mutation-coverage tests: birthday validation, label/note boundaries, aliases
// =============================================================================

use rstest::rstest;

// @scenario: field_validation :: Validate ISO 8601 birthday format
#[rstest]
#[case::canonical("1990-05-15", true)]
#[case::leap_day_div_by_4("2020-02-29", true)]
#[case::not_leap_div_by_100("1900-02-29", false)]
#[case::leap_div_by_400("2000-02-29", true)]
#[case::feb_28_non_leap("2021-02-28", true)]
#[case::feb_29_non_leap("2021-02-29", false)]
#[case::month_zero("1990-00-15", false)]
#[case::month_thirteen("1990-13-15", false)]
#[case::day_zero("1990-05-00", false)]
#[case::day_32_in_31day_month("1990-01-32", false)]
#[case::day_31_in_30day_month("1990-04-31", false)]
#[case::day_31_in_april_invalid("1990-04-31", false)]
#[case::day_30_in_april_valid("1990-04-30", true)]
#[case::day_31_in_july_valid("1990-07-31", true)]
#[case::wrong_separator("1990/05/15", false)]
#[case::short_format("1990-5-15", false)]
#[case::extra_chars("1990-05-15Z", false)]
#[case::empty("", false)]
fn test_validate_birthday_per_input(#[case] input: &str, #[case] expected_valid: bool) {
    let field = ContactField::new(FieldType::Birthday, "DOB", input, 0);
    let actual = field.validate().is_ok();
    assert_eq!(actual, expected_valid, "for {}", input);
}

// @scenario: field_validation :: Birthday days-per-month table
// Pins every month's max-day boundary at once. Kills mutations to the
// `match month` arms (e.g. swapping a 30/31 case).
// @internal
#[rstest]
#[case(1, 31, true)]
#[case(1, 32, false)]
#[case(2, 28, true)] // non-leap
#[case(2, 29, false)] // non-leap year (1990)
#[case(3, 31, true)]
#[case(4, 30, true)]
#[case(4, 31, false)]
#[case(5, 31, true)]
#[case(6, 30, true)]
#[case(6, 31, false)]
#[case(7, 31, true)]
#[case(8, 31, true)]
#[case(9, 30, true)]
#[case(9, 31, false)]
#[case(10, 31, true)]
#[case(11, 30, true)]
#[case(11, 31, false)]
#[case(12, 31, true)]
fn test_validate_birthday_month_max_day_table(
    #[case] month: u8,
    #[case] day: u8,
    #[case] expected: bool,
) {
    let s = format!("1990-{:02}-{:02}", month, day);
    let field = ContactField::new(FieldType::Birthday, "DOB", &s, 0);
    assert_eq!(field.validate().is_ok(), expected, "for {}", s);
}

// @scenario: field_validation :: Field label truncation at MAX_LABEL_LENGTH
#[test]
fn test_set_label_truncates_at_max_length() {
    use vauchi_core::contact_card::field::MAX_LABEL_LENGTH;
    let mut f = ContactField::new(FieldType::Custom, "x", "v", 0);
    let long_label = "a".repeat(MAX_LABEL_LENGTH + 17);
    f.set_label(&long_label);
    // Truncation pins the exact boundary: longer-than-MAX is cut, shorter
    // stays. Catches mutations to the `>` comparison or the `take` count.
    assert_eq!(f.label().chars().count(), MAX_LABEL_LENGTH);

    // Boundary: exactly MAX_LABEL_LENGTH must survive intact.
    let exact = "b".repeat(MAX_LABEL_LENGTH);
    f.set_label(&exact);
    assert_eq!(f.label(), exact);

    // Just under MAX must survive intact.
    let under = "c".repeat(MAX_LABEL_LENGTH - 1);
    f.set_label(&under);
    assert_eq!(f.label(), under);
}

// @scenario: field_validation :: Field note truncation boundary
#[test]
fn test_set_note_truncation_boundary() {
    use vauchi_core::contact_card::field::MAX_FIELD_NOTE_LEN;
    let f = ContactField::new(FieldType::Custom, "x", "v", 0);

    // Exactly MAX must survive intact.
    let exact = "y".repeat(MAX_FIELD_NOTE_LEN);
    let f2 = f.clone().with_note(exact.clone());
    assert_eq!(f2.note().unwrap().chars().count(), MAX_FIELD_NOTE_LEN);
    assert_eq!(f2.note().unwrap(), exact);

    // One over MAX gets truncated.
    let over = "z".repeat(MAX_FIELD_NOTE_LEN + 1);
    let f3 = f.clone().with_note(over);
    assert_eq!(f3.note().unwrap().chars().count(), MAX_FIELD_NOTE_LEN);

    // Empty note is canonicalized to None.
    let f4 = f.clone().with_note(String::new());
    assert_eq!(f4.note(), None);
}

// @scenario: field_validation :: set_value updates timestamp
#[test]
fn test_set_value_updates_timestamp() {
    // Caller-controlled timestamps are stamped verbatim. The prior test
    // relied on `SystemTime::now()` being "real" (` > 1_700_000_000`);
    // with the seam, we assert the exact values instead.
    let mut f = ContactField::new(FieldType::Custom, "x", "old", 1_700_000_000);
    assert_eq!(f.updated_at(), 1_700_000_000);

    f.set_value("new", 1_700_000_100);
    assert_eq!(
        f.updated_at(),
        1_700_000_100,
        "set_value must stamp the passed now"
    );
    assert_eq!(f.value(), "new");
}

// @scenario: contact_card_management :: Field type alias resolution exhaustive
#[rstest]
#[case::phone("phone", FieldType::Phone, None)]
#[case::tel("tel", FieldType::Phone, None)]
#[case::telephone("telephone", FieldType::Phone, None)]
#[case::email("email", FieldType::Email, None)]
#[case::mail("mail", FieldType::Email, None)]
#[case::address("address", FieldType::Address, None)]
#[case::addr("addr", FieldType::Address, None)]
#[case::home("home", FieldType::Address, None)]
#[case::website("website", FieldType::Website, None)]
#[case::web("web", FieldType::Website, None)]
#[case::url("url", FieldType::Website, None)]
#[case::birthday("birthday", FieldType::Birthday, None)]
#[case::bday("bday", FieldType::Birthday, None)]
#[case::dob("dob", FieldType::Birthday, None)]
#[case::social("social", FieldType::Social, None)]
#[case::twitter("twitter", FieldType::Social, Some("Twitter"))]
#[case::x_alias("x", FieldType::Social, Some("Twitter"))]
#[case::instagram("instagram", FieldType::Social, Some("Instagram"))]
#[case::ig("ig", FieldType::Social, Some("Instagram"))]
#[case::linkedin("linkedin", FieldType::Social, Some("LinkedIn"))]
#[case::github("github", FieldType::Social, Some("GitHub"))]
#[case::gh("gh", FieldType::Social, Some("GitHub"))]
#[case::custom("custom", FieldType::Custom, None)]
#[case::other("other", FieldType::Custom, None)]
#[case::note("note", FieldType::Custom, None)]
fn test_field_type_from_alias_table(
    #[case] alias: &str,
    #[case] expected_type: FieldType,
    #[case] expected_label: Option<&'static str>,
) {
    let (ft, label) = FieldType::from_alias(alias).expect("alias must resolve");
    assert_eq!(ft, expected_type);
    assert_eq!(label.as_deref(), expected_label);
}

// @internal
#[test]
fn test_field_type_is_social_per_variant() {
    // Pin one assertion per FieldType variant so a `matches!` mutation
    // (returning true for the wrong arm) gets caught.
    assert!(FieldType::Social.is_social());
    assert!(!FieldType::Phone.is_social());
    assert!(!FieldType::Email.is_social());
    assert!(!FieldType::Address.is_social());
    assert!(!FieldType::Website.is_social());
    assert!(!FieldType::Birthday.is_social());
    assert!(!FieldType::Custom.is_social());
}

// @internal
#[test]
fn test_validate_value_too_long_returns_specific_error() {
    use vauchi_core::contact_card::ValidationError;
    use vauchi_core::contact_card::field::MAX_VALUE_LENGTH;
    // Build a value that exceeds MAX_VALUE_LENGTH bytes.
    let f = ContactField::new(FieldType::Custom, "k", &"x".repeat(MAX_VALUE_LENGTH + 1), 0);
    let err = f.validate().unwrap_err();
    // Pin the exact variant — kills mutations that swap which error is
    // returned for the length-overflow path.
    match err {
        ValidationError::ValueTooLong { max } => assert_eq!(max, MAX_VALUE_LENGTH),
        other => panic!("expected ValueTooLong, got {:?}", other),
    }
}

// =============================================================================
// validate() dispatch table — kill `delete match arm` mutations
// =============================================================================
//
// `validate()` matches on FieldType and dispatches to per-type validators.
// A `delete match arm FieldType::Website` mutation makes Website fall
// through to the catch-all `_ => Ok(())`, silently accepting invalid
// URLs. Asserting that an invalid Website returns Err catches it.
// @internal
#[test]
fn test_validate_dispatches_to_website_validator() {
    let f = ContactField::new(
        FieldType::Website,
        "site",
        "no protocol no dot has space",
        0,
    );
    assert!(
        f.validate().is_err(),
        "Website dispatch must call validate_website (catches deletion of the Website match arm)"
    );
}

// =============================================================================
// validate_phone boundaries — kill `<`/`>` ↔ `<=`/`>=`/`==` mutations
// =============================================================================
// @internal
#[test]
fn test_validate_phone_exactly_seven_digits_is_valid() {
    // The check is `if digit_count < 7 { Err }`. A `< with <=` mutation
    // would reject 7-digit numbers; pinning a 7-digit-valid case kills it.
    let f = ContactField::new(FieldType::Phone, "p", "1234567", 0);
    f.validate().expect("7-digit phone must validate");
}

// @internal
#[test]
fn test_validate_phone_six_digits_is_invalid() {
    // Mirror of the above to keep the < boundary tight on both sides.
    let f = ContactField::new(FieldType::Phone, "p", "123456", 0);
    assert!(f.validate().is_err());
}

// @internal
#[test]
fn test_validate_phone_exactly_thirty_chars_is_valid() {
    // The check is `if value.len() > 30 { Err }`. Mutations to
    // `==` or `>=` would reject a 30-char phone. Build a 30-char
    // value using only allowed phone characters (digits, space,
    // dash, parens, plus — no '.').
    let exactly_30 = "+1 (555) 123-4567 0000 1234567";
    assert_eq!(exactly_30.len(), 30);
    let f = ContactField::new(FieldType::Phone, "p", exactly_30, 0);
    f.validate().expect("30-char phone must validate");
}

// @internal
#[test]
fn test_validate_phone_thirty_one_chars_is_invalid() {
    let v = "+1 (555) 123-4567 0000 12345678"; // 31 chars
    assert_eq!(v.len(), 31);
    let f = ContactField::new(FieldType::Phone, "p", v, 0);
    assert!(f.validate().is_err());
}

// =============================================================================
// validate_website branches — kill || ↔ && and missing-! mutations
// =============================================================================
// @internal
#[test]
fn test_validate_website_http_without_dot_is_valid() {
    // Pins the `starts_with("http://") || starts_with("https://")` branch
    // by using a URL with no dot in the authority. Catches:
    //   - 290:41 `|| with &&`: with &&, the protocol branch never fires
    //     (a string cannot start with both); falls through to `contains('.')`
    //     which is false → Err. Different from orig (Ok).
    //   - 288:9 `-> Ok(())`: trivially caught by the no-dot/has-space test below.
    let f = ContactField::new(FieldType::Website, "w", "http://localhost", 0);
    f.validate().expect("http://localhost must validate");
}

// @internal
#[test]
fn test_validate_website_plain_domain_is_valid() {
    // Pins `value.contains('.') && !value.contains(' ')` by using a
    // plain domain without protocol. Catches `delete !` (293:35): with
    // `contains('.') && contains(' ')` the test fails because there's
    // no space in "example.com".
    let f = ContactField::new(FieldType::Website, "w", "example.com", 0);
    f.validate().expect("plain domain must validate");
}

// @internal
#[test]
fn test_validate_website_dotted_with_space_is_invalid() {
    // Pins `value.contains('.') && !value.contains(' ')` by using a
    // value that contains both a dot AND a space. Catches `&& with ||`
    // (293:32): with ||, dot OR no-space is true → returns Ok.
    let f = ContactField::new(FieldType::Website, "w", "abc.de f", 0);
    assert!(f.validate().is_err());
}

// @internal
#[test]
fn test_validate_website_no_protocol_no_dot_is_invalid() {
    // Catches `validate_website -> Ok(())` (288:9): a body that
    // returns Ok unconditionally would pass this, but the real
    // implementation rejects values without protocol AND without
    // a dot.
    let f = ContactField::new(FieldType::Website, "w", "noproto", 0);
    assert!(f.validate().is_err());
}

// =============================================================================
// validate_birthday format check — kill || ↔ && in separator check
// =============================================================================
// @internal
#[test]
fn test_validate_birthday_wrong_seventh_byte_is_invalid() {
    // The format check is:
    //   if value.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' { Err }
    // A `|| with &&` on the LAST `||` (304:61) makes the check:
    //   len != 10 || (bytes[4] != b'-' && bytes[7] != b'-')
    // For "1990-05/15": len=10, bytes[4]='-', bytes[7]='/'. Orig
    // returns Err (third clause). Mutation: false || (false && true)
    // → false → falls through to parsing, eventually returns Ok.
    let f = ContactField::new(FieldType::Birthday, "b", "1990-05/15", 0);
    assert!(f.validate().is_err());
}

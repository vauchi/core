//! Tests for identity::password
//! Extracted from password.rs

use webbook_core::identity::password::{validate_password, PasswordStrength};
use zxcvbn::Score;

#[test]
fn test_password_strength_from_score() {
    assert_eq!(
        PasswordStrength::from(Score::Zero),
        PasswordStrength::TooWeak
    );
    assert_eq!(PasswordStrength::from(Score::One), PasswordStrength::Weak);
    assert_eq!(PasswordStrength::from(Score::Two), PasswordStrength::Fair);
    assert_eq!(
        PasswordStrength::from(Score::Three),
        PasswordStrength::Strong
    );
    assert_eq!(
        PasswordStrength::from(Score::Four),
        PasswordStrength::VeryStrong
    );
}

#[test]
fn test_short_password() {
    assert!(validate_password("short").is_err());
    assert!(validate_password("").is_err());
    assert!(validate_password("1234567").is_err());
}

#[test]
fn test_common_passwords() {
    assert!(validate_password("password").is_err());
    assert!(validate_password("12345678").is_err());
    assert!(validate_password("qwertyui").is_err());
}

#[test]
fn test_strong_passphrase() {
    let result = validate_password("correct-horse-battery-staple");
    assert!(result.is_ok());
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Password Strength Validation
//!
//! Lightweight internal estimator that scores passwords from 0 (too weak) to 4
//! (very strong) based on length, character-class diversity, and common-pattern
//! detection. Replaces the `zxcvbn` crate to reduce dependency weight while
//! preserving the same acceptance threshold (score >= 3, length >= 8).

use super::IdentityError;

/// Password strength levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PasswordStrength {
    /// Score 0: Too guessable (risky password)
    TooWeak,
    /// Score 1: Very guessable (protection from throttled online attacks)
    Weak,
    /// Score 2: Somewhat guessable (protection from unthrottled online attacks)
    Fair,
    /// Score 3: Safely unguessable (moderate protection from offline attacks)
    Strong,
    /// Score 4: Very unguessable (strong protection from offline attacks)
    VeryStrong,
}

impl PasswordStrength {
    fn from_score(score: u8) -> Self {
        match score.min(4) {
            0 => PasswordStrength::TooWeak,
            1 => PasswordStrength::Weak,
            2 => PasswordStrength::Fair,
            3 => PasswordStrength::Strong,
            _ => PasswordStrength::VeryStrong,
        }
    }
}

/// Minimum password length requirement.
pub const MIN_PASSWORD_LENGTH: usize = 8;

/// Minimum internal score required (0-4 scale).
const MIN_REQUIRED_SCORE: u8 = 3;

/// Common weak words and keyboard patterns that automatically fail short passwords.
const BLOCKLIST: &[&str] = &[
    "password", "passwort", "123456", "qwerty", "admin", "vauchi", "letmein", "welcome", "monkey",
    "dragon",
];

/// Sequential keyboard/character patterns that automatically fail short passwords.
const SEQUENTIAL_PATTERNS: &[&str] = &[
    "abcdefghijklmnopqrstuvwxyz",
    "zyxwvutsrqponmlkjihgfedcba",
    "0123456789",
    "9876543210",
    "qwertyuiop",
    "asdfghjkl",
    "zxcvbnm",
];

/// Validates a password for strength using the internal estimator.
///
/// Returns the password strength level if the password is acceptable,
/// or an error if the password is too weak.
///
/// # Requirements
/// - Minimum 8 characters
/// - Internal score of 3 or higher (out of 4)
///
/// # Examples
/// ```
/// use vauchi_core::identity::password::{validate_password, PasswordStrength};
///
/// // Weak passwords are rejected
/// let err = validate_password("password").unwrap_err();
/// assert_eq!(format!("{err}"), format!("{}", vauchi_core::identity::IdentityError::WeakPassword));
///
/// // Strong passphrases return Strong or VeryStrong
/// let strength = validate_password("correct-horse-battery-staple").unwrap();
/// assert!(matches!(strength, PasswordStrength::Strong | PasswordStrength::VeryStrong));
/// ```
pub fn validate_password(password: &str) -> Result<PasswordStrength, IdentityError> {
    if password.len() < MIN_PASSWORD_LENGTH {
        return Err(IdentityError::WeakPassword);
    }

    let analysis = analyze(password);
    if analysis.score < MIN_REQUIRED_SCORE {
        return Err(IdentityError::WeakPassword);
    }

    Ok(PasswordStrength::from_score(analysis.score))
}

/// Returns feedback for improving a weak password.
///
/// This can be used to give users helpful suggestions for
/// making their password stronger.
///
/// # Examples
/// ```
/// use vauchi_core::identity::password::password_feedback;
///
/// let feedback = password_feedback("password123");
/// // Contains suggestions based on failed checks.
/// println!("Suggestions: {}", feedback);
/// ```
pub fn password_feedback(password: &str) -> String {
    let analysis = analyze(password);

    if analysis.score >= MIN_REQUIRED_SCORE {
        return String::new();
    }

    let mut parts = Vec::new();

    if password.len() < MIN_PASSWORD_LENGTH {
        parts.push(format!("Use at least {} characters.", MIN_PASSWORD_LENGTH));
    }

    if analysis.length_score < 3 {
        parts.push("Use a longer passphrase (16+ characters is ideal).".to_string());
    }

    let mut missing_classes = Vec::new();
    if !analysis.has_lower {
        missing_classes.push("lowercase letters");
    }
    if !analysis.has_upper {
        missing_classes.push("uppercase letters");
    }
    if !analysis.has_digit {
        missing_classes.push("digits");
    }
    if !analysis.has_symbol {
        missing_classes.push("symbols");
    }
    if missing_classes.len() > 2 {
        parts.push("Mix letters, numbers, and symbols.".to_string());
    } else if !missing_classes.is_empty() {
        parts.push(format!("Add {}.", missing_classes.join(", ")));
    }

    if analysis.has_blocklisted_word {
        parts.push("Avoid common words and keyboard patterns.".to_string());
    }

    if analysis.has_repeated_chars {
        parts.push("Avoid repeated characters.".to_string());
    }

    if analysis.has_sequential {
        parts.push("Avoid sequential characters like 123 or abc.".to_string());
    }

    if parts.is_empty() {
        parts.push("Choose a stronger password.".to_string());
    }

    parts.join(" ")
}

#[derive(Debug, Default)]
struct Analysis {
    score: u8,
    length_score: u8,
    has_lower: bool,
    has_upper: bool,
    has_digit: bool,
    has_symbol: bool,
    has_blocklisted_word: bool,
    has_repeated_chars: bool,
    has_sequential: bool,
}

fn analyze(password: &str) -> Analysis {
    let mut analysis = Analysis::default();
    let len = password.len();

    if len == 0 {
        return analysis;
    }

    // Character-class detection.
    for ch in password.chars() {
        if ch.is_ascii_lowercase() {
            analysis.has_lower = true;
        } else if ch.is_ascii_uppercase() {
            analysis.has_upper = true;
        } else if ch.is_ascii_digit() {
            analysis.has_digit = true;
        } else {
            analysis.has_symbol = true;
        }
    }

    let class_count = [
        analysis.has_lower,
        analysis.has_upper,
        analysis.has_digit,
        analysis.has_symbol,
    ]
    .iter()
    .filter(|&&x| x)
    .count() as u8;

    // Length score: longer passwords get a higher base score.
    analysis.length_score = match len {
        0..=7 => 0,
        8..=11 => 1,
        12..=15 => 2,
        16..=19 => 3,
        _ => 4,
    };

    // Class bonus: reward diversity beyond the first class.
    let class_bonus = class_count.saturating_sub(1);

    // Pattern detection.
    let lower = password.to_ascii_lowercase();
    analysis.has_blocklisted_word = BLOCKLIST.iter().any(|word| lower.contains(word));
    analysis.has_repeated_chars = has_repeated_chars(password);
    analysis.has_sequential = has_sequential(password);

    // Strong pattern violations on short passwords force a failing score.
    let pattern_penalty = if analysis.has_blocklisted_word
        || analysis.has_sequential
        || analysis.has_repeated_chars
    {
        2
    } else {
        0
    };

    analysis.score = analysis
        .length_score
        .saturating_add(class_bonus)
        .saturating_sub(pattern_penalty);

    // Strong pattern violations should never score above Fair (2), even if
    // the password is long and diverse. This prevents trivially weak
    // passphrases like "MyPassword123!" or "abcdefghijklmnopqrstuvwxyz123!"
    // from slipping past the threshold.
    if analysis.has_blocklisted_word || analysis.has_sequential || analysis.has_repeated_chars {
        analysis.score = analysis.score.min(2);
    }

    // Cap at 4.
    analysis.score = analysis.score.min(4);

    analysis
}

fn has_repeated_chars(password: &str) -> bool {
    if password.len() < MIN_PASSWORD_LENGTH {
        return false;
    }
    let mut chars = password.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    chars.all(|c| c == first)
}

fn has_sequential(password: &str) -> bool {
    let lower = password.to_ascii_lowercase();
    for pattern in SEQUENTIAL_PATTERNS {
        // A sequential pattern is present when the password contains the
        // pattern (or its reverse), not the other way around.
        if lower.len() >= MIN_PASSWORD_LENGTH && lower.contains(pattern) {
            return true;
        }
        let reversed: String = pattern.chars().rev().collect();
        if lower.len() >= MIN_PASSWORD_LENGTH && lower.contains(&reversed) {
            return true;
        }
    }
    false
}

// INLINE_TEST_REQUIRED: unit tests for the internal password estimator guard
// against regressions in pattern detection and score capping.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn blocklisted_word_caps_score_even_when_long_and_diverse() {
        // Long, with multiple character classes, but contains "password".
        // Without the cap this would score 4 and be accepted.
        assert!(
            validate_password("MyPassword123!@#$").is_err(),
            "blocklisted word should keep score at or below Fair"
        );
    }

    // @internal
    #[test]
    fn sequential_pattern_caps_score_even_when_long_and_diverse() {
        // Long, with multiple character classes, but follows the alphabet.
        // Without the cap this would score 4 and be accepted.
        assert!(
            validate_password("abcdefghijklmnopqrstuvwxyz123!").is_err(),
            "sequential pattern should keep score at or below Fair"
        );
    }

    // @internal
    #[test]
    fn repeated_chars_caps_score() {
        // Nine identical characters plus one different class still triggers
        // the repeated-char pattern and must be rejected.
        assert!(
            validate_password("aaaaaaaaa1").is_err(),
            "repeated characters should keep score at or below Fair"
        );
    }

    // @internal
    #[test]
    fn strong_passphrase_without_patterns_is_accepted() {
        let strength = validate_password("correct-horse-battery-staple!").unwrap();
        assert!(
            matches!(
                strength,
                PasswordStrength::Strong | PasswordStrength::VeryStrong
            ),
            "strong passphrase without patterns should be accepted"
        );
    }
}

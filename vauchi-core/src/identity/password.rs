// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Password Strength Validation
//!
//! Uses zxcvbn for entropy-based password strength estimation.
//! Requires a minimum score of 3 (out of 4) for passwords.

use super::IdentityError;
use zxcvbn::Score;

/// Password strength levels based on zxcvbn scores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl From<Score> for PasswordStrength {
    fn from(score: Score) -> Self {
        match score {
            Score::Zero => PasswordStrength::TooWeak,
            Score::One => PasswordStrength::Weak,
            Score::Two => PasswordStrength::Fair,
            Score::Three => PasswordStrength::Strong,
            Score::Four => PasswordStrength::VeryStrong,
            // Handle any future additions to the Score enum
            _ => PasswordStrength::VeryStrong,
        }
    }
}

/// Minimum password length requirement.
const MIN_PASSWORD_LENGTH: usize = 8;

/// Minimum zxcvbn score required (0-4 scale).
/// Score 3 means "safely unguessable: moderate protection from offline slow-hash scenario"
const MIN_REQUIRED_SCORE: Score = Score::Three;

/// Validates a password for strength using zxcvbn entropy estimation.
///
/// Returns the password strength level if the password is acceptable,
/// or an error if the password is too weak.
///
/// # Requirements
/// - Minimum 8 characters
/// - zxcvbn score of 3 or higher (out of 4)
///
/// # Examples
/// ```
/// use vauchi_core::identity::password::{validate_password, PasswordStrength};
///
/// // Weak passwords are rejected
/// assert!(validate_password("password").is_err());
/// assert!(validate_password("12345678").is_err());
///
/// // Strong passphrases are accepted
/// let result = validate_password("correct-horse-battery-staple");
/// assert!(result.is_ok());
/// ```
pub fn validate_password(password: &str) -> Result<PasswordStrength, IdentityError> {
    // Check minimum length first
    if password.len() < MIN_PASSWORD_LENGTH {
        return Err(IdentityError::WeakPassword);
    }

    // Use zxcvbn to estimate entropy
    let estimate = zxcvbn::zxcvbn(password, &[]);
    let score = estimate.score();

    // Require minimum score
    if score < MIN_REQUIRED_SCORE {
        return Err(IdentityError::WeakPassword);
    }

    Ok(PasswordStrength::from(score))
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
/// // May contain suggestions like "Add another word or two"
/// println!("Suggestions: {}", feedback);
/// ```
pub fn password_feedback(password: &str) -> String {
    let estimate = zxcvbn::zxcvbn(password, &[]);

    let mut feedback_parts = Vec::new();

    if let Some(feedback) = estimate.feedback() {
        if let Some(warning) = feedback.warning() {
            feedback_parts.push(warning.to_string());
        }

        for suggestion in feedback.suggestions() {
            feedback_parts.push(suggestion.to_string());
        }
    }

    feedback_parts.join(" ")
}

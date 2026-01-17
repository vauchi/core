//! URI Generation for Contact Fields
//!
//! Converts contact fields to actionable URIs (tel:, mailto:, https:, etc.)
//! Implements security whitelist to block dangerous URI schemes.

use super::{ContactField, FieldType};

/// Actions that can be performed on a contact field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContactAction {
    /// Open phone dialer with number
    Call(String),
    /// Open SMS app with number
    SendSms(String),
    /// Open email client with address
    SendEmail(String),
    /// Open URL in browser
    OpenUrl(String),
    /// Open address in maps
    OpenMap(String),
    /// Copy value to clipboard (fallback)
    CopyToClipboard,
}

/// Allowed URI schemes (security whitelist).
const ALLOWED_SCHEMES: &[&str] = &["tel", "mailto", "sms", "https", "http", "geo"];

/// Blocked URI schemes (explicit blocklist for dangerous schemes).
const BLOCKED_SCHEMES: &[&str] = &["javascript", "vbscript", "data", "file", "ftp", "blob"];

/// Check if a URI scheme is allowed.
pub fn is_allowed_scheme(scheme: &str) -> bool {
    let lower = scheme.to_lowercase();
    ALLOWED_SCHEMES.contains(&lower.as_str())
}

/// Check if a URI scheme is explicitly blocked.
fn is_blocked_scheme(scheme: &str) -> bool {
    let lower = scheme.to_lowercase();
    BLOCKED_SCHEMES.contains(&lower.as_str())
}

/// Extract scheme from a URI string.
fn extract_scheme(uri: &str) -> Option<&str> {
    uri.split(':').next()
}

/// Social network URL templates.
/// Maps lowercase label names to their profile URL templates.
fn social_url_template(label: &str) -> Option<&'static str> {
    match label.to_lowercase().as_str() {
        "twitter" | "x" => Some("https://twitter.com/{username}"),
        "github" => Some("https://github.com/{username}"),
        "linkedin" => Some("https://linkedin.com/{username}"),
        "instagram" => Some("https://instagram.com/{username}"),
        "facebook" => Some("https://facebook.com/{username}"),
        "mastodon" => Some("https://mastodon.social/@{username}"),
        "youtube" => Some("https://youtube.com/@{username}"),
        "tiktok" => Some("https://tiktok.com/@{username}"),
        "reddit" => Some("https://reddit.com/u/{username}"),
        "bluesky" => Some("https://bsky.app/profile/{username}"),
        _ => None,
    }
}

/// Normalize a social media username (remove @ prefix if present).
fn normalize_social_username(value: &str) -> &str {
    value.strip_prefix('@').unwrap_or(value)
}

/// URL encode a string for use in query parameters.
fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                result.push(c);
            }
            ' ' => {
                result.push('+');
            }
            _ => {
                for byte in c.to_string().as_bytes() {
                    result.push('%');
                    result.push_str(&format!("{:02X}", byte));
                }
            }
        }
    }
    result
}

impl ContactField {
    /// Convert this field to an actionable URI.
    ///
    /// Returns `None` if:
    /// - The value is empty or whitespace-only
    /// - The URI scheme would be blocked
    /// - No URI can be generated (e.g., unknown social network)
    pub fn to_uri(&self) -> Option<String> {
        let value = self.value().trim();
        if value.is_empty() {
            return None;
        }

        // For Custom fields, use heuristic detection
        let effective_type = if self.field_type() == FieldType::Custom {
            self.detect_value_type().unwrap_or(FieldType::Custom)
        } else {
            self.field_type()
        };

        match effective_type {
            FieldType::Phone => Some(format!("tel:{}", value)),
            FieldType::Email => Some(format!("mailto:{}", value)),
            FieldType::Website => self.website_to_uri(value),
            FieldType::Social => self.social_to_uri(value),
            FieldType::Address => Some(format!("geo:0,0?q={}", url_encode(value))),
            FieldType::Custom => None, // No heuristic match, no URI
        }
    }

    /// Generate URI for website field.
    fn website_to_uri(&self, value: &str) -> Option<String> {
        // Check for blocked schemes first
        if let Some(scheme) = extract_scheme(value) {
            if is_blocked_scheme(scheme) {
                return None;
            }
        }

        // If already has valid protocol, use as-is
        if value.starts_with("https://") || value.starts_with("http://") {
            Some(value.to_string())
        } else if value.contains("://") {
            // Has some other scheme - check if allowed
            if let Some(scheme) = extract_scheme(value) {
                if is_allowed_scheme(scheme) {
                    Some(value.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            // No protocol - add https://
            Some(format!("https://{}", value))
        }
    }

    /// Generate profile URL for social field.
    fn social_to_uri(&self, value: &str) -> Option<String> {
        let template = social_url_template(self.label())?;
        let username = normalize_social_username(value);

        // Handle LinkedIn's special format (in/username)
        let username = if self.label().to_lowercase() == "linkedin" {
            // If already has "in/", use as-is; otherwise just use the value
            if username.starts_with("in/") {
                username.to_string()
            } else {
                format!("in/{}", username)
            }
        } else {
            username.to_string()
        };

        Some(template.replace("{username}", &username))
    }

    /// Get the primary action for this field.
    pub fn to_action(&self) -> ContactAction {
        let value = self.value().trim();
        if value.is_empty() {
            return ContactAction::CopyToClipboard;
        }

        // For Custom fields, use heuristic detection
        let effective_type = if self.field_type() == FieldType::Custom {
            self.detect_value_type().unwrap_or(FieldType::Custom)
        } else {
            self.field_type()
        };

        match effective_type {
            FieldType::Phone => ContactAction::Call(value.to_string()),
            FieldType::Email => ContactAction::SendEmail(value.to_string()),
            FieldType::Website => ContactAction::OpenUrl(value.to_string()),
            FieldType::Social => {
                if let Some(uri) = self.to_uri() {
                    ContactAction::OpenUrl(uri)
                } else {
                    ContactAction::CopyToClipboard
                }
            }
            FieldType::Address => ContactAction::OpenMap(value.to_string()),
            FieldType::Custom => ContactAction::CopyToClipboard,
        }
    }

    /// Detect the semantic type of the value using heuristics.
    ///
    /// Useful for Custom fields to determine if the value is
    /// actually a phone number, email, URL, etc.
    pub fn detect_value_type(&self) -> Option<FieldType> {
        let value = self.value().trim();
        if value.is_empty() {
            return None;
        }

        // Check for URL patterns first (most specific)
        if value.starts_with("https://") || value.starts_with("http://") {
            return Some(FieldType::Website);
        }

        // Check for email pattern
        if self.looks_like_email(value) {
            return Some(FieldType::Email);
        }

        // Check for phone pattern
        if self.looks_like_phone(value) {
            return Some(FieldType::Phone);
        }

        None
    }

    /// Heuristic check for email-like values.
    fn looks_like_email(&self, value: &str) -> bool {
        // Must contain @ with content before and after
        if !value.contains('@') {
            return false;
        }

        let parts: Vec<&str> = value.split('@').collect();
        if parts.len() != 2 {
            return false;
        }

        let local = parts[0];
        let domain = parts[1];

        // Basic validation
        !local.is_empty() && !domain.is_empty() && domain.contains('.')
    }

    /// Heuristic check for phone-like values.
    fn looks_like_phone(&self, value: &str) -> bool {
        // Count digits
        let digit_count = value.chars().filter(|c| c.is_ascii_digit()).count();

        // Must have at least 7 digits for a phone number
        if digit_count < 7 {
            return false;
        }

        // Check that most characters are phone-valid
        let valid_chars = value.chars().filter(|c| {
            c.is_ascii_digit() || *c == ' ' || *c == '-' || *c == '(' || *c == ')' || *c == '+'
        });

        // At least 80% of characters should be phone-valid
        let valid_count = valid_chars.count();
        let total_chars = value.chars().count();

        if total_chars == 0 {
            return false;
        }

        (valid_count * 100 / total_chars) >= 80
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encode_basic() {
        assert_eq!(url_encode("hello world"), "hello+world");
        assert_eq!(url_encode("test@example"), "test%40example");
    }

    #[test]
    fn test_is_allowed_scheme() {
        assert!(is_allowed_scheme("tel"));
        assert!(is_allowed_scheme("TEL")); // Case insensitive
        assert!(!is_allowed_scheme("javascript"));
    }

    #[test]
    fn test_normalize_social_username() {
        assert_eq!(normalize_social_username("@bobsmith"), "bobsmith");
        assert_eq!(normalize_social_username("bobsmith"), "bobsmith");
    }
}

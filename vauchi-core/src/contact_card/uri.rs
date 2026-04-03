// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! URI Generation for Contact Fields
//!
//! Converts contact fields to actionable URIs (tel:, mailto:, https:, etc.)
//! Implements security whitelist to block dangerous URI schemes.

use std::sync::LazyLock;

use super::{ContactField, FieldType};
use crate::social::SocialNetworkRegistry;

/// Default social network registry (loaded once, shared across calls).
static DEFAULT_REGISTRY: LazyLock<SocialNetworkRegistry> =
    LazyLock::new(SocialNetworkRegistry::with_defaults);

/// Actions that can be performed on a contact field.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
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
    /// Get directions to address (maps with route planning)
    GetDirections(String),
    /// Copy value to clipboard (fallback)
    CopyToClipboard,
}

/// Allowed URI schemes (security whitelist).
const ALLOWED_SCHEMES: &[&str] = &["tel", "mailto", "sms", "https", "http", "geo"];

/// Blocked URI schemes (explicit blocklist for dangerous schemes).
const BLOCKED_SCHEMES: &[&str] = &["javascript", "vbscript", "data", "file", "ftp", "blob"];

/// Validate whether a phone number string is well-formed enough to dial.
///
/// Rules:
/// - Strip all non-digit characters (except leading `+`)
/// - Require at least 7 digits
/// - Only allow digits, spaces, dashes, parentheses, dots, and leading `+`
///
/// # Examples
///
/// ```
/// use vauchi_core::contact_card::is_valid_phone;
///
/// assert!(is_valid_phone("+1-555-123-4567"));
/// assert!(is_valid_phone("(555) 123-4567"));
/// assert!(!is_valid_phone("not-a-number"));
/// assert!(!is_valid_phone("12"));
/// ```
pub fn is_valid_phone(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }

    // Count digits
    let digit_count = value.chars().filter(|c| c.is_ascii_digit()).count();

    // Must have at least 7 digits
    if digit_count < 7 {
        return false;
    }

    // All characters must be phone-valid: digits, spaces, dashes, parens, plus, dots
    value.chars().all(|c| {
        c.is_ascii_digit() || c == ' ' || c == '-' || c == '(' || c == ')' || c == '+' || c == '.'
    })
}

/// Check if a URI scheme is allowed.
pub fn is_allowed_scheme(scheme: &str) -> bool {
    let lower = scheme.to_lowercase();
    ALLOWED_SCHEMES.contains(&lower.as_str())
}

/// Check if a URI scheme is explicitly blocked.
pub fn is_blocked_scheme(scheme: &str) -> bool {
    let lower = scheme.to_lowercase();
    BLOCKED_SCHEMES.contains(&lower.as_str())
}

/// Check if a URL string is safe to open.
///
/// Returns `true` if the URL uses an allowed scheme (http, https, tel, mailto, sms, geo).
/// Returns `false` if:
/// - The URL uses a blocked scheme (javascript, vbscript, data, file, etc.)
/// - The URL uses an unknown scheme
/// - The URL is malformed
///
/// # Examples
///
/// ```
/// use vauchi_core::contact_card::is_safe_url;
///
/// assert!(is_safe_url("https://example.com"));
/// assert!(is_safe_url("tel:+1234567890"));
/// assert!(!is_safe_url("javascript:alert(1)"));
/// assert!(!is_safe_url("data:text/html,<script>"));
/// ```
pub fn is_safe_url(url: &str) -> bool {
    let url = url.trim();
    if url.is_empty() {
        return false;
    }

    // Extract scheme
    if let Some(scheme) = extract_scheme(url) {
        // Must have :// or : after scheme for it to be valid
        if !url.contains(':') {
            return false;
        }

        // Check if blocked first (explicit deny list)
        if is_blocked_scheme(scheme) {
            return false;
        }

        // Must be in allowed list
        is_allowed_scheme(scheme)
    } else {
        // No scheme found - not a valid URL
        false
    }
}

/// Validate a relay WebSocket URL.
///
/// Accepts:
/// - `wss://` for any host (production relay)
/// - `ws://` only for localhost/loopback (development)
///
/// Rejects all other schemes and non-loopback `ws://` hosts.
pub fn is_valid_relay_url(url: &str) -> bool {
    let url = url.trim();
    let lower = url.to_lowercase();

    if let Some(rest) = lower.strip_prefix("wss://") {
        // wss:// is always allowed if there's a host
        !rest.is_empty()
    } else if let Some(rest) = lower.strip_prefix("ws://") {
        // ws:// only for loopback
        let authority = rest.split('/').next().unwrap_or("");
        // Handle IPv6 bracket notation: [::1]:port
        let host = if authority.starts_with('[') {
            authority
                .split(']')
                .next()
                .unwrap_or("")
                .trim_start_matches('[')
        } else {
            authority.split(':').next().unwrap_or("")
        };
        host == "localhost" || host == "127.0.0.1" || host == "::1"
    } else {
        false
    }
}

/// Extract scheme from a URI string.
fn extract_scheme(uri: &str) -> Option<&str> {
    uri.split(':').next()
}

/// Normalize a social media username (remove @ prefix if present).
fn normalize_social_username(value: &str) -> &str {
    value.strip_prefix('@').unwrap_or(value)
}

/// Parse a Mastodon federated handle (user@instance) into a profile URL.
///
/// Returns `Some("https://{instance}/@{user}")` if the value contains `@` with
/// valid user and instance parts. Returns `None` for plain usernames.
fn parse_mastodon_federated(username: &str) -> Option<String> {
    // After normalize_social_username, leading @ is stripped.
    // So "@bob@mas.to" becomes "bob@mas.to" and "bob@mas.to" stays "bob@mas.to".
    if let Some(at_pos) = username.find('@') {
        let user = &username[..at_pos];
        let instance = &username[at_pos + 1..];
        if !user.is_empty() && !instance.is_empty() && instance.contains('.') {
            return Some(format!("https://{}/@{}", instance, user));
        }
    }
    None
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
            FieldType::Phone => {
                if is_valid_phone(value) {
                    Some(format!("tel:{}", value))
                } else {
                    None
                }
            }
            FieldType::Email => Some(format!("mailto:{}", value)),
            FieldType::Website => self.website_to_uri(value),
            FieldType::Social => self.social_to_uri(value),
            FieldType::Address => Some(format!("geo:0,0?q={}", url_encode(value))),
            FieldType::Birthday => None, // Birthday dates don't have a direct URI action
            FieldType::Custom => None,   // No heuristic match, no URI
        }
    }

    /// Generate URI for website field.
    fn website_to_uri(&self, value: &str) -> Option<String> {
        // Check for blocked schemes first
        if let Some(scheme) = extract_scheme(value)
            && is_blocked_scheme(scheme)
        {
            return None;
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
    ///
    /// Delegates to the `SocialNetworkRegistry` for URL templates, supporting
    /// all registered networks (38+) rather than a hardcoded subset.
    fn social_to_uri(&self, value: &str) -> Option<String> {
        // Normalize label aliases to registry IDs
        let label_lower = match self.label().to_lowercase().as_str() {
            "x" => "twitter".to_string(),
            other => other.to_string(),
        };
        let username = normalize_social_username(value);

        // Handle Mastodon federated handles (@user@instance or user@instance)
        if label_lower == "mastodon"
            && let Some(profile_url) = parse_mastodon_federated(username)
        {
            return Some(profile_url);
        }
        // Not a federated handle — fall through to registry

        // Strip LinkedIn "in/" prefix if user included it (template already has it)
        let username = if label_lower == "linkedin" {
            username.strip_prefix("in/").unwrap_or(username)
        } else {
            username
        };

        DEFAULT_REGISTRY.profile_url(&label_lower, username)
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
            FieldType::Phone => {
                if is_valid_phone(value) {
                    ContactAction::Call(value.to_string())
                } else {
                    ContactAction::CopyToClipboard
                }
            }
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
            FieldType::Birthday => ContactAction::CopyToClipboard, // Birthday dates copy to clipboard
            FieldType::Custom => ContactAction::CopyToClipboard,
        }
    }

    /// Get all applicable actions for this field (for context menus).
    ///
    /// Returns a vector of actions that can be performed on this field.
    /// Always includes `CopyToClipboard` as a fallback action.
    ///
    /// # Examples
    ///
    /// ```
    /// use vauchi_core::contact_card::{ContactField, FieldType, ContactAction};
    ///
    /// let field = ContactField::new(FieldType::Phone, "Mobile", "+1-555-123-4567");
    /// let actions = field.to_secondary_actions();
    /// assert!(actions.contains(&ContactAction::Call("+1-555-123-4567".to_string())));
    /// assert!(actions.contains(&ContactAction::SendSms("+1-555-123-4567".to_string())));
    /// assert!(actions.contains(&ContactAction::CopyToClipboard));
    /// ```
    pub fn to_secondary_actions(&self) -> Vec<ContactAction> {
        let value = self.value().trim();
        let mut actions: Vec<ContactAction> = Vec::new();

        // Empty values only offer copy
        if value.is_empty() {
            actions.push(ContactAction::CopyToClipboard);
            return actions;
        }

        // For Custom fields, use heuristic detection
        let effective_type = if self.field_type() == FieldType::Custom {
            self.detect_value_type().unwrap_or(FieldType::Custom)
        } else {
            self.field_type()
        };

        match effective_type {
            FieldType::Phone => {
                if is_valid_phone(value) {
                    actions.push(ContactAction::Call(value.to_string()));
                    actions.push(ContactAction::SendSms(value.to_string()));
                }
                // Invalid phones fall through to just CopyToClipboard below
            }
            FieldType::Email => {
                actions.push(ContactAction::SendEmail(value.to_string()));
            }
            FieldType::Website => {
                if let Some(uri) = self.to_uri() {
                    actions.push(ContactAction::OpenUrl(uri));
                }
            }
            FieldType::Social => {
                if let Some(uri) = self.to_uri() {
                    actions.push(ContactAction::OpenUrl(uri));
                }
            }
            FieldType::Address => {
                actions.push(ContactAction::OpenMap(value.to_string()));
                actions.push(ContactAction::GetDirections(value.to_string()));
            }
            FieldType::Birthday => {
                // No primary action for birthday dates
            }
            FieldType::Custom => {
                // No primary action for plain custom text
            }
        }

        // Always include copy as fallback
        actions.push(ContactAction::CopyToClipboard);
        actions
    }

    /// Generate a directions URI for this field.
    ///
    /// Returns a web maps URL with route planning for Address fields.
    /// Returns `None` for non-address fields or empty values.
    ///
    /// Uses OpenStreetMap-based URL which works cross-platform without
    /// requiring a specific maps provider.
    pub fn to_directions_uri(&self) -> Option<String> {
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

        if effective_type != FieldType::Address {
            return None;
        }

        Some(format!(
            "https://www.openstreetmap.org/directions?route=&to={}",
            url_encode(value)
        ))
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

// INLINE_TEST_REQUIRED: Tests private url_encode and normalize_social_username helper functions
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

    #[test]
    fn test_is_safe_url_allowed_schemes() {
        // Allowed schemes should pass
        assert!(is_safe_url("https://example.com"));
        assert!(is_safe_url("http://example.com"));
        assert!(is_safe_url("tel:+1234567890"));
        assert!(is_safe_url("mailto:test@example.com"));
        assert!(is_safe_url("sms:+1234567890"));
        assert!(is_safe_url("geo:0,0?q=address"));
    }

    #[test]
    fn test_is_safe_url_blocked_schemes() {
        // Blocked schemes should fail
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("vbscript:msgbox(1)"));
        assert!(!is_safe_url("data:text/html,<script>alert(1)</script>"));
        assert!(!is_safe_url("file:///etc/passwd"));
        assert!(!is_safe_url("ftp://example.com"));
        assert!(!is_safe_url("blob:http://example.com/uuid"));
    }

    #[test]
    fn test_is_safe_url_unknown_schemes() {
        // Unknown schemes should fail (not in allowlist)
        assert!(!is_safe_url("custom://something"));
        assert!(!is_safe_url("myapp://deeplink"));
    }

    #[test]
    fn test_is_safe_url_edge_cases() {
        // Empty/whitespace should fail
        assert!(!is_safe_url(""));
        assert!(!is_safe_url("   "));

        // No scheme should fail
        assert!(!is_safe_url("example.com"));
        assert!(!is_safe_url("just some text"));
    }

    #[test]
    fn test_is_blocked_scheme() {
        assert!(is_blocked_scheme("javascript"));
        assert!(is_blocked_scheme("JAVASCRIPT")); // Case insensitive
        assert!(is_blocked_scheme("data"));
        assert!(!is_blocked_scheme("https"));
        assert!(!is_blocked_scheme("tel"));
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! @feature: release_privacy_multidevice_certification
//! @scenario: Weaker transport cannot bypass OHTTP

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use vauchi_core::exchange::{DeviceLinkJoinInvitation, JoinInvitationError};

fn invitation_with_relay(relay_url: &str) -> String {
    let qr = URL_SAFE_NO_PAD.encode("whatever");
    let code = URL_SAFE_NO_PAD.encode("123456");
    let relay = URL_SAFE_NO_PAD.encode(relay_url.as_bytes());
    format!("vauchi://device-link?qr={qr}&code={code}&relay={relay}")
}

// @rg-8 @fail-closed
#[test]
fn test_parse_url_untrusted_relay_rejects_unsafe_endpoints() {
    let unsafe_relay_urls = [
        "http://relay.example.com",
        "https://127.0.0.1",
        "https://127.1",
        "https://2130706433",
        "https://0x7f000001",
        "https://0177.0.0.1",
        "https://user:secret@relay.example.com",
        "https://relay.example.com#fragment",
        "https://",
    ];

    for relay_url in unsafe_relay_urls {
        let result = DeviceLinkJoinInvitation::parse_url(&invitation_with_relay(relay_url));
        match result {
            Err(error) => {
                assert!(matches!(&error, JoinInvitationError::UnsupportedUrl));
                assert_eq!(error.to_string(), "unsupported invitation URL");
                assert!(
                    !error.to_string().contains(relay_url),
                    "parse error must not echo an attacker-controlled relay URL"
                );
            }
            Ok(invitation) => panic!(
                "unsafe invitation relay must be rejected, accepted {:?}",
                invitation.relay_url
            ),
        }
    }
}

// @rg-8 @fail-closed
#[test]
fn test_parse_url_oversized_unknown_parameter_rejects_whole_invitation() {
    let oversized = format!(
        "vauchi://device-link?qr={}&code={}&future={}",
        URL_SAFE_NO_PAD.encode("whatever"),
        URL_SAFE_NO_PAD.encode("123456"),
        "A".repeat(8_193)
    );

    match DeviceLinkJoinInvitation::parse_url(&oversized) {
        Err(error) => assert!(matches!(error, JoinInvitationError::UnsupportedUrl)),
        Ok(_) => panic!("oversized invitation must be rejected before retaining its raw URL"),
    }
}

// @rg-8 @fail-closed
#[test]
fn test_parse_url_oversized_relay_rejects_before_network_use() {
    let oversized_relay = format!("https://relay.example.com/{}", "a".repeat(1024));
    let result = DeviceLinkJoinInvitation::parse_url(&invitation_with_relay(&oversized_relay));

    match result {
        Err(error) => assert!(matches!(error, JoinInvitationError::UnsupportedUrl)),
        Ok(invitation) => panic!(
            "oversized invitation relay must be rejected, accepted {} bytes",
            invitation.relay_url.map_or(0, |url| url.len())
        ),
    }
}

// @rg-8 @fail-closed
#[test]
fn test_parse_url_public_https_relay_preserves_value() {
    let relay_url = "https://relay.example.com";
    let invitation = DeviceLinkJoinInvitation::parse_url(&invitation_with_relay(relay_url))
        .expect("public HTTPS invitation relay must remain supported");

    assert_eq!(invitation.relay_url.as_deref(), Some(relay_url));
}

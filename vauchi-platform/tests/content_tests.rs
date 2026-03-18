// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for mobile content type conversions and config defaults (content.rs).

use vauchi_platform::{MobileContentConfig, MobileContentType};

#[test]
fn test_content_type_networks_roundtrip() {
    let mobile = MobileContentType::Networks;
    let core: vauchi_core::content::ContentType = mobile.into();
    let back: MobileContentType = core.into();
    assert!(
        matches!(back, MobileContentType::Networks),
        "Networks should roundtrip"
    );
}

#[test]
fn test_content_type_locales_roundtrip() {
    let mobile = MobileContentType::Locales;
    let core: vauchi_core::content::ContentType = mobile.into();
    let back: MobileContentType = core.into();
    assert!(
        matches!(back, MobileContentType::Locales),
        "Locales should roundtrip"
    );
}

#[test]
fn test_content_type_themes_roundtrip() {
    let mobile = MobileContentType::Themes;
    let core: vauchi_core::content::ContentType = mobile.into();
    let back: MobileContentType = core.into();
    assert!(
        matches!(back, MobileContentType::Themes),
        "Themes should roundtrip"
    );
}

#[test]
fn test_content_type_help_roundtrip() {
    let mobile = MobileContentType::Help;
    let core: vauchi_core::content::ContentType = mobile.into();
    let back: MobileContentType = core.into();
    assert!(
        matches!(back, MobileContentType::Help),
        "Help should roundtrip"
    );
}

#[test]
fn test_default_config() {
    let config = MobileContentConfig::default();
    assert!(
        config.remote_updates_enabled,
        "Remote updates should be enabled by default"
    );
    assert_eq!(
        config.content_url, "https://cdn.vauchi.app/v1",
        "Default URL should be vauchi.app/app-files"
    );
    assert!(
        config.proxy_url.is_none(),
        "Proxy should be None by default"
    );
}

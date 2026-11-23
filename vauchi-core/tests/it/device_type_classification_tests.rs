// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device's model name maps to the kind of device it is.

use vauchi_core::identity::{DeviceType, classify_device_type};

// @internal
#[test]
fn test_classify_device_type_iphone_returns_phone() {
    assert_eq!(classify_device_type("iPhone 15 Pro"), DeviceType::Phone);
    assert_eq!(classify_device_type("iphone"), DeviceType::Phone);
    assert_eq!(classify_device_type("My iPhone"), DeviceType::Phone);
}

// @internal
#[test]
fn test_classify_device_type_android_phones_returns_phone() {
    assert_eq!(classify_device_type("Pixel 8"), DeviceType::Phone);
    assert_eq!(classify_device_type("Galaxy S24"), DeviceType::Phone);
}

// @internal
#[test]
fn test_classify_device_type_ipad_returns_tablet() {
    assert_eq!(classify_device_type("iPad Pro"), DeviceType::Tablet);
    assert_eq!(classify_device_type("ipad"), DeviceType::Tablet);
    assert_eq!(classify_device_type("Galaxy Tab S9"), DeviceType::Tablet);
}

// @internal
#[test]
fn test_classify_device_type_mac_returns_laptop() {
    assert_eq!(classify_device_type("MacBook Pro"), DeviceType::Laptop);
    assert_eq!(classify_device_type("mac"), DeviceType::Laptop);
    assert_eq!(classify_device_type("ThinkPad Laptop"), DeviceType::Laptop);
    assert_eq!(classify_device_type("Chromebook"), DeviceType::Laptop);
}

// @internal
#[test]
fn test_classify_device_type_watch_returns_watch() {
    assert_eq!(classify_device_type("Apple Watch"), DeviceType::Watch);
    assert_eq!(classify_device_type("watch"), DeviceType::Watch);
}

// @internal
#[test]
fn test_classify_device_type_desktop_returns_desktop() {
    assert_eq!(classify_device_type("Desktop PC"), DeviceType::Desktop);
    assert_eq!(classify_device_type("My PC"), DeviceType::Desktop);
    assert_eq!(classify_device_type("iMac"), DeviceType::Desktop);
}

// @internal
#[test]
fn test_classify_device_type_unknown_returns_unknown() {
    assert_eq!(classify_device_type(""), DeviceType::Unknown);
    assert_eq!(classify_device_type("some device"), DeviceType::Unknown);
    assert_eq!(classify_device_type("Primary Device"), DeviceType::Unknown);
}

// @internal
#[test]
fn test_classify_device_type_case_insensitive() {
    assert_eq!(classify_device_type("IPHONE"), DeviceType::Phone);
    assert_eq!(classify_device_type("MacBook"), DeviceType::Laptop);
    assert_eq!(classify_device_type("IPAD"), DeviceType::Tablet);
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use std::num::NonZeroUsize;
use std::sync::Arc;
use vauchi_core::platform::Event;
use vauchi_core::platform::executor::InboxLimits;
use vauchi_hardware::{BleScanner, UnsupportedDiscovery};

const SERVICE: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

fn limits() -> InboxLimits {
    InboxLimits {
        operations: NonZeroUsize::new(1).unwrap(),
        events: NonZeroUsize::new(1).unwrap(),
        event_bytes: NonZeroUsize::new(512).unwrap(),
    }
}

// @scenario: contact_exchange.feature :: Discover nearby Vauchi users via BLE
#[tokio::test]
async fn a_host_without_a_discovery_backend_reports_bluetooth_unavailable() {
    let mut scanner = BleScanner::with_backend(
        Arc::new(UnsupportedDiscovery),
        tokio::runtime::Handle::current(),
        limits(),
    )
    .unwrap();

    scanner.start(SERVICE.into()).unwrap();

    assert_eq!(
        scanner.next_event().await,
        Some(Event::HardwareUnavailable {
            transport: "BLE".into()
        })
    );
}

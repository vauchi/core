// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::delivery::EventSender;
use crate::ports::{Discovery, HardwareFailure};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Discovery on an OS this crate has no native backend for yet
/// (macOS #771, Windows #772): every scan reports Bluetooth unavailable.
pub struct UnsupportedDiscovery;

impl Discovery for UnsupportedDiscovery {
    fn scan(
        &self,
        _: String,
        _: EventSender,
    ) -> Pin<Box<dyn Future<Output = Result<(), HardwareFailure>> + Send + '_>> {
        Box::pin(async { Err(HardwareFailure::Unavailable) })
    }
}

/// The discovery backend for the OS this binary runs on, so hosts never
/// select one themselves.
#[cfg(target_os = "linux")]
pub fn host_discovery() -> Arc<dyn Discovery> {
    Arc::new(crate::bluez::BlueZDiscovery::system())
}

/// The discovery backend for the OS this binary runs on, so hosts never
/// select one themselves.
#[cfg(not(target_os = "linux"))]
pub fn host_discovery() -> Arc<dyn Discovery> {
    Arc::new(UnsupportedDiscovery)
}

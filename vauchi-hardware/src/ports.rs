// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::delivery::EventSender;
use std::future::Future;
use std::pin::Pin;
use vauchi_core::platform::Event;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HardwareFailure {
    Unavailable,
    PermissionDenied,
    Failed,
    InvalidObservation,
}

impl HardwareFailure {
    pub(crate) fn event(self) -> Event {
        match self {
            Self::Unavailable => Event::HardwareUnavailable {
                transport: "BLE".into(),
            },
            Self::PermissionDenied => Event::PermissionDenied {
                transport: "BLE".into(),
            },
            Self::Failed | Self::InvalidObservation => Event::HardwareError {
                transport: "BLE".into(),
                error: match self {
                    Self::InvalidObservation => "Bluetooth observation exceeds executor limits",
                    _ => "Bluetooth discovery failed",
                }
                .into(),
            },
        }
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExecutorError {
    #[error("event budget must be at least 256 bytes to report hardware failures")]
    EventBudgetTooSmall,
    #[error("native discovery worker failed")]
    WorkerFailed,
}

/// OS discovery port. Implementations bound capture buffers before constructing
/// events, and release native discovery when the returned future is dropped.
pub trait Discovery: Send + Sync + 'static {
    fn scan(
        &self,
        service_uuid: String,
        events: EventSender,
    ) -> Pin<Box<dyn Future<Output = Result<(), HardwareFailure>> + Send + '_>>;
}

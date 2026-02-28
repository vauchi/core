// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delivery service — bridges ACK events to delivery storage.

pub mod offline_manager;
pub mod retry_scheduler;
pub mod service;

pub use offline_manager::OfflineManager;
pub use retry_scheduler::{RetryScheduler, RetryTickResult};
pub use service::{CleanupResult, DeliveryAckStatus, DeliveryService};

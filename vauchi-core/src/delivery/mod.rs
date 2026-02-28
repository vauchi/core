// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delivery service — bridges ACK events to delivery storage.

pub mod retry_scheduler;
pub mod service;

pub use retry_scheduler::{RetryScheduler, RetryTickResult};
pub use service::{DeliveryAckStatus, DeliveryService};

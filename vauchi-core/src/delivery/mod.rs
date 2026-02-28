// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Delivery service — bridges ACK events to delivery storage.

pub mod service;

pub use service::{DeliveryAckStatus, DeliveryService};

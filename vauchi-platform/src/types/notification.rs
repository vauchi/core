// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI notification types for mobile platforms.

/// OS notification category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileNotificationCategory {
    EmergencyAlert,
    ContactAdded,
}

/// A pending OS notification for the frontend to render.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobilePendingNotification {
    pub event_key: String,
    pub category: MobileNotificationCategory,
    pub title: String,
    pub body: String,
    pub contact_id: String,
}

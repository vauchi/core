// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Local provenance for a fetch's registered daily mailbox window.

#[derive(Clone, Debug)]
pub(crate) struct MailboxContext {
    pub(crate) identity: [u8; 32],
    pub(crate) device: [u8; 32],
    pub(crate) application_relay: String,
    pub(crate) ohttp_relay: String,
    pub(crate) day: u64,
}

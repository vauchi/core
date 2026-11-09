// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Nothing built under one auth mode lives on under another (ADR-080,
//! vauchi/private#1117). Exchange sessions and cached screens hold copies
//! of the identity in front when they were built; a duress unlock must not
//! inherit the real one, nor a normal unlock the mask's. The check runs
//! where navigation and dispatch begin, so it covers the lock screen,
//! biometric unlock and the bindings' direct `Vauchi::authenticate`.

use super::AppEngine;

impl AppEngine {
    pub(super) fn end_sessions_from_another_auth_mode(&mut self) {
        let mode = self.vauchi.auth_mode();
        if mode == self.sessions_auth_mode {
            return;
        }
        self.sessions_auth_mode = mode;
        self.engine_cache.clear();
        self.cancel_ble_handshake_session();
        self.cancel_multi_stage_session();
        #[cfg(all(feature = "network-http", feature = "storage"))]
        self.cancel_device_link_session();
        self.glance_display_qr = None;
        self.glance_display_nonce = None;
        self.glance_scanned = None;
        self.engine = Self::create_engine(
            &self.vauchi,
            &self.screen,
            self.preview_as_contact.as_deref(),
            &self.device_capabilities,
            &self.transport_readiness,
            &self.render_context,
            &self.pending_exchange_groups,
            None,
            &self.ble_attempt_token,
        );
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Glance one-sided-QR bootstrap on the engine-owned BLE handshake: the
//! displayer's QR, the scanner's pin, and the scanner's pinned dial.

use super::{AppEngine, AppScreen};
use crate::orchestrator::ble_handshake_machine::{BleOobBinding, BleRole};
use crate::ui::{EngineOutput, EngineUpdate};

/// Shortest advertised token prefix a Glance scanner accepts: the phones'
/// 16-bit service-UUID token. Anything shorter would match nearly everyone.
const MIN_ADVERTISED_PREFIX_BYTES: usize = 2;

/// How many pre-scan Glance advertisers are remembered. Adverts come from
/// any nearby device, so the memory is bounded and keeps the newest.
const MAX_PRE_SCAN_DISCOVERIES: usize = 8;

/// An Android central's dial to an iPhone can end in GATT status 133 before
/// any link exists, typically transiently (vauchi/private#9, GXP-8c). Only
/// the Glance scanner dials, so without a redial nobody reconnects.
pub(super) const MAX_GLANCE_REDIALS: u8 = 2;

pub(super) struct GlanceDial {
    device_id: String,
    redials: u8,
}

impl AppEngine {
    /// Begin the Glance one-sided-QR display: build this device's OOB bootstrap
    /// payload and remember the nonce it must require as the responder. Returns
    /// the base64 payload for a `Component::QrCode`. The QR's exchange key is
    /// the identity's X3DH public key — the same value the handshake feeds into
    /// the DH — so the scanner's exchange-key pin accepts the honest peer (a
    /// fresh ephemeral would be rejected).
    pub fn begin_glance_display(&mut self) -> Option<String> {
        let now = self.vauchi.clock().unix_seconds();
        let qr = {
            let identity = self.vauchi.identity()?;
            let ephemeral = identity.x3dh_keypair();
            vauchi_core::exchange::oob_bootstrap::OobBootstrapQr::generate(
                identity, &ephemeral, now,
            )
        };
        self.glance_display_nonce = Some(qr.oob_nonce());
        Some(qr.to_data_string())
    }

    /// Apply a scanned Glance QR: verify it (signature + expiry), latch this
    /// device into the scanner role, and pin the displayer's identity +
    /// exchange key + co-presence nonce. A tampered or expired QR returns an
    /// error and latches nothing.
    pub fn apply_glance_scan(&mut self, data: &str) -> Result<(), vauchi_core::ExchangeError> {
        let now = self.vauchi.clock().unix_seconds();
        let qr = vauchi_core::exchange::oob_bootstrap::OobBootstrapQr::verified_from_data_string(
            data, now,
        )?;
        self.glance_scanned = Some(BleOobBinding {
            expected_peer: Some(*qr.identity_key()),
            expected_exchange_key: Some(*qr.exchange_key()),
            oob_nonce_echo: Some(qr.oob_nonce()),
            required_oob_nonce: None,
        });
        // The scanned peer was most likely discovered before the scan and will
        // not be reported again; re-check what we saw through the same gate.
        for (device_id, adv_data) in std::mem::take(&mut self.glance_pre_scan_discoveries) {
            self.handle_glance_discovery(&device_id, &adv_data);
        }
        Ok(())
    }

    /// Pin the peer from a Glance code the user typed instead of scanning
    /// ([`EngineOutput::GlancePeerCode`]) through the same
    /// [`Self::apply_glance_scan`] a camera decode reaches, so the manual
    /// route cannot drift from the scan route. Runs after every action on
    /// the Glance screen; a pin already latched short-circuits, and the
    /// engine has verified the code, so a rejection here means it expired
    /// between commit and pin — surfaced as the retry screen, not swallowed.
    pub(super) fn pin_typed_glance_code(&mut self) {
        if self.glance_scanned.is_some()
            || !matches!(
                self.screen,
                AppScreen::BleExchange {
                    mode: vauchi_core::exchange::mode::ExchangeMode::Glance
                }
            )
        {
            return;
        }
        let Some(EngineOutput::GlancePeerCode { data }) = self.engine.engine_output() else {
            return;
        };
        if let Err(error) = self.apply_glance_scan(&data) {
            self.engine.apply_update(EngineUpdate::BleForceFailure {
                reason: Some(error.user_message().to_string()),
            });
        }
    }

    /// Scanner-side discovery gate for Glance: connect to a discovered device
    /// ONLY if its advertised identity is the one this device scanned. Builds
    /// the initiator session with the scanned pins and drains a
    /// `Command::BleConnect`. A no-op for a non-scanner or a non-matching
    /// advertiser — asymmetric discovery, no tiebreak, no latch race.
    fn remember_pre_scan_discovery(&mut self, device_id: &str, adv_data: &[u8]) {
        self.glance_pre_scan_discoveries
            .retain(|(known, _)| known != device_id);
        if self.glance_pre_scan_discoveries.len() == MAX_PRE_SCAN_DISCOVERIES {
            self.glance_pre_scan_discoveries.remove(0);
        }
        self.glance_pre_scan_discoveries
            .push((device_id.to_string(), adv_data.to_vec()));
    }

    pub fn handle_glance_discovery(&mut self, device_id: &str, adv_data: &[u8]) {
        // Dev instrumentation (vauchi/private#9 GL-6): lengths and decisions
        // only — no identity bytes.
        let Some(binding) = self.glance_scanned else {
            tracing::info!(
                "[Glance] discovery adv_len={} — not scanned yet",
                adv_data.len()
            );
            self.remember_pre_scan_discovery(device_id, adv_data);
            return; // not the scanner — this device waits to be connected to
        };
        let Some(expected) = binding.expected_peer else {
            tracing::info!("[Glance] discovery — scan binding has no pinned peer");
            return;
        };
        // Phones advertise only a 2-byte prefix of the token (a 16-bit service
        // UUID), so match on the prefix; the handshake still pins the full
        // identity, exchange key and OOB nonce, so a prefix twin cannot finish.
        if adv_data.len() < MIN_ADVERTISED_PREFIX_BYTES || !expected.starts_with(adv_data) {
            tracing::info!(
                "[Glance] discovery adv_len={} — not the scanned peer",
                adv_data.len()
            );
            return; // an advertiser we did not scan — ignore it
        }
        if self.ble_handshake_session_active() {
            tracing::info!("[Glance] discovery — session already active");
            return;
        }
        tracing::info!(
            "[Glance] discovery adv_len={} — scanned peer, connecting",
            adv_data.len()
        );
        let Some((identity_key, x3dh, card)) = self.build_ble_session_inputs() else {
            tracing::warn!("BLE: cannot start Glance scanner handshake — no identity / card");
            return;
        };
        self.ensure_ble_handshake_session(
            BleRole::Initiator,
            identity_key,
            x3dh,
            card,
            Some(binding),
        );
        self.glance_dial = Some(GlanceDial {
            device_id: device_id.to_string(),
            redials: 0,
        });
        self.extend_pending_commands(vec![vauchi_core::Command::BleConnect {
            device_id: device_id.to_string(),
        }]);
    }

    /// Redial the Glance scanner's pinned peer when its dial ended before any
    /// link existed, within [`MAX_GLANCE_REDIALS`]. Returns `true` when the
    /// disconnect was consumed by a redial. A lost *established* link is not
    /// redialled: that stays a failure.
    pub(crate) fn redial_glance_after_failed_dial(&mut self, device_id: &str) -> bool {
        let before_any_link = self.ble_machine_awaits_first_link();
        let Some(dial) = self.glance_dial.as_mut() else {
            return false;
        };
        if !before_any_link || dial.device_id != device_id || dial.redials >= MAX_GLANCE_REDIALS {
            return false;
        }
        dial.redials += 1;
        tracing::info!(
            "[Glance] dial ended before a link — redial {}/{}",
            dial.redials,
            MAX_GLANCE_REDIALS
        );
        self.extend_pending_commands(vec![vauchi_core::Command::BleConnect {
            device_id: device_id.to_string(),
        }]);
        true
    }
}

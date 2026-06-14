// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange-family screen factory — split from `screens.rs` (see
//! `create_engine`), which dispatches the matching `AppScreen`
//! variants here.

use super::AppEngine;
use super::AppScreen;
use crate::ui::engine::WorkflowEngine;
use crate::ui::exchange::{ExchangeConfig, ExchangeEngine};
use crate::ui::{ActionResult, UserAction};
use vauchi_core::api::Vauchi;

impl AppEngine {
    pub(super) fn create_exchange_engine(
        vauchi: &Vauchi,
        screen: &AppScreen,
        device_capabilities: &vauchi_core::exchange::capability::types::DeviceCapabilities,
        transport_readiness: &vauchi_core::exchange::capability::TransportReadiness,
        pending_groups: &[String],
    ) -> Box<dyn WorkflowEngine> {
        match screen {
            AppScreen::Exchange => {
                let card = vauchi.own_card().ok().flatten();
                let all_groups = vauchi.list_groups().unwrap_or_default();
                let available_groups = all_groups
                    .iter()
                    .map(|g| (g.id().to_string(), g.name().to_string()))
                    .collect();
                let snapshot_now = vauchi.clock().unix_seconds();
                let card_snapshot = card.as_ref().cloned().map(|c| {
                    vauchi_core::exchange::card_snapshot::CardSnapshot::freeze(c, snapshot_now)
                });
                let config = ExchangeConfig {
                    own_name: card
                        .as_ref()
                        .map(|c| c.display_name().to_string())
                        .unwrap_or_default(),
                    own_qr_data: vauchi.public_id().unwrap_or_default(),
                    available_groups,
                    device_capabilities: device_capabilities.clone(),
                    transport_readiness: transport_readiness.clone(),
                    mode: None, // triggers mode selection screen
                    card_snapshot,
                    available_group_data: all_groups,
                };

                // ADR-031: Create a protocol session if identity + card are available.
                // Identity is cloned via storage serialization (it intentionally
                // doesn't impl Clone because it contains private key material).
                // The intermediate buffer is zeroized to avoid leaking key material.
                //
                // Site 3 of `2026-05-21-silent-failures-in-security-paths`: the
                // `from_storage_bytes` round-trip is a contract invariant pinned
                // by `identity_storage_bytes_roundtrip_preserves_all_fields` in
                // `core/vauchi-core/tests/it/identity_tests.rs`. A failure here
                // means either a bug in the serializer/parser pair or genuine
                // memory corruption — not a recoverable runtime condition. Pre-
                // 2026-05-23 both sites used `.ok()` and silently dropped the
                // error, so the user tapping "start exchange" got no feedback.
                // We now surface the violation via tracing and keep the
                // graceful-degradation fallback (engine without pre-built
                // session / NFC identity) so the user retains an entry point.
                let session = vauchi
                    .identity()
                    .and_then(reconstruct_identity_via_storage_bytes)
                    .and_then(|identity| {
                        card.map(|c| {
                            let proximity =
                                vauchi_core::exchange::ManualConfirmationVerifier::new();
                            vauchi_core::exchange::ExchangeSession::new_qr(
                                identity,
                                c,
                                proximity,
                                vauchi_core::clock::SystemClock::shared(),
                            )
                        })
                    });

                // NFC graduation: TapTap now routes to the dedicated
                // `NfcExchangeEngine` (reached via `StartNfcExchange`), which
                // reconstructs its own signing identity in the
                // `AppScreen::NfcExchange` factory arm. The legacy
                // `ExchangeEngine` no longer holds an NFC identity.
                let clock = vauchi.clock().clone();
                let engine = match session {
                    Some(s) => ExchangeEngine::with_session(config, s, clock),
                    None => ExchangeEngine::new(config, clock),
                };
                Box::new(engine)
            }
            AppScreen::DeepLinkConsent { payload } => {
                Box::new(crate::ui::DeepLinkConsentEngine::new(payload.clone()))
            }
            AppScreen::DeepLinkResponder { payload } => {
                Box::new(crate::ui::LinkResponderEngine::new(payload.clone()))
            }
            AppScreen::LinkExchange => Box::new(crate::ui::LinkExchangeEngine::new()),
            AppScreen::BleExchange { mode } => {
                // Role-tiebreak token: this device's stable signing public
                // key. The engine advertises it; on discovery each peer
                // compares its own token against the other's and exactly one
                // initiates the connection (ADR-043 — core owns the tiebreak,
                // retiring the Android `compareTokens` frontend logic).
                let own_token = vauchi
                    .identity()
                    .map(|id| id.signing_public_key().to_vec())
                    .unwrap_or_default();
                Box::new(crate::ui::BleExchangeEngine::new(
                    *mode,
                    device_capabilities.has_camera,
                    own_token,
                    vauchi.clock().clone(),
                ))
            }
            AppScreen::NfcExchange => {
                // The NFC engine signs its key offer with the full identity, so
                // reconstruct it via the storage-bytes round-trip (Identity has
                // no Clone — same path the legacy `set_nfc_identity` used).
                let display_name = vauchi
                    .own_card()
                    .ok()
                    .flatten()
                    .map(|c| c.display_name().to_string())
                    .unwrap_or_default();
                let identity = vauchi
                    .identity()
                    .and_then(reconstruct_identity_via_storage_bytes);
                Box::new(crate::ui::NfcExchangeEngine::new(
                    identity,
                    display_name,
                    device_capabilities.has_camera,
                    vauchi.clock().clone(),
                ))
            }
            AppScreen::DirectTransport => {
                // The Cable engine signs its key offer with the full identity
                // and sends its own card, so reconstruct both via the
                // storage-bytes round-trip (Identity has no Clone). A missing
                // identity/own-card degrades to the engine's Failed screen
                // (the legacy factory's graceful-degradation contract). The
                // desktop is always the USB initiator.
                let identity = vauchi
                    .identity()
                    .and_then(reconstruct_identity_via_storage_bytes);
                // G2 privacy filter (shared chokepoint with the BLE path):
                // share only the fields the selected exchange group(s) may see.
                let card =
                    crate::ui::exchange::group_filter::filtered_own_card(vauchi, pending_groups);
                let clock = vauchi.clock().clone();
                Box::new(crate::ui::DirectTransportEngine::new(
                    identity,
                    card,
                    vauchi_core::exchange::UsbRole::Initiator,
                    clock,
                ))
            }
            AppScreen::MultiStageExchange { mode } => {
                // The cycle-thread session lives in vauchi-platform —
                // the bridge from MultiStageSessionListener callbacks
                // into this engine's `set_state` / `set_qr_payload` /
                // `set_finalized` / `set_session_ended` setters is
                // wired at the platform-binding layer.
                //
                // Phase 1.E of `2026-05-11-hover-graduation-plan.md`
                // made the constructor mode-aware. Hover gets
                // `new_hover()` (front camera + audio-handshake
                // trigger registered); other supported modes (Glance
                // today; Broadcast / TapHoverShake on future
                // graduations) get `new_glance()` (back camera +
                // audio-quiet). The autonomous audio-handshake
                // trigger in `MobileMultiStageSession` is gated on
                // `is_active_engine_multi_stage_hover()` per the
                // 1.C polish commit, so Glance flows never fire
                // spurious audio chrome.
                let engine = match mode {
                    vauchi_core::exchange::mode::ExchangeMode::Hover => {
                        crate::ui::MultiStageExchangeEngine::new_hover()
                    }
                    vauchi_core::exchange::mode::ExchangeMode::TapHoverShake => {
                        crate::ui::MultiStageExchangeEngine::new_tap_hover_shake()
                    }
                    _ => crate::ui::MultiStageExchangeEngine::new_glance(),
                };
                Box::new(engine)
            }
            other => unreachable!("non-exchange screen {other:?} routed to exchange factory"),
        }
    }
}

/// Round-trips an `Identity` reference via `to_storage_bytes` /
/// `from_storage_bytes` to obtain an owned copy. `Identity` deliberately
/// does not implement `Clone` because it contains private key material;
/// the serialization round-trip is the documented clone path.
///
/// The intermediate buffer is wrapped in `zeroize::Zeroizing` to scrub
/// the serialized form when this fn returns.
///
/// Returns `None` only on contract violation: `from_storage_bytes` is
/// guaranteed to accept the output of `to_storage_bytes`
/// (`identity_storage_bytes_roundtrip_preserves_all_fields` in
/// `core/vauchi-core/tests/it/identity_tests.rs` pins this). A failure
/// here therefore means a bug in the serializer/parser pair or memory
/// corruption — surfaced via `tracing::error!` instead of silently
/// dropped (site 3 of
/// `2026-05-21-silent-failures-in-security-paths`). The caller falls
/// through to the existing graceful-degradation path so the user keeps
/// an entry point into the exchange flow rather than getting a hung
/// "tap does nothing" no-op.
fn reconstruct_identity_via_storage_bytes(
    id_ref: &vauchi_core::identity::Identity,
) -> Option<vauchi_core::identity::Identity> {
    let bytes = zeroize::Zeroizing::new(id_ref.to_storage_bytes());
    match vauchi_core::identity::Identity::from_storage_bytes(
        &bytes,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    ) {
        Ok(identity) => Some(identity),
        Err(e) => {
            tracing::error!(
                target: "vauchi.ui.app_engine.screens",
                error = %e,
                "Identity round-trip via to_storage_bytes -> from_storage_bytes failed; \
                 falling back to engine without pre-built session. This is a contract \
                 violation — see identity_storage_bytes_roundtrip_preserves_all_fields."
            );
            None
        }
    }
}

impl AppEngine {
    /// Re-evaluate the exchange mode picker against the current readiness
    /// ledger. Always drops the cached `Exchange` engine so the next visit
    /// rebuilds from the ledger; when the picker is the *active* screen, also
    /// rebuilds `self.engine` in place so a permission change (a grant-affordance
    /// tap, or a live `PermissionDenied`) shows without a navigate-away round
    /// trip. Guarded on `AppScreen::Exchange` so an in-flight transport screen
    /// (`BleExchange`, `NfcExchange`, …) is never clobbered.
    pub(super) fn rebuild_exchange_engine(&mut self) {
        self.engine_cache.remove(&AppScreen::Exchange);
        // Rebuild in place ONLY when the mode picker itself is showing. The
        // GroupSelection / FieldPreview sub-steps also live under
        // `AppScreen::Exchange`; rebuilding there would discard the user's
        // selected mode + groups (e.g. BLE revoked mid-exchange on Android).
        // The unconditional cache remove above still re-evaluates on next visit.
        if self.screen == AppScreen::Exchange
            && self.engine.current_screen().screen_id == "exchange_mode_selection"
        {
            let screen = self.screen.clone();
            self.engine = Self::create_engine(
                &self.vauchi,
                &screen,
                self.preview_as_contact.as_deref(),
                &self.device_capabilities,
                &self.transport_readiness,
                &self.render_context,
                &self.pending_exchange_groups,
            );
        }
    }

    /// Intercept a grant-affordance tap (`grant:<mode>:<requirement>`) the mode
    /// picker renders for a present-but-denied transport. Records the
    /// requirement as granted on the device-wide ledger — the source of truth,
    /// from which the picker's snapshot is rebuilt — then re-renders the picker
    /// so the mode becomes selectable. There is no OS "permission granted" event
    /// (ADR-030/031), so the affordance is how the ledger re-learns; if the OS
    /// still withholds the permission, the next attempt re-emits
    /// `PermissionDenied` and the affordance returns.
    pub(super) fn intercept_grant_permission(
        &mut self,
        action: &UserAction,
    ) -> Option<ActionResult> {
        if self.screen != AppScreen::Exchange {
            return None;
        }
        let UserAction::ListItemSelected { item_id, .. } = action else {
            return None;
        };
        let token = item_id.strip_prefix("grant:")?.rsplit_once(':')?.1;
        let requirement = crate::ui::exchange::mode_selection::parse_requirement(token)?;
        self.transport_readiness.note_granted(requirement);
        self.rebuild_exchange_engine();
        Some(ActionResult::UpdateScreen(self.engine.current_screen()))
    }
}

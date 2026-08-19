// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The own-QR caption for a multi-stage exchange.
//!
//! Extracted from `multi_stage_exchange` when that file crossed the
//! file-size threshold. It is a pure `ProtocolState` to copy mapping with no
//! other dependency on the surface builder, so it splits cleanly.

use vauchi_core::exchange::ProtocolState;

use crate::i18n::{Locale, get_string, get_string_with_args};

/// The own-QR caption, which doubles as the exchange status.
///
/// "Show this" while waiting for a peer, then a short live-progress
/// string once the exchange is running (e.g. "Transferring 3/5"). Folding
/// the status into the QR label replaced the separate `StatusIndicator`
/// row so the non-scrolling exchange layout fits the full-width QR +
/// camera + buttons on a compact screen. Pure helper so frontend tests
/// assert on the same per-state mapping the engine emits.
///
/// Proximity (audio/accel) narration is intentionally not surfaced here —
/// the caption stays short enough to sit under the QR.
pub(crate) fn own_qr_label(state: &ProtocolState, locale: Locale) -> String {
    match state {
        ProtocolState::Idle | ProtocolState::Advertising => {
            get_string(locale, "multi_stage.own_qr_show_this")
        }
        ProtocolState::Discovered => get_string(locale, "multi_stage.own_qr_connecting"),
        ProtocolState::Transferring {
            chunks_sent,
            chunks_total,
            chunks_received,
            peer_chunks_total,
        } => {
            // Both directions, because an exchange is two transfers and a
            // single number cannot say which it counts. On device a peer
            // reading "Transferring 1/3" was equally consistent with having
            // sent one chunk or received one, and those imply opposite
            // diagnoses — with iOS logs unavailable, this label is the only
            // instrument on that device
            // (`2026-08-18-hover-transfer-stalls-on-the-last-chunk`).
            if *chunks_total > 0 && *peer_chunks_total > 0 {
                get_string_with_args(
                    locale,
                    "multi_stage.own_qr_transferring_both",
                    &[
                        ("sent", &chunks_sent.to_string()),
                        ("total", &chunks_total.to_string()),
                        ("recv", &chunks_received.to_string()),
                        ("peer_total", &peer_chunks_total.to_string()),
                    ],
                )
            } else if *chunks_total > 0 {
                // The peer's size is unknown until its first chunk arrives.
                get_string_with_args(
                    locale,
                    "multi_stage.own_qr_transferring_progress",
                    &[
                        ("sent", &chunks_sent.to_string()),
                        ("total", &chunks_total.to_string()),
                    ],
                )
            } else {
                get_string(locale, "multi_stage.own_qr_transferring_ellipsis")
            }
        }
        ProtocolState::Verifying => get_string(locale, "multi_stage.own_qr_verifying"),
        ProtocolState::Confirming => get_string(locale, "multi_stage.own_qr_confirming"),
        ProtocolState::Complete | ProtocolState::RetryReady | ProtocolState::Finalized => {
            get_string(locale, "multi_stage.own_qr_almost_done")
        }
        ProtocolState::Failed(_) => get_string(locale, "exchange.failed_title"),
        // ProtocolState is #[non_exhaustive]; future variants surface a
        // generic caption until they get dedicated copy.
        _ => get_string(locale, "multi_stage.own_qr_working"),
    }
}

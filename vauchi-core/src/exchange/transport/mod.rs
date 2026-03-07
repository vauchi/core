// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pluggable transport layer for exchange protocols.
//!
//! Provides a unified [`TransportChannel`] trait that all transports implement,
//! with automatic negotiation, fallback chains, and diagnostic tooling.

pub mod animated_qr;
pub mod caps;
pub mod channel;
pub mod diagnostics;
pub mod mock;
pub mod negotiation;
pub mod orchestrator;
pub mod protocol;
pub mod trace;
pub mod wifi_aware;

pub use caps::TransportCaps;
pub use channel::{PeerInfo, TransportChannel, TransportError, TransportType};
pub use mock::MockTransportChannel;
pub use negotiation::negotiate_transport;
pub use orchestrator::{FallbackPolicy, TransportChain};

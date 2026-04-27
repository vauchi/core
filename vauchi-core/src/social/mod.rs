// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Social Network Support
//!
//! Provides a registry of known social networks with profile URL templates.

#[cfg(feature = "testing")]
pub mod registry;
#[cfg(not(feature = "testing"))]
mod registry;

pub use registry::{SocialNetwork, SocialNetworkRegistry};

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Social Network Support
//!
//! Provides a registry of known social networks with profile URL templates.

#[cfg(feature = "testing")]
pub mod registry;
#[cfg(not(feature = "testing"))]
mod registry;

mod network_data;

pub(crate) use network_data::is_valid_network_key;
pub use network_data::{NetworkDataError, UsernameCharset, UsernameRule};
pub use registry::{SocialNetwork, SocialNetworkRegistry};

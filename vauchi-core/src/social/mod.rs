// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Social Network Support
//!
//! This module provides:
//! - A registry of known social networks with profile URL templates
//! - Crowd-sourced validation of social profile ownership

#[cfg(feature = "testing")]
pub mod registry;
#[cfg(not(feature = "testing"))]
mod registry;

#[cfg(feature = "testing")]
pub mod validation;
#[cfg(not(feature = "testing"))]
mod validation;

pub use registry::{SocialNetwork, SocialNetworkRegistry};
pub use validation::{ProfileValidation, TrustLevel, ValidationStatus};

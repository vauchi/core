//! Social Network Support
//!
//! This module provides:
//! - A registry of known social networks with profile URL templates
//! - Crowd-sourced validation of social profile ownership

mod registry;
mod validation;

pub use registry::{SocialNetwork, SocialNetworkRegistry};
pub use validation::{ProfileValidation, TrustLevel, ValidationStatus};

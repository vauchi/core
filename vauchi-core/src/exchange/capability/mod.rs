// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device Capability and Feature Gating Module
//!
//! Provides types and logic for hardware-dependent feature gating.
//! The `FeatureGate` evaluates static device capabilities and dynamic
//! runtime state to determine which features are available.
//!
//! ## Design
//!
//! - `DeviceCapabilities`: Static hardware inventory (NFC, BLE, camera, etc.)
//! - `RuntimeStateProvider`: Trait for dynamic state callbacks (battery, network, storage)
//! - `FeatureGate`: Evaluates capabilities + runtime state to gate features
//!
//! ## Invariant
//!
//! QR display is always available regardless of device capabilities.
//! At least one exchange method is always available.

pub mod gate;
pub mod readiness;
pub mod runtime;
pub mod types;

pub use gate::{Action, ActionStatus, Feature, FeatureGate, FeatureStatus};
pub use readiness::{PermissionState, RequirementReadiness, TransportReadiness};
pub use runtime::{ConnectionType, RuntimeStateProvider};
pub use types::{BiometricType, DeviceCapabilities, Platform};

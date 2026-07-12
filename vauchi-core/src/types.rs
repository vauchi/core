// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compatibility facade for types that have moved to cohesive modules.
//!
//! Historically a shared-types grab-bag; each type now lives in its own
//! domain or neutral-leaf module (see the re-exports below). This facade
//! keeps the `crate::types::` path — and the crate-root re-exports in
//! `lib.rs` — stable, so no consumer changed. New code should import from
//! the owning module directly.
//!
//! The neutral-leaf modules (`emergency`, `settings`, `reminders`,
//! `exchange_types`, `visibility`, `consent`) preserve the original
//! anti-cycle intent: they are always compiled and depend on nothing, so
//! the storage↔api and contact↔contact_card cycles these types once
//! guarded against stay broken.

pub use crate::consent::{ConsentRecord, ConsentType};
pub use crate::emergency::{
    BiometricUnlockOutcome, DEFAULT_EMERGENCY_MESSAGE, DuressSettings, EmergencyBroadcastConfig,
    EmergencyWipeStatus, MAX_TRUSTED_CONTACTS,
};
pub use crate::exchange::ExchangeDefaults;
pub use crate::exchange_types::{
    AudioCapability, EventOrigin, ExchangeTransport, ProximityConfidence,
};
pub use crate::onboarding::{
    AhaMomentTracker, AhaMomentType, DemoContactState, OnboardingProgress, OnboardingStep,
};
pub use crate::reminders::{BackupReminderState, OwnCardRepropagateState, ReminderFrequency};
pub use crate::settings::SettingsFlags;
pub use crate::visibility::{FieldVisibility, VisibilityRules};

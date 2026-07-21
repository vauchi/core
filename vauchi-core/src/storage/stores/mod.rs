// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Per-domain persistence views.
//!
//! Each `*Store<'a>` borrows the shared connection, encryption key, and clock
//! from [`Storage`](super::Storage) and exposes only one domain's operations,
//! turning the domain↔persistence boundary into a type-level one. Stores are
//! constructed solely by accessors on `Storage` (e.g. `Storage::recovery`).
//!
//! Rollout is phased — see problem record
//! `2026-06-09-storage-per-domain-store-boundaries`.

mod activity_log;
mod consent;
mod contact;
mod contact_display;
mod contact_ops;
mod contact_row;
mod decoy;
mod delivery;
mod device;
mod device_delivery;
mod duress;
mod emergency;
mod field_notes;
mod identity;
mod labels;
mod ohttp_cache;
mod pending;
mod pin_cache;
mod place;
mod ratchet;
mod recovery;
mod replay;
mod retry;
mod safety_alert;
mod sync;
mod tag;
mod ux;

pub use activity_log::ActivityLogStore;
pub use consent::ConsentStore;
pub use contact::ContactStore;
pub use decoy::DecoyStore;
pub use delivery::DeliveryStore;
pub use device::DeviceStore;
pub use device_delivery::DeviceDeliveryStore;
pub use duress::DuressStore;
pub use emergency::EmergencyStore;
pub use field_notes::FieldNoteStore;
pub use identity::IdentityStore;
pub use labels::LabelStore;
pub use ohttp_cache::OhttpCacheStore;
pub use pending::PendingStore;
pub use pin_cache::PinCacheStore;
pub use place::PlaceStore;
pub use ratchet::RatchetStore;
pub use recovery::RecoveryStore;
pub use replay::ReplayStore;
pub use retry::RetryStore;
pub use safety_alert::{GenesisFactWrite, SafetyAlertFactStore, StoredSafetyAlertFact};
pub use sync::SyncStore;
pub use tag::TagStore;
pub use ux::UxStore;

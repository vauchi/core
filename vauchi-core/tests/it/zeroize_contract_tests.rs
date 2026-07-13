// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! VRS01 — compile-time zeroize contract (ADR-002/ADR-019).
//!
//! Every type carrying secret key material must implement
//! `ZeroizeOnDrop`; this file is the enumerated, reviewable list. A
//! listed type without the trait fails to compile. The runtime contract also
//! requires drop glue, preventing a marker-only `ZeroizeOnDrop`
//! implementation from passing. New secret types must be added here in the
//! same MR that introduces them.
//! Page: `_private/docs/lint-errors/vrs01-zeroize-contract.md`
//!
//! Deliberately NOT listed (each with its reason):
//! - x3dh key containers — hold dalek `StaticSecret`, which zeroizes
//!   itself on drop; the container cannot derive `Zeroize` because
//!   `StaticSecret` does not expose it (field-level guarantee).
//! - `ConfirmationEscrowKeys` — carries only public hex gate/slot
//!   hashes, no key material.
//! - `DoubleRatchetState` — secret material at rest (ADR-015) but a
//!   compound state whose derive needs per-field work; tracked in
//!   `problems/2026-07-05-prose-rules-to-deterministic-lints/`.

use zeroize::ZeroizeOnDrop;

fn has_zeroizing_drop<T: ZeroizeOnDrop>() -> bool {
    std::mem::needs_drop::<T>()
}

// @internal
#[test]
fn secret_type_contract() {
    assert!(has_zeroizing_drop::<vauchi_core::crypto::SymmetricKey>());
    assert!(has_zeroizing_drop::<
        vauchi_core::crypto::signing::SigningKeyPair,
    >());
    assert!(has_zeroizing_drop::<
        vauchi_core::crypto::shredding::ShreddingMasterKey,
    >());
    assert!(has_zeroizing_drop::<vauchi_core::crypto::ChainKey>());
    assert!(has_zeroizing_drop::<vauchi_core::crypto::MessageKey>());
    assert!(has_zeroizing_drop::<
        vauchi_core::crypto::cek::ContentEncryptionKey,
    >());
    assert!(has_zeroizing_drop::<
        vauchi_core::exchange::transport::protocol::SharedKey,
    >());
    assert!(has_zeroizing_drop::<vauchi_core::identifiers::MailboxToken>());
    assert!(has_zeroizing_drop::<
        vauchi_core::sync::device_sync::ContactSyncData,
    >());
    assert!(has_zeroizing_drop::<
        vauchi_core::exchange::escrow::EscrowKeys,
    >());
    assert!(has_zeroizing_drop::<vauchi_core::crypto::shamir::Share>());
    assert!(has_zeroizing_drop::<vauchi_core::BackupKeyShard>());
}

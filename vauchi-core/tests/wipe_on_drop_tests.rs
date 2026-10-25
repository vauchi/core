// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Key material is gone from memory once its owner is dropped
//! (ADR-002, vauchi/private#817).
//!
//! The compile-time contract (`it/zeroize_contract_tests.rs`, VRS01) only
//! sees that a type has a wiping `Drop`; these tests drop a value where it
//! stands and read its storage back. They use `unsafe` for that read, which
//! the library forbids; each read is of initialized bytes. `test:formal`
//! runs the file under Miri to show it, and in release, where dead-store
//! elimination could remove an unguarded wipe:
//! `cargo +nightly miri test -p vauchi-core --no-default-features
//! --features testing --test wipe_on_drop_tests` and
//! `cargo test --release -p vauchi-core --features testing --test
//! wipe_on_drop_tests`.

use std::mem::{ManuallyDrop, size_of};

use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::crypto::{MessageKey, SymmetricKey, X3DHKeyPair};
#[cfg(feature = "testing")]
use vauchi_core::identity::Identity;

const SECRET: [u8; 32] = [0xA5; 32];

/// Drops `value` where it stands and returns the bytes left in its storage.
/// Only for types without padding, whose bytes are all initialized.
fn bytes_left_after_drop<T>(value: T) -> Vec<u8> {
    let mut slot = ManuallyDrop::new(value);
    // SAFETY: `slot` is dropped exactly once, here, and never used as a `T`
    // again; its storage stays alive until this function returns.
    unsafe { ManuallyDrop::drop(&mut slot) };
    let base = std::ptr::addr_of!(slot).cast::<u8>();
    (0..size_of::<T>())
        // SAFETY: in bounds of `slot`. The callers pass types whose bytes are
        // all initialized after drop (Miri checks this); volatile so the read
        // is not folded into what the compiler believes the drop left.
        .map(|i| unsafe { base.add(i).read_volatile() })
        .collect()
}

/// Drops `value` where it stands and returns the `N` bytes at `offset` in
/// its storage: for a type with padding, where only a field's bytes are
/// known to be initialized.
#[cfg(feature = "testing")]
fn field_left_after_drop<T, const N: usize>(value: T, offset: usize) -> [u8; N] {
    assert!(offset + N <= size_of::<T>());
    let mut slot = ManuallyDrop::new(value);
    // SAFETY: as in `bytes_left_after_drop`.
    unsafe { ManuallyDrop::drop(&mut slot) };
    let base = std::ptr::addr_of!(slot).cast::<u8>();
    // SAFETY: in bounds (asserted above); the caller names a byte-array
    // field, which the drop leaves initialized; [u8; N] needs no alignment.
    unsafe { base.add(offset).cast::<[u8; N]>().read_volatile() }
}

fn still_holds(bytes: &[u8], secret: &[u8]) -> bool {
    bytes.windows(secret.len()).any(|window| window == secret)
}

// @scenario: ble_exchange :: Symmetric key is zeroized on drop
#[test]
fn a_symmetric_key_is_all_zero_after_drop() {
    let left = bytes_left_after_drop(SymmetricKey::from_bytes(SECRET));

    assert_eq!(left, vec![0u8; size_of::<SymmetricKey>()]);
}

// @internal
#[test]
fn a_content_encryption_key_is_all_zero_after_drop() {
    let left = bytes_left_after_drop(ContentEncryptionKey::from_bytes(SECRET));

    assert_eq!(left, vec![0u8; size_of::<ContentEncryptionKey>()]);
}

// @internal
#[test]
fn a_message_key_is_all_zero_after_drop() {
    let left = bytes_left_after_drop(MessageKey::from_bytes(SECRET));

    assert_eq!(left, vec![0u8; size_of::<MessageKey>()]);
}

/// The public half may stay; the secret must not.
// @internal
#[test]
fn an_x3dh_key_pair_holds_no_secret_after_drop() {
    let left = bytes_left_after_drop(X3DHKeyPair::from_bytes(SECRET));

    assert!(!still_holds(&left, &SECRET));
}

/// `shamir` is public only with `testing`; without it (the Miri run) VRS01
/// still checks `Share`'s wipe at compile time.
// @internal
#[cfg(feature = "testing")]
#[test]
fn a_shamir_share_holds_no_secret_after_drop() {
    let mut shares = vauchi_core::crypto::shamir::split(&SECRET, 2, 3).unwrap();
    let share = shares.swap_remove(0);
    let value = *share.value();

    let left = bytes_left_after_drop(share);

    assert!(!still_holds(&left, &value));
}

/// Replaces `identity_drop_zeroizes_seed`, which could not observe the
/// wipe it was named for. `Identity` has padding, so only the seed's own
/// bytes are read back (Miri rejects reading the whole struct).
// @internal
#[cfg(feature = "testing")]
#[test]
fn an_identity_master_seed_is_all_zero_after_drop() {
    let identity = Identity::create("Alice", 0);
    assert_ne!(*identity.master_seed(), [0u8; 32]);

    let left: [u8; 32] = field_left_after_drop(identity, Identity::MASTER_SEED_OFFSET);

    assert_eq!(left, [0u8; 32]);
}

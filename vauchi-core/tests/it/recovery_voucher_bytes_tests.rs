// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A voucher carrying a guardian token survives a round trip and is
//! refused, not panicked on, when its token bytes are cut short
//! (vauchi/private#522).

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::recovery::guardian::GuardianToken;
use vauchi_core::*;

fn voucher_with_token() -> RecoveryVoucher {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let claim = RecoveryClaim::new(&old_pk, &new_pk, 0);
    let voucher_keys = SigningKeyPair::generate();
    let token = GuardianToken::create(&SigningKeyPair::generate(), voucher_keys.public_key(), 0);
    RecoveryVoucher::create_from_claim(&claim, &voucher_keys, Some(token), 0).unwrap()
}

// @internal
#[test]
fn a_voucher_with_a_guardian_token_round_trips() {
    let voucher = voucher_with_token();
    let bytes = voucher.to_bytes();

    let restored = RecoveryVoucher::from_bytes(&bytes).unwrap();

    assert_eq!(
        restored.guardian_token().map(|t| t.to_bytes()),
        voucher.guardian_token().map(|t| t.to_bytes())
    );
}

/// A token length near `u32::MAX` makes `173 + token_len` wrap on a 32-bit
/// `usize`, so the length check passes and the slice panics (#784, the
/// shape of #756). Anyone can hand over a voucher.
// @internal
#[test]
fn a_voucher_whose_token_length_wraps_a_32_bit_usize_is_refused() {
    let bytes = voucher_with_token().to_bytes();

    for declared in [u32::MAX, u32::MAX - 172, u32::MAX - 100, 0x8000_0000] {
        let mut forged = bytes.clone();
        forged[169..173].copy_from_slice(&declared.to_le_bytes());
        assert!(
            matches!(
                RecoveryVoucher::from_bytes(&forged),
                Err(RecoveryError::InvalidFormat)
            ),
            "declared token length {declared:#x}"
        );
    }
}

// @internal
#[test]
fn a_voucher_whose_token_is_cut_short_is_refused() {
    let bytes = voucher_with_token().to_bytes();

    for cut in 1..=4 {
        let truncated = &bytes[..bytes.len() - cut];
        assert!(
            matches!(
                RecoveryVoucher::from_bytes(truncated),
                Err(RecoveryError::InvalidFormat)
            ),
            "cut {cut} bytes"
        );
    }
}

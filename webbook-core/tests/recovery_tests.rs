//! Tests for recovery
//! Extracted from mod.rs

use webbook_core::*;

#[test]
fn test_recovery_claim_roundtrip() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];

    let claim = RecoveryClaim::new(&old_pk, &new_pk);
    let bytes = claim.to_bytes();
    let restored = RecoveryClaim::from_bytes(&bytes).unwrap();

    assert_eq!(restored.old_pk(), &old_pk);
    assert_eq!(restored.new_pk(), &new_pk);
}

#[test]
fn test_recovery_voucher_roundtrip() {
    let old_pk = [0x01u8; 32];
    let new_pk = [0x02u8; 32];
    let keypair = SigningKeyPair::generate();

    let voucher = RecoveryVoucher::create(&old_pk, &new_pk, &keypair);
    let bytes = voucher.to_bytes();
    let restored = RecoveryVoucher::from_bytes(&bytes).unwrap();

    assert!(restored.verify());
}

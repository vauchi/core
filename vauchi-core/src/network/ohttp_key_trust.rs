// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Whether a fetched OHTTP gateway key may be used (#288).
//!
//! The outer relay sits on the key-fetch path, so a key is trusted only
//! through the signature chain anchor → intermediate → (window, KeyConfig),
//! never because of where it came from. Bootstrap and every refetch go
//! through [`accept_signed_key`]; there is no unverified fallback.

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use vauchi_protocol::ohttp_key::{
    AnchorRollover, IntermediateCert, SignedKeyConfig, backup_commitment_message,
    key_id_for_window, window_of,
};

/// The gateway key a client holds, and the window it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldOhttpKey {
    pub window: u64,
    pub key_config: Vec<u8>,
}

/// Why a signed key record was refused. Messages carry windows and key ids
/// only, never key bytes (DC-05).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OhttpKeyRejection {
    #[error("the OHTTP intermediate is not signed by the trust anchor")]
    IntermediateSignature,
    #[error("the OHTTP intermediate is not valid yet")]
    IntermediateNotYetValid,
    #[error("the OHTTP intermediate has expired")]
    IntermediateExpired,
    #[error("the OHTTP key is not signed by its intermediate")]
    KeySignature,
    #[error("OHTTP key window {window} is outside current window {current} ±1")]
    WindowOutOfRange { window: u64, current: u64 },
    #[error("OHTTP key id {found} does not match window key id {expected}")]
    KeyIdMismatch { expected: u8, found: u8 },
    #[error("a different OHTTP key was offered for held window {window}")]
    Equivocation { window: u64 },
    #[error("OHTTP key window {window} is older than held window {held}")]
    OlderThanHeld { window: u64, held: u64 },
}

/// Accept `record` only if the anchor certifies its intermediate for `now`,
/// the intermediate signed (window, KeyConfig), the window is within one of
/// the current one and matches the key id, and it neither predates nor
/// contradicts the key already `held`.
pub fn accept_signed_key(
    record: &SignedKeyConfig,
    anchor: &[u8; 32],
    now_unix: u64,
    held: Option<&HeldOhttpKey>,
) -> Result<HeldOhttpKey, OhttpKeyRejection> {
    let intermediate = verify_intermediate(&record.intermediate, anchor, now_unix)?;
    verify(
        &intermediate,
        &SignedKeyConfig::signing_message(record.window, &record.key_config),
        &record.signature,
    )
    .ok_or(OhttpKeyRejection::KeySignature)?;

    let current = window_of(now_unix);
    if record.window.abs_diff(current) > 1 {
        return Err(OhttpKeyRejection::WindowOutOfRange {
            window: record.window,
            current,
        });
    }
    let expected = key_id_for_window(record.window);
    // The decoder guarantees a non-empty KeyConfig; its first byte is the
    // RFC 9458 key id.
    let found = record.key_config.first().copied().unwrap_or(!expected);
    if found != expected {
        return Err(OhttpKeyRejection::KeyIdMismatch { expected, found });
    }

    if let Some(held) = held {
        if record.window < held.window {
            return Err(OhttpKeyRejection::OlderThanHeld {
                window: record.window,
                held: held.window,
            });
        }
        if record.window == held.window && record.key_config != held.key_config {
            return Err(OhttpKeyRejection::Equivocation {
                window: record.window,
            });
        }
    }

    Ok(HeldOhttpKey {
        window: record.window,
        key_config: record.key_config.clone(),
    })
}

fn verify_intermediate(
    cert: &IntermediateCert,
    anchor: &[u8; 32],
    now_unix: u64,
) -> Result<VerifyingKey, OhttpKeyRejection> {
    let anchor =
        VerifyingKey::from_bytes(anchor).map_err(|_| OhttpKeyRejection::IntermediateSignature)?;
    verify(
        &anchor,
        &IntermediateCert::signing_message(&cert.public_key, cert.not_before, cert.not_after),
        &cert.anchor_signature,
    )
    .ok_or(OhttpKeyRejection::IntermediateSignature)?;
    if now_unix < cert.not_before {
        return Err(OhttpKeyRejection::IntermediateNotYetValid);
    }
    if now_unix > cert.not_after {
        return Err(OhttpKeyRejection::IntermediateExpired);
    }
    VerifyingKey::from_bytes(&cert.public_key).map_err(|_| OhttpKeyRejection::KeySignature)
}

fn verify(key: &VerifyingKey, message: &[u8], signature: &[u8; 64]) -> Option<()> {
    key.verify_strict(message, &Signature::from_bytes(signature))
        .ok()
}

/// A relay's trust anchor and the commitment to the backup that may replace
/// it (decision 0.13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OhttpAnchor {
    pub anchor: [u8; 32],
    pub backup_commitment: [u8; 32],
}

/// Why a rollover chain was refused; `step` is the record's index.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RolloverRejection {
    #[error("OHTTP anchor rollover step {step} names a key the client never committed to")]
    NotTheCommittedBackup { step: usize },
    #[error("OHTTP anchor rollover step {step} is not signed by its new anchor")]
    BadSignature { step: usize },
}

/// The commitment a client holds to a backup anchor:
/// `SHA-256("vauchi-ohttp-backup-anchor-v1" || backup)`.
pub fn backup_commitment(backup_anchor: &[u8; 32]) -> [u8; 32] {
    Sha256::digest(backup_commitment_message(backup_anchor)).into()
}

/// Follow `chain` (oldest first, as the relay serves it) from `held`.
/// Records up to the one that installed `held.anchor` are skipped; every
/// later one must name the committed backup and be signed by it, or the
/// whole chain is refused and `held` stays. The retired anchor signs
/// nothing, so losing it does not strand the client and stealing it does
/// not let the thief choose the successor.
pub fn accept_anchor_rollover(
    held: &OhttpAnchor,
    chain: &[AnchorRollover],
) -> Result<OhttpAnchor, RolloverRejection> {
    let start = chain
        .iter()
        .rposition(|rollover| rollover.new_anchor == held.anchor)
        .map_or(0, |installed| installed + 1);
    let mut current = held.clone();
    for (step, rollover) in chain.iter().enumerate().skip(start) {
        if backup_commitment(&rollover.new_anchor) != current.backup_commitment {
            return Err(RolloverRejection::NotTheCommittedBackup { step });
        }
        let new_anchor = VerifyingKey::from_bytes(&rollover.new_anchor)
            .map_err(|_| RolloverRejection::BadSignature { step })?;
        verify(
            &new_anchor,
            &AnchorRollover::signing_message(&rollover.new_anchor, &rollover.next_commitment),
            &rollover.signature,
        )
        .ok_or(RolloverRejection::BadSignature { step })?;
        current = OhttpAnchor {
            anchor: rollover.new_anchor,
            backup_commitment: rollover.next_commitment,
        };
    }
    Ok(current)
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! OHTTP stale-key error classification for the sync retry path.
//!
//! A stale/rotated OHTTP gateway key surfaces as HTTP 400 (the gateway
//! rejecting decapsulation) or — because the OHTTP relay masks that decap-400
//! as 502 — as HTTP 502. The sync path must detect this on EITHER leg (the
//! receive leg returns a typed `Err`, the send leg folds failures into
//! `Ok { errors }`) and refetch the key once, so key rotation survives without
//! a reinstall. Problem record: 2026-05-25-relay-ohttp-forward-hop-502.

use super::VauchiSyncOutcome;
use crate::api::error::{VauchiError, VauchiResult};

/// Heuristic: does this error look like a stale/rejected OHTTP key?
///
/// Matches HTTP 400/502 or messages containing "ohttp". 400 is the gateway
/// rejecting decapsulation (stale/rotated key); 502 is the OHTTP relay masking
/// that decap-400 as Bad Gateway — it never forwards the upstream status, so a
/// stale key reaches the client as 502, not 400. A false positive costs one
/// extra key fetch (cheap); a false negative just retries on the next sync.
fn is_ohttp_key_error(err: &VauchiError) -> bool {
    matches!(err, VauchiError::Network(ne) if is_ohttp_key_error_msg(&ne.to_string()))
}

/// The 400/502/ohttp stale-key heuristic over a raw error message. Shared by
/// the typed [`is_ohttp_key_error`] (receive leg, `Err`) and
/// [`outcome_has_ohttp_key_error`] (send leg, `Ok { errors }`) so the literal
/// match lives in exactly one place.
fn is_ohttp_key_error_msg(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("400") || m.contains("502") || m.contains("ohttp")
}

/// True when a *successful* sync outcome nonetheless recorded a per-message
/// OHTTP key error in its `errors` list. The send leg folds delivery failures
/// into `VauchiSyncOutcome::Ok { errors }` rather than returning `Err`, so a
/// stale-key 502 on the send leg would otherwise never reach the refetch+retry
/// path the receive leg gets via `Err` (send-phase swallow).
fn outcome_has_ohttp_key_error(outcome: &VauchiSyncOutcome) -> bool {
    matches!(
        outcome,
        VauchiSyncOutcome::Ok { errors, .. } if errors.iter().any(|e| is_ohttp_key_error_msg(e))
    )
}

/// Whether a first sync attempt warrants evicting the cached OHTTP key and
/// retrying once. Covers both legs symmetrically: a receive-leg failure
/// surfaces as a typed `Err`, a send-leg failure as `Ok { errors }`. Without
/// the `Ok` arm a transient forward-hop 502 that only hits the send leg is
/// silently deferred a full sync cadence.
pub(crate) fn should_refetch_key_and_retry(result: &VauchiResult<VauchiSyncOutcome>) -> bool {
    match result {
        Err(e) => is_ohttp_key_error(e),
        Ok(outcome) => outcome_has_ohttp_key_error(outcome),
    }
}

// INLINE_TEST_REQUIRED: is_ohttp_key_error / should_refetch_key_and_retry and
// the heuristic helpers are private / pub(crate) free functions — they cannot
// be reached from the tests/ integration crate without making them fully
// public. Inline tests are the least-invasive way to cover them.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::NetworkError;
    use crate::storage::StorageError;

    // =========================================================================
    // W-3: is_ohttp_key_error heuristic
    // =========================================================================

    // @scenario: ohttp_sync :: key error heuristic matches HTTP 400
    #[test]
    fn test_is_ohttp_key_error_http_400() {
        let err = VauchiError::Network(NetworkError::ConnectionFailed(
            "400 Bad Request".to_string(),
        ));
        assert!(
            is_ohttp_key_error(&err),
            "Network error containing '400' must be classified as an OHTTP key error"
        );
    }

    // @scenario: ohttp_sync :: key error heuristic matches ohttp keyword
    #[test]
    fn test_is_ohttp_key_error_ohttp_in_message() {
        let err = VauchiError::Network(NetworkError::RelayRejected(
            "ohttp decapsulation failed".to_string(),
        ));
        assert!(
            is_ohttp_key_error(&err),
            "Network error containing 'ohttp' must be classified as an OHTTP key error"
        );
    }

    // @scenario: ohttp_sync :: key error heuristic matches HTTP 502 (relay-masked decap failure)
    #[test]
    fn test_is_ohttp_key_error_http_502() {
        // The OHTTP relay maps the gateway's decapsulation rejection (HTTP 400
        // on a stale/rotated key) to 502 Bad Gateway before it reaches the
        // client — it never forwards the upstream status. So a stale key
        // surfaces to the client as 502, not 400, and the stale-key
        // refetch+retry path must trigger on 502 too. Without this, a client
        // holding a stale gateway key never refetches and stays broken across
        // every key rotation until reinstall.
        // Problem record: 2026-05-25-relay-ohttp-forward-hop-502.
        let err = VauchiError::Network(NetworkError::ConnectionFailed("HTTP 502".to_string()));
        assert!(
            is_ohttp_key_error(&err),
            "Network error containing '502' (relay-masked gateway decap failure) must be classified as an OHTTP key error"
        );
    }

    // @scenario: ohttp_sync :: key error heuristic rejects storage errors
    #[test]
    fn test_is_ohttp_key_error_storage_error_is_false() {
        let err = VauchiError::Storage(StorageError::NotFound("key".to_string()));
        assert!(
            !is_ohttp_key_error(&err),
            "Storage error must NOT be classified as an OHTTP key error"
        );
    }

    // @scenario: ohttp_sync :: key error heuristic rejects connection refused
    #[test]
    fn test_is_ohttp_key_error_connection_refused_is_false() {
        let err = VauchiError::Network(NetworkError::ConnectionFailed(
            "connection refused".to_string(),
        ));
        assert!(
            !is_ohttp_key_error(&err),
            "Connection-refused error must NOT be classified as an OHTTP key error"
        );
    }

    // @scenario: ohttp_sync :: key error heuristic rejects timeout
    #[test]
    fn test_is_ohttp_key_error_timeout_is_false() {
        let err = VauchiError::Network(NetworkError::Timeout);
        assert!(
            !is_ohttp_key_error(&err),
            "Timeout error must NOT be classified as an OHTTP key error"
        );
    }

    // =========================================================================
    // W-3b: send-leg OHTTP key-error retry routing
    // (2026-05-25-relay-ohttp-forward-hop-502 — send-phase swallow)
    // =========================================================================

    fn ok_outcome_with_errors(errors: Vec<String>) -> VauchiSyncOutcome {
        VauchiSyncOutcome::Ok {
            received: 0,
            fetched: 0,
            rejected: 0,
            unresolved: 0,
            reject_reasons: String::new(),
            sent: 0,
            acknowledged: 0,
            errors,
            version_policy: None,
        }
    }

    // @scenario: ohttp_sync :: send-leg 502 in Ok{errors} triggers key refetch+retry
    #[test]
    fn test_should_refetch_on_send_leg_502_in_ok_errors() {
        let result: VauchiResult<VauchiSyncOutcome> = Ok(ok_outcome_with_errors(vec![
            "alice: network error: HTTP 502".to_string(),
        ]));
        assert!(
            should_refetch_key_and_retry(&result),
            "A send-leg 502 recorded in Ok{{errors}} must trigger the same key refetch+retry as a receive-leg Err — otherwise the send leg defers a full sync cadence (send-phase swallow)"
        );
    }

    // @scenario: ohttp_sync :: receive-leg 502 Err still triggers refetch+retry
    #[test]
    fn test_should_refetch_on_receive_leg_502_err() {
        let result: VauchiResult<VauchiSyncOutcome> = Err(VauchiError::Network(
            NetworkError::ConnectionFailed("HTTP 502".to_string()),
        ));
        assert!(
            should_refetch_key_and_retry(&result),
            "A receive-leg 502 (typed Err) must trigger key refetch+retry"
        );
    }

    // @scenario: ohttp_sync :: clean Ok does not trigger refetch
    #[test]
    fn test_should_not_refetch_on_clean_ok() {
        let result: VauchiResult<VauchiSyncOutcome> = Ok(ok_outcome_with_errors(vec![]));
        assert!(
            !should_refetch_key_and_retry(&result),
            "A clean sync (no errors) must NOT refetch the key"
        );
    }

    // @scenario: ohttp_sync :: unrelated send error does not trigger refetch
    #[test]
    fn test_should_not_refetch_on_unrelated_send_error() {
        let result: VauchiResult<VauchiSyncOutcome> = Ok(ok_outcome_with_errors(vec![
            "alice: contact not found".to_string(),
        ]));
        assert!(
            !should_refetch_key_and_retry(&result),
            "A non-key send error must NOT evict the OHTTP key"
        );
    }

    // @scenario: ohttp_sync :: non-Ok outcome variant carries no send errors
    #[test]
    fn test_should_not_refetch_on_not_connected() {
        let result: VauchiResult<VauchiSyncOutcome> = Ok(VauchiSyncOutcome::NotConnected);
        assert!(
            !should_refetch_key_and_retry(&result),
            "A non-Ok outcome (NotConnected) carries no send errors and must NOT refetch"
        );
    }

    // @scenario: ohttp_sync :: storage Err is not an OHTTP key error
    #[test]
    fn test_should_not_refetch_on_storage_err() {
        let result: VauchiResult<VauchiSyncOutcome> = Err(VauchiError::Storage(
            StorageError::NotFound("x".to_string()),
        ));
        assert!(
            !should_refetch_key_and_retry(&result),
            "A storage Err is not an OHTTP key error and must NOT refetch"
        );
    }
}

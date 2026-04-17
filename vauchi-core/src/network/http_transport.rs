// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP Transport for Relay v2 Protocol
//!
//! Sync HTTP client using `ureq` for the v2 relay API.
//! Replaces WebSocket for relay communication — request/response model
//! suited for contact card sync (not real-time chat).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use vauchi_protocol::v2::{
    FetchedBlob, V2AckRequest, V2ExchangeClaimRequest, V2ExchangeCompleteRequest,
    V2ExchangeOfferRequest, V2FetchRequest, V2GuardianDeleteRequest, V2GuardianEntry,
    V2GuardianQueryRequest, V2GuardianStoreRequest, V2PurgeRequest, V2RecoveryStoreRequest,
    V2Response, V2SendRequest,
};

use super::error::NetworkError;
use crate::version::{APP_COMPAT_VERSION, VersionPolicy};

/// Default retry delay when the server doesn't specify Retry-After.
const DEFAULT_RATE_LIMIT_RETRY_SECS: u64 = 10;

/// Verify and parse a signed pin-config response.
///
/// **Wire format**: `[64-byte Ed25519 signature][N * 32-byte SPKI fingerprints]`
///
/// The signature covers the pin data (everything after the first 64 bytes).
/// This prevents a MITM — even one passing the current TLS pin check —
/// from injecting replacement pins into the cache.
///
/// Requires at least one pin (96 bytes minimum: 64 sig + 32 pin).
pub fn verify_signed_pin_config(
    body: &[u8],
    verify_key: &[u8; 32],
) -> Result<Vec<PinnedCertificate>, NetworkError> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    // Must have at least a 64-byte signature + at least one 32-byte pin
    if body.len() < 64 + 32 {
        return Err(NetworkError::InvalidMessage(
            "pin-config response too short: need 64-byte signature + at least one pin".into(),
        ));
    }

    let (sig_bytes, pin_data) = body.split_at(64);
    if pin_data.len() % 32 != 0 {
        return Err(NetworkError::InvalidMessage(
            "pin-config pin data must be a multiple of 32 bytes".into(),
        ));
    }

    // Verify Ed25519 signature over the pin data
    let vk = VerifyingKey::from_bytes(verify_key)
        .map_err(|e| NetworkError::InvalidMessage(format!("invalid pin-config verify key: {e}")))?;
    let sig = Signature::from_slice(sig_bytes)
        .map_err(|e| NetworkError::InvalidMessage(format!("invalid pin-config signature: {e}")))?;
    vk.verify(pin_data, &sig).map_err(|_| {
        NetworkError::InvalidMessage(
            "pin-config signature verification failed — relay response is not authentic".into(),
        )
    })?;

    Ok(pin_data
        .chunks_exact(32)
        .map(|chunk| {
            let mut fingerprint = [0u8; 32];
            fingerprint.copy_from_slice(chunk);
            PinnedCertificate::new(fingerprint)
        })
        .collect())
}

/// Convert a relay error response to the appropriate NetworkError.
/// Detects rate limit errors by checking the error string.
fn response_error(action: &str, error_msg: &str) -> NetworkError {
    if error_msg.contains("rate limit") || error_msg.contains("quota exceeded") {
        NetworkError::RateLimited {
            retry_after_secs: DEFAULT_RATE_LIMIT_RETRY_SECS,
        }
    } else {
        NetworkError::InvalidMessage(format!("{action} failed: {error_msg}"))
    }
}
use super::ohttp_client::OhttpClient;
use super::pinning::PinnedCertificate;
use super::transport::ProxyConfig;

/// Configuration for the HTTP transport.
#[derive(Debug, Clone)]
pub struct HttpTransportConfig {
    /// Base URL of the relay (or OHTTP relay).
    /// Example: `https://ohttp-relay.example.com`
    pub relay_url: String,
    /// Request timeout in milliseconds.
    pub timeout_ms: u64,
    /// Optional SOCKS5 or HTTP CONNECT proxy.
    pub proxy: ProxyConfig,
    /// Allow direct (non-OHTTP) data requests.
    ///
    /// When `false` (default), `post_action` returns an error if OHTTP is not
    /// configured. This prevents accidental IP leaks. Set to `true` only for
    /// testing or when the user explicitly opts in.
    pub allow_direct: bool,
    /// SPKI SHA-256 pins for the TLS leaf certificate.
    ///
    /// When non-empty, every TLS connection verifies the server's SPKI
    /// against these pins after standard CA validation. Fail-closed on
    /// mismatch. Empty means no pinning (CA-only validation).
    pub pinned_certs: Vec<PinnedCertificate>,
}

impl Default for HttpTransportConfig {
    fn default() -> Self {
        Self {
            relay_url: String::new(),
            timeout_ms: 30_000,
            proxy: ProxyConfig::None,
            allow_direct: false,
            pinned_certs: Vec::new(),
        }
    }
}

impl HttpTransportConfig {
    /// Test-only constructor that sets `allow_direct: true` with no pins
    /// or proxy, so tests can hit a local relay over plain HTTP.
    ///
    /// Production callers must never use this — `allow_direct: true` leaks
    /// the client's source IP to the relay, defeating ADR-037. The CI
    /// `check-no-allow-direct` lint treats every non-test `allow_direct: true`
    /// as a hard failure; this helper exists so tests do not trip it, and
    /// the lint's allow-list explicitly names `for_testing`.
    ///
    /// Not feature-gated because downstream test binaries (vauchi-platform,
    /// e2e) consume vauchi-core as a regular dependency where `cfg(test)`
    /// is not active; the lint guard is the real enforcement mechanism.
    pub fn for_testing(relay_url: impl Into<String>, timeout_ms: u64) -> Self {
        Self {
            relay_url: relay_url.into(),
            timeout_ms,
            proxy: ProxyConfig::None,
            allow_direct: true,
            pinned_certs: Vec::new(),
        }
    }
}

/// HTTP transport for relay v2 protocol.
///
/// Uses `ureq` (sync HTTP) — no async runtime required. Each method
/// is a single HTTP request/response round-trip.
///
/// When an [`OhttpClient`] is set, all data requests (send, fetch, ack,
/// register, purge) are encrypted via OHTTP and sent to `/v2/ohttp`.
/// Health checks always use the direct endpoint.
pub struct HttpTransport {
    config: HttpTransportConfig,
    agent: Result<ureq::Agent, NetworkError>,
    ohttp: Option<OhttpClient>,
    last_version_policy: Mutex<Option<VersionPolicy>>,
    /// Count of `post_action` calls that took the direct-HTTP branch
    /// because OHTTP was not configured. Every tick represents one
    /// request where the relay saw the client's source IP — a privacy
    /// degradation. Frontends and diagnostics should surface this
    /// counter; the 2026-04-17 audit remediation (problem record
    /// `2026-04-17-ohttp-allow-direct-fallback`) defines the expected
    /// steady-state value as zero.
    direct_fallback_count: AtomicU64,
}

impl HttpTransport {
    /// Creates a new HTTP transport with the given configuration.
    ///
    /// The ureq agent (with proxy and timeout) is built once and reused
    /// for all requests, enabling TCP/TLS connection pooling.
    pub fn new(config: HttpTransportConfig) -> Self {
        let agent = Self::build_agent_from_config(&config);
        Self {
            config,
            agent,
            ohttp: None,
            last_version_policy: Mutex::new(None),
            direct_fallback_count: AtomicU64::new(0),
        }
    }

    /// Number of requests that have fallen back to direct HTTP because
    /// OHTTP was not configured. See the `direct_fallback_count` field
    /// docstring for the privacy interpretation.
    pub fn direct_fallback_count(&self) -> u64 {
        self.direct_fallback_count.load(Ordering::Relaxed)
    }

    /// Set the OHTTP client for encrypted requests.
    ///
    /// When set, all data requests are encrypted via OHTTP. Call with a fresh
    /// client when the gateway key rotates (HTTP 400 on stale key).
    ///
    /// TODO(OHTTP-03): The caller that fetches the OHTTP gateway key should
    /// validate the `Key-Fingerprint` response header against a pinned value
    /// before passing the key here.
    pub fn set_ohttp(&mut self, client: OhttpClient) {
        self.ohttp = Some(client);
    }

    /// Remove the OHTTP client, reverting to direct HTTP requests.
    ///
    /// Use when key bootstrap fails during rotation and the transport needs
    /// to fall back to re-fetching the key before re-enabling OHTTP.
    pub fn clear_ohttp(&mut self) {
        self.ohttp = None;
    }

    /// Returns whether OHTTP encryption is active.
    pub fn has_ohttp(&self) -> bool {
        self.ohttp.is_some()
    }

    /// Returns the relay URL.
    pub fn relay_url(&self) -> &str {
        &self.config.relay_url
    }

    /// Returns the proxy configuration.
    pub fn proxy(&self) -> &ProxyConfig {
        &self.config.proxy
    }

    /// Returns the last version policy received from the relay.
    ///
    /// Updated after every successful HTTP response that includes version headers.
    pub fn last_version_policy(&self) -> Option<VersionPolicy> {
        self.last_version_policy.lock().ok()?.clone()
    }

    /// Fetch the version policy from the CDN manifest.
    ///
    /// Returns `None` on any failure (network, parse, etc.) — callers
    /// should fall back to the relay's inline headers.
    pub fn fetch_cdn_version_policy(&self) -> Option<VersionPolicy> {
        const CDN_URL: &str = "https://version.vauchi.app/v1/policy.json";
        let agent = self.agent.as_ref().ok()?;
        let resp = agent.get(CDN_URL).call().ok()?;
        let body: String = resp.into_body().read_to_string().ok()?;
        VersionPolicy::from_cdn_json(&body).ok()
    }

    /// Fetch the relay's OHTTP gateway public key.
    ///
    /// Always uses direct HTTP (not OHTTP) — this is the
    /// bootstrap step before OHTTP can be activated.
    /// Returns the raw encoded key config bytes (RFC 9458).
    pub fn fetch_ohttp_key(&self) -> Result<Vec<u8>, NetworkError> {
        let agent = self.agent.as_ref().map_err(|e| e.clone())?;
        let url = format!("{}/v2/ohttp-key", self.config.relay_url);
        let resp = agent
            .get(&url)
            .header("X-App-Compat-Version", &APP_COMPAT_VERSION.to_string())
            .call()
            .map_err(Self::map_ureq_error)?;
        self.check_response_status(&resp)?;
        let body = resp
            .into_body()
            .read_to_vec()
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        if body.is_empty() {
            return Err(NetworkError::InvalidMessage(
                "empty OHTTP key response".into(),
            ));
        }
        Ok(body)
    }

    /// Fetch the relay's signed certificate pin configuration.
    ///
    /// Returns a list of SPKI SHA-256 fingerprints the relay currently
    /// advertises. Clients merge these with bundled pins to support
    /// key rotation without app updates.
    ///
    /// The `verify_key` is the relay operator's Ed25519 public key,
    /// bundled in `RelayConfig::pin_config_verify_key`. See
    /// [`verify_signed_pin_config`] for the wire format and verification.
    pub fn fetch_pin_config(
        &self,
        verify_key: &[u8; 32],
    ) -> Result<Vec<PinnedCertificate>, NetworkError> {
        let agent = self.agent.as_ref().map_err(|e| e.clone())?;
        let url = format!("{}/v2/pin-config", self.config.relay_url);
        let resp = agent
            .get(&url)
            .header("X-App-Compat-Version", &APP_COMPAT_VERSION.to_string())
            .call()
            .map_err(Self::map_ureq_error)?;
        self.check_response_status(&resp)?;
        let body = resp
            .into_body()
            .read_to_vec()
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        verify_signed_pin_config(&body, verify_key)
    }

    /// Checks if the relay is reachable.
    pub fn health_check(&self) -> Result<(), NetworkError> {
        let url = format!("{}/v2/health", self.config.relay_url);
        let resp: V2Response = self.get_json(&url)?;
        if resp.status == "ok" {
            Ok(())
        } else {
            Err(NetworkError::InvalidMessage(format!(
                "health check failed: {}",
                resp.error.unwrap_or_default()
            )))
        }
    }

    /// Sends an encrypted update to a recipient's mailbox.
    pub fn send_update(
        &self,
        recipient_id: &str,
        ciphertext_b64: &str,
    ) -> Result<String, NetworkError> {
        let req = V2SendRequest {
            recipient_id: recipient_id.to_string(),
            ciphertext: ciphertext_b64.to_string(),
        };
        let resp = self.post_action("send", &req)?;
        if resp.status == "ok" {
            resp.blob_id
                .ok_or_else(|| NetworkError::InvalidMessage("missing blob_id in response".into()))
        } else {
            Err(response_error("send", &resp.error.unwrap_or_default()))
        }
    }

    /// Fetches pending blobs for the given mailbox tokens.
    pub fn fetch(&self, mailbox_tokens: &[String]) -> Result<Vec<FetchedBlob>, NetworkError> {
        let req = V2FetchRequest {
            mailbox_tokens: mailbox_tokens.to_vec(),
        };
        let resp = self.post_action("fetch", &req)?;
        if resp.status == "ok" {
            Ok(resp.blobs.unwrap_or_default())
        } else {
            Err(response_error("fetch", &resp.error.unwrap_or_default()))
        }
    }

    /// Acknowledges receipt of a blob (removes it from the relay).
    pub fn acknowledge(&self, recipient_id: &str, blob_id: &str) -> Result<bool, NetworkError> {
        let req = V2AckRequest {
            recipient_id: recipient_id.to_string(),
            blob_id: blob_id.to_string(),
        };
        let resp = self.post_action("ack", &req)?;
        if resp.status == "ok" {
            Ok(resp.acknowledged.unwrap_or(false))
        } else {
            Err(response_error("ack", &resp.error.unwrap_or_default()))
        }
    }

    /// Purges all blobs for a recipient.
    ///
    /// Requires Ed25519 signature authentication (same as relay-side verification).
    /// Returns the number of blobs deleted by the relay, if reported.
    pub fn purge(
        &self,
        recipient_id: &str,
        public_key: &str,
        purge_token: &str,
        signature: &str,
        timestamp: u64,
    ) -> Result<Option<usize>, NetworkError> {
        let req = V2PurgeRequest {
            recipient_id: recipient_id.to_string(),
            public_key: public_key.to_string(),
            purge_token: purge_token.to_string(),
            signature: signature.to_string(),
            timestamp,
        };
        let resp = self.post_action("purge", &req)?;
        if resp.status == "ok" {
            Ok(resp.blobs_deleted)
        } else {
            Err(response_error("purge", &resp.error.unwrap_or_default()))
        }
    }

    /// Posts an exchange offer payload to the relay.
    ///
    /// Returns the exchange code on success, which the initiator displays
    /// as a short numeric code for the other party to enter.
    pub fn exchange_offer(
        &self,
        payload_b64: &str,
        expires_secs: Option<u64>,
    ) -> Result<String, NetworkError> {
        let req = V2ExchangeOfferRequest {
            payload: payload_b64.to_string(),
            expires_secs,
        };
        let resp = self.post_action("exchange_offer", &req)?;
        if resp.status == "ok" {
            resp.code
                .ok_or_else(|| NetworkError::InvalidMessage("missing code in response".into()))
        } else {
            Err(response_error(
                "exchange_offer",
                &resp.error.unwrap_or_default(),
            ))
        }
    }

    /// Claims an exchange offer by code, submitting the responder's payload.
    ///
    /// Returns the initiator's payload (from the offer) on success.
    pub fn exchange_claim(&self, code: &str, response_b64: &str) -> Result<String, NetworkError> {
        let req = V2ExchangeClaimRequest {
            code: code.to_string(),
            response: response_b64.to_string(),
        };
        let resp = self.post_action("exchange_claim", &req)?;
        if resp.status == "ok" {
            resp.payload
                .ok_or_else(|| NetworkError::InvalidMessage("missing payload in response".into()))
        } else {
            Err(response_error(
                "exchange_claim",
                &resp.error.unwrap_or_default(),
            ))
        }
    }

    /// Polls for the responder's payload after the initiator's offer has been claimed.
    ///
    /// Returns `Ok(Some(payload))` when the responder has claimed the offer,
    /// `Ok(None)` when the offer has not yet been claimed ("not yet claimed"),
    /// or an error for other failure cases.
    pub fn exchange_complete(&self, code: &str) -> Result<Option<String>, NetworkError> {
        let req = V2ExchangeCompleteRequest {
            code: code.to_string(),
        };
        let resp = self.post_action("exchange_complete", &req);
        match resp {
            Ok(r) if r.status == "ok" => Ok(r.response),
            Ok(r) => {
                let err_msg = r.error.unwrap_or_default();
                if err_msg.contains("not yet claimed") {
                    Ok(None)
                } else {
                    Err(response_error("exchange_complete", &err_msg))
                }
            }
            Err(NetworkError::InvalidMessage(ref msg)) if msg.contains("not yet claimed") => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Stores a recovery proof on the relay.
    ///
    /// The proof is opaque to the relay (encrypted). Keyed by a hash of
    /// the old identity's public key.
    pub fn recovery_store(&self, key_hash: &str, proof_data_b64: &str) -> Result<(), NetworkError> {
        let req = V2RecoveryStoreRequest {
            key_hash: key_hash.to_string(),
            proof_data: proof_data_b64.to_string(),
        };
        let resp = self.post_action("recovery_store", &req)?;
        if resp.status == "ok" {
            Ok(())
        } else {
            Err(response_error(
                "recovery_store",
                &resp.error.unwrap_or_default(),
            ))
        }
    }

    /// Stores (replaces) all guardian entries for a hash.
    ///
    /// Guardian queries go through OHTTP to prevent the relay from correlating
    /// the uploading device with the guardian hash.
    pub fn guardian_store(
        &self,
        guardian_hash: &str,
        entries: Vec<V2GuardianEntry>,
    ) -> Result<(), NetworkError> {
        let req = V2GuardianStoreRequest {
            guardian_hash: guardian_hash.to_string(),
            entries,
        };
        let resp = self.post_action("guardian_store", &req)?;
        if resp.status == "ok" {
            Ok(())
        } else {
            Err(response_error(
                "guardian_store",
                &resp.error.unwrap_or_default(),
            ))
        }
    }

    /// Queries guardian entries by hash.
    ///
    /// Returns the encrypted entries (opaque blobs). The caller decrypts
    /// using their X25519 secret key.
    pub fn guardian_query(
        &self,
        guardian_hash: &str,
    ) -> Result<Vec<V2GuardianEntry>, NetworkError> {
        let req = V2GuardianQueryRequest {
            guardian_hash: guardian_hash.to_string(),
        };
        let resp = self.post_action("guardian_query", &req)?;
        if resp.status == "ok" {
            Ok(resp.guardians.unwrap_or_default())
        } else {
            Err(response_error(
                "guardian_query",
                &resp.error.unwrap_or_default(),
            ))
        }
    }

    /// Deletes all guardian entries for a hash (identity purge).
    pub fn guardian_delete(&self, guardian_hash: &str) -> Result<(), NetworkError> {
        let req = V2GuardianDeleteRequest {
            guardian_hash: guardian_hash.to_string(),
        };
        let resp = self.post_action("guardian_delete", &req)?;
        if resp.status == "ok" {
            Ok(())
        } else {
            Err(response_error(
                "guardian_delete",
                &resp.error.unwrap_or_default(),
            ))
        }
    }

    // ── Internal helpers ────────────────────────────────────────────

    /// Post a request, routing through OHTTP if configured.
    ///
    /// When OHTTP is active: serializes as `{"action": action, ...fields}`,
    /// encrypts, POSTs to `/v2/ohttp`, decrypts response.
    /// When OHTTP is not active and `allow_direct` is true: POSTs JSON directly.
    /// When OHTTP is not active and `allow_direct` is false: returns error
    /// (fail-closed — prevents accidental IP leaks).
    fn post_action<Req: Serialize>(
        &self,
        action: &str,
        body: &Req,
    ) -> Result<V2Response, NetworkError> {
        if let Some(ohttp) = &self.ohttp {
            self.post_via_ohttp(ohttp, action, body)
        } else if self.config.allow_direct {
            // Direct-HTTP fallback exposes the caller's source IP to the
            // relay. Count every occurrence so diagnostics can detect
            // the privacy regression (see `direct_fallback_count`).
            self.direct_fallback_count.fetch_add(1, Ordering::Relaxed);
            let url = format!("{}/v2/{action}", self.config.relay_url);
            self.post_json(&url, body)
        } else {
            Err(NetworkError::ConnectionFailed(
                "OHTTP not configured and direct connections are disabled".into(),
            ))
        }
    }

    /// Build the OHTTP inner envelope JSON: merges the serialized body with
    /// `action` and `version` fields.
    fn build_ohttp_envelope<Req: Serialize>(
        action: &str,
        body: &Req,
    ) -> Result<Vec<u8>, NetworkError> {
        let mut inner =
            serde_json::to_value(body).map_err(|e| NetworkError::Serialization(e.to_string()))?;
        let Some(obj) = inner.as_object_mut() else {
            return Err(NetworkError::Serialization(
                "OHTTP inner request must serialize to a JSON object".into(),
            ));
        };
        obj.insert(
            "action".to_string(),
            serde_json::Value::String(action.to_string()),
        );
        obj.insert("version".to_string(), serde_json::Value::Number(2.into()));
        serde_json::to_vec(&inner).map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    /// Build and pad an OHTTP request payload.
    ///
    /// Serializes the action envelope, then pads to a fixed bucket size so the
    /// OHTTP relay cannot distinguish action types by encrypted blob size.
    /// Matches relay-side `padding::unpad()` (relay/src/http_api.rs).
    fn build_padded_ohttp_payload<Req: Serialize>(
        action: &str,
        body: &Req,
    ) -> Result<Vec<u8>, NetworkError> {
        let envelope = Self::build_ohttp_envelope(action, body)?;
        Ok(crate::crypto::padding::pad(&envelope))
    }

    /// Unpad and parse an OHTTP response from the relay.
    ///
    /// The relay pads all OHTTP responses to bucket sizes before encryption
    /// (relay/src/http_api.rs). This reverses that padding before JSON parsing.
    fn parse_padded_ohttp_response(padded: &[u8]) -> Result<V2Response, NetworkError> {
        if !crate::crypto::padding::is_valid_bucket_size(padded.len()) {
            return Err(NetworkError::InvalidMessage(format!(
                "OHTTP response has non-bucket size {} — possible relay bug or tampering",
                padded.len()
            )));
        }
        let unpadded = crate::crypto::padding::unpad(padded).ok_or_else(|| {
            NetworkError::InvalidMessage("invalid padding in OHTTP response".into())
        })?;
        serde_json::from_slice(&unpadded).map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    /// Encrypt a request via OHTTP and decrypt the response.
    fn post_via_ohttp<Req: Serialize>(
        &self,
        ohttp: &OhttpClient,
        action: &str,
        body: &Req,
    ) -> Result<V2Response, NetworkError> {
        let padded_payload = Self::build_padded_ohttp_payload(action, body)?;

        // Encrypt padded payload
        let (encrypted, response_decryptor) = ohttp.encapsulate(&padded_payload)?;

        // POST encrypted blob
        let agent = self.get_agent()?;
        let ohttp_url = format!("{}/v2/ohttp", self.config.relay_url);
        let resp = agent
            .post(&ohttp_url)
            .header("X-App-Compat-Version", &APP_COMPAT_VERSION.to_string())
            .content_type("message/ohttp-req")
            .send(encrypted)
            .map_err(Self::map_ureq_error)?;
        self.check_response_status(&resp)?;

        // Read raw response bytes
        let enc_response = resp
            .into_body()
            .read_to_vec()
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        // Decrypt
        let plain_response = response_decryptor.decapsulate(&enc_response)?;

        // Unpad and parse
        Self::parse_padded_ohttp_response(&plain_response)
    }

    /// Returns a reference to the cached agent, or the construction error.
    fn get_agent(&self) -> Result<&ureq::Agent, NetworkError> {
        self.agent
            .as_ref()
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))
    }

    /// Map a ureq transport error to [`NetworkError`].
    fn map_ureq_error(e: ureq::Error) -> NetworkError {
        NetworkError::ConnectionFailed(e.to_string())
    }

    /// Check the HTTP status code and extract version policy from headers.
    ///
    /// Must be called before consuming the response body.
    /// Extracts version headers on every response, then maps non-2xx to errors.
    fn check_response_status(
        &self,
        resp: &ureq::http::Response<ureq::Body>,
    ) -> Result<(), NetworkError> {
        self.extract_version_policy(resp);

        let status = resp.status().as_u16();
        match status {
            200..=299 => Ok(()),
            426 => {
                let min_version = resp
                    .headers()
                    .get("X-Min-Version")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u16>().ok())
                    .unwrap_or(0);
                Err(NetworkError::UpgradeRequired { min_version })
            }
            429 => {
                let retry_after_secs = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(DEFAULT_RATE_LIMIT_RETRY_SECS);
                Err(NetworkError::RateLimited { retry_after_secs })
            }
            _ => Err(NetworkError::ConnectionFailed(format!("HTTP {status}"))),
        }
    }

    /// Extract version policy from response headers and store it.
    fn extract_version_policy(&self, resp: &ureq::http::Response<ureq::Body>) {
        let min = resp
            .headers()
            .get("X-Min-Version")
            .and_then(|v| v.to_str().ok());
        let warn = resp
            .headers()
            .get("X-Warn-Version")
            .and_then(|v| v.to_str().ok());
        let deadline = resp
            .headers()
            .get("X-Upgrade-Deadline")
            .and_then(|v| v.to_str().ok());

        let policy = VersionPolicy::from_headers(min, warn, deadline);
        if !policy.is_none_policy()
            && let Ok(mut stored) = self.last_version_policy.lock()
        {
            *stored = Some(policy);
        }
    }

    fn post_json<Req: Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &Req,
    ) -> Result<Resp, NetworkError> {
        let agent = self.get_agent()?;
        let resp = agent
            .post(url)
            .header("X-App-Compat-Version", &APP_COMPAT_VERSION.to_string())
            .send_json(body)
            .map_err(Self::map_ureq_error)?;
        self.check_response_status(&resp)?;

        resp.into_body()
            .read_json::<Resp>()
            .map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    fn get_json<Resp: serde::de::DeserializeOwned>(&self, url: &str) -> Result<Resp, NetworkError> {
        let agent = self.get_agent()?;
        let resp = agent
            .get(url)
            .header("X-App-Compat-Version", &APP_COMPAT_VERSION.to_string())
            .call()
            .map_err(Self::map_ureq_error)?;
        self.check_response_status(&resp)?;

        resp.into_body()
            .read_json::<Resp>()
            .map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    /// Build an agent from config. Called once at construction.
    ///
    /// When `pinned_certs` is non-empty, uses a custom rustls `ServerCertVerifier`
    /// that does WebPKI + SPKI pin verification (fail-closed on mismatch).
    fn build_agent_from_config(config: &HttpTransportConfig) -> Result<ureq::Agent, NetworkError> {
        let timeout = Duration::from_millis(config.timeout_ms);

        if !config.pinned_certs.is_empty() {
            // Use pinned agent with custom TLS verifier
            let proxy = Self::build_proxy_from_config(config)?;
            return super::tls_pinning::build_pinned_agent(&config.pinned_certs, timeout, proxy);
        }

        // Standard agent (no pinning)
        let mut builder = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false);

        if let Some(proxy) = Self::build_proxy_from_config(config)? {
            builder = builder.proxy(Some(proxy));
        }

        Ok(builder.build().new_agent())
    }

    fn build_proxy_from_config(
        config: &HttpTransportConfig,
    ) -> Result<Option<ureq::Proxy>, NetworkError> {
        match &config.proxy {
            ProxyConfig::None => Ok(None),
            ProxyConfig::Socks5 {
                host,
                port,
                username,
                password,
            } => {
                let pb = ureq::Proxy::builder(ureq::ProxyProtocol::Socks5)
                    .host(host)
                    .port(*port);
                let pb = if let (Some(u), Some(p)) = (username, password) {
                    pb.username(u).password(p)
                } else {
                    pb
                };
                let proxy = pb.build().map_err(|e| {
                    NetworkError::ConnectionFailed(format!("SOCKS5 proxy config error: {e}"))
                })?;
                Ok(Some(proxy))
            }
            ProxyConfig::HttpConnect { host, port } => {
                let proxy = ureq::Proxy::builder(ureq::ProxyProtocol::Http)
                    .host(host)
                    .port(*port)
                    .build()
                    .map_err(|e| {
                        NetworkError::ConnectionFailed(format!("HTTP proxy config error: {e}"))
                    })?;
                Ok(Some(proxy))
            }
        }
    }
}

// INLINE_TEST_REQUIRED: tests use private HttpTransport internals (config, post_action, build_ohttp_envelope)
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_transport_config_defaults() {
        let config = HttpTransportConfig::default();
        assert!(config.relay_url.is_empty());
        assert_eq!(config.timeout_ms, 30_000);
        assert_eq!(config.proxy, ProxyConfig::None);
        assert!(
            !config.allow_direct,
            "allow_direct must default to false (fail-closed)"
        );
    }

    #[test]
    fn test_http_transport_creation() {
        let transport = HttpTransport::new(HttpTransportConfig::for_testing(
            "http://localhost:8080",
            5000,
        ));
        assert_eq!(transport.relay_url(), "http://localhost:8080");
        assert_eq!(transport.proxy(), &ProxyConfig::None);
    }

    // @internal
    #[test]
    fn test_last_version_policy_initially_none() {
        let transport = HttpTransport::new(HttpTransportConfig::default());
        assert!(
            transport.last_version_policy().is_none(),
            "version policy must be None before any request"
        );
    }

    // @internal
    #[test]
    fn test_upgrade_required_error_format() {
        let err = NetworkError::UpgradeRequired { min_version: 5 };
        assert_eq!(err.to_string(), "Upgrade required: minimum version 5");
    }

    #[test]
    fn test_http_transport_with_socks5_config() {
        let transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://relay.example.com".into(),
            timeout_ms: 5000,
            proxy: ProxyConfig::socks5("127.0.0.1", 1080),
            allow_direct: false,
            pinned_certs: vec![],
        });
        assert!(matches!(transport.proxy(), ProxyConfig::Socks5 { .. }));
    }

    #[test]
    fn test_health_check_connection_refused() {
        // Health check always uses direct endpoint (not OHTTP)
        let transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://127.0.0.1:1".into(),
            timeout_ms: 1000,
            proxy: ProxyConfig::None,
            allow_direct: false,
            pinned_certs: vec![],
        });
        let result = transport.health_check();
        assert!(result.is_err());
    }

    #[test]
    fn test_send_blocked_without_ohttp_or_allow_direct() {
        let transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://127.0.0.1:1".into(),
            timeout_ms: 1000,
            proxy: ProxyConfig::None,
            allow_direct: false,
            pinned_certs: vec![],
        });
        let result = transport.send_update(&"a".repeat(64), "dGVzdA==");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("OHTTP not configured"),
            "should fail-closed without OHTTP, got: {err}"
        );
    }

    #[test]
    fn test_send_update_direct_connection_refused() {
        let transport =
            HttpTransport::new(HttpTransportConfig::for_testing("http://127.0.0.1:1", 1000));
        let result = transport.send_update(&"a".repeat(64), "dGVzdA==");
        assert!(result.is_err());
    }

    // @internal
    #[test]
    fn test_direct_fallback_count_starts_zero() {
        let transport =
            HttpTransport::new(HttpTransportConfig::for_testing("http://127.0.0.1:1", 1000));
        assert_eq!(transport.direct_fallback_count(), 0);
    }

    // @internal
    #[test]
    fn test_direct_fallback_count_increments_on_each_direct_post() {
        let transport =
            HttpTransport::new(HttpTransportConfig::for_testing("http://127.0.0.1:1", 1000));
        // Fails at network layer (connection refused), but the counter
        // must have ticked because post_action took the direct branch.
        let _ = transport.send_update(&"a".repeat(64), "dGVzdA==");
        assert_eq!(transport.direct_fallback_count(), 1);
        let _ = transport.send_update(&"b".repeat(64), "dGVzdA==");
        assert_eq!(transport.direct_fallback_count(), 2);
    }

    // @internal
    #[test]
    fn test_direct_fallback_count_stays_zero_when_blocked() {
        // allow_direct = false → post_action returns the fail-closed error
        // before reaching the direct branch. The counter must stay at zero.
        let transport = HttpTransport::new(HttpTransportConfig::default());
        let _ = transport.send_update(&"a".repeat(64), "dGVzdA==");
        assert_eq!(transport.direct_fallback_count(), 0);
    }

    #[test]
    fn test_ohttp_not_configured_by_default() {
        let transport = HttpTransport::new(HttpTransportConfig::default());
        assert!(!transport.has_ohttp());
    }

    #[test]
    fn test_set_ohttp_activates_encryption() {
        use ohttp::{KeyConfig, SymmetricSuite, hpke};

        let config = KeyConfig::new(
            0,
            hpke::Kem::X25519Sha256,
            vec![SymmetricSuite::new(
                hpke::Kdf::HkdfSha256,
                hpke::Aead::Aes128Gcm,
            )],
        )
        .unwrap();
        let encoded = config.encode().unwrap();
        let ohttp_client = OhttpClient::new(encoded).unwrap();

        let mut transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://127.0.0.1:1".into(),
            timeout_ms: 1000,
            proxy: ProxyConfig::None,
            allow_direct: false,
            pinned_certs: vec![],
        });
        assert!(!transport.has_ohttp());
        transport.set_ohttp(ohttp_client);
        assert!(transport.has_ohttp());
    }

    #[test]
    fn test_send_via_ohttp_connection_refused() {
        use ohttp::{KeyConfig, SymmetricSuite, hpke};

        let config = KeyConfig::new(
            0,
            hpke::Kem::X25519Sha256,
            vec![SymmetricSuite::new(
                hpke::Kdf::HkdfSha256,
                hpke::Aead::Aes128Gcm,
            )],
        )
        .unwrap();
        let encoded = config.encode().unwrap();
        let ohttp_client = OhttpClient::new(encoded).unwrap();

        let mut transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://127.0.0.1:1".into(),
            timeout_ms: 1000,
            proxy: ProxyConfig::None,
            allow_direct: false,
            pinned_certs: vec![],
        });
        transport.set_ohttp(ohttp_client);

        // Should try to POST to /v2/ohttp and fail (connection refused)
        let result = transport.send_update(&"a".repeat(64), "dGVzdA==");
        assert!(result.is_err());
    }

    #[test]
    fn test_ohttp_envelope_includes_version_2() {
        #[derive(Serialize)]
        struct TestBody {
            mailbox_id: String,
        }

        let body = TestBody {
            mailbox_id: "abc123".to_string(),
        };
        let envelope_bytes =
            HttpTransport::build_ohttp_envelope("send", &body).expect("envelope must build");
        let envelope: serde_json::Value =
            serde_json::from_slice(&envelope_bytes).expect("envelope must be valid JSON");

        assert_eq!(
            envelope.get("version"),
            Some(&serde_json::Value::Number(2.into())),
            "OHTTP inner envelope must include version=2"
        );
        assert_eq!(
            envelope.get("action"),
            Some(&serde_json::Value::String("send".into())),
            "OHTTP inner envelope must include the action field"
        );
        assert_eq!(
            envelope.get("mailbox_id"),
            Some(&serde_json::Value::String("abc123".into())),
            "OHTTP inner envelope must include body fields"
        );
    }

    // ── C3: Payload padding tests ──────────────────────────────────

    // @scenario: sync:OHTTP payload padding
    #[test]
    fn test_ohttp_request_padded_to_valid_bucket() {
        use crate::crypto::padding;

        #[derive(Serialize)]
        struct TestBody {
            mailbox_id: String,
        }

        let body = TestBody {
            mailbox_id: "abc123".to_string(),
        };
        let padded = HttpTransport::build_padded_ohttp_payload("send", &body)
            .expect("padded payload must build");
        assert!(
            padding::is_valid_bucket_size(padded.len()),
            "padded request size {} must be a valid bucket",
            padded.len()
        );
    }

    // @scenario: sync:OHTTP payload padding
    #[test]
    fn test_ohttp_request_padding_roundtrips_with_relay_unpad() {
        use crate::crypto::padding;

        #[derive(Serialize)]
        struct TestBody {
            recipient_id: String,
            ciphertext: String,
        }

        let body = TestBody {
            recipient_id: "a".repeat(64),
            ciphertext: "dGVzdA==".to_string(),
        };
        let padded = HttpTransport::build_padded_ohttp_payload("send", &body)
            .expect("padded payload must build");

        // Simulate relay-side unpad (relay/src/http_api.rs:353)
        let unpadded = padding::unpad(&padded).expect("relay must be able to unpad client request");
        let parsed: serde_json::Value =
            serde_json::from_slice(&unpadded).expect("unpadded payload must be valid JSON");

        assert_eq!(parsed["action"], "send");
        assert_eq!(parsed["version"], 2);
        assert_eq!(parsed["recipient_id"], "a".repeat(64));
    }

    // @scenario: sync:OHTTP payload padding
    #[test]
    fn test_ohttp_response_unpadding() {
        use crate::crypto::padding;

        // Simulate relay-side response padding (relay/src/http_api.rs:397)
        let response = serde_json::json!({
            "status": "ok",
            "blob_id": "abc123"
        });
        let resp_bytes = serde_json::to_vec(&response).unwrap();
        let padded_response = padding::pad(&resp_bytes);

        // Client-side unpad
        let parsed = HttpTransport::parse_padded_ohttp_response(&padded_response)
            .expect("client must parse padded relay response");

        assert_eq!(parsed.status, "ok");
        assert_eq!(parsed.blob_id.as_deref(), Some("abc123"));
    }

    // @scenario: sync:OHTTP payload padding
    #[test]
    fn test_ohttp_response_unpadding_rejects_invalid() {
        // Garbage bytes should fail cleanly, not panic
        let garbage = vec![0xFF; 100];
        let result = HttpTransport::parse_padded_ohttp_response(&garbage);
        assert!(result.is_err(), "garbage bytes must produce an error");
    }

    // @scenario: sync:OHTTP payload padding
    #[test]
    fn test_ohttp_full_crypto_roundtrip_with_padding() {
        use crate::crypto::padding;
        use ohttp::{KeyConfig, SymmetricSuite, hpke};

        // Server key pair
        let key_config = KeyConfig::new(
            0,
            hpke::Kem::X25519Sha256,
            vec![SymmetricSuite::new(
                hpke::Kdf::HkdfSha256,
                hpke::Aead::Aes128Gcm,
            )],
        )
        .unwrap();
        let encoded = key_config.encode().unwrap();

        // === Client: build + pad + OHTTP encrypt ===
        #[derive(Serialize)]
        struct TestBody {
            mailbox_id: String,
        }
        let body = TestBody {
            mailbox_id: "test".to_string(),
        };
        let padded_request = HttpTransport::build_padded_ohttp_payload("fetch", &body).unwrap();
        assert!(padding::is_valid_bucket_size(padded_request.len()));

        let client = OhttpClient::new(encoded).unwrap();
        let (encrypted_request, response_nonce) = client.encapsulate(&padded_request).unwrap();

        // === Server: OHTTP decrypt + unpad + parse ===
        let server = ohttp::Server::new(key_config).unwrap();
        let (decrypted, srv_resp) = server.decapsulate(&encrypted_request).unwrap();
        let unpadded = padding::unpad(&decrypted).expect("server must unpad");
        let parsed: serde_json::Value = serde_json::from_slice(&unpadded).unwrap();
        assert_eq!(parsed["action"], "fetch");

        // === Server: respond + pad + OHTTP encrypt ===
        let response = serde_json::json!({"status": "ok", "blobs": []});
        let resp_bytes = serde_json::to_vec(&response).unwrap();
        let padded_resp = padding::pad(&resp_bytes);
        let encrypted_resp = srv_resp.encapsulate(&padded_resp).unwrap();

        // === Client: OHTTP decrypt + unpad + parse ===
        let decrypted_resp = response_nonce.decapsulate(&encrypted_resp).unwrap();
        let parsed_resp = HttpTransport::parse_padded_ohttp_response(&decrypted_resp).unwrap();
        assert_eq!(parsed_resp.status, "ok");
    }
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP Transport for Relay v2 Protocol
//!
//! Sync HTTP client using `ureq` for the v2 relay API.
//! Replaces WebSocket for relay communication — request/response model
//! suited for contact card sync (not real-time chat).

use std::time::Duration;

use serde::Serialize;
use vauchi_protocol::v2::{
    FetchedBlob, V2AckRequest, V2FetchRequest, V2PurgeRequest, V2Response, V2SendRequest,
};

use super::error::NetworkError;

/// Default retry delay when the server doesn't specify Retry-After.
const DEFAULT_RATE_LIMIT_RETRY_SECS: u64 = 10;

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
}

impl Default for HttpTransportConfig {
    fn default() -> Self {
        Self {
            relay_url: String::new(),
            timeout_ms: 30_000,
            proxy: ProxyConfig::None,
            allow_direct: false,
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
        }
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
    pub fn purge(
        &self,
        recipient_id: &str,
        public_key: &str,
        purge_token: &str,
        signature: &str,
        timestamp: u64,
    ) -> Result<(), NetworkError> {
        let req = V2PurgeRequest {
            recipient_id: recipient_id.to_string(),
            public_key: public_key.to_string(),
            purge_token: purge_token.to_string(),
            signature: signature.to_string(),
            timestamp,
        };
        let resp = self.post_action("purge", &req)?;
        if resp.status == "ok" {
            Ok(())
        } else {
            Err(response_error("purge", &resp.error.unwrap_or_default()))
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

    /// Encrypt a request via OHTTP and decrypt the response.
    fn post_via_ohttp<Req: Serialize>(
        &self,
        ohttp: &OhttpClient,
        action: &str,
        body: &Req,
    ) -> Result<V2Response, NetworkError> {
        let inner_bytes = Self::build_ohttp_envelope(action, body)?;

        // Encrypt
        let (encrypted, response_decryptor) = ohttp.encapsulate(&inner_bytes)?;

        // POST encrypted blob
        let agent = self.get_agent()?;
        let ohttp_url = format!("{}/v2/ohttp", self.config.relay_url);
        let resp = agent
            .post(&ohttp_url)
            .content_type("message/ohttp-req")
            .send(encrypted)
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        // Read raw response bytes
        let enc_response = resp
            .into_body()
            .read_to_vec()
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        // Decrypt
        let plain_response = response_decryptor.decapsulate(&enc_response)?;

        // Parse
        serde_json::from_slice(&plain_response)
            .map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    /// Returns a reference to the cached agent, or the construction error.
    fn get_agent(&self) -> Result<&ureq::Agent, NetworkError> {
        self.agent
            .as_ref()
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))
    }

    fn post_json<Req: Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &Req,
    ) -> Result<Resp, NetworkError> {
        let agent = self.get_agent()?;
        let resp = agent
            .post(url)
            .send_json(body)
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        resp.into_body()
            .read_json::<Resp>()
            .map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    fn get_json<Resp: serde::de::DeserializeOwned>(&self, url: &str) -> Result<Resp, NetworkError> {
        let agent = self.get_agent()?;
        let resp = agent
            .get(url)
            .call()
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        resp.into_body()
            .read_json::<Resp>()
            .map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    /// Build an agent from config. Called once at construction.
    fn build_agent_from_config(config: &HttpTransportConfig) -> Result<ureq::Agent, NetworkError> {
        let timeout = Duration::from_millis(config.timeout_ms);
        let mut builder = ureq::Agent::config_builder().timeout_global(Some(timeout));

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
        let transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://localhost:8080".into(),
            timeout_ms: 5000,
            proxy: ProxyConfig::None,
            allow_direct: true,
        });
        assert_eq!(transport.relay_url(), "http://localhost:8080");
        assert_eq!(transport.proxy(), &ProxyConfig::None);
    }

    #[test]
    fn test_http_transport_with_socks5_config() {
        let transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://relay.example.com".into(),
            timeout_ms: 5000,
            proxy: ProxyConfig::socks5("127.0.0.1", 1080),
            allow_direct: false,
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
        let transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://127.0.0.1:1".into(),
            timeout_ms: 1000,
            proxy: ProxyConfig::None,
            allow_direct: true,
        });
        let result = transport.send_update(&"a".repeat(64), "dGVzdA==");
        assert!(result.is_err());
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
}

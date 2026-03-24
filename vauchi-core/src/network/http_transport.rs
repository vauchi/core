// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! HTTP Transport for Relay v2 Protocol
//!
//! Sync HTTP client using `ureq` for the v2 relay API.
//! Replaces WebSocket for relay communication — request/response model
//! suited for contact card sync (not real-time chat).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::error::NetworkError;
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
}

impl Default for HttpTransportConfig {
    fn default() -> Self {
        Self {
            relay_url: String::new(),
            timeout_ms: 30_000,
            proxy: ProxyConfig::None,
        }
    }
}

/// v2 send request body.
#[derive(Debug, Serialize)]
pub struct V2SendRequest {
    pub recipient_id: String,
    pub ciphertext: String, // base64-encoded
}

/// v2 fetch request body.
#[derive(Debug, Serialize)]
pub struct V2FetchRequest {
    pub mailbox_tokens: Vec<String>,
}

/// v2 acknowledge request body.
#[derive(Debug, Serialize)]
pub struct V2AckRequest {
    pub recipient_id: String,
    pub blob_id: String,
}

/// v2 purge request body.
#[derive(Debug, Serialize)]
pub struct V2PurgeRequest {
    pub recipient_id: String,
}

/// A fetched blob from the relay.
#[derive(Debug, Deserialize)]
pub struct FetchedBlob {
    pub blob_id: String,
    pub ciphertext: String, // base64-encoded
    pub created_at: u64,
}

/// Standard v2 response envelope.
#[derive(Debug, Deserialize)]
pub struct V2Response {
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub blob_id: Option<String>,
    #[serde(default)]
    pub blobs: Option<Vec<FetchedBlob>>,
    #[serde(default)]
    pub acknowledged: Option<bool>,
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
    ohttp: Option<OhttpClient>,
}

impl HttpTransport {
    /// Creates a new HTTP transport with the given configuration.
    pub fn new(config: HttpTransportConfig) -> Self {
        Self {
            config,
            ohttp: None,
        }
    }

    /// Set the OHTTP client for encrypted requests.
    ///
    /// When set, all data requests are encrypted via OHTTP. Call with a fresh
    /// client when the gateway key rotates (HTTP 400 on stale key).
    pub fn set_ohttp(&mut self, client: OhttpClient) {
        self.ohttp = Some(client);
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
            Err(NetworkError::InvalidMessage(format!(
                "send failed: {}",
                resp.error.unwrap_or_default()
            )))
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
            Err(NetworkError::InvalidMessage(format!(
                "fetch failed: {}",
                resp.error.unwrap_or_default()
            )))
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
            Err(NetworkError::InvalidMessage(format!(
                "ack failed: {}",
                resp.error.unwrap_or_default()
            )))
        }
    }

    /// Purges all blobs for a recipient.
    pub fn purge(&self, recipient_id: &str) -> Result<(), NetworkError> {
        let req = V2PurgeRequest {
            recipient_id: recipient_id.to_string(),
        };
        let resp = self.post_action("purge", &req)?;
        if resp.status == "ok" {
            Ok(())
        } else {
            Err(NetworkError::InvalidMessage(format!(
                "purge failed: {}",
                resp.error.unwrap_or_default()
            )))
        }
    }

    // ── Internal helpers ────────────────────────────────────────────

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.config.timeout_ms)
    }

    /// Post a request, routing through OHTTP if configured.
    ///
    /// When OHTTP is active: serializes as `{"action": action, ...fields}`,
    /// encrypts, POSTs to `/v2/ohttp`, decrypts response.
    /// When OHTTP is not active: POSTs JSON directly to the endpoint.
    fn post_action<Req: Serialize>(
        &self,
        action: &str,
        body: &Req,
    ) -> Result<V2Response, NetworkError> {
        if let Some(ohttp) = &self.ohttp {
            self.post_via_ohttp(ohttp, action, body)
        } else {
            let url = format!("{}/v2/{action}", self.config.relay_url);
            self.post_json(&url, body)
        }
    }

    /// Encrypt a request via OHTTP and decrypt the response.
    fn post_via_ohttp<Req: Serialize>(
        &self,
        ohttp: &OhttpClient,
        action: &str,
        body: &Req,
    ) -> Result<V2Response, NetworkError> {
        // Build inner envelope: {"action": "send", ...body_fields}
        let mut inner =
            serde_json::to_value(body).map_err(|e| NetworkError::Serialization(e.to_string()))?;
        if let Some(obj) = inner.as_object_mut() {
            obj.insert(
                "action".to_string(),
                serde_json::Value::String(action.to_string()),
            );
        }
        let inner_bytes =
            serde_json::to_vec(&inner).map_err(|e| NetworkError::Serialization(e.to_string()))?;

        // Encrypt
        let (encrypted, response_decryptor) = ohttp.encapsulate(&inner_bytes)?;

        // POST encrypted blob
        let agent = self.build_agent()?;
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

    fn post_json<Req: Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &Req,
    ) -> Result<Resp, NetworkError> {
        let agent = self.build_agent()?;
        let resp = agent
            .post(url)
            .send_json(body)
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        resp.into_body()
            .read_json::<Resp>()
            .map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    fn get_json<Resp: serde::de::DeserializeOwned>(&self, url: &str) -> Result<Resp, NetworkError> {
        let agent = self.build_agent()?;
        let resp = agent
            .get(url)
            .call()
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        resp.into_body()
            .read_json::<Resp>()
            .map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    fn build_agent(&self) -> Result<ureq::Agent, NetworkError> {
        let mut builder = ureq::Agent::config_builder().timeout_global(Some(self.timeout()));

        if let Some(proxy) = self.build_proxy()? {
            builder = builder.proxy(Some(proxy));
        }

        Ok(builder.build().new_agent())
    }

    fn build_proxy(&self) -> Result<Option<ureq::Proxy>, NetworkError> {
        match &self.config.proxy {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_transport_config_defaults() {
        let config = HttpTransportConfig::default();
        assert!(config.relay_url.is_empty());
        assert_eq!(config.timeout_ms, 30_000);
        assert_eq!(config.proxy, ProxyConfig::None);
    }

    #[test]
    fn test_http_transport_creation() {
        let transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://localhost:8080".into(),
            timeout_ms: 5000,
            proxy: ProxyConfig::None,
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
        });
        assert!(matches!(transport.proxy(), ProxyConfig::Socks5 { .. }));
    }

    #[test]
    fn test_health_check_connection_refused() {
        let transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://127.0.0.1:1".into(), // unreachable port
            timeout_ms: 1000,
            proxy: ProxyConfig::None,
        });
        let result = transport.health_check();
        assert!(result.is_err());
    }

    #[test]
    fn test_send_update_connection_refused() {
        let transport = HttpTransport::new(HttpTransportConfig {
            relay_url: "http://127.0.0.1:1".into(),
            timeout_ms: 1000,
            proxy: ProxyConfig::None,
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
        });
        transport.set_ohttp(ohttp_client);

        // Should try to POST to /v2/ohttp and fail (connection refused)
        let result = transport.send_update(&"a".repeat(64), "dGVzdA==");
        assert!(result.is_err());
    }
}

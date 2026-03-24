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
pub struct HttpTransport {
    config: HttpTransportConfig,
}

impl HttpTransport {
    /// Creates a new HTTP transport with the given configuration.
    pub fn new(config: HttpTransportConfig) -> Self {
        Self { config }
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
        let url = format!("{}/v2/send", self.config.relay_url);
        let req = V2SendRequest {
            recipient_id: recipient_id.to_string(),
            ciphertext: ciphertext_b64.to_string(),
        };
        let resp: V2Response = self.post_json(&url, &req)?;
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
        let url = format!("{}/v2/fetch", self.config.relay_url);
        let req = V2FetchRequest {
            mailbox_tokens: mailbox_tokens.to_vec(),
        };
        let resp: V2Response = self.post_json(&url, &req)?;
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
        let url = format!("{}/v2/ack", self.config.relay_url);
        let req = V2AckRequest {
            recipient_id: recipient_id.to_string(),
            blob_id: blob_id.to_string(),
        };
        let resp: V2Response = self.post_json(&url, &req)?;
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
        let url = format!("{}/v2/purge", self.config.relay_url);
        let req = V2PurgeRequest {
            recipient_id: recipient_id.to_string(),
        };
        let resp: V2Response = self.post_json(&url, &req)?;
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

    fn post_json<Req: Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &Req,
    ) -> Result<Resp, NetworkError> {
        let agent = self.build_agent();
        let resp = agent
            .post(url)
            .send_json(body)
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        resp.into_body()
            .read_json::<Resp>()
            .map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    fn get_json<Resp: serde::de::DeserializeOwned>(&self, url: &str) -> Result<Resp, NetworkError> {
        let agent = self.build_agent();
        let resp = agent
            .get(url)
            .call()
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        resp.into_body()
            .read_json::<Resp>()
            .map_err(|e| NetworkError::Serialization(e.to_string()))
    }

    fn build_agent(&self) -> ureq::Agent {
        let mut builder = ureq::Agent::config_builder().timeout_global(Some(self.timeout()));

        if let Some(proxy) = self.build_proxy() {
            builder = builder.proxy(Some(proxy));
        }

        builder.build().new_agent()
    }

    fn build_proxy(&self) -> Option<ureq::Proxy> {
        match &self.config.proxy {
            ProxyConfig::None => None,
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
                pb.build().ok()
            }
            ProxyConfig::HttpConnect { host, port } => {
                ureq::Proxy::builder(ureq::ProxyProtocol::Http)
                    .host(host)
                    .port(*port)
                    .build()
                    .ok()
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
}

// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content fetcher for downloading remote content
//!
//! This module provides HTTP-based content fetching with:
//! - Checksum verification
//! - Size limits
//! - Proxy support (SOCKS5)
//! - Timeout configuration

#[cfg(feature = "content-updates")]
use sha2::Digest;
use thiserror::Error;

#[cfg(feature = "content-updates")]
use super::config::ContentConfig;
use super::integrity::IntegrityError;
#[cfg(feature = "content-updates")]
use super::types::ContentManifest;

#[cfg(feature = "content-updates")]
use ureq::Agent;

#[cfg(not(feature = "content-updates"))]
use super::config::ContentConfig;

/// Fetches content from remote server
#[cfg(feature = "content-updates")]
pub struct ContentFetcher {
    agent: Agent,
    base_url: String,
    max_content_size: u64,
    /// Publisher key for manifest signature verification.
    publisher_public_key: vauchi_core::crypto::signing::PublicKey,
}

#[cfg(feature = "content-updates")]
impl ContentFetcher {
    /// Create a new content fetcher from config
    pub fn new(config: &ContentConfig) -> Result<Self, FetchError> {
        let timeout = config.timeout;
        let mut builder = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .max_redirects(0)
            .user_agent(format!(
                "Vauchi/{}",
                option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0")
            ));

        // Support proxy if configured
        if let Some(proxy_url) = &config.proxy_url {
            let proxy = ureq::Proxy::new(proxy_url)
                .map_err(|e| FetchError::NetworkError(format!("invalid proxy URL: {e}")))?;
            builder = builder.proxy(Some(proxy));
        }

        let agent = builder.build().new_agent();

        Ok(Self {
            agent,
            base_url: config.content_url.clone(),
            max_content_size: config.max_content_size,
            publisher_public_key: config.publisher_public_key.clone(),
        })
    }

    /// Fetch manifest from remote
    pub async fn fetch_manifest(&self) -> Result<ContentManifest, FetchError> {
        let url = format!("{}/manifest.json", self.base_url);
        let agent = self.agent.clone();

        let body = tokio::task::spawn_blocking(move || {
            let response = agent
                .get(&url)
                .call()
                .map_err(|e| FetchError::NetworkError(e.to_string()))?;

            let status = response.status().as_u16();
            if !(200..300).contains(&status) {
                return Err(FetchError::HttpError(status));
            }

            response
                .into_body()
                .read_to_string()
                .map_err(|e| FetchError::NetworkError(e.to_string()))
        })
        .await
        .map_err(|e| FetchError::NetworkError(e.to_string()))??;

        let manifest: ContentManifest = serde_json::from_str(&body)?;

        super::integrity::verify_manifest_signature(&manifest, &self.publisher_public_key)?;

        Ok(manifest)
    }

    /// Fetch content file from remote with streaming checksum verification (#146).
    ///
    /// Downloads in chunks, incrementally hashing each chunk. Aborts early if
    /// the running total exceeds `max_content_size`, preventing a compromised
    /// CDN from forcing the client to buffer arbitrary data.
    pub async fn fetch_content(
        &self,
        path: &str,
        expected_checksum: &str,
    ) -> Result<Vec<u8>, FetchError> {
        let url = format!("{}/{}", self.base_url, path);
        let max_content_size = self.max_content_size;
        let agent = self.agent.clone();
        let expected_checksum = expected_checksum.to_owned();

        tokio::task::spawn_blocking(move || {
            let response = agent
                .get(&url)
                .call()
                .map_err(|e| FetchError::NetworkError(e.to_string()))?;

            let status = response.status().as_u16();
            if !(200..300).contains(&status) {
                return Err(FetchError::HttpError(status));
            }

            // Check content length before downloading
            if let Some(len) = response
                .headers()
                .get("Content-Length")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                && len > max_content_size
            {
                return Err(FetchError::TooLarge {
                    size: len,
                    max: max_content_size,
                });
            }

            // Stream-verify: download in chunks with incremental SHA-256 hash (#146)
            let mut hasher = sha2::Sha256::new();
            let mut data = Vec::new();
            let mut total: u64 = 0;
            let mut reader = response.into_body().into_reader();
            let mut chunk = [0u8; 8192];

            loop {
                let n = std::io::Read::read(&mut reader, &mut chunk)
                    .map_err(|e| FetchError::NetworkError(e.to_string()))?;
                if n == 0 {
                    break;
                }
                total += n as u64;
                if total > max_content_size {
                    return Err(FetchError::TooLarge {
                        size: total,
                        max: max_content_size,
                    });
                }
                hasher.update(&chunk[..n]);
                data.extend_from_slice(&chunk[..n]);
            }

            // Verify checksum against expected
            let expected_hex = expected_checksum
                .strip_prefix("sha256:")
                .ok_or(super::integrity::IntegrityError::InvalidFormat)?;
            let digest = sha2::Digest::finalize(hasher);
            let computed_hex = hex::encode(&digest[..]);
            if computed_hex != expected_hex {
                return Err(super::integrity::IntegrityError::ChecksumMismatch.into());
            }

            Ok(data)
        })
        .await
        .map_err(|e| FetchError::NetworkError(e.to_string()))?
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

/// Stub fetcher when content-updates feature is not enabled
#[cfg(not(feature = "content-updates"))]
pub struct ContentFetcher {
    _private: (),
}

#[cfg(not(feature = "content-updates"))]
impl ContentFetcher {
    /// Create a new content fetcher (stub - always fails)
    pub fn new(_config: &ContentConfig) -> Result<Self, FetchError> {
        Err(FetchError::FeatureDisabled)
    }
}

/// Errors that can occur during content fetching
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FetchError {
    /// HTTP error with status code
    #[error("HTTP error: {0}")]
    HttpError(u16),

    /// Network/request error
    #[cfg(feature = "content-updates")]
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Content too large
    #[error("Content too large: {size} bytes (max {max})")]
    TooLarge {
        /// Actual size in bytes
        size: u64,
        /// Maximum allowed size in bytes
        max: u64,
    },

    /// Integrity verification failed
    #[error("Integrity error: {0}")]
    IntegrityError(#[from] IntegrityError),

    /// JSON parsing error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Content updates feature is not enabled
    #[error("Content updates feature is not enabled")]
    FeatureDisabled,
}

// INLINE_TEST_REQUIRED: tests access private internals
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_error_display() {
        let err = FetchError::HttpError(404);
        assert_eq!(err.to_string(), "HTTP error: 404");

        let err = FetchError::TooLarge {
            size: 10_000_000,
            max: 5_000_000,
        };
        assert!(err.to_string().contains("too large"));
    }

    #[test]
    fn test_fetch_error_from_integrity() {
        let integrity_err = IntegrityError::InvalidFormat;
        let fetch_err: FetchError = integrity_err.into();
        assert!(matches!(fetch_err, FetchError::IntegrityError(_)));
    }
}

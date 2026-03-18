// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content fetcher for downloading remote content
//!
//! This module provides HTTP-based content fetching with:
//! - Checksum verification
//! - Size limits
//! - Proxy support (for Tor)
//! - Timeout configuration

use thiserror::Error;

#[cfg(feature = "content-updates")]
use super::config::ContentConfig;
use super::integrity::IntegrityError;
#[cfg(feature = "content-updates")]
use super::types::ContentManifest;

#[cfg(feature = "content-updates")]
use reqwest::Client;

#[cfg(not(feature = "content-updates"))]
use super::config::ContentConfig;

/// Fetches content from remote server
#[cfg(feature = "content-updates")]
pub struct ContentFetcher {
    client: Client,
    base_url: String,
    max_content_size: u64,
    /// Publisher key for manifest signature verification (Tracker #145).
    publisher_public_key: Option<crate::crypto::signing::PublicKey>,
}

#[cfg(feature = "content-updates")]
impl ContentFetcher {
    /// Create a new content fetcher from config
    pub fn new(config: &ContentConfig) -> Result<Self, FetchError> {
        let mut builder = Client::builder()
            .timeout(config.timeout)
            .user_agent(format!(
                "Vauchi/{}",
                option_env!("CARGO_PKG_VERSION").unwrap_or("0.1.0")
            ));

        // Support proxy if configured (for Tor)
        if let Some(proxy_url) = &config.proxy_url {
            builder = builder.proxy(reqwest::Proxy::all(proxy_url)?);
        }

        Ok(Self {
            client: builder.build()?,
            base_url: config.content_url.clone(),
            max_content_size: config.max_content_size,
            publisher_public_key: config.publisher_public_key.clone(),
        })
    }

    /// Fetch manifest from remote
    pub async fn fetch_manifest(&self) -> Result<ContentManifest, FetchError> {
        let url = format!("{}/manifest.json", self.base_url);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(FetchError::HttpError(response.status().as_u16()));
        }

        let manifest: ContentManifest = response.json().await?;

        // Verify manifest signature if publisher key is configured (Tracker #145)
        if let Some(ref pub_key) = self.publisher_public_key {
            super::integrity::verify_manifest_signature(&manifest, pub_key)?;
        }

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
        let mut response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            return Err(FetchError::HttpError(response.status().as_u16()));
        }

        // Check content length before downloading
        if let Some(len) = response.content_length() {
            if len > self.max_content_size {
                return Err(FetchError::TooLarge {
                    size: len,
                    max: self.max_content_size,
                });
            }
        }

        // Stream-verify: download in chunks with incremental SHA-256 hash (#146)
        let mut hasher = sha2::Sha256::new();
        let mut data = Vec::new();
        let mut total: u64 = 0;

        while let Some(chunk) = response.chunk().await? {
            total += chunk.len() as u64;
            if total > self.max_content_size {
                return Err(FetchError::TooLarge {
                    size: total,
                    max: self.max_content_size,
                });
            }
            hasher.update(&chunk);
            data.extend_from_slice(&chunk);
        }

        // Verify checksum against expected
        let expected_hex = expected_checksum
            .strip_prefix("sha256:")
            .ok_or(super::integrity::IntegrityError::InvalidFormat)?;
        let digest = sha2::Digest::finalize(hasher);
        let computed_hex = hex::encode(digest.as_ref());
        if computed_hex != expected_hex {
            return Err(super::integrity::IntegrityError::ChecksumMismatch.into());
        }

        Ok(data)
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
pub enum FetchError {
    /// HTTP error with status code
    #[error("HTTP error: {0}")]
    HttpError(u16),

    /// Network/request error
    #[cfg(feature = "content-updates")]
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),

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

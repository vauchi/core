// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! API Configuration
//!
//! Configuration types for the Vauchi API layer.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::crypto::SymmetricKey;
use crate::network::{
    MultiRelayConfig, PinnedCertificate, ProxyConfig, RelayClientConfig, TransportConfig,
};
/// Configuration for Vauchi instance.
///
/// Use `VauchiConfig::default()` with field overrides:
/// ```ignore
/// let config = VauchiConfig {
///     storage_path: "/custom/path".into(),
///     ..Default::default()
/// };
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct VauchiConfig {
    /// Storage directory for identity, contacts, and sync state.
    pub storage_path: PathBuf,

    /// Relay server configuration.
    pub relay: RelayConfig,

    /// Sync configuration.
    pub sync: SyncConfig,

    /// Auto-save configuration.
    pub auto_save: bool,

    /// Storage encryption key.
    /// If None, a random key will be generated (not persistent across sessions).
    pub storage_key: Option<SymmetricKey>,

    /// Whether to send delivery receipts for received messages.
    pub delivery_receipts_enabled: bool,

    /// Whether to suppress presence (online/offline status) at the relay.
    /// When true, the relay will not notify contacts of this client's online status.
    pub suppress_presence: bool,

    /// Recovery configuration for social key recovery.
    pub recovery: RecoveryConfig,

    /// Multi-relay configuration (for federation client support).
    pub relay_list: Option<MultiRelayConfig>,

    /// OHTTP privacy configuration.
    pub ohttp: OhttpConfig,

    /// Whether to send OS notification when a new contact is added via sync.
    /// Default: false (opt-in).
    pub contact_added_notifications: bool,
}

impl Default for VauchiConfig {
    fn default() -> Self {
        VauchiConfig {
            storage_path: PathBuf::from("./vauchi_data"),
            relay: RelayConfig::default(),
            sync: SyncConfig::default(),
            auto_save: true,
            storage_key: None,
            delivery_receipts_enabled: true,
            suppress_presence: false,
            recovery: RecoveryConfig::default(),
            relay_list: None,
            ohttp: OhttpConfig::default(),
            contact_added_notifications: false,
        }
    }
}

impl VauchiConfig {
    /// Creates a new configuration with the given storage path.
    pub fn with_storage_path(storage_path: impl Into<PathBuf>) -> Self {
        VauchiConfig {
            storage_path: storage_path.into(),
            ..Default::default()
        }
    }

    /// Sets the relay server URL.
    pub fn with_relay_url(mut self, url: impl Into<String>) -> Self {
        self.relay.server_url = url.into();
        self
    }

    /// Sets the multi-relay configuration.
    pub fn with_relay_list(mut self, config: MultiRelayConfig) -> Self {
        self.relay_list = Some(config);
        self
    }

    /// Disables auto-save.
    pub fn without_auto_save(mut self) -> Self {
        self.auto_save = false;
        self
    }

    /// Sets the storage encryption key.
    /// Use this to persist data across sessions.
    pub fn with_storage_key(mut self, key: SymmetricKey) -> Self {
        self.storage_key = Some(key);
        self
    }
}

/// Relay server configuration.
#[derive(Debug, Clone)]
pub struct RelayConfig {
    /// Relay server URL.
    pub server_url: String,

    /// Connection timeout in milliseconds.
    pub connect_timeout_ms: u64,

    /// Read/write timeout in milliseconds.
    pub io_timeout_ms: u64,

    /// Maximum reconnection attempts.
    pub max_reconnect_attempts: u32,

    /// Base delay for exponential backoff (milliseconds).
    pub reconnect_base_delay_ms: u64,

    /// Maximum concurrent pending messages.
    pub max_pending_messages: usize,

    /// Acknowledgment timeout in milliseconds.
    pub ack_timeout_ms: u64,

    /// Maximum message retries before giving up.
    pub max_retries: u32,

    /// Proxy configuration (SOCKS5 proxy support).
    pub proxy: ProxyConfig,

    /// Relay's Noise NK public key for inner transport encryption.
    /// When set, the transport performs a Noise NK handshake after connect
    /// and wraps all subsequent messages with Noise encryption.
    pub relay_noise_pubkey: Option<[u8; 32]>,

    /// Pinned relay certificates for TLS certificate pinning.
    /// When non-empty, verifies the server's leaf certificate matches a pin.
    pub pinned_certs: Vec<PinnedCertificate>,

    /// TTL for cached pin configurations in seconds (default 86400 = 24h).
    ///
    /// Controls how long relay-served pin updates are trusted before
    /// the client re-fetches. Shorter = more frequent checks, more
    /// network traffic. Longer = more risk of stale pins after rotation.
    pub pin_ttl_secs: u64,

    /// Ed25519 public key for verifying signed pin-config responses.
    ///
    /// When `Some`, the client fetches and verifies pin updates from
    /// the relay's `/v2/pin-config` endpoint. The relay must sign
    /// pin-config responses with the corresponding private key.
    ///
    /// When `None` (default), pin rotation is disabled — only the
    /// bundled `pinned_certs` are used. This is the safe default:
    /// unauthenticated pin updates would allow a MITM to replace
    /// the pin set permanently.
    pub pin_config_verify_key: Option<[u8; 32]>,
}

/// SPKI SHA-256 pin for relay.vauchi.app leaf certificate.
///
/// Extracted via:
/// ```sh
/// echo | openssl s_client -connect relay.vauchi.app:443 -servername relay.vauchi.app 2>/dev/null \
///   | openssl x509 -pubkey -noout | openssl pkey -pubin -outform DER | openssl dgst -sha256 -binary | xxd -p
/// ```
///
/// Update when the relay's TLS key pair rotates (not on every cert renewal —
/// SPKI pinning survives renewals that reuse the same key).
const RELAY_PROD_SPKI_PIN: [u8; 32] = [
    0xba, 0xae, 0x88, 0x27, 0xcb, 0xce, 0xf3, 0xe5, 0xa1, 0xcc, 0xe3, 0xe0, 0x00, 0x9d, 0x4e, 0x06,
    0xe1, 0x70, 0x0f, 0xb1, 0x00, 0xeb, 0x37, 0x84, 0xb8, 0xc3, 0x4f, 0x4e, 0x26, 0xb0, 0x6d, 0x00,
];

impl Default for RelayConfig {
    /// Production relay configuration with SPKI certificate pinning.
    ///
    /// Self-hosters should use [`RelayConfig::unpinned`] and set their own
    /// `server_url` and optionally `pinned_certs`.
    fn default() -> Self {
        RelayConfig {
            server_url: "https://relay.vauchi.app".to_string(),
            connect_timeout_ms: 10_000,
            io_timeout_ms: 30_000,
            max_reconnect_attempts: 5,
            reconnect_base_delay_ms: 1_000,
            max_pending_messages: 100,
            ack_timeout_ms: 30_000,
            max_retries: 5,
            proxy: ProxyConfig::None,
            relay_noise_pubkey: None,
            pinned_certs: vec![PinnedCertificate::new(RELAY_PROD_SPKI_PIN)],
            pin_ttl_secs: 86_400,        // 24 hours
            pin_config_verify_key: None, // disabled until relay signs pin-config
        }
    }
}

impl RelayConfig {
    /// Returns the production relay certificate pins.
    ///
    /// Use this when constructing `HttpTransportConfig` outside of
    /// `RelayConfig` (e.g. CLI GDPR, TUI health check) to ensure
    /// the production relay pin is applied.
    pub fn default_pins() -> Vec<PinnedCertificate> {
        vec![PinnedCertificate::new(RELAY_PROD_SPKI_PIN)]
    }

    /// Creates a relay config with no pinned certificates.
    ///
    /// Intended for self-hosted relays where the operator controls the TLS
    /// certificate. Callers should set `server_url` to their relay address
    /// and optionally provide their own `pinned_certs`.
    pub fn unpinned(server_url: String) -> Self {
        RelayConfig {
            server_url,
            pinned_certs: Vec::new(),
            ..Default::default()
        }
    }

    /// Converts to TransportConfig for the network layer.
    pub fn to_transport_config(&self) -> TransportConfig {
        TransportConfig {
            server_url: self.server_url.clone(),
            connect_timeout_ms: self.connect_timeout_ms,
            io_timeout_ms: self.io_timeout_ms,
            max_reconnect_attempts: self.max_reconnect_attempts,
            reconnect_base_delay_ms: self.reconnect_base_delay_ms,
            proxy: self.proxy.clone(),
            relay_noise_pubkey: self.relay_noise_pubkey,
            pinned_certs: self.pinned_certs.clone(),
        }
    }

    /// Converts to RelayClientConfig for the network layer.
    pub fn to_relay_client_config(
        &self,
        delivery_receipts_enabled: bool,
        suppress_presence: bool,
    ) -> RelayClientConfig {
        RelayClientConfig {
            transport: self.to_transport_config(),
            max_pending_messages: self.max_pending_messages,
            ack_timeout_ms: self.ack_timeout_ms,
            max_retries: self.max_retries,
            delivery_receipts_enabled,
            suppress_presence,
        }
    }
}

/// Sync configuration.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Automatically sync on contact card changes.
    pub auto_sync: bool,

    /// Sync interval in milliseconds (0 = manual only).
    pub sync_interval_ms: u64,

    /// Maximum pending updates before forcing sync.
    pub max_pending_updates: usize,

    /// Maximum updates to send per sync cycle (#64).
    /// Prevents blocking the thread when a large backlog exists
    /// (e.g., after a long offline period). Remaining updates are
    /// sent in subsequent sync cycles.
    /// `None` means no limit (send all pending).
    pub batch_size: Option<usize>,

    /// Minimum delay before first sync after exchange (milliseconds).
    /// Prevents timing correlation between exchange and first relay contact.
    pub post_exchange_delay_min_ms: u64,

    /// Maximum delay before first sync after exchange (milliseconds).
    pub post_exchange_delay_max_ms: u64,

    /// Jitter percentage applied to `sync_interval_ms` (0-50, default 15).
    /// Actual interval = `sync_interval_ms` +/- jitter%.
    pub sync_interval_jitter_percent: u32,

    /// Enable payload padding to bucket sizes (256, 512, 1024, 4096).
    /// Prevents message size analysis. Aligned with relay bucket sizes.
    pub padding_enabled: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            auto_sync: true,
            sync_interval_ms: 60_000, // 1 minute
            max_pending_updates: 50,
            batch_size: Some(20),
            post_exchange_delay_min_ms: 30_000,  // 30 seconds
            post_exchange_delay_max_ms: 300_000, // 5 minutes
            sync_interval_jitter_percent: 15,
            padding_enabled: true,
        }
    }
}

impl SyncConfig {
    /// Returns a random delay duration in the configured post-exchange range.
    ///
    /// Prevents timing correlation between an in-person exchange event
    /// and the first relay contact that follows.
    pub fn random_post_exchange_delay(&self, rng: &dyn crate::rng::SecureRng) -> Duration {
        let min = self.post_exchange_delay_min_ms;
        let max = self.post_exchange_delay_max_ms;
        if min >= max {
            return Duration::from_millis(min);
        }
        let ms = rng.random_in_range_u64(min, max);
        Duration::from_millis(ms)
    }

    /// Returns the sync interval with random jitter applied.
    ///
    /// Jitter is capped at 50% to prevent degenerate intervals.
    pub fn jittered_sync_interval(&self, rng: &dyn crate::rng::SecureRng) -> Duration {
        let base = self.sync_interval_ms;
        let pct = self.sync_interval_jitter_percent.min(50) as u64;
        if pct == 0 {
            return Duration::from_millis(base);
        }
        let delta = base * pct / 100;
        let min = base.saturating_sub(delta);
        let max = base + delta;
        let ms = rng.random_in_range_u64(min, max);
        Duration::from_millis(ms)
    }
}

/// OHTTP privacy configuration.
#[derive(Debug, Clone)]
pub struct OhttpConfig {
    /// Client-side key TTL in seconds (default 43200 = 12h).
    pub key_ttl_secs: u64,
    /// Allow direct (non-OHTTP) data requests.
    /// Only for dev/testing — production must be false.
    pub allow_direct: bool,
    /// Bundled OHTTP gateway key config bytes (RFC 9458).
    ///
    /// When set, used as the initial key without making a direct HTTPS
    /// connection to the relay (which would leak the client's IP).
    /// The cached key from storage takes precedence if fresher.
    pub bundled_gateway_key: Option<Vec<u8>>,
}

/// Bundled OHTTP gateway key config (RFC 9458) for relay.vauchi.app.
///
/// Fetched from the production OHTTP relay via:
///   curl -s https://ohttp.vauchi.app/v2/ohttp-key > ohttp-key.bin
///
/// Bundling eliminates the bootstrap IP leak — the client uses this
/// key for the first OHTTP request instead of fetching it directly.
/// The cached key from storage takes precedence if fresher.
///
/// AEAD must be ChaCha20-Poly1305 (RFC 9180 codepoint `0x0003`) per
/// ADR-046. The regression test below
/// (`bundled_ohttp_key_advertises_only_chacha20_poly1305`) fails
/// loudly if the next regen accidentally re-introduces an AES suite.
///
/// Update when the relay rotates its OHTTP key pair.
const BUNDLED_OHTTP_KEY: &[u8] = &[
    0x00, 0x00, 0x20, 0xc6, 0xb5, 0xa3, 0xed, 0xe1, 0xaa, 0xdf, 0xfb, 0xbe, 0xc7, 0xf0, 0xea, 0x55,
    0xb6, 0x58, 0x96, 0x6a, 0xa4, 0xd9, 0x90, 0x2d, 0xf1, 0xc8, 0x5c, 0x82, 0xc6, 0x21, 0x7a, 0xcb,
    0xd2, 0x44, 0x68, 0x00, 0x04, 0x00, 0x01, 0x00, 0x03,
];

impl Default for OhttpConfig {
    fn default() -> Self {
        Self {
            key_ttl_secs: 43200,
            allow_direct: false,
            bundled_gateway_key: Some(BUNDLED_OHTTP_KEY.to_vec()),
        }
    }
}

/// Configuration for social key recovery.
///
/// Controls how many vouchers (trusted contacts) are needed
/// to recover an identity and whether automatic reminders are sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Number of vouchers needed to recover an identity.
    pub threshold: u32,

    /// Whether to automatically send reminders to vouchers.
    pub auto_remind: bool,

    /// Number of days between reminder messages.
    pub remind_interval_days: u32,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        RecoveryConfig {
            threshold: 3,
            auto_remind: true,
            remind_interval_days: 7,
        }
    }
}

// INLINE_TEST_REQUIRED: BUNDLED_OHTTP_KEY is a private const; an external
// test would need a pub accessor, widening API surface for nothing. The
// guard sits next to the bytes it protects so the next regen sees it.
#[cfg(test)]
mod tests {
    use super::*;

    /// ADR-046: the bundled OHTTP key must advertise only
    /// ChaCha20-Poly1305 (RFC 9180 AEAD codepoint `0x0003`). An AES
    /// codepoint here means every fresh cli with `allow_direct: false`
    /// (the documented production posture per ADR-037) silently encaps
    /// with the wrong cipher and the gateway rejects with
    /// `ohttp::Error::Unsupported`. See problem record
    /// `2026-05-04-ohttp-gateway-decap-unsupported-via-outer-hop`.
    ///
    /// Wire format (RFC 9458 §3.1):
    ///   1B key_id || 2B kem || pub_key || 2B suites_len || (2B kdf || 2B aead)+
    /// AEAD codepoints (RFC 9180):
    ///   0x0001 AES-128-GCM      (FORBIDDEN, ADR-019/046)
    ///   0x0002 AES-256-GCM      (FORBIDDEN, ADR-019)
    ///   0x0003 ChaCha20-Poly1305 (REQUIRED post-ADR-046)
    // @internal
    #[test]
    fn bundled_ohttp_key_advertises_only_chacha20_poly1305() {
        let key = BUNDLED_OHTTP_KEY;

        // X25519 KEM (`0x0020`) → 32-byte pub key → suites_len at offset 35.
        // Sanity-check the prefix so a future KEM swap fails here loudly
        // instead of misparsing into a bogus suite list.
        assert_eq!(
            u16::from_be_bytes([key[1], key[2]]),
            0x0020,
            "bundled key KEM must remain X25519 (0x0020) — \
             update this test if the KEM is intentionally changed"
        );

        let suites_len = u16::from_be_bytes([key[35], key[36]]) as usize;
        assert_eq!(
            suites_len % 4,
            0,
            "suites_len ({suites_len}) must be a multiple of 4 \
             (each suite is 2B kdf + 2B aead per RFC 9458)"
        );
        assert!(
            suites_len > 0,
            "bundled key must advertise at least one suite"
        );
        assert_eq!(
            key.len(),
            37 + suites_len,
            "bundled key length ({}) must equal 37 + suites_len ({suites_len}) — \
             trailing bytes or truncation indicate a regen error",
            key.len()
        );

        for chunk in key[37..37 + suites_len].chunks_exact(4) {
            let kdf = u16::from_be_bytes([chunk[0], chunk[1]]);
            let aead = u16::from_be_bytes([chunk[2], chunk[3]]);
            assert_eq!(
                kdf, 0x0001,
                "bundled key KDF must remain HKDF-SHA256 (0x0001), got 0x{kdf:04x}"
            );
            assert_eq!(
                aead, 0x0003,
                "ADR-046: bundled key must advertise ChaCha20-Poly1305 only \
                 (got AEAD 0x{aead:04x}); regenerate via \
                 `curl -s https://ohttp.vauchi.app/v2/ohttp-key`"
            );
        }
    }
}

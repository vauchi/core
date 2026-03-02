// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Certificate pinning for TLS connections.
//!
//! This module provides TLS certificate pinning support for relay connections,
//! ensuring that only connections to servers with a known certificate are allowed.
//! Uses rustls for pure-Rust TLS (no OpenSSL dependency - works on Android/iOS).

use rustls::pki_types::CertificateDer;
use rustls::ClientConfig;
use std::sync::Arc;
use std::time::Duration;

/// Type alias for the async WebSocket stream.
pub type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Parse PEM-encoded certificates into DER format.
fn parse_pem_certs(pem: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    let mut certs = Vec::new();
    let mut current_cert = String::new();
    let mut in_cert = false;

    for line in pem.lines() {
        if line.contains("-----BEGIN CERTIFICATE-----") {
            in_cert = true;
            current_cert.clear();
        } else if line.contains("-----END CERTIFICATE-----") {
            in_cert = false;
            if !current_cert.is_empty() {
                use base64::Engine;
                let der = base64::engine::general_purpose::STANDARD
                    .decode(&current_cert)
                    .map_err(|e| format!("Invalid base64 in certificate: {}", e))?;
                certs.push(CertificateDer::from(der));
            }
        } else if in_cert {
            current_cert.push_str(line.trim());
        }
    }

    if certs.is_empty() {
        return Err("No certificates found in PEM".to_string());
    }

    Ok(certs)
}

/// Create a rustls ClientConfig with certificate pinning.
fn create_pinned_config(pinned_cert_pem: &str) -> Result<Arc<ClientConfig>, String> {
    let certs = parse_pem_certs(pinned_cert_pem)?;

    let mut root_store = rustls::RootCertStore::empty();
    for cert in certs {
        root_store
            .add(cert)
            .map_err(|e| format!("Failed to add certificate: {}", e))?;
    }

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(Arc::new(config))
}

/// Create a rustls ClientConfig using system/webpki roots (no pinning).
fn create_default_config() -> Result<Arc<ClientConfig>, String> {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(Arc::new(config))
}

/// Connect to a WebSocket server with optional certificate pinning (async).
///
/// If `pinned_cert_pem` is None, uses standard TLS without pinning (for development).
pub async fn connect_with_pinning(
    url_str: &str,
    pinned_cert_pem: Option<&str>,
) -> Result<WsStream, String> {
    let connector = match pinned_cert_pem {
        Some(pem) => {
            let config = create_pinned_config(pem)?;
            Some(tokio_tungstenite::Connector::Rustls(config))
        }
        None => {
            let config = create_default_config()?;
            Some(tokio_tungstenite::Connector::Rustls(config))
        }
    };

    let (ws, _) = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async_tls_with_config(url_str, None, false, connector),
    )
    .await
    .map_err(|_| "Connection timed out".to_string())?
    .map_err(|e| format!("WebSocket connection failed: {}", e))?;

    Ok(ws)
}

// INLINE_TEST_REQUIRED: tests access private functions parse_pem_certs, create_pinned_config, create_default_config
#[cfg(test)]
mod tests {
    use super::*;

    // Self-signed test certificate (generated for testing only, not a real CA).
    const TEST_CERT_PEM: &str = "-----BEGIN CERTIFICATE-----
MIIC/zCCAeegAwIBAgIUW2lF36AYXAAxVZY9vmRkfnNdyXYwDQYJKoZIhvcNAQEL
BQAwDzENMAsGA1UEAwwEdGVzdDAeFw0yNjAzMDIxMjUxMTFaFw0yNzAzMDIxMjUx
MTFaMA8xDTALBgNVBAMMBHRlc3QwggEiMA0GCSqGSIb3DQEBAQUAA4IBDwAwggEK
AoIBAQCkOSSfAJOszXsHnGOSEo5AkVbioFCmVRCKFj50YKr6oez9Rb43fo3rfkeK
yDjmLeFoVN6lFxq0uqG/AazdH21AB/LG9fa0CqFpXwXSebUtlKjLbnvUrf9/MmAE
K8wKHutwUC6Y3l7Rp6mT3DDq7rmY0ZQePfQegR6Mts76ecuZACQQBjhfmM/FvXKl
H+ap2oRJroCn44BhHovC3slnWk9BzLEII+Hjr7KE5ncUPZyYS4XW4CgmNluikO4W
rjw97O3qx6IHzFIBIBxzR8725Gn+Q0cIvrsHj40bJFslOGMinD1tHuHWaJiBKmzV
w8z+e06X7eYkPUIKK8e8of470WWrAgMBAAGjUzBRMB0GA1UdDgQWBBSXQ7YaSAxp
1wo5yOri0Jka8IxDdzAfBgNVHSMEGDAWgBSXQ7YaSAxp1wo5yOri0Jka8IxDdzAP
BgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4IBAQBGA2jE/P4+zpKCSgjm
7ShKFH+3kMeVCT49wChelVvAqrYI/zGUvVaapyLEdoTxSN160eZlidE4k4gecqgb
mba7Ib6HrlzZc8vrZE/KHdtDH/eQW61VHsi8GHuWH/cNIvXqXikGVbBwaoZcPjTn
Qu5vDQelumEnrF8MdSuBYUB8NgQGaYZjNAH3uuqtf6U7pd/vQnGnx9nhBDjQ36Lc
DD8EArSqYfHlfsdBaAyQpnVZv/Qr+KafKVNsZuNEUEvXlIUJVozAhzh97U6dSoRA
z+3RaUqEOQe76dQgQBCmp9zA7IX7zZ9/oL6FBCAhlkHOgGNnCXYaOWc8bUSvIx5M
wupY
-----END CERTIFICATE-----";

    #[test]
    fn test_parse_pem_valid_single_cert() {
        let result = parse_pem_certs(TEST_CERT_PEM);
        assert!(result.is_ok(), "Valid PEM should parse: {:?}", result.err());
        let certs = result.unwrap();
        assert_eq!(certs.len(), 1, "Should parse exactly 1 certificate");
    }

    #[test]
    fn test_parse_pem_multiple_certs() {
        let two_certs = format!("{}\n{}", TEST_CERT_PEM, TEST_CERT_PEM);
        let result = parse_pem_certs(&two_certs);
        assert!(result.is_ok(), "Two PEMs should parse: {:?}", result.err());
        let certs = result.unwrap();
        assert_eq!(certs.len(), 2, "Should parse exactly 2 certificates");
    }

    #[test]
    fn test_parse_pem_empty() {
        let result = parse_pem_certs("");
        assert!(result.is_err(), "Empty input should fail");
        assert_eq!(
            result.unwrap_err(),
            "No certificates found in PEM",
            "Should report no certificates found"
        );
    }

    #[test]
    fn test_parse_pem_no_markers() {
        let result = parse_pem_certs("just some random text\nwithout any PEM markers");
        assert!(result.is_err(), "No PEM markers should fail");
        assert_eq!(
            result.unwrap_err(),
            "No certificates found in PEM",
            "Should report no certificates found"
        );
    }

    #[test]
    fn test_parse_pem_invalid_base64() {
        let bad_pem = "-----BEGIN CERTIFICATE-----\n!!invalid-base64!!\n-----END CERTIFICATE-----";
        let result = parse_pem_certs(bad_pem);
        assert!(result.is_err(), "Invalid base64 should fail");
        let err = result.unwrap_err();
        assert!(
            err.contains("Invalid base64"),
            "Should mention invalid base64, got: {err}"
        );
    }

    #[test]
    fn test_parse_pem_no_end_marker() {
        let no_end = "-----BEGIN CERTIFICATE-----\nMIIBkTCB+wIJALRiMLAh2wZSMA0=\n";
        let result = parse_pem_certs(no_end);
        assert!(result.is_err(), "Missing END marker should fail");
        assert_eq!(
            result.unwrap_err(),
            "No certificates found in PEM",
            "Should report no certificates found"
        );
    }

    #[test]
    fn test_create_pinned_config_valid() {
        let result = create_pinned_config(TEST_CERT_PEM);
        assert!(
            result.is_ok(),
            "Valid PEM should create config: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_create_default_config() {
        let result = create_default_config();
        assert!(
            result.is_ok(),
            "Default config should succeed: {:?}",
            result.err()
        );
    }
}

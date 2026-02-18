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
pub type WsStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

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


// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Certificate pinning for TLS connections.
//!
//! This module provides TLS certificate pinning support for relay connections,
//! ensuring that only connections to servers with a known certificate are allowed.
//! Uses rustls for pure-Rust TLS (no OpenSSL dependency - works on Android/iOS).

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::ClientConfig;
use std::net::TcpStream;
use std::sync::Arc;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::WebSocket;
use url::Url;

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
///
/// The `pinned_cert_pem` should be the PEM-encoded certificate of the relay server.
/// Only connections to servers presenting this exact certificate will be allowed.
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

/// Connect to a WebSocket server with optional certificate pinning.
///
/// If `pinned_cert_pem` is None, uses standard TLS without pinning (for development).
pub fn connect_with_pinning(
    url_str: &str,
    pinned_cert_pem: Option<&str>,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let url = Url::parse(url_str).map_err(|e| format!("Invalid URL: {}", e))?;
    let is_wss = url.scheme() == "wss";

    if !is_wss {
        // Plain WebSocket (ws://) - no TLS
        let request = url_str
            .into_client_request()
            .map_err(|e| format!("Invalid URL: {}", e))?;
        return tungstenite::connect(request)
            .map(|(ws, _)| ws)
            .map_err(|e| format!("Connection failed: {}", e));
    }

    // WSS connection - use TLS
    let host = url.host_str().ok_or("No host in URL")?;
    let port = url.port().unwrap_or(443);
    let addr = format!("{}:{}", host, port);

    // Create TCP connection
    let tcp_stream =
        TcpStream::connect(&addr).map_err(|e| format!("TCP connection failed: {}", e))?;

    // Create TLS config (with or without pinning)
    let tls_config = match pinned_cert_pem {
        Some(pem) => create_pinned_config(pem)?,
        None => create_default_config()?,
    };

    // Create TLS connection
    let server_name: ServerName<'_> = host
        .try_into()
        .map_err(|_| format!("Invalid server name: {}", host))?;

    let tls_conn = rustls::ClientConnection::new(tls_config, server_name.to_owned())
        .map_err(|e| format!("TLS connection setup failed: {}", e))?;

    let tls_stream = rustls::StreamOwned::new(tls_conn, tcp_stream);

    // Upgrade to WebSocket
    let request = url_str
        .into_client_request()
        .map_err(|e| format!("Invalid WebSocket request: {}", e))?;

    let (ws, _) = tungstenite::client(request, MaybeTlsStream::Rustls(tls_stream))
        .map_err(|e| format!("WebSocket upgrade failed: {}", e))?;

    Ok(ws)
}
